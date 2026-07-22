//! Own the fixed complete-support schema and public ecosystem catalog projections.

use crate::language::{LanguageCapabilitySupport, language_capability, language_registry_digest};
use blake3::Hasher;
use serde::Serialize;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::sync::OnceLock;

/// Version of the fixed complete-support evidence schema.
pub const COMPLETE_SUPPORT_SCHEMA_VERSION: u32 = 1;

/// Version of the public language and ecosystem catalog.
pub const SUPPORT_CATALOG_VERSION: u32 = 1;

/// Stable identity of one support profile.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SupportProfileId(&'static str);

impl SupportProfileId {
    /// Construct a static support-profile identity.
    #[must_use]
    const fn new(value: &'static str) -> Self {
        Self(value)
    }

    /// Return the stable profile identity.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Closed semantic kind of one support profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportProfileKind {
    /// A language with its own detection and navigation contract.
    Language,
    /// A language dialect requiring evidence beyond a shared extension.
    Dialect,
    /// A non-language source or configuration format.
    DomainFormat,
    /// A framework projection bound to one exact host profile.
    FrameworkProjection,
}

impl SupportProfileKind {
    /// Return the stable public label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::Dialect => "dialect",
            Self::DomainFormat => "domain_format",
            Self::FrameworkProjection => "framework_projection",
        }
    }

    /// Return this kind's fixed evidence contract.
    #[must_use]
    pub const fn evidence_contract(self) -> ProfileEvidenceContract {
        let admitted_not_applicable = match self {
            Self::Language => LANGUAGE_NOT_APPLICABLE,
            Self::Dialect => DIALECT_NOT_APPLICABLE,
            Self::DomainFormat => DOMAIN_FORMAT_NOT_APPLICABLE,
            Self::FrameworkProjection => FRAMEWORK_NOT_APPLICABLE,
        };
        ProfileEvidenceContract {
            required_slots: EvidenceSlot::ALL,
            admitted_not_applicable,
        }
    }
}

/// Stable user-facing catalog grouping, separate from semantic profile kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationCategory {
    /// Server and service implementation languages.
    Backend,
    /// Browser, component, and web source.
    FrontendWeb,
    /// Systems implementation languages.
    Systems,
    /// Mobile application source.
    Mobile,
    /// Data and scientific source.
    DataScientific,
    /// Enterprise and legacy-modernization source.
    EnterpriseLegacyModernization,
    /// Database and query source.
    DatabaseQuery,
    /// Infrastructure and cloud configuration.
    InfrastructureCloud,
    /// Build, configuration, and template source.
    BuildConfigTemplate,
    /// Test framework projections.
    TestingFrameworks,
    /// Hardware design languages, separate from software systems source.
    HardwareDesign,
}

impl PresentationCategory {
    /// Every category in public rendering order.
    pub const ALL: &'static [Self] = &[
        Self::Backend,
        Self::FrontendWeb,
        Self::Systems,
        Self::Mobile,
        Self::DataScientific,
        Self::EnterpriseLegacyModernization,
        Self::DatabaseQuery,
        Self::InfrastructureCloud,
        Self::BuildConfigTemplate,
        Self::TestingFrameworks,
        Self::HardwareDesign,
    ];

    /// Return the public heading.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Backend => "Backend",
            Self::FrontendWeb => "Frontend and web",
            Self::Systems => "Systems",
            Self::Mobile => "Mobile",
            Self::DataScientific => "Data and scientific",
            Self::EnterpriseLegacyModernization => "Enterprise and legacy modernization",
            Self::DatabaseQuery => "Database and query",
            Self::InfrastructureCloud => "Infrastructure and cloud",
            Self::BuildConfigTemplate => "Build, configuration, and template",
            Self::TestingFrameworks => "Testing frameworks",
            Self::HardwareDesign => "Hardware design",
        }
    }

    /// Return the stable digest label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Backend => "backend",
            Self::FrontendWeb => "frontend-web",
            Self::Systems => "systems",
            Self::Mobile => "mobile",
            Self::DataScientific => "data-scientific",
            Self::EnterpriseLegacyModernization => "enterprise-legacy-modernization",
            Self::DatabaseQuery => "database-query",
            Self::InfrastructureCloud => "infrastructure-cloud",
            Self::BuildConfigTemplate => "build-config-template",
            Self::TestingFrameworks => "testing-frameworks",
            Self::HardwareDesign => "hardware-design",
        }
    }
}

/// Optional presentation tags that never change semantic capability counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationTag {
    /// Source where exact evidence is useful for modernization assessment.
    LegacyModernization,
}

impl PresentationTag {
    /// Return the stable public tag.
    const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyModernization => "legacy-modernization",
        }
    }
}

/// Independent evidence used to distinguish a dialect from shared syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialectEvidenceKind {
    /// A project or workspace manifest selects the dialect.
    ProjectManifest,
    /// A repository-relative path convention selects the dialect.
    PathConvention,
    /// A bounded configuration value selects the dialect.
    Configuration,
    /// A bounded content signature selects the dialect.
    ContentSignature,
}

impl DialectEvidenceKind {
    /// Return the stable evidence label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectManifest => "project-manifest",
            Self::PathConvention => "path-convention",
            Self::Configuration => "configuration",
            Self::ContentSignature => "content-signature",
        }
    }
}

/// Fixed evidence slots owned by the complete-support schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSlot {
    /// Deterministic detection evidence.
    Detection,
    /// Independent dialect-disambiguation evidence.
    DialectEvidence,
    /// Grammar or format parsing evidence.
    Parsing,
    /// Malformed or partial-input behavior.
    MalformedPartial,
    /// Symbols or domain facts.
    Facts,
    /// At least one applicable non-empty accepted relation family.
    Relations,
    /// Exact source occurrences.
    Occurrences,
    /// Resolved, ambiguous, unresolved, and external outcomes.
    ResolutionOutcomes,
    /// Real `SQLite` publish, reopen, and incremental convergence.
    Publication,
    /// Owning unit fixtures.
    UnitFixtures,
    /// Owning integration fixtures.
    IntegrationFixtures,
    /// Representative-repository measurement.
    RepresentativeRepository,
    /// Bounded agent-navigation evaluation.
    AgentNavigation,
}

impl EvidenceSlot {
    /// Every fixed evidence slot in machine-stable order.
    pub const ALL: &'static [Self; 13] = &[
        Self::Detection,
        Self::DialectEvidence,
        Self::Parsing,
        Self::MalformedPartial,
        Self::Facts,
        Self::Relations,
        Self::Occurrences,
        Self::ResolutionOutcomes,
        Self::Publication,
        Self::UnitFixtures,
        Self::IntegrationFixtures,
        Self::RepresentativeRepository,
        Self::AgentNavigation,
    ];

    /// Return the stable evidence-slot label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Detection => "detection",
            Self::DialectEvidence => "dialect-evidence",
            Self::Parsing => "parsing",
            Self::MalformedPartial => "malformed-partial",
            Self::Facts => "facts",
            Self::Relations => "relations",
            Self::Occurrences => "occurrences",
            Self::ResolutionOutcomes => "resolution-outcomes",
            Self::Publication => "publication",
            Self::UnitFixtures => "unit-fixtures",
            Self::IntegrationFixtures => "integration-fixtures",
            Self::RepresentativeRepository => "representative-repository",
            Self::AgentNavigation => "agent-navigation",
        }
    }
}

/// Schema-admitted reason for a reviewed `not_applicable` slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotApplicableReason {
    /// The profile is not a dialect.
    ProfileIsNotDialect,
    /// The exact host profile owns syntax parsing for this projection.
    ExactHostOwnsParsing,
    /// This format has no meaningful navigable relation semantics.
    NoRelationSemantics,
    /// This format has no static resolution outcomes.
    NoStaticResolution,
}

impl NotApplicableReason {
    /// Return the stable reviewed-reason label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProfileIsNotDialect => "profile-is-not-dialect",
            Self::ExactHostOwnsParsing => "exact-host-owns-parsing",
            Self::NoRelationSemantics => "no-relation-semantics",
            Self::NoStaticResolution => "no-static-resolution",
        }
    }
}

/// One schema-admitted evidence-slot exception.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdmittedNotApplicable {
    /// Evidence slot that may not apply.
    pub slot: EvidenceSlot,
    /// Exact admitted reason.
    pub reason: NotApplicableReason,
}

/// Kind-owned fixed evidence contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileEvidenceContract {
    /// Mandatory slots in machine-stable order.
    pub required_slots: &'static [EvidenceSlot; 13],
    /// Narrow typed exceptions accepted for this kind.
    pub admitted_not_applicable: &'static [AdmittedNotApplicable],
}

/// Typed exceptions admitted for language profiles.
const LANGUAGE_NOT_APPLICABLE: &[AdmittedNotApplicable] = &[AdmittedNotApplicable {
    slot: EvidenceSlot::DialectEvidence,
    reason: NotApplicableReason::ProfileIsNotDialect,
}];
/// Dialect profiles admit no evidence omissions.
const DIALECT_NOT_APPLICABLE: &[AdmittedNotApplicable] = &[];
/// Typed exceptions admitted for domain-format profiles.
const DOMAIN_FORMAT_NOT_APPLICABLE: &[AdmittedNotApplicable] = &[
    AdmittedNotApplicable {
        slot: EvidenceSlot::DialectEvidence,
        reason: NotApplicableReason::ProfileIsNotDialect,
    },
    AdmittedNotApplicable {
        slot: EvidenceSlot::Relations,
        reason: NotApplicableReason::NoRelationSemantics,
    },
    AdmittedNotApplicable {
        slot: EvidenceSlot::ResolutionOutcomes,
        reason: NotApplicableReason::NoStaticResolution,
    },
];
/// Typed exceptions admitted for exact-host framework projections.
const FRAMEWORK_NOT_APPLICABLE: &[AdmittedNotApplicable] = &[
    AdmittedNotApplicable {
        slot: EvidenceSlot::DialectEvidence,
        reason: NotApplicableReason::ProfileIsNotDialect,
    },
    AdmittedNotApplicable {
        slot: EvidenceSlot::Parsing,
        reason: NotApplicableReason::ExactHostOwnsParsing,
    },
    AdmittedNotApplicable {
        slot: EvidenceSlot::MalformedPartial,
        reason: NotApplicableReason::ExactHostOwnsParsing,
    },
];

/// Machine-checkable evidence bound to every capability owner relevant to navigation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceReference {
    /// Versioned language registry identity.
    pub registry: &'static str,
    /// Parser or format-parser identity.
    pub parser: &'static str,
    /// Fact or semantic-provider identity.
    pub provider: &'static str,
    /// Accepted relation-family identity.
    pub relation: &'static str,
    /// Publication contract identity.
    pub publication: &'static str,
    /// Final navigation contract identity.
    pub navigation: &'static str,
    /// Test, fixture, workflow, or measurement locator.
    pub artifact: &'static str,
}

/// Independently reviewed typed `not_applicable` evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReviewedNotApplicable {
    /// Schema-admitted reason.
    pub reason: NotApplicableReason,
    /// Stable independent review locator.
    pub review: &'static str,
}

/// State of one mandatory complete-support evidence slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEvidence {
    /// Evidence has not passed; valid only for candidates.
    Pending,
    /// Evidence passed and is bound to every applicable owner identity.
    Passed(EvidenceReference),
    /// Evidence does not apply for a schema-admitted independently reviewed reason.
    NotApplicable(ReviewedNotApplicable),
}

/// One accepted complete-support profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CompleteSupportProfile {
    /// Stable profile identity.
    pub id: SupportProfileId,
    /// Closed semantic profile kind.
    pub kind: SupportProfileKind,
    /// Exact host profile, required for framework projections.
    pub host: Option<SupportProfileId>,
    /// Independent dialect evidence, required for dialect profiles.
    pub dialect_evidence: Option<DialectEvidenceKind>,
    /// Evidence values aligned exactly with [`EvidenceSlot::ALL`].
    pub evidence: [ProfileEvidence; 13],
    /// Stable independent review locator confirming the complete claim.
    pub independent_review: &'static str,
}

/// Accepted complete-support inventory. It stays empty until task 7.3 revalidates
/// candidates against the final MCP navigation surface and independent review.
pub const ACCEPTED_COMPLETE_SUPPORT_PROFILES: &[CompleteSupportProfile] = &[];

/// Documentation assessment kept separate from runtime capability truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogAssessment {
    /// A lower-tier runtime row exists and remains a candidate, never complete.
    RuntimeCandidate {
        /// Exact language registry identity.
        registry_id: &'static str,
    },
    /// A future profile is classified but has no runtime capability claim.
    Planned,
    /// The profile is classified but unavailable at the fixed support contract.
    Unavailable {
        /// Typed reason that prevents a runtime/complete claim.
        reason: CatalogUnavailableReason,
    },
}

/// Why a documentation-only ecosystem row is unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogUnavailableReason {
    /// No runtime capability row owns the profile.
    NoRuntimeCapability,
    /// Independent dialect evidence has not been admitted.
    MissingIndependentDialectEvidence,
    /// No exact framework projection has been admitted.
    MissingFrameworkProjection,
}

impl CatalogUnavailableReason {
    /// Return the stable unavailable-reason label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::NoRuntimeCapability => "no-runtime-capability",
            Self::MissingIndependentDialectEvidence => "missing-independent-dialect-evidence",
            Self::MissingFrameworkProjection => "missing-framework-projection",
        }
    }
}

/// One documentation and candidate-profile catalog row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EcosystemCatalogRow {
    /// Stable catalog profile identity.
    pub id: SupportProfileId,
    /// Human-facing family name.
    pub label: &'static str,
    /// Closed semantic profile kind.
    pub kind: SupportProfileKind,
    /// Presentation-only category.
    pub category: PresentationCategory,
    /// Presentation-only tags.
    pub tags: &'static [PresentationTag],
    /// Exact host profile for dialects and framework projections.
    pub host: Option<SupportProfileId>,
    /// Required independent dialect-evidence class.
    pub dialect_evidence: Option<DialectEvidenceKind>,
    /// Runtime candidate or documentation-only state.
    pub assessment: CatalogAssessment,
}

/// Shared legacy-modernization presentation tag.
const LEGACY: &[PresentationTag] = &[PresentationTag::LegacyModernization];
/// Empty presentation-tag set.
const NO_TAGS: &[PresentationTag] = &[];

/// Construct one static catalog row without a builder abstraction.
const fn catalog_row(
    id: &'static str,
    label: &'static str,
    kind: SupportProfileKind,
    category: PresentationCategory,
    tags: &'static [PresentationTag],
    host: Option<&'static str>,
    dialect_evidence: Option<DialectEvidenceKind>,
    assessment: CatalogAssessment,
) -> EcosystemCatalogRow {
    EcosystemCatalogRow {
        id: SupportProfileId::new(id),
        label,
        kind,
        category,
        tags,
        host: match host {
            Some(value) => Some(SupportProfileId::new(value)),
            None => None,
        },
        dialect_evidence,
        assessment,
    }
}

/// Construct a lower-tier runtime candidate assessment.
const fn candidate(registry_id: &'static str) -> CatalogAssessment {
    CatalogAssessment::RuntimeCandidate { registry_id }
}

/// Construct a documentation-only unavailable assessment.
const fn unavailable(reason: CatalogUnavailableReason) -> CatalogAssessment {
    CatalogAssessment::Unavailable { reason }
}

/// Versioned documentation and candidate-profile catalog.
pub const ECOSYSTEM_CATALOG: &[EcosystemCatalogRow] = &[
    catalog_row(
        "abap",
        "ABAP",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "abl",
        "OpenEdge ABL",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        candidate("abl"),
    ),
    catalog_row(
        "cobol",
        "COBOL",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        candidate("cobol"),
    ),
    catalog_row(
        "cobol-fixed",
        "COBOL fixed form",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("cobol"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "cobol-free",
        "COBOL free form",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("cobol"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "fortran",
        "Fortran",
        SupportProfileKind::Language,
        PresentationCategory::DataScientific,
        LEGACY,
        None,
        None,
        candidate("fortran"),
    ),
    catalog_row(
        "fortran-fixed",
        "Fortran fixed form",
        SupportProfileKind::Dialect,
        PresentationCategory::DataScientific,
        LEGACY,
        Some("fortran"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "fortran-free",
        "Fortran free form",
        SupportProfileKind::Dialect,
        PresentationCategory::DataScientific,
        LEGACY,
        Some("fortran"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "pli",
        "PL/I",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "rpg",
        "RPG",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "ile-rpg",
        "ILE RPG",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("rpg"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "jcl",
        "JCL",
        SupportProfileKind::DomainFormat,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "rexx",
        "REXX",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "ibmi-cl",
        "IBM i CL",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "hlasm",
        "HLASM",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "assembler",
        "Other assembler families",
        SupportProfileKind::Language,
        PresentationCategory::Systems,
        LEGACY,
        None,
        None,
        candidate("asm"),
    ),
    catalog_row(
        "ada",
        "Ada",
        SupportProfileKind::Language,
        PresentationCategory::Systems,
        LEGACY,
        None,
        None,
        candidate("ada"),
    ),
    catalog_row(
        "pascal",
        "Pascal",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        candidate("pascal"),
    ),
    catalog_row(
        "object-pascal",
        "Object Pascal",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("pascal"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "delphi",
        "Delphi",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("pascal"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "visual-basic",
        "Visual Basic source (unqualified)",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        candidate("vb"),
    ),
    catalog_row(
        "vb6",
        "Visual Basic 6",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("visual-basic"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "vbnet",
        "VB.NET",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("visual-basic"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "vba",
        "VBA",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("visual-basic"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "vbscript",
        "VBScript",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "classic-asp",
        "Classic ASP",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("vbscript"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "sas",
        "SAS",
        SupportProfileKind::Language,
        PresentationCategory::DataScientific,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "natural",
        "Natural",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "mumps",
        "MUMPS / M",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "powerbuilder",
        "PowerBuilder",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("powerscript"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "powerscript",
        "PowerScript",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "xbase",
        "xBase",
        SupportProfileKind::Language,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "clipper",
        "Clipper",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("xbase"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "foxpro",
        "FoxPro",
        SupportProfileKind::Dialect,
        PresentationCategory::EnterpriseLegacyModernization,
        LEGACY,
        Some("xbase"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "perl",
        "Perl",
        SupportProfileKind::Language,
        PresentationCategory::Backend,
        LEGACY,
        None,
        None,
        candidate("perl"),
    ),
    catalog_row(
        "cfml",
        "ColdFusion / CFML",
        SupportProfileKind::Language,
        PresentationCategory::Backend,
        LEGACY,
        None,
        None,
        candidate("cfml"),
    ),
    catalog_row(
        "actionscript",
        "ActionScript",
        SupportProfileKind::Language,
        PresentationCategory::FrontendWeb,
        LEGACY,
        None,
        None,
        candidate("actionscript"),
    ),
    catalog_row(
        "flex",
        "Apache Flex",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::FrontendWeb,
        LEGACY,
        Some("actionscript"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "swift",
        "Swift",
        SupportProfileKind::Language,
        PresentationCategory::Mobile,
        NO_TAGS,
        None,
        None,
        candidate("swift"),
    ),
    catalog_row(
        "kotlin",
        "Kotlin",
        SupportProfileKind::Language,
        PresentationCategory::Mobile,
        NO_TAGS,
        None,
        None,
        candidate("kotlin"),
    ),
    catalog_row(
        "dart",
        "Dart",
        SupportProfileKind::Language,
        PresentationCategory::Mobile,
        NO_TAGS,
        None,
        None,
        candidate("dart"),
    ),
    catalog_row(
        "vhdl",
        "VHDL",
        SupportProfileKind::Language,
        PresentationCategory::HardwareDesign,
        NO_TAGS,
        None,
        None,
        candidate("vhdl"),
    ),
    catalog_row(
        "verilog",
        "Verilog",
        SupportProfileKind::Language,
        PresentationCategory::HardwareDesign,
        NO_TAGS,
        None,
        None,
        candidate("verilog"),
    ),
    catalog_row(
        "systemverilog",
        "SystemVerilog",
        SupportProfileKind::Language,
        PresentationCategory::HardwareDesign,
        NO_TAGS,
        None,
        None,
        candidate("systemverilog"),
    ),
    catalog_row(
        "sql",
        "SQL (unqualified)",
        SupportProfileKind::Language,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        None,
        None,
        candidate("sql"),
    ),
    catalog_row(
        "oracle-plsql",
        "Oracle PL/SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "postgres-plpgsql",
        "PostgreSQL PL/pgSQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "tsql",
        "T-SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::ContentSignature),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "mysql",
        "MySQL SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "mariadb",
        "MariaDB SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "sqlite-sql",
        "SQLite SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "bigquery-sql",
        "BigQuery SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "snowflake-sql",
        "Snowflake SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "redshift-sql",
        "Redshift SQL",
        SupportProfileKind::Dialect,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        Some(DialectEvidenceKind::Configuration),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "dbt-jinja-sql",
        "dbt / Jinja SQL",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::DatabaseQuery,
        NO_TAGS,
        Some("sql"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "terraform",
        "Terraform",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        candidate("terraform"),
    ),
    catalog_row(
        "opentofu",
        "OpenTofu",
        SupportProfileKind::Dialect,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("terraform"),
        Some(DialectEvidenceKind::ProjectManifest),
        unavailable(CatalogUnavailableReason::MissingIndependentDialectEvidence),
    ),
    catalog_row(
        "hcl",
        "HCL",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        candidate("hcl"),
    ),
    catalog_row(
        "bicep",
        "Bicep",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        candidate("bicep"),
    ),
    catalog_row(
        "arm-template",
        "Azure ARM template",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "cloudformation",
        "AWS CloudFormation",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "sam",
        "AWS SAM",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "pulumi-typescript",
        "Pulumi TypeScript constructs",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("typescript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "pulumi-python",
        "Pulumi Python constructs",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("python"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "kubernetes",
        "Kubernetes manifests",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        unavailable(CatalogUnavailableReason::NoRuntimeCapability),
    ),
    catalog_row(
        "helm",
        "Helm charts",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("yaml"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "kustomize",
        "Kustomize",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("yaml"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "crossplane",
        "Crossplane",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("yaml"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "ansible",
        "Ansible",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("yaml"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "nix",
        "Nix",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        candidate("nix"),
    ),
    catalog_row(
        "cue",
        "CUE",
        SupportProfileKind::DomainFormat,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        None,
        None,
        candidate("cue"),
    ),
    catalog_row(
        "dockerfile",
        "Dockerfile",
        SupportProfileKind::DomainFormat,
        PresentationCategory::BuildConfigTemplate,
        NO_TAGS,
        None,
        None,
        candidate("dockerfile"),
    ),
    catalog_row(
        "compose",
        "Docker Compose",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("yaml"),
        None,
        unavailable(CatalogUnavailableReason::MissingFrameworkProjection),
    ),
    catalog_row(
        "aws-cdk-typescript",
        "AWS CDK TypeScript constructs",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("typescript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "aws-cdk-python",
        "AWS CDK Python constructs",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::InfrastructureCloud,
        NO_TAGS,
        Some("python"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "playwright-javascript",
        "Playwright for JavaScript",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("javascript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "playwright-typescript",
        "Playwright for TypeScript",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("typescript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "jest-javascript",
        "Jest for JavaScript",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("javascript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "jest-typescript",
        "Jest for TypeScript",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("typescript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "vitest-typescript",
        "Vitest for TypeScript",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("typescript"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "pytest",
        "pytest",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("python"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "junit",
        "JUnit",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("java"),
        None,
        CatalogAssessment::Planned,
    ),
    catalog_row(
        "xunit-csharp",
        "xUnit.net",
        SupportProfileKind::FrameworkProjection,
        PresentationCategory::TestingFrameworks,
        NO_TAGS,
        Some("csharp"),
        None,
        CatalogAssessment::Planned,
    ),
];

/// One lower-tier candidate derived from a live registry row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateSupportProfile {
    /// Catalog profile identity.
    pub id: SupportProfileId,
    /// Closed profile kind.
    pub kind: SupportProfileKind,
    /// Exact host profile when applicable.
    pub host: Option<SupportProfileId>,
    /// Exact runtime registry identity.
    pub registry_id: &'static str,
    /// Runtime-achieved capability axes; never a complete-support claim.
    pub achieved: LanguageCapabilitySupport,
}

/// Derive lower-tier candidates from catalog rows with exact live registry owners.
#[must_use]
pub fn candidate_support_profiles() -> Vec<CandidateSupportProfile> {
    ECOSYSTEM_CATALOG
        .iter()
        .filter_map(|row| match row.assessment {
            CatalogAssessment::RuntimeCandidate { registry_id } => language_capability(registry_id)
                .map(|capability| CandidateSupportProfile {
                    id: row.id,
                    kind: row.kind,
                    host: row.host,
                    registry_id,
                    achieved: capability.support,
                }),
            CatalogAssessment::Planned | CatalogAssessment::Unavailable { .. } => None,
        })
        .collect()
}

/// Shared identity emitted by the Markdown and Pages projections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SupportCatalogIdentity {
    /// Fixed complete-support schema version.
    pub schema_version: u32,
    /// Documentation catalog version.
    pub catalog_version: u32,
    /// Digest binding the catalog rows and live registry identity.
    pub digest: String,
}

/// Return the catalog identity shared by every public projection.
#[must_use]
pub fn support_catalog_identity() -> SupportCatalogIdentity {
    SupportCatalogIdentity {
        schema_version: COMPLETE_SUPPORT_SCHEMA_VERSION,
        catalog_version: SUPPORT_CATALOG_VERSION,
        digest: support_catalog_digest().to_owned(),
    }
}

/// Return the cached deterministic catalog digest.
fn support_catalog_digest() -> &'static str {
    static DIGEST: OnceLock<String> = OnceLock::new();
    DIGEST.get_or_init(|| {
        let mut hasher = Hasher::new();
        hash_value(&mut hasher, &COMPLETE_SUPPORT_SCHEMA_VERSION.to_string());
        hash_value(&mut hasher, &SUPPORT_CATALOG_VERSION.to_string());
        hash_value(&mut hasher, &language_registry_digest());
        for kind in [
            SupportProfileKind::Language,
            SupportProfileKind::Dialect,
            SupportProfileKind::DomainFormat,
            SupportProfileKind::FrameworkProjection,
        ] {
            hash_value(&mut hasher, kind.as_str());
            let contract = kind.evidence_contract();
            for slot in contract.required_slots {
                hash_value(&mut hasher, slot.as_str());
            }
            for admitted in contract.admitted_not_applicable {
                hash_value(&mut hasher, admitted.slot.as_str());
                hash_value(&mut hasher, admitted.reason.as_str());
            }
        }
        for row in ECOSYSTEM_CATALOG {
            hash_value(&mut hasher, row.id.as_str());
            hash_value(&mut hasher, row.label);
            hash_value(&mut hasher, row.kind.as_str());
            hash_value(&mut hasher, row.category.as_str());
            hash_value(&mut hasher, row.host.map_or("", SupportProfileId::as_str));
            hash_value(
                &mut hasher,
                row.dialect_evidence.map_or("", DialectEvidenceKind::as_str),
            );
            for tag in row.tags {
                hash_value(&mut hasher, tag.as_str());
            }
            match row.assessment {
                CatalogAssessment::RuntimeCandidate { registry_id } => {
                    hash_value(&mut hasher, "runtime-candidate");
                    hash_value(&mut hasher, registry_id);
                }
                CatalogAssessment::Planned => hash_value(&mut hasher, "planned"),
                CatalogAssessment::Unavailable { reason } => {
                    hash_value(&mut hasher, "unavailable");
                    hash_value(&mut hasher, reason.as_str());
                }
            }
        }
        hasher.finalize().to_hex().to_string()
    })
}

/// Hash one length-delimited catalog value.
fn hash_value(hasher: &mut Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

/// Catalog validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportCatalogError(String);

impl fmt::Display for SupportCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for SupportCatalogError {}

/// Validate the fixed schema, accepted inventory, and documentation catalog.
///
/// # Errors
///
/// Returns a deterministic error for duplicate/empty identities, invalid host
/// or dialect bindings, missing runtime candidates, or incomplete accepted
/// complete-support evidence.
pub fn validate_support_catalog() -> Result<(), SupportCatalogError> {
    let mut ids = BTreeSet::new();
    for row in ECOSYSTEM_CATALOG {
        if row.id.as_str().is_empty() || row.label.is_empty() {
            return Err(invalid("catalog identity and label must be non-empty"));
        }
        if !ids.insert(row.id) {
            return Err(invalid(format!(
                "duplicate ecosystem catalog profile {:?}",
                row.id.as_str()
            )));
        }
        validate_kind_bindings(row.kind, row.host, row.dialect_evidence, row.id)?;
        if let CatalogAssessment::RuntimeCandidate { registry_id } = row.assessment
            && language_capability(registry_id).is_none()
        {
            return Err(invalid(format!(
                "candidate {:?} has no runtime registry owner {registry_id:?}",
                row.id.as_str()
            )));
        }
    }
    for row in ECOSYSTEM_CATALOG {
        if let Some(host) = row.host
            && !ids.contains(&host)
            && language_capability(host.as_str()).is_none()
        {
            return Err(invalid(format!(
                "profile {:?} has unknown exact host {:?}",
                row.id.as_str(),
                host.as_str()
            )));
        }
    }
    let mut complete_ids = BTreeSet::new();
    for profile in ACCEPTED_COMPLETE_SUPPORT_PROFILES {
        if !complete_ids.insert(profile.id) || !ids.contains(&profile.id) {
            return Err(invalid(format!(
                "accepted complete profile {:?} is duplicate or absent from the catalog",
                profile.id.as_str()
            )));
        }
        validate_complete_profile(profile)?;
    }
    Ok(())
}

/// Validate host and dialect evidence required by a profile kind.
fn validate_kind_bindings(
    kind: SupportProfileKind,
    host: Option<SupportProfileId>,
    dialect_evidence: Option<DialectEvidenceKind>,
    id: SupportProfileId,
) -> Result<(), SupportCatalogError> {
    if matches!(kind, SupportProfileKind::FrameworkProjection) && host.is_none() {
        return Err(invalid(format!(
            "framework projection {:?} has no exact host profile",
            id.as_str()
        )));
    }
    if matches!(kind, SupportProfileKind::Dialect) && (host.is_none() || dialect_evidence.is_none())
    {
        return Err(invalid(format!(
            "dialect {:?} lacks an exact host or independent dialect evidence",
            id.as_str()
        )));
    }
    Ok(())
}

/// Validate one accepted complete profile against its fixed evidence contract.
fn validate_complete_profile(profile: &CompleteSupportProfile) -> Result<(), SupportCatalogError> {
    if profile.id.as_str().is_empty() {
        return Err(invalid("complete profile identity must be non-empty"));
    }
    validate_kind_bindings(
        profile.kind,
        profile.host,
        profile.dialect_evidence,
        profile.id,
    )?;
    if let Some(host) = profile.host
        && !ECOSYSTEM_CATALOG.iter().any(|row| row.id == host)
        && language_capability(host.as_str()).is_none()
    {
        return Err(invalid(format!(
            "complete profile {:?} has unknown exact host {:?}",
            profile.id.as_str(),
            host.as_str()
        )));
    }
    if profile.independent_review.is_empty() {
        return Err(invalid(format!(
            "complete profile {:?} has no independent review",
            profile.id.as_str()
        )));
    }
    let contract = profile.kind.evidence_contract();
    for (index, slot) in contract.required_slots.iter().enumerate() {
        match profile.evidence[index] {
            ProfileEvidence::Pending => {
                return Err(invalid(format!(
                    "complete profile {:?} has pending {} evidence",
                    profile.id.as_str(),
                    slot.as_str()
                )));
            }
            ProfileEvidence::Passed(reference) => {
                validate_evidence_reference(profile.id, *slot, reference)?;
            }
            ProfileEvidence::NotApplicable(reviewed) => {
                let admitted = contract
                    .admitted_not_applicable
                    .iter()
                    .any(|entry| entry.slot == *slot && entry.reason == reviewed.reason);
                if !admitted || reviewed.review.is_empty() {
                    return Err(invalid(format!(
                        "complete profile {:?} has unreviewed or unadmitted not_applicable {} evidence",
                        profile.id.as_str(),
                        slot.as_str()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Validate that evidence is bound to every applicable owner identity.
fn validate_evidence_reference(
    profile: SupportProfileId,
    slot: EvidenceSlot,
    reference: EvidenceReference,
) -> Result<(), SupportCatalogError> {
    let fields = [
        reference.registry,
        reference.parser,
        reference.provider,
        reference.relation,
        reference.publication,
        reference.navigation,
        reference.artifact,
    ];
    if fields.iter().any(|field| field.is_empty()) {
        return Err(invalid(format!(
            "complete profile {:?} has unbound {} evidence",
            profile.as_str(),
            slot.as_str()
        )));
    }
    Ok(())
}

/// Construct one deterministic catalog validation error.
fn invalid(message: impl Into<String>) -> SupportCatalogError {
    SupportCatalogError(message.into())
}

/// Append the grouped catalog projection to the generated Markdown authority.
///
/// # Errors
///
/// Returns a formatting error if writing to the owned `String` fails.
pub fn append_support_catalog_markdown(output: &mut String) -> fmt::Result {
    let identity = support_catalog_identity();
    let candidates = candidate_support_profiles();
    let planned = ECOSYSTEM_CATALOG
        .iter()
        .filter(|row| matches!(row.assessment, CatalogAssessment::Planned))
        .count();
    let unavailable = ECOSYSTEM_CATALOG.len() - candidates.len() - planned;
    output.push_str("\n## Language & Ecosystem Support\n\n");
    writeln!(
        output,
        "Complete-support schema version: `{}`. Ecosystem catalog version: `{}`. Catalog digest: `{}`.",
        identity.schema_version, identity.catalog_version, identity.digest
    )?;
    output.push_str(
        "\n`Complete` means conformance to the fixed ProjectAtlas navigation contract, not compiler, build-system, runtime, or whole-language completeness. The initial accepted complete-support inventory is empty; candidates remain at their achieved detected/parsed/symbol/semantic/benchmarked tiers until the final MCP navigation surface and independent review pass in task 7.3.\n\n",
    );
    writeln!(
        output,
        "Catalog profile counts stay separate: **{}** languages, **{}** dialects, **{}** domain formats, and **{}** framework projections. Current assessment: **{}** lower-tier runtime candidates, **{}** planned documentation rows, **{}** unavailable documentation rows, and **{}** accepted complete profiles.",
        count_kind(SupportProfileKind::Language),
        count_kind(SupportProfileKind::Dialect),
        count_kind(SupportProfileKind::DomainFormat),
        count_kind(SupportProfileKind::FrameworkProjection),
        candidates.len(),
        planned,
        unavailable,
        ACCEPTED_COMPLETE_SUPPORT_PROFILES.len()
    )?;
    output.push_str(
        "\n### Detection-to-navigation pipeline\n\nProjectAtlas first applies deterministic registry-owned detection. Built-in or explicitly enabled contained optional parsing then produces honest parse coverage; fact providers retain their own provenance; typed resolution preserves resolved, ambiguous, unresolved, and external outcomes; one atomic SQLite generation publishes exact occurrences and relations; freshness-aware MCP navigation returns bounded source selectors and exact evidence. We reuse maintained license-compatible Tree-sitter grammars, generated parser/node metadata, and trustworthy standard queries before adding bounded ProjectAtlas queries or concrete Rust semantic logic. ProjectAtlas never executes repository code, and an absent optional pack leaves default-core startup and navigation independent.\n\n",
    );
    output.push_str(
        "The `legacy-modernization` tag identifies source where trustworthy dependency and exact-evidence navigation is valuable. It does **not** claim automatic conversion or select a target language. Planned and unavailable rows below are documentation classifications only: they create no runtime registry row and contribute to no capability or complete-support total.\n\n",
    );
    output.push_str("### Architecture paths\n\n");
    for link in ARCHITECTURE_LINKS {
        writeln!(output, "- [{}]({})", link.label, link.markdown_href)?;
    }
    for category in PresentationCategory::ALL {
        writeln!(output, "\n### {}\n", category.label())?;
        output.push_str("| Profile | Kind | Host | Assessment | Dialect evidence | Tags |\n");
        output.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for row in ECOSYSTEM_CATALOG
            .iter()
            .filter(|row| row.category == *category)
        {
            writeln!(
                output,
                "| {} | `{}` | {} | {} | {} | {} |",
                row.label,
                row.kind.as_str(),
                row.host.map_or("—", SupportProfileId::as_str),
                assessment_label(*row),
                row.dialect_evidence
                    .map_or("—", DialectEvidenceKind::as_str),
                tags_label(row.tags)
            )?;
        }
    }
    Ok(())
}

/// Render the grouped GitHub Pages projection from the same catalog identity.
///
/// # Errors
///
/// Returns a formatting error if writing to the owned `String` fails.
pub fn render_language_support_html() -> Result<String, fmt::Error> {
    let identity = support_catalog_identity();
    let mut output = String::new();
    output.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>ProjectAtlas Language &amp; Ecosystem Support</title><style>body{font:16px/1.55 system-ui,sans-serif;color:#1f2933;background:#f7f9fb;margin:0}main{max-width:1100px;margin:auto;padding:40px 24px}section{background:#fff;border:1px solid #d8e1e8;border-radius:8px;padding:18px;margin:18px 0}table{border-collapse:collapse;width:100%}th,td{border-bottom:1px solid #d8e1e8;padding:8px;text-align:left;vertical-align:top}code{overflow-wrap:anywhere}.meta{color:#52616f}.tag{white-space:nowrap}</style></head><body><main>");
    output.push_str("<h1>Language &amp; Ecosystem Support</h1>");
    write!(
        output,
        "<p class=\"meta\">Complete-support schema <code>{}</code>; ecosystem catalog <code>{}</code>; catalog digest <code>{}</code>.</p>",
        identity.schema_version, identity.catalog_version, identity.digest
    )?;
    output.push_str("<p><strong>Complete</strong> means the fixed ProjectAtlas navigation contract, not compiler, runtime, or whole-language completeness. The accepted complete inventory is empty until final MCP navigation revalidation and independent review. Planned and unavailable rows are documentation only and add nothing to runtime capability totals.</p>");
    output.push_str("<p>Detection selects a registry row; contained parsing and concrete fact providers retain honest provenance; typed resolution and atomic SQLite publication preserve exact occurrences; freshness-aware MCP navigation returns bounded selectors and exact source evidence. Maintained compatible grammars and standard queries are reused first. ProjectAtlas does not execute repository code, and absent optional packs do not burden core.</p>");
    output.push_str("<p>The <code>legacy-modernization</code> tag is navigation context, not an automatic-conversion or target-language claim. See the <a href=\"https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/language-support.md\">generated Markdown authority</a>.</p>");
    output.push_str("<h2>Architecture paths</h2><ul>");
    for link in ARCHITECTURE_LINKS {
        write!(
            output,
            "<li><a href=\"{}\">{}</a></li>",
            link.pages_href, link.label
        )?;
    }
    output.push_str("</ul>");
    for category in PresentationCategory::ALL {
        write!(
            output,
            "<section><h2>{}</h2><table><thead><tr><th>Profile</th><th>Kind</th><th>Host</th><th>Assessment</th><th>Dialect evidence</th><th>Tags</th></tr></thead><tbody>",
            category.label()
        )?;
        for row in ECOSYSTEM_CATALOG
            .iter()
            .filter(|row| row.category == *category)
        {
            write!(
                output,
                "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td class=\"tag\">{}</td></tr>",
                row.label,
                row.kind.as_str(),
                row.host.map_or("—", SupportProfileId::as_str),
                assessment_label(*row),
                row.dialect_evidence
                    .map_or("—", DialectEvidenceKind::as_str),
                tags_label(row.tags)
            )?;
        }
        output.push_str("</tbody></table></section>");
    }
    output.push_str("</main></body></html>\n");
    Ok(output)
}

/// Count catalog rows for one semantic profile kind.
fn count_kind(kind: SupportProfileKind) -> usize {
    ECOSYSTEM_CATALOG
        .iter()
        .filter(|row| row.kind == kind)
        .count()
}

/// Render one assessment without promoting documentation-only rows.
fn assessment_label(row: EcosystemCatalogRow) -> String {
    match row.assessment {
        CatalogAssessment::RuntimeCandidate { registry_id } => language_capability(registry_id)
            .map_or_else(
                || format!("invalid runtime binding `{registry_id}`"),
                |capability| {
                    let support = capability.support;
                    format!(
                        "candidate: detected {}, parsed {}, symbols {}, semantic {}, benchmarked {}",
                        support.detected.as_str(),
                        support.parsed.as_str(),
                        support.symbols.as_str(),
                        support.semantic.as_str(),
                        support.benchmarked.as_str()
                    )
                },
            ),
        CatalogAssessment::Planned => "planned (documentation only)".to_owned(),
        CatalogAssessment::Unavailable { reason } => {
            format!("unavailable: {}", reason.as_str())
        }
    }
}

/// Render presentation-only tags.
fn tags_label(tags: &[PresentationTag]) -> String {
    if tags.is_empty() {
        "—".to_owned()
    } else {
        tags.iter()
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One durable architecture route rendered in both projections.
struct ArchitectureLink {
    /// Human-facing link label.
    label: &'static str,
    /// Repository-relative Markdown target.
    markdown_href: &'static str,
    /// Absolute GitHub target used by Pages.
    pages_href: &'static str,
}

/// Architecture links required by the catalog contract.
const ARCHITECTURE_LINKS: &[ArchitectureLink] = &[
    ArchitectureLink {
        label: "Canonical Mermaid architecture views",
        markdown_href: "projectatlas-3-architecture.md#architecture-views",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#architecture-views",
    },
    ArchitectureLink {
        label: "System and component ownership",
        markdown_href: "projectatlas-3-architecture.md#system-and-component-architecture",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#system-and-component-architecture",
    },
    ArchitectureLink {
        label: "Crate dependency and ownership",
        markdown_href: "projectatlas-3-architecture.md#crate-dependency-and-ownership",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#crate-dependency-and-ownership",
    },
    ArchitectureLink {
        label: "Database authority",
        markdown_href: "projectatlas-3-architecture.md#database-authority-and-responsibility",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#database-authority-and-responsibility",
    },
    ArchitectureLink {
        label: "Graph physical model",
        markdown_href: "projectatlas-3-architecture.md#normalized-graph-physical-model",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#normalized-graph-physical-model",
    },
    ArchitectureLink {
        label: "Bounded graph read",
        markdown_href: "projectatlas-3-architecture.md#bounded-graph-read-with-purpose-projection",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#bounded-graph-read-with-purpose-projection",
    },
    ArchitectureLink {
        label: "MCP read communication",
        markdown_href: "projectatlas-3-architecture.md#mcp-read-communication-sequence",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#mcp-read-communication-sequence",
    },
    ArchitectureLink {
        label: "Transactional publication",
        markdown_href: "projectatlas-3-architecture.md#index-and-transactional-publication-flow",
        pages_href: "https://github.com/styler-ai/ProjectAtlas/blob/dev/docs/projectatlas-3-architecture.md#index-and-transactional-publication-flow",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_valid_and_complete_inventory_is_truthfully_empty() -> Result<(), Box<dyn Error>> {
        validate_support_catalog()?;
        require_test(
            ACCEPTED_COMPLETE_SUPPORT_PROFILES.is_empty(),
            "complete-support inventory was promoted before final navigation review",
        )?;
        require_test(
            !candidate_support_profiles().is_empty(),
            "runtime candidate projection is unexpectedly empty",
        )?;
        Ok(())
    }

    #[test]
    fn required_families_are_distinct_and_frameworks_have_exact_hosts() {
        let ids = ECOSYSTEM_CATALOG
            .iter()
            .map(|row| row.id.as_str())
            .collect::<BTreeSet<_>>();
        for required in [
            "abap",
            "abl",
            "cobol",
            "fortran",
            "pli",
            "rpg",
            "ile-rpg",
            "jcl",
            "rexx",
            "ibmi-cl",
            "hlasm",
            "ada",
            "pascal",
            "object-pascal",
            "delphi",
            "vb6",
            "vbnet",
            "vba",
            "vbscript",
            "classic-asp",
            "sas",
            "natural",
            "mumps",
            "powerbuilder",
            "powerscript",
            "xbase",
            "clipper",
            "foxpro",
            "perl",
            "cfml",
            "actionscript",
            "flex",
            "vhdl",
            "verilog",
            "systemverilog",
            "sql",
            "oracle-plsql",
            "postgres-plpgsql",
            "tsql",
            "mysql",
            "mariadb",
            "sqlite-sql",
            "bigquery-sql",
            "snowflake-sql",
            "redshift-sql",
            "dbt-jinja-sql",
            "terraform",
            "opentofu",
            "hcl",
            "bicep",
            "arm-template",
            "cloudformation",
            "sam",
            "kubernetes",
            "helm",
            "kustomize",
            "crossplane",
            "ansible",
            "nix",
            "cue",
            "dockerfile",
            "compose",
            "playwright-javascript",
            "playwright-typescript",
        ] {
            assert!(ids.contains(required), "missing classification {required}");
        }
        assert_ne!(SupportProfileId::new("abap"), SupportProfileId::new("abl"));
        assert!(
            ECOSYSTEM_CATALOG
                .iter()
                .filter(|row| matches!(row.kind, SupportProfileKind::FrameworkProjection))
                .all(|row| row.host.is_some())
        );
        assert!(
            ECOSYSTEM_CATALOG
                .iter()
                .filter(|row| matches!(row.kind, SupportProfileKind::Dialect))
                .all(|row| row.host.is_some() && row.dialect_evidence.is_some())
        );
    }

    #[test]
    fn planned_and_unavailable_rows_create_no_runtime_or_complete_claim() {
        for row in ECOSYSTEM_CATALOG {
            if !matches!(row.assessment, CatalogAssessment::RuntimeCandidate { .. }) {
                assert!(
                    !candidate_support_profiles()
                        .iter()
                        .any(|candidate| candidate.id == row.id)
                );
                assert!(
                    !ACCEPTED_COMPLETE_SUPPORT_PROFILES
                        .iter()
                        .any(|profile| profile.id == row.id)
                );
            }
        }
    }

    #[test]
    fn kind_bindings_reject_missing_framework_hosts_and_dialect_evidence() {
        assert!(
            validate_kind_bindings(
                SupportProfileKind::FrameworkProjection,
                None,
                None,
                SupportProfileId::new("framework"),
            )
            .is_err()
        );
        assert!(
            validate_kind_bindings(
                SupportProfileKind::Dialect,
                Some(SupportProfileId::new("language")),
                None,
                SupportProfileId::new("dialect"),
            )
            .is_err()
        );
    }

    #[test]
    fn complete_profile_rejects_pending_and_unadmitted_not_applicable() {
        let pending = CompleteSupportProfile {
            id: SupportProfileId::new("test-language"),
            kind: SupportProfileKind::Language,
            host: None,
            dialect_evidence: None,
            evidence: [ProfileEvidence::Pending; 13],
            independent_review: "independent-review",
        };
        assert!(validate_complete_profile(&pending).is_err());

        let reference = EvidenceReference {
            registry: "registry-v1",
            parser: "parser-v1",
            provider: "provider-v1",
            relation: "relation-v1",
            publication: "publication-v1",
            navigation: "navigation-v1",
            artifact: "fixture",
        };
        let mut evidence = [ProfileEvidence::Passed(reference); 13];
        evidence[1] = ProfileEvidence::NotApplicable(ReviewedNotApplicable {
            reason: NotApplicableReason::ProfileIsNotDialect,
            review: "independent-review",
        });
        let valid = CompleteSupportProfile {
            evidence,
            ..pending
        };
        assert!(validate_complete_profile(&valid).is_ok());

        let mut unbound_evidence = evidence;
        unbound_evidence[0] = ProfileEvidence::Passed(EvidenceReference {
            artifact: "",
            ..reference
        });
        assert!(
            validate_complete_profile(&CompleteSupportProfile {
                evidence: unbound_evidence,
                ..valid
            })
            .is_err()
        );

        let mut invalid_evidence = evidence;
        invalid_evidence[5] = ProfileEvidence::NotApplicable(ReviewedNotApplicable {
            reason: NotApplicableReason::NoRelationSemantics,
            review: "independent-review",
        });
        assert!(
            validate_complete_profile(&CompleteSupportProfile {
                evidence: invalid_evidence,
                ..valid
            })
            .is_err()
        );
    }

    #[test]
    fn markdown_and_pages_share_catalog_identity_and_durable_links() -> Result<(), Box<dyn Error>> {
        let identity = support_catalog_identity();
        let markdown = crate::language::render_language_support_markdown()?;
        let html = render_language_support_html()?;
        for projection in [&markdown, &html] {
            require_test(
                projection.contains(&identity.digest),
                "catalog identity missing from a public projection",
            )?;
            require_test(
                projection.contains("System and component ownership"),
                "system architecture link missing from a public projection",
            )?;
            require_test(
                projection.contains("Transactional publication"),
                "publication architecture link missing from a public projection",
            )?;
            require_test(
                projection.contains("legacy-modernization"),
                "modernization disclaimer tag missing from a public projection",
            )?;
        }
        require_test(
            html.contains("Language &amp; Ecosystem Support"),
            "Pages projection title is missing",
        )?;
        require_test(
            markdown.contains("## Hardware design"),
            "hardware-design category is missing",
        )?;
        Ok(())
    }

    #[test]
    fn repository_and_pages_landing_links_are_declared() {
        let readme = include_str!("../../../README.md");
        let workflow = include_str!("../../../.github/workflows/04-docs.yml");
        assert!(readme.contains("https://styler-ai.github.io/ProjectAtlas/language-support/"));
        assert!(workflow.contains("href=\"language-support/\""));
        assert!(workflow.contains("--html target/doc/language-support/index.html"));
    }

    /// Return a test error without panicking inside a result-returning test.
    fn require_test(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }
}
