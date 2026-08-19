//! Validated accepted-capability manifest for the optional native parser pack.

use crate::language::{
    ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION, BROAD_PARSER_PACK_ID,
    LANGUAGE_CAPABILITY_REGISTRY_VERSION, OPTIONAL_GRAMMAR_CATALOG,
    OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION, OPTIONAL_GRAMMAR_CATALOG_VERSION,
    OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS, accepted_language_capability_digest,
    language_capability, language_registry_digest,
};
use crate::optional_parser_protocol::{
    PARSER_WORKER_JOB_MEMORY_BYTES, PARSER_WORKER_PROCESS_MEMORY_BYTES,
};
use blake3::Hasher;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

/// Schema version of the logical optional parser-pack manifest.
pub const OPTIONAL_PARSER_PACK_MANIFEST_SCHEMA_VERSION: u32 = 2;
/// Compatibility version of the accepted optional parser capabilities.
pub const OPTIONAL_PARSER_PACK_CAPABILITY_SET_VERSION: u32 = 2;
/// Stable identity of the one logical optional native parser pack.
pub const OPTIONAL_PARSER_PACK_ID: &str = BROAD_PARSER_PACK_ID;
/// SHA-256 of the exactly pinned broad-grammar Cargo archive.
pub const OPTIONAL_GRAMMAR_CATALOG_CRATE_SHA256: &str =
    "44dc94ef7a5f7f4247d88d5acdd26d842c8fc6f5eaf491a970c8e3d8fc9c9287";
/// Full VCS revision embedded in the exactly pinned Cargo archive.
pub const OPTIONAL_GRAMMAR_CATALOG_CRATE_REVISION: &str =
    "ce9e9c0974731d25b4b9426711a62d544d993368";
/// Cargo archive path recorded by its embedded VCS metadata.
pub const OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS: &str = "crates/ts-pack-core";
/// Exact upstream tag that owns the parser-source and native release assets.
pub const OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG: &str = "v1.13.2";
/// SHA-256 of the exactly pinned broad-grammar parser-source bundle.
pub const OPTIONAL_GRAMMAR_CATALOG_SOURCE_BUNDLE_SHA256: &str =
    "d684799dc664553c9c746d5fe676a5b599f9efcec4cad5450bec7ec5a29574a9";
/// Exact `ProjectAtlas` release line selected to consume the first pack.
pub const OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION: &str = "0.4.5-rc3";
/// Exact Tree-sitter runtime selected by the consuming parser worker.
pub const OPTIONAL_PARSER_PACK_TREE_SITTER_VERSION: &str = "0.26.9";
/// Oldest grammar ABI accepted by the selected Tree-sitter runtime.
pub const OPTIONAL_PARSER_PACK_MINIMUM_ABI: u32 = 13;
/// Newest grammar ABI accepted by the selected Tree-sitter runtime.
pub const OPTIONAL_PARSER_PACK_MAXIMUM_ABI: u32 = 15;
/// Largest accepted serialized logical manifest.
pub const OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES: usize = 32 * 1024 * 1024;
/// Schema version of one immutable platform artifact manifest.
pub const OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION: u32 = 2;
/// Schema version of the normalized native-audit report inside each platform artifact.
pub const OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION: u32 = 3;
/// Schema version of the immutable native-import policy packaged with each artifact.
pub const OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_SCHEMA_VERSION: u32 = 3;
/// Exact ELF interpreter admitted for the Linux x86-64 parser worker.
pub const OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME: &str = "ld-linux-x86-64.so.2";
/// External runtime family required by the artifact-bound Windows containment broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY: &str = "windows-net-framework-clr-v4";
/// Native entry point of the managed Windows containment broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT: &str = "0x0000000000000000";
/// Exact size of the CLR 2.0 runtime header carried by the managed broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE: u32 = 72;
/// Complete ordinary and delay-loaded PE dependency set for the managed broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES: &[&str] = &[];
/// Complete managed P/Invoke module set for the shipped Windows containment broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES: &[&str] =
    &["advapi32.dll", "kernel32.dll", "userenv.dll"];
/// Schema version of one fresh-runner platform proof.
pub const OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION: u32 = 2;
/// Schema version of the exact supported-platform aggregate proof.
pub const OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION: u32 = 2;
/// Deliberately reduced Linux ceiling used by exact-artifact release verification.
pub const OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES: u64 = 1024 * 1024;
/// Smallest committed-memory ceiling accepted by the shipped Windows containment broker.
pub const OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES: u64 = 16 * 1024 * 1024;
/// Compressed-byte ceiling for one platform pack archive.
pub const OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
/// Expanded-byte ceiling for one platform pack artifact.
pub const OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
/// Byte ceiling for one payload file.
pub const OPTIONAL_PARSER_PACK_MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
/// Byte ceiling for the immutable native-import policy consumed before containment.
pub const OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_MAX_BYTES: u64 = 1024 * 1024;
/// UTF-8 byte ceiling for one artifact-relative path.
pub const OPTIONAL_PARSER_PACK_MAX_PATH_BYTES: usize = 256;
/// File-entry ceiling for one platform pack artifact.
pub const OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES: usize = 512;

/// Absolute ceiling for accepted grammar rows in one logical pack.
const MAX_ACCEPTED_GRAMMARS: usize = 512;
/// Absolute ceiling for deduplicated exact license records.
const MAX_LICENSE_RECORDS: usize = 1_024;
/// Largest bounded identity or provenance field.
const MAX_IDENTITY_BYTES: usize = 4_096;
/// Largest exact license text retained in the logical manifest.
const MAX_LICENSE_TEXT_BYTES: usize = 256 * 1024;
/// Largest exact UTF-8 fixture source retained per polarity.
const MAX_FIXTURE_SOURCE_BYTES: usize = 64 * 1024;
/// Fixed non-grammar payload files common to every platform artifact.
const OPTIONAL_PARSER_PACK_COMMON_PAYLOAD_FILES: usize = 6;
/// Domain separator for one accepted grammar capability digest.
const CAPABILITY_DIGEST_DOMAIN: &str = "projectatlas.optional-parser-capability.v1";
/// Domain separator for the complete logical-manifest digest.
const MANIFEST_DIGEST_DOMAIN: &str = "projectatlas.optional-parser-pack-manifest.v2";

/// Failure while parsing or validating an optional parser-pack manifest.
#[derive(Debug, Error)]
pub enum OptionalParserPackManifestError {
    /// Serialized input exceeded the bounded logical-manifest limit.
    #[error("optional parser-pack manifest is {actual} bytes; maximum is {maximum}")]
    ManifestTooLarge {
        /// Observed serialized bytes.
        actual: usize,
        /// Accepted serialized-byte ceiling.
        maximum: usize,
    },
    /// JSON decoding failed before domain validation.
    #[error("invalid optional parser-pack manifest JSON")]
    InvalidJson {
        /// JSON parser failure.
        #[source]
        source: serde_json::Error,
    },
    /// One field violated its local representation contract.
    #[error("invalid {field} for {owner}: {reason}")]
    InvalidField {
        /// Record or manifest identity that owns the field.
        owner: String,
        /// Stable field name.
        field: &'static str,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// Manifest metadata does not match the selected source or registry authority.
    #[error("optional parser-pack binding {field} is {actual:?}; expected {expected:?}")]
    BindingMismatch {
        /// Mismatched binding field.
        field: &'static str,
        /// Required value.
        expected: String,
        /// Manifest value.
        actual: String,
    },
    /// A deterministic sequence was not strictly sorted and unique.
    #[error("optional parser-pack {field} must be strictly sorted and unique")]
    NotSortedUnique {
        /// Sequence that violated canonical ordering.
        field: &'static str,
    },
    /// Accepted grammar or license membership violated its hard bound.
    #[error("optional parser-pack {field} count {actual} is outside {minimum}..={maximum}")]
    CountOutOfBounds {
        /// Bounded collection.
        field: &'static str,
        /// Observed rows.
        actual: usize,
        /// Required minimum.
        minimum: usize,
        /// Absolute maximum.
        maximum: usize,
    },
    /// A grammar identity is not a canonical optional registry row.
    #[error("grammar {language_id:?} is not a canonical optional language capability")]
    UnknownOptionalLanguage {
        /// Rejected language identity.
        language_id: String,
    },
    /// An optional row attempted to overlap a default-core capability owner.
    #[error("grammar {language_id:?} overlaps a default-core language capability")]
    BuiltInOverlap {
        /// Default-core language identity.
        language_id: String,
    },
    /// A grammar references a missing license record.
    #[error("grammar {language_id:?} references unknown license record {license_id:?}")]
    UnknownLicense {
        /// Grammar language identity.
        language_id: String,
        /// Missing license record identity.
        license_id: String,
    },
    /// A referenced license did not come from the grammar's pinned source revision.
    #[error(
        "grammar {language_id:?} license {license_id:?} does not match its source repository and revision"
    )]
    LicenseSourceMismatch {
        /// Grammar language identity.
        language_id: String,
        /// Mismatched license record.
        license_id: String,
    },
    /// Two accepted grammars claimed one runtime-loading identity.
    #[error("optional parser-pack {field} {value:?} is owned by more than one grammar")]
    DuplicateRuntimeIdentity {
        /// Colliding identity class.
        field: &'static str,
        /// Colliding identity value.
        value: String,
    },
    /// A grammar ABI claim does not fit the consuming runtime.
    #[error("grammar {language_id:?} ABI claim is outside the consuming runtime window")]
    AbiMismatch {
        /// Grammar language identity.
        language_id: String,
    },
    /// Embedded content or a canonical capability projection was changed without its digest.
    #[error("{field} digest mismatch for {owner}")]
    DigestMismatch {
        /// Record whose content drifted.
        owner: String,
        /// Digest-bearing field.
        field: &'static str,
    },
}

/// Validated lowercase SHA-256 digest used for fetched or compiled artifacts.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Validate a lowercase 64-character SHA-256 hexadecimal digest.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not canonical lowercase hexadecimal.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        validate_hex_digest(&value, "sha256")?;
        Ok(Self(value))
    }

    /// Borrow the canonical hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validated lowercase BLAKE3 digest used for embedded and canonical content.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Blake3Digest(String);

impl Blake3Digest {
    /// Validate a lowercase 64-character BLAKE3 hexadecimal digest.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not canonical lowercase hexadecimal.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        validate_hex_digest(&value, "blake3")?;
        Ok(Self(value))
    }

    /// Hash exact bytes through the manifest's canonical content algorithm.
    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    /// Borrow the canonical hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Blake3Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Blake3Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Exact full Git revision used for source and license provenance.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SourceRevision(String);

impl SourceRevision {
    /// Validate a full lowercase 40-character Git revision.
    ///
    /// # Errors
    ///
    /// Returns an error for abbreviated or non-canonical revisions.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        if value.len() != 40
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_field(
                "source revision",
                "revision",
                "expected 40 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the full revision.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SourceRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for SourceRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Safe C-compatible grammar export symbol.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GrammarExportSymbol(String);

impl GrammarExportSymbol {
    /// Validate a C identifier without any path or loader syntax.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, oversized, or non-identifier value.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let valid_first = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if value.len() > 256
            || !valid_first
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(invalid_field(
                &value,
                "export_symbol",
                "expected a C identifier of at most 256 ASCII bytes",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the validated export symbol.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GrammarExportSymbol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Safe platform-neutral grammar library stem.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GrammarLibraryStem(String);

impl GrammarLibraryStem {
    /// Validate a lowercase basename without directory or extension syntax.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe loader or path characters.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 256
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
            || !value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
        {
            return Err(invalid_field(
                &value,
                "library_stem",
                "expected a lowercase ASCII basename of at most 256 bytes",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the validated library stem.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GrammarLibraryStem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Native target with an accepted optional parser-pack artifact contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum PackPlatform {
    /// Linux x86-64 GNU target.
    #[serde(rename = "x86_64-unknown-linux-gnu")]
    LinuxX86_64,
    /// Windows x86-64 MSVC target.
    #[serde(rename = "x86_64-pc-windows-msvc")]
    WindowsX86_64,
}

impl PackPlatform {
    /// Complete optional-pack artifact target set in canonical order.
    pub const ALL: &'static [Self] = &[Self::LinuxX86_64, Self::WindowsX86_64];

    /// Return the canonical Rust target triple used by manifest digests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
        }
    }

    /// Return the executable name shipped at the pack root.
    #[must_use]
    pub const fn worker_file_name(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "projectatlas-parser-worker.exe",
            Self::LinuxX86_64 => "projectatlas-parser-worker",
        }
    }

    /// Return the artifact-bound runtime-containment broker when the platform requires one.
    #[must_use]
    pub const fn containment_broker_file_name(self) -> Option<&'static str> {
        match self {
            Self::LinuxX86_64 => None,
            Self::WindowsX86_64 => Some("projectatlas-parser-containment.exe"),
        }
    }

    /// Return the platform-native filename for a validated grammar-library stem.
    #[must_use]
    pub fn grammar_library_file_name(self, stem: &GrammarLibraryStem) -> String {
        match self {
            Self::LinuxX86_64 => format!("lib{}.so", stem.as_str()),
            Self::WindowsX86_64 => format!("{}.dll", stem.as_str()),
        }
    }
}

/// Canonical artifact-relative UTF-8 path with no traversal or platform syntax.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackRelativePath(String);

impl PackRelativePath {
    /// Validate one slash-separated artifact-relative path.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, absolute, traversing, non-ASCII, or oversized paths.
    pub fn new(value: impl Into<String>) -> Result<Self, OptionalParserPackManifestError> {
        let value = value.into();
        let valid_bytes = value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
        let valid_components = !value.is_empty()
            && !value.starts_with('/')
            && !value.ends_with('/')
            && value
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != "..");
        if value.len() > OPTIONAL_PARSER_PACK_MAX_PATH_BYTES || !valid_bytes || !valid_components {
            return Err(invalid_field(
                &value,
                "relative_path",
                "expected a safe slash-separated ASCII path within the pack path bound",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the canonical slash-separated relative path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Candidate source-state classification bound into release-only pack evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserPackCandidateSourceState {
    /// Candidate commit and tracked worktree are exact and clean.
    Clean,
    /// Local development artifact was produced from uncommitted source.
    Dirty,
}

/// Exact source and toolchain identity used to build a parser-pack worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackCandidateIdentity {
    /// Exact `ProjectAtlas` source revision.
    pub projectatlas_revision: SourceRevision,
    /// Cargo package version compiled into the worker.
    pub cargo_package_version: String,
    /// Intended `ProjectAtlas` release line for the capability manifest.
    pub intended_release_version: String,
    /// SHA-256 of the exact workspace lockfile.
    pub cargo_lock_sha256: Sha256Digest,
    /// Rust compiler release version without host-specific prose.
    pub rustc_release: String,
    /// Rust compiler commit hash.
    pub rustc_commit_hash: String,
    /// Whether tracked candidate source was clean when constructed.
    pub source_state: ParserPackCandidateSourceState,
}

/// Closed physical egress-denial mechanism for artifact construction and verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserPackNetworkIsolation {
    /// Linux network namespace or container with no network device route.
    LinuxNetworkNamespace,
    /// Windows Firewall denial scoped to a disposable construction principal.
    WindowsPrincipalFirewall,
    /// Windows zero-capability `AppContainer` used by fresh verification and runtime containment.
    WindowsAppContainer,
}

impl ParserPackNetworkIsolation {
    /// Return the accepted offline-construction mechanism for a required target.
    const fn for_construction(platform: PackPlatform) -> Self {
        match platform {
            PackPlatform::LinuxX86_64 => Self::LinuxNetworkNamespace,
            PackPlatform::WindowsX86_64 => Self::WindowsPrincipalFirewall,
        }
    }

    /// Return the accepted fresh-verification mechanism for a required target.
    const fn for_fresh_runner(platform: PackPlatform) -> Self {
        match platform {
            PackPlatform::LinuxX86_64 => Self::LinuxNetworkNamespace,
            PackPlatform::WindowsX86_64 => Self::WindowsAppContainer,
        }
    }
}

/// Required three-path egress canary result under physical network denial.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackNetworkDenial {
    /// Platform-specific physical containment mechanism.
    pub mechanism: ParserPackNetworkIsolation,
    /// DNS resolution or query attempt was denied.
    pub dns_denied: bool,
    /// Direct TCP connection attempt was denied.
    pub direct_tcp_denied: bool,
    /// HTTPS connection attempt was denied.
    pub https_denied: bool,
}

/// Typed success state for one required construction or fresh-runner control.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserPackVerifiedControl {
    /// The owning workflow enforced and verified the control.
    Verified,
}

/// Offline-construction controls recorded with an immutable platform artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackOfflineConstruction {
    /// Cargo ran in frozen mode against the exact lockfile.
    pub cargo_frozen: ParserPackVerifiedControl,
    /// Cargo ran in offline mode after the bounded acquisition stage.
    pub cargo_offline: ParserPackVerifiedControl,
    /// The grammar dependency's own offline mode was forced.
    pub dependency_offline: ParserPackVerifiedControl,
    /// The `ProjectAtlas` worker embedded no grammar libraries.
    pub zero_embedded_grammars: ParserPackVerifiedControl,
    /// The dependency's broad language-selection variable was absent.
    pub language_selector_absent: ParserPackVerifiedControl,
    /// The dependency's failed-grammar override was absent.
    pub failed_grammar_override_absent: ParserPackVerifiedControl,
    /// Physical egress denial and canary outcome.
    pub network_denial: ParserPackNetworkDenial,
}

/// Exact pinned upstream native asset consumed by one platform realization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackSourceAsset {
    /// Exact release tag that owns the asset.
    pub release_tag: String,
    /// Exact release revision that owns the asset.
    pub release_revision: SourceRevision,
    /// Safe release-asset basename.
    pub name: String,
    /// SHA-256 of the complete downloaded asset.
    pub sha256: Sha256Digest,
    /// Exact compressed asset bytes.
    pub bytes: u64,
    /// SHA-256 of the upstream asset-inventory manifest.
    pub parsers_manifest_sha256: Sha256Digest,
}

/// Closed payload-file role in one platform artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum ParserPackPayloadRole {
    /// Grammar-affined `ProjectAtlas` parser worker.
    Worker,
    /// Artifact-bound platform admission and containment broker.
    ContainmentBroker,
    /// Byte-identical accepted logical capability manifest.
    AcceptedManifest,
    /// Byte-identical retained positive/negative fixture corpus.
    FixtureCorpus,
    /// `ProjectAtlas` distribution license.
    ProjectLicense,
    /// Closed native import/export/dependency policy.
    NativeImportPolicy,
    /// Normalized per-library native audit evidence.
    NativeAuditReport,
    /// One exact accepted native grammar library.
    GrammarLibrary {
        /// Canonical accepted language identity.
        language_id: String,
    },
}

/// One exact payload file covered by an artifact manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackPayloadFile {
    /// Canonical artifact-relative path.
    pub path: PackRelativePath,
    /// Closed payload responsibility.
    pub role: ParserPackPayloadRole,
    /// Exact payload bytes.
    pub bytes: u64,
    /// SHA-256 of the exact payload bytes.
    pub sha256: Sha256Digest,
}

/// Derived bounded measurements for artifact payload files.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackPayloadMeasurements {
    /// Number of manifest-listed payload files.
    pub files: u32,
    /// Number of accepted native grammar libraries.
    pub grammar_libraries: u32,
    /// Sum of manifest-listed payload bytes, excluding the manifest itself.
    pub payload_bytes: u64,
    /// Largest manifest-listed payload file.
    pub largest_file_bytes: u64,
    /// Longest canonical relative path in UTF-8 bytes.
    pub longest_path_bytes: u32,
}

/// Closed native binary-audit summary for one platform realization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackNativeAudit {
    /// SHA-256 of the closed checked-in native audit policy.
    pub policy_sha256: Sha256Digest,
    /// SHA-256 of the normalized per-library audit report.
    pub report_sha256: Sha256Digest,
    /// Number of accepted grammar libraries audited.
    pub audited_libraries: u32,
    /// Forbidden imported symbols found across accepted libraries.
    pub forbidden_imports: u32,
    /// Dependencies outside the platform allowlist.
    pub unexpected_dependencies: u32,
    /// Required grammar constructors missing from accepted libraries.
    pub missing_exports: u32,
    /// Unexpected Tree-sitter constructor/helper exports.
    pub unexpected_exports: u32,
}

/// Immutable payload and construction manifest for one platform pack artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackArtifactManifest {
    /// Artifact-manifest schema version.
    pub schema_version: u32,
    /// Stable logical pack identity.
    pub pack_id: String,
    /// Intended `ProjectAtlas` release line.
    pub projectatlas_version: String,
    /// Required native target.
    pub platform: PackPlatform,
    /// Exact source/toolchain identity for the packaged worker.
    pub candidate: ParserPackCandidateIdentity,
    /// SHA-256 of the exact accepted logical manifest bytes.
    pub accepted_manifest_sha256: Sha256Digest,
    /// Logical accepted capability digest.
    pub capability_set_digest: Blake3Digest,
    /// SHA-256 of the exact retained fixture corpus bytes.
    pub fixture_corpus_sha256: Sha256Digest,
    /// Pinned upstream native asset identity.
    pub source_asset: ParserPackSourceAsset,
    /// Network-disabled and dependency-offline construction state.
    pub construction: ParserPackOfflineConstruction,
    /// Closed native audit result.
    pub native_audit: ParserPackNativeAudit,
    /// Derived payload measurements.
    pub measurements: ParserPackPayloadMeasurements,
    /// Strictly path-sorted payload inventory, excluding this manifest.
    pub files: Vec<ParserPackPayloadFile>,
}

/// Fresh-runner isolation state recorded after extracting a completed artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackFreshRunner {
    /// Verification ran in a newly allocated host job or machine image.
    pub fresh_host: ParserPackVerifiedControl,
    /// No repository source or build output was available to the verifier.
    pub repository_inputs_absent: ParserPackVerifiedControl,
    /// Verification invoked neither Cargo nor a compiler.
    pub build_tools_not_invoked: ParserPackVerifiedControl,
    /// Verification current directory was outside the extracted pack.
    pub working_directory_outside_pack: ParserPackVerifiedControl,
    /// Ambient dynamic-library search paths were cleared.
    pub ambient_library_paths_cleared: ParserPackVerifiedControl,
    /// Physical egress denial and canary outcome during packaged loading.
    pub network_denial: ParserPackNetworkDenial,
}

impl ParserPackFreshRunner {
    /// Validate clean-runner and physical-isolation controls for one accepted target.
    ///
    /// # Errors
    ///
    /// Returns the first missing, mismatched, or unverified runner control.
    pub fn validate(&self, platform: PackPlatform) -> Result<(), OptionalParserPackManifestError> {
        validate_fresh_runner(self, platform)
    }
}

/// One accepted grammar's packaged worker probe on a fresh runner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackGrammarProbe {
    /// Canonical accepted language identity supplied to the worker.
    pub language_id: String,
    /// Worker loaded the manifest-approved library, matched ABI, and proved both fixtures.
    pub worker_probe_passed: bool,
}

/// Platform memory-control path exercised by the exact packaged worker probe.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserPackMemoryControl {
    /// A delegated Linux cgroup-v2 `memory.max` ceiling enforced the limit.
    LinuxCgroupV2,
    /// The Linux supervisor sampled `/proc/<pid>/status` and killed the process group.
    LinuxProcStatus,
    /// A Windows no-breakaway Job Object enforced committed-memory ceilings.
    WindowsJobObject,
}

/// Hosted memory-limit and process-cleanup proof for one exact packaged worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPackMemoryProbe {
    /// Platform control that observed or enforced the deliberately reduced probe limit.
    pub control: ParserPackMemoryControl,
    /// Deliberately reduced per-process ceiling used only by the hosted probe.
    pub process_limit_bytes: u64,
    /// Deliberately reduced process-tree or Job ceiling used by the hosted probe.
    pub process_tree_limit_bytes: u64,
    /// Declared maximum sampling interval for the `/proc` fallback, when applicable.
    pub observation_interval_millis: Option<u64>,
    /// Highest sampled resident bytes at the first confirmed `/proc` breach.
    pub peak_observed_bytes: Option<u64>,
    /// Hosted-measured maximum bytes above the sampled `/proc` ceiling.
    pub maximum_observed_overshoot_bytes: Option<u64>,
    /// The configured limit terminated or rejected the exact worker process tree.
    pub limit_enforced: ParserPackVerifiedControl,
    /// The supervisor or broker confirmed bounded worker/process-tree cleanup.
    pub process_tree_cleaned: ParserPackVerifiedControl,
}

/// Fresh-runner receipt for one exact completed platform archive.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackPlatformProof {
    /// Platform-proof schema version.
    pub schema_version: u32,
    /// Stable logical pack identity.
    pub pack_id: String,
    /// Required native target.
    pub platform: PackPlatform,
    /// Exact candidate identity repeated for cross-platform comparison.
    pub candidate: ParserPackCandidateIdentity,
    /// Safe completed archive basename.
    pub archive_name: String,
    /// SHA-256 of the complete archive, including its artifact manifest.
    pub archive_sha256: Sha256Digest,
    /// Exact completed archive bytes.
    pub archive_bytes: u64,
    /// Expanded bytes including the artifact manifest.
    pub expanded_bytes: u64,
    /// SHA-256 of the immutable artifact manifest inside the archive.
    pub artifact_manifest_sha256: Sha256Digest,
    /// SHA-256 of the byte-identical accepted logical manifest.
    pub accepted_manifest_sha256: Sha256Digest,
    /// Byte-identical logical accepted capability digest.
    pub capability_set_digest: Blake3Digest,
    /// SHA-256 of the byte-identical retained fixture corpus.
    pub fixture_corpus_sha256: Sha256Digest,
    /// SHA-256 of the normalized native audit report bound by the artifact manifest.
    pub native_audit_report_sha256: Sha256Digest,
    /// Clean-machine and physical-isolation state.
    pub runner: ParserPackFreshRunner,
    /// Strictly language-sorted accepted grammar worker probes.
    pub grammars: Vec<ParserPackGrammarProbe>,
    /// Exact-host memory-boundary and cleanup probe.
    pub memory: ParserPackMemoryProbe,
}

/// Exact supported-platform optional parser-pack proof aggregate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackProofAggregate {
    /// Aggregate schema version.
    pub schema_version: u32,
    /// Stable logical pack identity.
    pub pack_id: String,
    /// Intended `ProjectAtlas` release line.
    pub projectatlas_version: String,
    /// SHA-256 of the shared accepted logical manifest bytes.
    pub accepted_manifest_sha256: Sha256Digest,
    /// Shared logical accepted capability digest.
    pub capability_set_digest: Blake3Digest,
    /// SHA-256 of the shared retained fixture corpus bytes.
    pub fixture_corpus_sha256: Sha256Digest,
    /// Required platform receipts in canonical platform order.
    pub platforms: Vec<OptionalParserPackPlatformProof>,
}

/// Closed `ProjectAtlas` consumer selected for native optional grammars.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserPackConsumer {
    /// Separately packaged grammar-affined `ProjectAtlas` parser worker.
    #[serde(rename = "projectatlas-parser-worker")]
    ProjectAtlasParserWorker,
}

impl ParserPackConsumer {
    /// Return the stable consuming executable name.
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::ProjectAtlasParserWorker => "projectatlas-parser-worker",
        }
    }
}

/// Required behavior when an optional grammar overlaps default-core ownership.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltInParserPrecedence {
    /// The default-core owner is authoritative and overlap is rejected.
    BuiltInAuthoritative,
}

impl BuiltInParserPrecedence {
    /// Return the stable precedence-policy name.
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::BuiltInAuthoritative => "built-in-authoritative",
        }
    }
}

/// Exact broad-grammar source package selected for the logical pack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackSource {
    /// Cargo source package.
    pub package: String,
    /// Exact published package version.
    pub version: String,
    /// Exact published Cargo archive identity.
    pub cargo_archive: OptionalParserCargoArchive,
    /// Exact upstream release identity that owns native assets.
    pub native_release: OptionalParserNativeRelease,
}

/// Published Cargo archive provenance for the consuming API/runtime package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserCargoArchive {
    /// SHA-256 of the published crate archive.
    pub sha256: Sha256Digest,
    /// Full VCS revision embedded in `.cargo_vcs_info.json`.
    pub vcs_revision: SourceRevision,
    /// Monorepo-relative crate path embedded in `.cargo_vcs_info.json`.
    pub path_in_vcs: String,
}

/// Upstream release provenance for parser-source and native assets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserNativeRelease {
    /// Exact version tag that owns the release assets.
    pub tag: String,
    /// Full Git revision named by the release tag.
    pub revision: SourceRevision,
    /// SHA-256 of the pinned parser-source bundle.
    pub source_bundle_sha256: Sha256Digest,
}

/// Exact `ProjectAtlas` consumer and ABI window for the logical pack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackRuntime {
    /// Closed consuming executable identity.
    pub consumer: ParserPackConsumer,
    /// Exact `ProjectAtlas` runtime version used to consume the manifest.
    pub projectatlas_version: String,
    /// Exact Tree-sitter runtime package version.
    pub tree_sitter_version: String,
    /// Oldest accepted Tree-sitter grammar ABI.
    pub minimum_abi: u32,
    /// Newest accepted Tree-sitter grammar ABI.
    pub maximum_abi: u32,
}

/// Exact binding to the language registry that admitted optional candidates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalParserPackRegistryBinding {
    /// Language capability registry schema version.
    pub registry_version: u32,
    /// Digest of achieved registry truth and detector rules.
    pub registry_digest: Blake3Digest,
    /// Accepted language capability-set version.
    pub accepted_set_version: u32,
    /// Digest of accepted language minimums.
    pub accepted_set_digest: Blake3Digest,
}

impl OptionalParserPackRegistryBinding {
    /// Capture the current authoritative language-registry identity.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal registry digest is not canonical.
    pub fn current() -> Result<Self, OptionalParserPackManifestError> {
        Ok(Self {
            registry_version: LANGUAGE_CAPABILITY_REGISTRY_VERSION,
            registry_digest: Blake3Digest::new(language_registry_digest())?,
            accepted_set_version: ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION,
            accepted_set_digest: Blake3Digest::new(accepted_language_capability_digest())?,
        })
    }
}

/// Pinned source subtree and deterministic compile-input identity for one grammar.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarSourceProvenance {
    /// HTTPS source repository.
    pub repository_url: String,
    /// Full pinned repository revision.
    pub revision: SourceRevision,
    /// Repository-relative grammar subtree, or `.` for repository root.
    pub subdirectory: String,
    /// SHA-256 of every admitted grammar compile input in canonical order.
    pub compile_input_sha256: Sha256Digest,
}

/// Exact applicable license text retained once and referenced by grammar rows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarLicense {
    /// Stable manifest-local license record identity.
    pub id: String,
    /// HTTPS repository containing the exact text.
    pub repository_url: String,
    /// Repository-relative license source path.
    pub source_path: String,
    /// Full repository revision at which the text was read.
    pub revision: SourceRevision,
    /// Exact applicable license text.
    pub text: String,
    /// BLAKE3 of the exact UTF-8 license text.
    pub text_blake3: Blake3Digest,
    /// Optional declarative SPDX expression; exact text remains authoritative.
    pub spdx_expression: Option<String>,
}

impl GrammarLicense {
    /// Construct a license record and bind its exact text digest.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        repository_url: impl Into<String>,
        source_path: impl Into<String>,
        revision: SourceRevision,
        text: impl Into<String>,
        spdx_expression: Option<String>,
    ) -> Self {
        let text = text.into();
        Self {
            id: id.into(),
            repository_url: repository_url.into(),
            source_path: source_path.into(),
            revision,
            text_blake3: Blake3Digest::for_bytes(text.as_bytes()),
            text,
            spdx_expression,
        }
    }
}

/// ABI and loader identities for one compiled grammar.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarAbiExport {
    /// Oldest ABI accepted for this grammar.
    pub minimum_abi: u32,
    /// Newest ABI accepted for this grammar.
    pub maximum_abi: u32,
    /// ABI reported by the compiled grammar.
    pub expected_abi: u32,
    /// Exact exported language function.
    pub export_symbol: GrammarExportSymbol,
    /// Platform-neutral dynamic-library stem.
    pub library_stem: GrammarLibraryStem,
}

/// Closed provenance and transformation classes for accepted fixtures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GrammarFixtureOrigin {
    /// Natural case from an upstream Tree-sitter corpus.
    #[serde(rename = "upstream-tree-sitter-corpus")]
    UpstreamTreeSitterCorpus,
    /// Natural example from an upstream language repository.
    #[serde(rename = "upstream-language-example")]
    UpstreamLanguageExample,
    /// Malformed or rejected case authored in an upstream corpus.
    #[serde(rename = "upstream-corpus-error-case")]
    UpstreamCorpusErrorCase,
    /// Incomplete editor-state case derived from an upstream corpus case.
    #[serde(rename = "projectatlas-incomplete-upstream-case")]
    ProjectAtlasIncompleteUpstreamCase,
    /// Incomplete editor-state case derived from an upstream language example.
    #[serde(rename = "projectatlas-incomplete-upstream-example")]
    ProjectAtlasIncompleteUpstreamExample,
}

impl GrammarFixtureOrigin {
    /// Return the stable serialized fixture-origin identity.
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::UpstreamTreeSitterCorpus => "upstream-tree-sitter-corpus",
            Self::UpstreamLanguageExample => "upstream-language-example",
            Self::UpstreamCorpusErrorCase => "upstream-corpus-error-case",
            Self::ProjectAtlasIncompleteUpstreamCase => "projectatlas-incomplete-upstream-case",
            Self::ProjectAtlasIncompleteUpstreamExample => {
                "projectatlas-incomplete-upstream-example"
            }
        }
    }

    /// Return whether the origin is valid for a natural positive case.
    const fn is_positive(self) -> bool {
        matches!(
            self,
            Self::UpstreamTreeSitterCorpus | Self::UpstreamLanguageExample
        )
    }

    /// Return whether the origin is valid for a non-vacuous negative case.
    const fn is_negative(self) -> bool {
        matches!(
            self,
            Self::UpstreamCorpusErrorCase
                | Self::ProjectAtlasIncompleteUpstreamCase
                | Self::ProjectAtlasIncompleteUpstreamExample
        )
    }
}

/// Exact source fixture with retained origin, case, path, and digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarFixture {
    /// Stable fixture provenance or transformation class.
    pub origin: GrammarFixtureOrigin,
    /// Exact upstream repository-relative fixture path.
    pub path: String,
    /// Exact upstream case name or stable example label.
    pub case_name: String,
    /// Exact natural source bytes represented as UTF-8.
    pub source: String,
    /// BLAKE3 of the exact fixture source.
    pub source_blake3: Blake3Digest,
}

impl GrammarFixture {
    /// Construct a fixture and bind its provenance plus exact source digest.
    #[must_use]
    pub fn new(
        origin: GrammarFixtureOrigin,
        path: impl Into<String>,
        case_name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let source = source.into();
        Self {
            origin,
            path: path.into(),
            case_name: case_name.into(),
            source_blake3: Blake3Digest::for_bytes(source.as_bytes()),
            source,
        }
    }
}

/// Natural positive and non-vacuous negative grammar fixtures.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarFixtures {
    /// Source expected to load and parse through the selected grammar.
    pub positive: GrammarFixture,
    /// Distinct source that protects rejection, error, or non-match behavior.
    pub negative: GrammarFixture,
}

/// One accepted non-built-in grammar capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedGrammar {
    /// Canonical language-registry identity.
    pub language_id: String,
    /// Exact grammar source and compile inputs.
    pub source: GrammarSourceProvenance,
    /// Sorted non-empty applicable license record identities.
    pub license_record_ids: Vec<String>,
    /// ABI, export, and library identity.
    pub abi_export: GrammarAbiExport,
    /// Exact positive and negative fixtures.
    pub fixtures: GrammarFixtures,
    /// Required platforms on which this row must be realized.
    pub required_platforms: Vec<PackPlatform>,
    /// Default-core overlap policy.
    pub built_in_precedence: BuiltInParserPrecedence,
    /// BLAKE3 of this canonical grammar capability row.
    pub capability_digest: Blake3Digest,
}

impl AcceptedGrammar {
    /// Construct one row with required platform and precedence policy and seal its digest.
    #[must_use]
    pub fn new(
        language_id: impl Into<String>,
        source: GrammarSourceProvenance,
        license_record_ids: Vec<String>,
        abi_export: GrammarAbiExport,
        fixtures: GrammarFixtures,
    ) -> Self {
        let mut grammar = Self {
            language_id: language_id.into(),
            source,
            license_record_ids,
            abi_export,
            fixtures,
            required_platforms: PackPlatform::ALL.to_vec(),
            built_in_precedence: BuiltInParserPrecedence::BuiltInAuthoritative,
            capability_digest: Blake3Digest::for_bytes(&[]),
        };
        grammar.capability_digest = grammar.computed_capability_digest();
        grammar
    }

    /// Compute the canonical capability digest from every row-owned field.
    #[must_use]
    pub fn computed_capability_digest(&self) -> Blake3Digest {
        let mut hasher = Hasher::new();
        hash_value(&mut hasher, CAPABILITY_DIGEST_DOMAIN);
        hash_value(&mut hasher, &self.language_id);
        hash_source_provenance(&mut hasher, &self.source);
        for license_id in &self.license_record_ids {
            hash_value(&mut hasher, license_id);
        }
        hasher.update(&self.abi_export.minimum_abi.to_le_bytes());
        hasher.update(&self.abi_export.maximum_abi.to_le_bytes());
        hasher.update(&self.abi_export.expected_abi.to_le_bytes());
        hash_value(&mut hasher, self.abi_export.export_symbol.as_str());
        hash_value(&mut hasher, self.abi_export.library_stem.as_str());
        hash_fixture(&mut hasher, &self.fixtures.positive);
        hash_fixture(&mut hasher, &self.fixtures.negative);
        for platform in &self.required_platforms {
            hash_value(&mut hasher, platform.as_str());
        }
        hash_value(&mut hasher, self.built_in_precedence.canonical_name());
        Blake3Digest(hasher.finalize().to_hex().to_string())
    }
}

/// One validated logical optional parser-pack accepted-capability manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OptionalParserPackManifest {
    /// Logical manifest schema version.
    schema_version: u32,
    /// Stable single-pack identity.
    pack_id: String,
    /// Accepted capability-set compatibility version.
    capability_set_version: u32,
    /// Exact broad-grammar source package.
    source: OptionalParserPackSource,
    /// Exact `ProjectAtlas` consuming runtime.
    runtime: OptionalParserPackRuntime,
    /// Language registry authority that admitted these rows.
    registry: OptionalParserPackRegistryBinding,
    /// Complete required platform set.
    required_platforms: Vec<PackPlatform>,
    /// Sorted unique exact license inventory.
    licenses: Vec<GrammarLicense>,
    /// Sorted unique accepted non-built-in grammar rows.
    grammars: Vec<AcceptedGrammar>,
    /// BLAKE3 of the complete canonical logical manifest.
    capability_set_digest: Blake3Digest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// Raw serde projection validated before it becomes a public manifest.
struct OptionalParserPackManifestWire {
    /// Logical schema version.
    schema_version: u32,
    /// Stable pack identity.
    pack_id: String,
    /// Accepted capability-set version.
    capability_set_version: u32,
    /// Exact broad-grammar source package.
    source: OptionalParserPackSource,
    /// Exact consuming runtime.
    runtime: OptionalParserPackRuntime,
    /// Current language-registry binding.
    registry: OptionalParserPackRegistryBinding,
    /// Complete required platform set.
    required_platforms: Vec<PackPlatform>,
    /// Deduplicated exact license records.
    licenses: Vec<GrammarLicense>,
    /// Accepted non-built-in grammar rows.
    grammars: Vec<AcceptedGrammar>,
    /// Complete logical-manifest digest.
    capability_set_digest: Blake3Digest,
}

impl OptionalParserPackManifest {
    /// Construct and seal one logical manifest from sorted accepted records.
    ///
    /// # Errors
    ///
    /// Returns an error when the records violate any accepted-capability invariant.
    pub fn new(
        source: OptionalParserPackSource,
        runtime: OptionalParserPackRuntime,
        licenses: Vec<GrammarLicense>,
        mut grammars: Vec<AcceptedGrammar>,
    ) -> Result<Self, OptionalParserPackManifestError> {
        for grammar in &mut grammars {
            grammar.capability_digest = grammar.computed_capability_digest();
        }
        let mut manifest = Self {
            schema_version: OPTIONAL_PARSER_PACK_MANIFEST_SCHEMA_VERSION,
            pack_id: OPTIONAL_PARSER_PACK_ID.to_string(),
            capability_set_version: OPTIONAL_PARSER_PACK_CAPABILITY_SET_VERSION,
            source,
            runtime,
            registry: OptionalParserPackRegistryBinding::current()?,
            required_platforms: PackPlatform::ALL.to_vec(),
            licenses,
            grammars,
            capability_set_digest: Blake3Digest::for_bytes(&[]),
        };
        manifest.capability_set_digest = manifest.computed_capability_set_digest();
        manifest.validate()?;
        Ok(manifest)
    }

    /// Return the logical manifest schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Borrow the stable logical pack identity.
    #[must_use]
    pub fn pack_id(&self) -> &str {
        &self.pack_id
    }

    /// Return the accepted capability-set version.
    #[must_use]
    pub const fn capability_set_version(&self) -> u32 {
        self.capability_set_version
    }

    /// Borrow the exact broad-grammar source pin.
    #[must_use]
    pub const fn source(&self) -> &OptionalParserPackSource {
        &self.source
    }

    /// Borrow the exact consuming runtime binding.
    #[must_use]
    pub const fn runtime(&self) -> &OptionalParserPackRuntime {
        &self.runtime
    }

    /// Borrow the accepted language-registry binding.
    #[must_use]
    pub const fn registry(&self) -> &OptionalParserPackRegistryBinding {
        &self.registry
    }

    /// Borrow the complete optional-pack artifact target set.
    #[must_use]
    pub fn required_platforms(&self) -> &[PackPlatform] {
        &self.required_platforms
    }

    /// Borrow the sorted exact license inventory.
    #[must_use]
    pub fn licenses(&self) -> &[GrammarLicense] {
        &self.licenses
    }

    /// Borrow the sorted accepted non-built-in grammar rows.
    #[must_use]
    pub fn grammars(&self) -> &[AcceptedGrammar] {
        &self.grammars
    }

    /// Borrow the complete canonical capability-set digest.
    #[must_use]
    pub const fn capability_set_digest(&self) -> &Blake3Digest {
        &self.capability_set_digest
    }

    /// Parse and validate one bounded JSON manifest.
    ///
    /// # Errors
    ///
    /// Returns a typed size, JSON, or domain-validation error.
    pub fn from_json(bytes: &[u8]) -> Result<Self, OptionalParserPackManifestError> {
        if bytes.len() > OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES {
            return Err(OptionalParserPackManifestError::ManifestTooLarge {
                actual: bytes.len(),
                maximum: OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
            });
        }
        let wire: OptionalParserPackManifestWire = serde_json::from_slice(bytes)
            .map_err(|source| OptionalParserPackManifestError::InvalidJson { source })?;
        Self::try_from(wire)
    }

    /// Validate all local, cross-record, registry, and deterministic digest invariants.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic invalid field, binding, row, or digest.
    pub fn validate(&self) -> Result<(), OptionalParserPackManifestError> {
        validate_binding(
            "schema_version",
            &OPTIONAL_PARSER_PACK_MANIFEST_SCHEMA_VERSION,
            &self.schema_version,
        )?;
        validate_binding("pack_id", OPTIONAL_PARSER_PACK_ID, self.pack_id.as_str())?;
        validate_binding(
            "capability_set_version",
            &OPTIONAL_PARSER_PACK_CAPABILITY_SET_VERSION,
            &self.capability_set_version,
        )?;
        validate_source(&self.source)?;
        validate_runtime(&self.runtime)?;
        validate_registry_binding(&self.registry)?;
        validate_required_platforms("manifest", &self.required_platforms)?;
        validate_count("licenses", self.licenses.len(), 1, MAX_LICENSE_RECORDS)?;
        validate_count(
            "grammars",
            self.grammars.len(),
            OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS,
            MAX_ACCEPTED_GRAMMARS,
        )?;
        if !strictly_sorted_by(&self.licenses, |license| license.id.as_str()) {
            return Err(OptionalParserPackManifestError::NotSortedUnique { field: "licenses" });
        }
        if !strictly_sorted_by(&self.grammars, |grammar| grammar.language_id.as_str()) {
            return Err(OptionalParserPackManifestError::NotSortedUnique { field: "grammars" });
        }

        let mut license_by_id = BTreeMap::new();
        for license in &self.licenses {
            validate_license(license)?;
            license_by_id.insert(license.id.as_str(), license);
        }

        let mut source_owners = BTreeSet::new();
        let mut export_symbols = BTreeSet::new();
        let mut library_stems = BTreeSet::new();
        for grammar in &self.grammars {
            validate_grammar(
                grammar,
                &self.runtime,
                &self.required_platforms,
                &license_by_id,
            )?;
            let source_identity = (
                grammar.source.repository_url.as_str(),
                grammar.source.revision.as_str(),
                grammar.source.subdirectory.as_str(),
            );
            if !source_owners.insert(source_identity) {
                return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "grammar_source",
                    value: format!(
                        "{}@{}:{}",
                        source_identity.0, source_identity.1, source_identity.2
                    ),
                });
            }
            if !export_symbols.insert(grammar.abi_export.export_symbol.as_str()) {
                return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "export_symbol",
                    value: grammar.abi_export.export_symbol.as_str().to_string(),
                });
            }
            if !library_stems.insert(grammar.abi_export.library_stem.as_str()) {
                return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "library_stem",
                    value: grammar.abi_export.library_stem.as_str().to_string(),
                });
            }
        }

        if self.computed_capability_set_digest() != self.capability_set_digest {
            return Err(OptionalParserPackManifestError::DigestMismatch {
                owner: self.pack_id.clone(),
                field: "capability_set_digest",
            });
        }
        Ok(())
    }

    /// Compute the deterministic digest of the complete logical manifest.
    #[must_use]
    pub fn computed_capability_set_digest(&self) -> Blake3Digest {
        let mut hasher = Hasher::new();
        hash_value(&mut hasher, MANIFEST_DIGEST_DOMAIN);
        hasher.update(&self.schema_version.to_le_bytes());
        hash_value(&mut hasher, &self.pack_id);
        hasher.update(&self.capability_set_version.to_le_bytes());
        hash_value(&mut hasher, &self.source.package);
        hash_value(&mut hasher, &self.source.version);
        hash_value(&mut hasher, self.source.cargo_archive.sha256.as_str());
        hash_value(&mut hasher, self.source.cargo_archive.vcs_revision.as_str());
        hash_value(&mut hasher, &self.source.cargo_archive.path_in_vcs);
        hash_value(&mut hasher, &self.source.native_release.tag);
        hash_value(&mut hasher, self.source.native_release.revision.as_str());
        hash_value(
            &mut hasher,
            self.source.native_release.source_bundle_sha256.as_str(),
        );
        hash_value(&mut hasher, self.runtime.consumer.canonical_name());
        hash_value(&mut hasher, &self.runtime.projectatlas_version);
        hash_value(&mut hasher, &self.runtime.tree_sitter_version);
        hasher.update(&self.runtime.minimum_abi.to_le_bytes());
        hasher.update(&self.runtime.maximum_abi.to_le_bytes());
        hasher.update(&self.registry.registry_version.to_le_bytes());
        hash_value(&mut hasher, self.registry.registry_digest.as_str());
        hasher.update(&self.registry.accepted_set_version.to_le_bytes());
        hash_value(&mut hasher, self.registry.accepted_set_digest.as_str());
        for platform in &self.required_platforms {
            hash_value(&mut hasher, platform.as_str());
        }
        for license in &self.licenses {
            hash_value(&mut hasher, &license.id);
            hash_value(&mut hasher, &license.repository_url);
            hash_value(&mut hasher, &license.source_path);
            hash_value(&mut hasher, license.revision.as_str());
            hash_value(&mut hasher, license.text_blake3.as_str());
            hash_value(
                &mut hasher,
                license.spdx_expression.as_deref().unwrap_or(""),
            );
        }
        for grammar in &self.grammars {
            hash_value(&mut hasher, &grammar.language_id);
            hash_value(&mut hasher, grammar.capability_digest.as_str());
        }
        Blake3Digest(hasher.finalize().to_hex().to_string())
    }
}

impl TryFrom<OptionalParserPackManifestWire> for OptionalParserPackManifest {
    type Error = OptionalParserPackManifestError;

    fn try_from(wire: OptionalParserPackManifestWire) -> Result<Self, Self::Error> {
        let manifest = Self {
            schema_version: wire.schema_version,
            pack_id: wire.pack_id,
            capability_set_version: wire.capability_set_version,
            source: wire.source,
            runtime: wire.runtime,
            registry: wire.registry,
            required_platforms: wire.required_platforms,
            licenses: wire.licenses,
            grammars: wire.grammars,
            capability_set_digest: wire.capability_set_digest,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

impl ParserPackPayloadMeasurements {
    /// Derive bounded measurements from an exact manifest-listed payload inventory.
    ///
    /// # Errors
    ///
    /// Returns an error when a file or aggregate measurement exceeds a pack ceiling.
    pub fn from_files(
        files: &[ParserPackPayloadFile],
    ) -> Result<Self, OptionalParserPackManifestError> {
        validate_count(
            "artifact payload files",
            files.len(),
            1,
            OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES,
        )?;
        let mut payload_bytes = 0_u64;
        let mut largest_file_bytes = 0_u64;
        let mut longest_path_bytes = 0_usize;
        let mut grammar_libraries = 0_usize;
        for file in files {
            if file.bytes == 0 || file.bytes > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES {
                return Err(invalid_field(
                    file.path.as_str(),
                    "bytes",
                    "expected a non-empty payload file within the per-file byte ceiling",
                ));
            }
            payload_bytes = payload_bytes.checked_add(file.bytes).ok_or_else(|| {
                invalid_field(
                    OPTIONAL_PARSER_PACK_ID,
                    "payload_bytes",
                    "payload byte sum overflowed",
                )
            })?;
            if payload_bytes > OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES {
                return Err(invalid_field(
                    OPTIONAL_PARSER_PACK_ID,
                    "payload_bytes",
                    "payload byte sum exceeds the expanded artifact ceiling",
                ));
            }
            largest_file_bytes = largest_file_bytes.max(file.bytes);
            longest_path_bytes = longest_path_bytes.max(file.path.as_str().len());
            if matches!(&file.role, ParserPackPayloadRole::GrammarLibrary { .. }) {
                grammar_libraries = grammar_libraries.checked_add(1).ok_or_else(|| {
                    invalid_field(
                        OPTIONAL_PARSER_PACK_ID,
                        "grammar_libraries",
                        "grammar-library count overflowed",
                    )
                })?;
            }
        }
        Ok(Self {
            files: u32::try_from(files.len()).map_err(|_error| {
                invalid_field(
                    OPTIONAL_PARSER_PACK_ID,
                    "files",
                    "payload-file count cannot be represented",
                )
            })?,
            grammar_libraries: u32::try_from(grammar_libraries).map_err(|_error| {
                invalid_field(
                    OPTIONAL_PARSER_PACK_ID,
                    "grammar_libraries",
                    "grammar-library count cannot be represented",
                )
            })?,
            payload_bytes,
            largest_file_bytes,
            longest_path_bytes: u32::try_from(longest_path_bytes).map_err(|_error| {
                invalid_field(
                    OPTIONAL_PARSER_PACK_ID,
                    "longest_path_bytes",
                    "relative-path length cannot be represented",
                )
            })?,
        })
    }
}

impl OptionalParserPackArtifactManifest {
    /// Validate one immutable platform artifact against the accepted logical manifest.
    ///
    /// The artifact manifest intentionally excludes its own file from `files`; the complete
    /// archive digest in the later platform proof binds that manifest together with its payload.
    ///
    /// # Errors
    ///
    /// Returns the first binding, construction, native-audit, inventory, or bound violation.
    pub fn validate(
        &self,
        logical: &OptionalParserPackManifest,
    ) -> Result<(), OptionalParserPackManifestError> {
        logical.validate()?;
        validate_binding(
            "artifact.schema_version",
            &OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
            &self.schema_version,
        )?;
        validate_binding("artifact.pack_id", logical.pack_id(), self.pack_id.as_str())?;
        validate_binding(
            "artifact.projectatlas_version",
            logical.runtime().projectatlas_version.as_str(),
            self.projectatlas_version.as_str(),
        )?;
        validate_required_platform("artifact.platform", self.platform)?;
        validate_candidate_identity(&self.candidate, false, logical)?;
        validate_binding(
            "artifact.capability_set_digest",
            logical.capability_set_digest(),
            &self.capability_set_digest,
        )?;
        validate_source_asset(&self.source_asset, logical)?;
        validate_offline_construction(&self.construction, self.platform)?;
        validate_native_audit(&self.native_audit, logical.grammars().len())?;
        validate_payload_files(self, logical)?;
        let measured = ParserPackPayloadMeasurements::from_files(&self.files)?;
        if measured != self.measurements {
            return Err(invalid_field(
                self.pack_id.as_str(),
                "measurements",
                "stored artifact measurements differ from the payload inventory",
            ));
        }
        Ok(())
    }
}

impl OptionalParserPackPlatformProof {
    /// Validate one fresh-runner receipt against the accepted logical manifest.
    ///
    /// # Errors
    ///
    /// Returns the first candidate, archive, isolation, or grammar-probe violation.
    pub fn validate(
        &self,
        logical: &OptionalParserPackManifest,
    ) -> Result<(), OptionalParserPackManifestError> {
        logical.validate()?;
        validate_binding(
            "platform_proof.schema_version",
            &OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION,
            &self.schema_version,
        )?;
        validate_binding(
            "platform_proof.pack_id",
            logical.pack_id(),
            self.pack_id.as_str(),
        )?;
        validate_required_platform("platform_proof.platform", self.platform)?;
        validate_candidate_identity(&self.candidate, true, logical)?;
        validate_safe_basename("platform proof", "archive_name", &self.archive_name)?;
        if self.archive_bytes == 0 || self.archive_bytes > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES {
            return Err(invalid_field(
                self.archive_name.as_str(),
                "archive_bytes",
                "completed archive is empty or exceeds the compressed-byte ceiling",
            ));
        }
        if self.expanded_bytes == 0 || self.expanded_bytes > OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES
        {
            return Err(invalid_field(
                self.archive_name.as_str(),
                "expanded_bytes",
                "expanded artifact is empty or exceeds the expanded-byte ceiling",
            ));
        }
        validate_binding(
            "platform_proof.capability_set_digest",
            logical.capability_set_digest(),
            &self.capability_set_digest,
        )?;
        validate_fresh_runner(&self.runner, self.platform)?;
        validate_grammar_probes(&self.grammars, logical)?;
        validate_memory_probe(&self.memory, self.platform)?;
        Ok(())
    }
}

impl OptionalParserPackProofAggregate {
    /// Validate the exact required platform set and shared logical proof identity.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing, duplicate, failed, dirty, or logically divergent proof.
    pub fn validate(
        &self,
        logical: &OptionalParserPackManifest,
    ) -> Result<(), OptionalParserPackManifestError> {
        logical.validate()?;
        validate_binding(
            "proof_aggregate.schema_version",
            &OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION,
            &self.schema_version,
        )?;
        validate_binding(
            "proof_aggregate.pack_id",
            logical.pack_id(),
            self.pack_id.as_str(),
        )?;
        validate_binding(
            "proof_aggregate.projectatlas_version",
            logical.runtime().projectatlas_version.as_str(),
            self.projectatlas_version.as_str(),
        )?;
        validate_binding(
            "proof_aggregate.capability_set_digest",
            logical.capability_set_digest(),
            &self.capability_set_digest,
        )?;
        let platforms = self
            .platforms
            .iter()
            .map(|proof| proof.platform)
            .collect::<Vec<_>>();
        validate_required_platforms("proof aggregate", &platforms)?;
        let mut archive_names = BTreeSet::new();
        let mut archive_digests = BTreeSet::new();
        let first_candidate = self.platforms.first().map(|proof| &proof.candidate);
        for proof in &self.platforms {
            proof.validate(logical)?;
            validate_binding(
                "proof_aggregate.accepted_manifest_sha256",
                &self.accepted_manifest_sha256,
                &proof.accepted_manifest_sha256,
            )?;
            validate_binding(
                "proof_aggregate.capability_set_digest",
                &self.capability_set_digest,
                &proof.capability_set_digest,
            )?;
            validate_binding(
                "proof_aggregate.fixture_corpus_sha256",
                &self.fixture_corpus_sha256,
                &proof.fixture_corpus_sha256,
            )?;
            if first_candidate.is_some_and(|candidate| candidate != &proof.candidate) {
                return Err(invalid_field(
                    self.pack_id.as_str(),
                    "candidate",
                    "platform proofs were not built from one exact candidate identity",
                ));
            }
            if !archive_names.insert(proof.archive_name.as_str()) {
                return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "archive_name",
                    value: proof.archive_name.clone(),
                });
            }
            if !archive_digests.insert(proof.archive_sha256.as_str()) {
                return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "archive_sha256",
                    value: proof.archive_sha256.as_str().to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Validate one exact `ProjectAtlas` candidate identity.
fn validate_candidate_identity(
    candidate: &ParserPackCandidateIdentity,
    require_release_candidate: bool,
    logical: &OptionalParserPackManifest,
) -> Result<(), OptionalParserPackManifestError> {
    validate_identity(
        "candidate",
        "cargo_package_version",
        &candidate.cargo_package_version,
    )?;
    validate_binding(
        "candidate.intended_release_version",
        logical.runtime().projectatlas_version.as_str(),
        candidate.intended_release_version.as_str(),
    )?;
    validate_identity("candidate", "rustc_release", &candidate.rustc_release)?;
    validate_hex_value(
        "candidate rustc commit hash",
        "rustc_commit_hash",
        &candidate.rustc_commit_hash,
        40,
    )?;
    if require_release_candidate {
        validate_binding(
            "candidate.cargo_package_version",
            logical.runtime().projectatlas_version.as_str(),
            candidate.cargo_package_version.as_str(),
        )?;
        if candidate.source_state != ParserPackCandidateSourceState::Clean {
            return Err(invalid_field(
                candidate.projectatlas_revision.as_str(),
                "source_state",
                "fresh-runner proof requires one exact clean candidate commit",
            ));
        }
    }
    Ok(())
}

/// Validate one pinned upstream asset binding without duplicating platform hashes in Rust.
fn validate_source_asset(
    asset: &ParserPackSourceAsset,
    logical: &OptionalParserPackManifest,
) -> Result<(), OptionalParserPackManifestError> {
    validate_binding(
        "source_asset.release_tag",
        logical.source().native_release.tag.as_str(),
        asset.release_tag.as_str(),
    )?;
    validate_binding(
        "source_asset.release_revision",
        &logical.source().native_release.revision,
        &asset.release_revision,
    )?;
    validate_safe_basename("source asset", "name", &asset.name)?;
    if asset.bytes == 0 || asset.bytes > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES {
        return Err(invalid_field(
            asset.name.as_str(),
            "bytes",
            "source asset is empty or exceeds the acquisition ceiling",
        ));
    }
    Ok(())
}

/// Validate all offline and physical egress-denial controls.
fn validate_offline_construction(
    construction: &ParserPackOfflineConstruction,
    platform: PackPlatform,
) -> Result<(), OptionalParserPackManifestError> {
    validate_network_denial(
        &construction.network_denial,
        platform,
        ParserPackNetworkIsolation::for_construction(platform),
    )
}

/// Validate physical isolation method and all three egress canaries.
fn validate_network_denial(
    denial: &ParserPackNetworkDenial,
    platform: PackPlatform,
    expected: ParserPackNetworkIsolation,
) -> Result<(), OptionalParserPackManifestError> {
    if denial.mechanism != expected {
        return Err(invalid_field(
            platform.as_str(),
            "network_isolation",
            "physical network-isolation mechanism does not match the platform",
        ));
    }
    if !denial.dns_denied || !denial.direct_tcp_denied || !denial.https_denied {
        return Err(invalid_field(
            platform.as_str(),
            "network_denial",
            "DNS, direct TCP, and HTTPS canaries must all be denied",
        ));
    }
    Ok(())
}

/// Validate the closed native audit result.
fn validate_native_audit(
    audit: &ParserPackNativeAudit,
    expected_grammars: usize,
) -> Result<(), OptionalParserPackManifestError> {
    let audited = usize::try_from(audit.audited_libraries).map_err(|_error| {
        invalid_field(
            OPTIONAL_PARSER_PACK_ID,
            "audited_libraries",
            "native audit count cannot be represented",
        )
    })?;
    validate_binding(
        "native_audit.audited_libraries",
        &expected_grammars,
        &audited,
    )?;
    if audit.forbidden_imports != 0
        || audit.unexpected_dependencies != 0
        || audit.missing_exports != 0
        || audit.unexpected_exports != 0
    {
        return Err(invalid_field(
            OPTIONAL_PARSER_PACK_ID,
            "native_audit",
            "closed import, dependency, and export audit must have zero violations",
        ));
    }
    Ok(())
}

/// Validate the complete exact payload inventory and role-to-path mapping.
fn validate_payload_files(
    artifact: &OptionalParserPackArtifactManifest,
    logical: &OptionalParserPackManifest,
) -> Result<(), OptionalParserPackManifestError> {
    let platform_fixed_files = OPTIONAL_PARSER_PACK_COMMON_PAYLOAD_FILES
        + usize::from(artifact.platform.containment_broker_file_name().is_some());
    let expected_files = logical
        .grammars()
        .len()
        .checked_add(platform_fixed_files)
        .ok_or_else(|| {
            invalid_field(
                logical.pack_id(),
                "files",
                "expected payload-file count overflowed",
            )
        })?;
    validate_count(
        "artifact payload files",
        artifact.files.len(),
        expected_files,
        expected_files,
    )?;
    if !strictly_sorted_by(&artifact.files, |file| file.path.as_str()) {
        return Err(OptionalParserPackManifestError::NotSortedUnique {
            field: "artifact payload paths",
        });
    }
    let grammar_by_id = logical
        .grammars()
        .iter()
        .map(|grammar| (grammar.language_id.as_str(), grammar))
        .collect::<BTreeMap<_, _>>();
    let mut fixed_roles = BTreeSet::new();
    let mut grammar_ids = BTreeSet::new();
    for file in &artifact.files {
        if file.path.as_str() == "artifact-manifest.json" {
            return Err(invalid_field(
                file.path.as_str(),
                "files",
                "artifact manifest must not recursively list itself",
            ));
        }
        let expected_path = match &file.role {
            ParserPackPayloadRole::Worker => {
                fixed_roles.insert("worker");
                artifact.platform.worker_file_name().to_string()
            }
            ParserPackPayloadRole::ContainmentBroker => {
                fixed_roles.insert("containment-broker");
                artifact
                    .platform
                    .containment_broker_file_name()
                    .ok_or_else(|| {
                        invalid_field(
                            artifact.platform.as_str(),
                            "files",
                            "platform does not admit a runtime-containment broker",
                        )
                    })?
                    .to_string()
            }
            ParserPackPayloadRole::AcceptedManifest => {
                fixed_roles.insert("accepted-manifest");
                validate_binding(
                    "artifact.accepted_manifest_sha256",
                    &artifact.accepted_manifest_sha256,
                    &file.sha256,
                )?;
                "accepted-capabilities.json".to_string()
            }
            ParserPackPayloadRole::FixtureCorpus => {
                fixed_roles.insert("fixture-corpus");
                validate_binding(
                    "artifact.fixture_corpus_sha256",
                    &artifact.fixture_corpus_sha256,
                    &file.sha256,
                )?;
                "optional-parser-pack-corpus.json".to_string()
            }
            ParserPackPayloadRole::ProjectLicense => {
                fixed_roles.insert("project-license");
                "LICENSE".to_string()
            }
            ParserPackPayloadRole::NativeImportPolicy => {
                if file.bytes > OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_MAX_BYTES {
                    return Err(invalid_field(
                        file.path.as_str(),
                        "bytes",
                        "native-import policy exceeds its pre-containment byte ceiling",
                    ));
                }
                fixed_roles.insert("native-import-policy");
                validate_binding(
                    "artifact.native_audit.policy_sha256",
                    &artifact.native_audit.policy_sha256,
                    &file.sha256,
                )?;
                "native-import-policy.json".to_string()
            }
            ParserPackPayloadRole::NativeAuditReport => {
                fixed_roles.insert("native-audit-report");
                validate_binding(
                    "artifact.native_audit.report_sha256",
                    &artifact.native_audit.report_sha256,
                    &file.sha256,
                )?;
                "native-audit-report.json".to_string()
            }
            ParserPackPayloadRole::GrammarLibrary { language_id } => {
                validate_record_id("artifact grammar", language_id)?;
                if !grammar_ids.insert(language_id.as_str()) {
                    return Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                        field: "artifact grammar",
                        value: language_id.clone(),
                    });
                }
                let grammar = grammar_by_id.get(language_id.as_str()).ok_or_else(|| {
                    OptionalParserPackManifestError::UnknownOptionalLanguage {
                        language_id: language_id.clone(),
                    }
                })?;
                format!(
                    "lib/{}",
                    artifact
                        .platform
                        .grammar_library_file_name(&grammar.abi_export.library_stem)
                )
            }
        };
        validate_binding(
            "artifact payload path",
            expected_path.as_str(),
            file.path.as_str(),
        )?;
    }
    validate_count(
        "artifact fixed payload roles",
        fixed_roles.len(),
        platform_fixed_files,
        platform_fixed_files,
    )?;
    if grammar_by_id
        .keys()
        .copied()
        .ne(grammar_ids.iter().copied())
    {
        return Err(invalid_field(
            artifact.pack_id.as_str(),
            "grammar_set",
            "artifact grammar identities differ from the accepted logical manifest",
        ));
    }
    Ok(())
}

/// Validate one fresh-runner environment contract.
fn validate_fresh_runner(
    runner: &ParserPackFreshRunner,
    platform: PackPlatform,
) -> Result<(), OptionalParserPackManifestError> {
    validate_network_denial(
        &runner.network_denial,
        platform,
        ParserPackNetworkIsolation::for_fresh_runner(platform),
    )
}

/// Validate exact sorted grammar probes and their declared success.
fn validate_grammar_probes(
    probes: &[ParserPackGrammarProbe],
    logical: &OptionalParserPackManifest,
) -> Result<(), OptionalParserPackManifestError> {
    validate_count(
        "platform proof grammars",
        probes.len(),
        logical.grammars().len(),
        logical.grammars().len(),
    )?;
    if !strictly_sorted_by(probes, |probe| probe.language_id.as_str()) {
        return Err(OptionalParserPackManifestError::NotSortedUnique {
            field: "platform proof grammars",
        });
    }
    for (probe, grammar) in probes.iter().zip(logical.grammars()) {
        validate_binding(
            "platform proof language_id",
            grammar.language_id.as_str(),
            probe.language_id.as_str(),
        )?;
        if !probe.worker_probe_passed {
            return Err(invalid_field(
                probe.language_id.as_str(),
                "worker_probe_passed",
                "manifest approval, native loading, ABI, positive, and negative fixtures must pass",
            ));
        }
    }
    Ok(())
}

/// Validate one exact-host worker memory-limit and cleanup receipt.
fn validate_memory_probe(
    probe: &ParserPackMemoryProbe,
    platform: PackPlatform,
) -> Result<(), OptionalParserPackManifestError> {
    if probe.process_limit_bytes == 0
        || probe.process_limit_bytes > PARSER_WORKER_PROCESS_MEMORY_BYTES
        || probe.process_tree_limit_bytes < probe.process_limit_bytes
        || probe.process_tree_limit_bytes > PARSER_WORKER_JOB_MEMORY_BYTES
    {
        return Err(invalid_field(
            platform.as_str(),
            "memory_limits",
            "probe limits must be non-zero, ordered, and no stronger than the runtime ceilings",
        ));
    }

    let sampled = match (platform, probe.control) {
        (PackPlatform::LinuxX86_64, ParserPackMemoryControl::LinuxProcStatus) => true,
        (PackPlatform::LinuxX86_64, ParserPackMemoryControl::LinuxCgroupV2)
        | (PackPlatform::WindowsX86_64, ParserPackMemoryControl::WindowsJobObject) => false,
        _ => {
            return Err(invalid_field(
                platform.as_str(),
                "memory.control",
                "memory control does not match the platform proof",
            ));
        }
    };

    let expected_process_limit = match platform {
        PackPlatform::LinuxX86_64 => OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES,
        PackPlatform::WindowsX86_64 => OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES,
    };
    if probe.process_limit_bytes != expected_process_limit
        || probe.process_tree_limit_bytes != expected_process_limit
    {
        return Err(invalid_field(
            platform.as_str(),
            "memory_limits",
            "memory probe must use the exact closed release-verification ceiling",
        ));
    }

    if sampled {
        let Some(interval) = probe.observation_interval_millis else {
            return Err(invalid_field(
                platform.as_str(),
                "memory.observation_interval_millis",
                "sampled Linux RSS proof requires the declared observation interval",
            ));
        };
        let Some(peak) = probe.peak_observed_bytes else {
            return Err(invalid_field(
                platform.as_str(),
                "memory.peak_observed_bytes",
                "sampled Linux RSS proof requires the hosted peak observation",
            ));
        };
        let Some(overshoot) = probe.maximum_observed_overshoot_bytes else {
            return Err(invalid_field(
                platform.as_str(),
                "memory.maximum_observed_overshoot_bytes",
                "sampled Linux RSS proof requires the hosted maximum overshoot",
            ));
        };
        if interval == 0
            || peak < probe.process_limit_bytes
            || overshoot != peak.saturating_sub(probe.process_limit_bytes)
        {
            return Err(invalid_field(
                platform.as_str(),
                "memory.sampled_measurement",
                "sampled Linux RSS interval, peak, and overshoot are inconsistent",
            ));
        }
    } else if probe.observation_interval_millis.is_some()
        || probe.peak_observed_bytes.is_some()
        || probe.maximum_observed_overshoot_bytes.is_some()
    {
        return Err(invalid_field(
            platform.as_str(),
            "memory.hard_limit_measurement",
            "kernel-hard controls must not claim sampled RSS interval or overshoot values",
        ));
    }
    Ok(())
}

/// Validate a required platform without accepting future or duplicate values.
fn validate_required_platform(
    owner: &'static str,
    platform: PackPlatform,
) -> Result<(), OptionalParserPackManifestError> {
    if !PackPlatform::ALL.contains(&platform) {
        return Err(invalid_field(
            owner,
            "platform",
            "platform is not part of the optional-pack artifact target set",
        ));
    }
    Ok(())
}

/// Validate a release archive or asset basename without path syntax.
fn validate_safe_basename(
    owner: &str,
    field: &'static str,
    value: &str,
) -> Result<(), OptionalParserPackManifestError> {
    if value.is_empty()
        || value.len() > OPTIONAL_PARSER_PACK_MAX_PATH_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_field(
            owner,
            field,
            "expected a safe ASCII basename within the pack path bound",
        ));
    }
    Ok(())
}

/// Validate an exact lowercase hexadecimal value with a caller-owned field name.
fn validate_hex_value(
    owner: &str,
    field: &'static str,
    value: &str,
    expected_len: usize,
) -> Result<(), OptionalParserPackManifestError> {
    if value.len() != expected_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_field(
            owner,
            field,
            "expected canonical lowercase hexadecimal",
        ));
    }
    Ok(())
}

/// Validate the selected source package against the language-registry pin.
fn validate_source(
    source: &OptionalParserPackSource,
) -> Result<(), OptionalParserPackManifestError> {
    validate_binding(
        "source.package",
        OPTIONAL_GRAMMAR_CATALOG,
        source.package.as_str(),
    )?;
    validate_binding(
        "source.version",
        OPTIONAL_GRAMMAR_CATALOG_VERSION,
        source.version.as_str(),
    )?;
    validate_binding(
        "source.cargo_archive.sha256",
        OPTIONAL_GRAMMAR_CATALOG_CRATE_SHA256,
        source.cargo_archive.sha256.as_str(),
    )?;
    validate_binding(
        "source.cargo_archive.vcs_revision",
        OPTIONAL_GRAMMAR_CATALOG_CRATE_REVISION,
        source.cargo_archive.vcs_revision.as_str(),
    )?;
    validate_binding(
        "source.cargo_archive.path_in_vcs",
        OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS,
        source.cargo_archive.path_in_vcs.as_str(),
    )?;
    validate_binding(
        "source.native_release.tag",
        OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG,
        source.native_release.tag.as_str(),
    )?;
    validate_binding(
        "source.native_release.revision",
        OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
        source.native_release.revision.as_str(),
    )?;
    validate_binding(
        "source.native_release.source_bundle_sha256",
        OPTIONAL_GRAMMAR_CATALOG_SOURCE_BUNDLE_SHA256,
        source.native_release.source_bundle_sha256.as_str(),
    )
}

/// Validate the concrete consumer version and supported ABI window.
fn validate_runtime(
    runtime: &OptionalParserPackRuntime,
) -> Result<(), OptionalParserPackManifestError> {
    validate_binding(
        "runtime.projectatlas_version",
        OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION,
        runtime.projectatlas_version.as_str(),
    )?;
    validate_binding(
        "runtime.tree_sitter_version",
        OPTIONAL_PARSER_PACK_TREE_SITTER_VERSION,
        runtime.tree_sitter_version.as_str(),
    )?;
    validate_binding(
        "runtime.minimum_abi",
        &OPTIONAL_PARSER_PACK_MINIMUM_ABI,
        &runtime.minimum_abi,
    )?;
    validate_binding(
        "runtime.maximum_abi",
        &OPTIONAL_PARSER_PACK_MAXIMUM_ABI,
        &runtime.maximum_abi,
    )
}

/// Validate that the manifest is bound to current accepted registry truth.
fn validate_registry_binding(
    binding: &OptionalParserPackRegistryBinding,
) -> Result<(), OptionalParserPackManifestError> {
    let current = OptionalParserPackRegistryBinding::current()?;
    validate_binding(
        "registry.registry_version",
        &current.registry_version,
        &binding.registry_version,
    )?;
    validate_binding(
        "registry.registry_digest",
        current.registry_digest.as_str(),
        binding.registry_digest.as_str(),
    )?;
    validate_binding(
        "registry.accepted_set_version",
        &current.accepted_set_version,
        &binding.accepted_set_version,
    )?;
    validate_binding(
        "registry.accepted_set_digest",
        current.accepted_set_digest.as_str(),
        binding.accepted_set_digest.as_str(),
    )
}

/// Validate one exact deduplicated license record.
fn validate_license(license: &GrammarLicense) -> Result<(), OptionalParserPackManifestError> {
    validate_record_id("license", &license.id)?;
    validate_https_url(&license.id, &license.repository_url)?;
    validate_relative_path(&license.id, "source_path", &license.source_path, false)?;
    if license.text.is_empty() || license.text.len() > MAX_LICENSE_TEXT_BYTES {
        return Err(invalid_field(
            &license.id,
            "text",
            "expected 1..=262144 exact UTF-8 bytes",
        ));
    }
    if Blake3Digest::for_bytes(license.text.as_bytes()) != license.text_blake3 {
        return Err(OptionalParserPackManifestError::DigestMismatch {
            owner: license.id.clone(),
            field: "text_blake3",
        });
    }
    if let Some(expression) = &license.spdx_expression {
        validate_identity(&license.id, "spdx_expression", expression)?;
    }
    Ok(())
}

/// Validate one accepted grammar against registry, license, ABI, and platform authority.
fn validate_grammar(
    grammar: &AcceptedGrammar,
    runtime: &OptionalParserPackRuntime,
    required_platforms: &[PackPlatform],
    license_by_id: &BTreeMap<&str, &GrammarLicense>,
) -> Result<(), OptionalParserPackManifestError> {
    let Some(capability) = language_capability(&grammar.language_id) else {
        return Err(OptionalParserPackManifestError::UnknownOptionalLanguage {
            language_id: grammar.language_id.clone(),
        });
    };
    if capability.id != grammar.language_id {
        return Err(OptionalParserPackManifestError::UnknownOptionalLanguage {
            language_id: grammar.language_id.clone(),
        });
    }
    if capability.optional_pack != Some(OPTIONAL_PARSER_PACK_ID) {
        return Err(OptionalParserPackManifestError::BuiltInOverlap {
            language_id: grammar.language_id.clone(),
        });
    }
    validate_https_url(&grammar.language_id, &grammar.source.repository_url)?;
    validate_relative_path(
        &grammar.language_id,
        "subdirectory",
        &grammar.source.subdirectory,
        true,
    )?;
    if grammar.license_record_ids.is_empty() {
        return Err(invalid_field(
            &grammar.language_id,
            "license_record_ids",
            "expected at least one applicable exact license record",
        ));
    }
    if !strictly_sorted_by(&grammar.license_record_ids, String::as_str) {
        return Err(OptionalParserPackManifestError::NotSortedUnique {
            field: "grammar.license_record_ids",
        });
    }
    for license_id in &grammar.license_record_ids {
        let Some(license) = license_by_id.get(license_id.as_str()) else {
            return Err(OptionalParserPackManifestError::UnknownLicense {
                language_id: grammar.language_id.clone(),
                license_id: license_id.clone(),
            });
        };
        if license.repository_url != grammar.source.repository_url
            || license.revision != grammar.source.revision
        {
            return Err(OptionalParserPackManifestError::LicenseSourceMismatch {
                language_id: grammar.language_id.clone(),
                license_id: license_id.clone(),
            });
        }
    }
    if grammar.abi_export.minimum_abi == 0
        || grammar.abi_export.maximum_abi < grammar.abi_export.minimum_abi
        || grammar.abi_export.expected_abi < grammar.abi_export.minimum_abi
        || grammar.abi_export.expected_abi > grammar.abi_export.maximum_abi
        || grammar.abi_export.minimum_abi != runtime.minimum_abi
        || grammar.abi_export.maximum_abi != runtime.maximum_abi
    {
        return Err(OptionalParserPackManifestError::AbiMismatch {
            language_id: grammar.language_id.clone(),
        });
    }
    validate_fixture(&grammar.language_id, "positive", &grammar.fixtures.positive)?;
    validate_fixture(&grammar.language_id, "negative", &grammar.fixtures.negative)?;
    if !grammar.fixtures.positive.origin.is_positive()
        || !grammar.fixtures.negative.origin.is_negative()
    {
        return Err(invalid_field(
            &grammar.language_id,
            "fixtures.origin",
            "expected a natural upstream positive and an upstream-error or incomplete-editor-state negative",
        ));
    }
    if grammar.fixtures.positive.source_blake3 == grammar.fixtures.negative.source_blake3 {
        return Err(invalid_field(
            &grammar.language_id,
            "fixtures",
            "positive and negative fixture source must be distinct",
        ));
    }
    validate_required_platforms(&grammar.language_id, &grammar.required_platforms)?;
    if grammar.required_platforms != required_platforms {
        return Err(invalid_field(
            &grammar.language_id,
            "required_platforms",
            "grammar and manifest platform sets differ",
        ));
    }
    if grammar.built_in_precedence != BuiltInParserPrecedence::BuiltInAuthoritative {
        return Err(invalid_field(
            &grammar.language_id,
            "built_in_precedence",
            "default-core ownership must remain authoritative",
        ));
    }
    if grammar.computed_capability_digest() != grammar.capability_digest {
        return Err(OptionalParserPackManifestError::DigestMismatch {
            owner: grammar.language_id.clone(),
            field: "capability_digest",
        });
    }
    Ok(())
}

/// Validate one exact bounded provenance-bearing fixture.
fn validate_fixture(
    owner: &str,
    role: &'static str,
    fixture: &GrammarFixture,
) -> Result<(), OptionalParserPackManifestError> {
    validate_relative_path(owner, "fixture.path", &fixture.path, false)?;
    validate_identity(owner, "fixture.case_name", &fixture.case_name)?;
    if fixture.source.is_empty() || fixture.source.len() > MAX_FIXTURE_SOURCE_BYTES {
        return Err(invalid_field(
            owner,
            role,
            "expected 1..=65536 exact UTF-8 source bytes",
        ));
    }
    if Blake3Digest::for_bytes(fixture.source.as_bytes()) != fixture.source_blake3 {
        return Err(OptionalParserPackManifestError::DigestMismatch {
            owner: owner.to_string(),
            field: "fixture.source_blake3",
        });
    }
    Ok(())
}

/// Require the complete closed optional-pack artifact target set in canonical order.
fn validate_required_platforms(
    owner: &str,
    platforms: &[PackPlatform],
) -> Result<(), OptionalParserPackManifestError> {
    if platforms != PackPlatform::ALL {
        return Err(invalid_field(
            owner,
            "required_platforms",
            "expected the complete canonical optional-pack artifact target set",
        ));
    }
    Ok(())
}

/// Validate one bounded manifest collection size.
fn validate_count(
    field: &'static str,
    actual: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), OptionalParserPackManifestError> {
    if !(minimum..=maximum).contains(&actual) {
        return Err(OptionalParserPackManifestError::CountOutOfBounds {
            field,
            actual,
            minimum,
            maximum,
        });
    }
    Ok(())
}

/// Compare one manifest binding with its selected authority.
fn validate_binding<T>(
    field: &'static str,
    expected: &T,
    actual: &T,
) -> Result<(), OptionalParserPackManifestError>
where
    T: Eq + fmt::Display + ?Sized,
{
    if expected != actual {
        return Err(OptionalParserPackManifestError::BindingMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

/// Validate one canonical 256-bit lowercase hexadecimal digest.
fn validate_hex_digest(
    value: &str,
    field: &'static str,
) -> Result<(), OptionalParserPackManifestError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_field(
            "digest",
            field,
            "expected 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

/// Validate one bounded unpadded control-free identity field.
fn validate_identity(
    owner: &str,
    field: &'static str,
    value: &str,
) -> Result<(), OptionalParserPackManifestError> {
    if value.is_empty()
        || value.len() > MAX_IDENTITY_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(invalid_field(
            owner,
            field,
            "expected non-empty, unpadded, control-free bounded text",
        ));
    }
    Ok(())
}

/// Validate one stable manifest-local record identifier.
fn validate_record_id(owner: &str, value: &str) -> Result<(), OptionalParserPackManifestError> {
    validate_identity(owner, "id", value)?;
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) || !value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
    {
        return Err(invalid_field(
            owner,
            "id",
            "expected a lowercase ASCII record identifier",
        ));
    }
    Ok(())
}

/// Validate one non-parameterized HTTPS provenance URL.
fn validate_https_url(owner: &str, value: &str) -> Result<(), OptionalParserPackManifestError> {
    validate_identity(owner, "repository_url", value)?;
    if !value.starts_with("https://") || value.contains(['?', '#']) {
        return Err(invalid_field(
            owner,
            "repository_url",
            "expected an HTTPS repository URL without query or fragment",
        ));
    }
    Ok(())
}

/// Validate one normalized slash-separated repository-relative path.
fn validate_relative_path(
    owner: &str,
    field: &'static str,
    value: &str,
    allow_root: bool,
) -> Result<(), OptionalParserPackManifestError> {
    validate_identity(owner, field, value)?;
    if allow_root && value == "." {
        return Ok(());
    }
    if value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(invalid_field(
            owner,
            field,
            "expected a normalized repository-relative slash path",
        ));
    }
    Ok(())
}

/// Build one deterministic invalid-field diagnostic.
fn invalid_field(
    owner: &str,
    field: &'static str,
    reason: &'static str,
) -> OptionalParserPackManifestError {
    OptionalParserPackManifestError::InvalidField {
        owner: owner.to_string(),
        field,
        reason,
    }
}

/// Return whether selected string keys are strictly sorted and unique.
fn strictly_sorted_by<T>(values: &[T], key: impl Fn(&T) -> &str) -> bool {
    values.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

/// Add one grammar's pinned source projection to a canonical digest.
fn hash_source_provenance(hasher: &mut Hasher, source: &GrammarSourceProvenance) {
    hash_value(hasher, &source.repository_url);
    hash_value(hasher, source.revision.as_str());
    hash_value(hasher, &source.subdirectory);
    hash_value(hasher, source.compile_input_sha256.as_str());
}

/// Add one exact fixture identity to a canonical digest.
fn hash_fixture(hasher: &mut Hasher, fixture: &GrammarFixture) {
    hash_value(hasher, fixture.origin.canonical_name());
    hash_value(hasher, &fixture.path);
    hash_value(hasher, &fixture.case_name);
    hash_value(hasher, fixture.source_blake3.as_str());
}

/// Add one length-delimited string to a canonical digest.
fn hash_value(hasher: &mut Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::language_documentation_rows;
    use std::error::Error;
    use std::io;

    const RELEASE_REVISION: &str = "6258abac30304283763a0d2dc8a48cb87fbcf438";
    const CRATE_VCS_REVISION: &str = "ce9e9c0974731d25b4b9426711a62d544d993368";
    const CRATE_SHA256: &str = "44dc94ef7a5f7f4247d88d5acdd26d842c8fc6f5eaf491a970c8e3d8fc9c9287";
    const SOURCE_BUNDLE_SHA256: &str =
        "d684799dc664553c9c746d5fe676a5b599f9efcec4cad5450bec7ec5a29574a9";
    const REPOSITORY_URL: &str = "https://example.invalid/optional-grammars.git";

    /// Convert a behavior assertion into the crate's non-panicking result-test style.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn test_manifest() -> Result<OptionalParserPackManifest, Box<dyn Error>> {
        let revision = SourceRevision::new(RELEASE_REVISION)?;
        let licenses = vec![
            GrammarLicense::new(
                "apache-root",
                REPOSITORY_URL,
                "LICENSE-APACHE",
                revision.clone(),
                "Apache License\nVersion 2.0 fixture text.",
                Some("Apache-2.0".to_string()),
            ),
            GrammarLicense::new(
                "mit-root",
                REPOSITORY_URL,
                "LICENSE-MIT",
                revision.clone(),
                "MIT License\n\nPermission is hereby granted for the fixture.",
                Some("MIT".to_string()),
            ),
        ];
        let mut grammars = language_documentation_rows()
            .iter()
            .filter(|capability| capability.optional_pack.is_some())
            .take(OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS)
            .enumerate()
            .map(|(index, capability)| {
                let ordinal = index + 1;
                let license_record_ids = if index == 0 {
                    vec!["apache-root".to_string(), "mit-root".to_string()]
                } else {
                    vec!["mit-root".to_string()]
                };
                Ok(AcceptedGrammar::new(
                    capability.id,
                    GrammarSourceProvenance {
                        repository_url: REPOSITORY_URL.to_string(),
                        revision: revision.clone(),
                        subdirectory: format!("grammars/{ordinal}"),
                        compile_input_sha256: Sha256Digest::new(format!("{ordinal:064x}"))?,
                    },
                    license_record_ids,
                    GrammarAbiExport {
                        minimum_abi: 13,
                        maximum_abi: 15,
                        expected_abi: 15,
                        export_symbol: GrammarExportSymbol::new(format!(
                            "tree_sitter_optional_{ordinal}"
                        ))?,
                        library_stem: GrammarLibraryStem::new(format!(
                            "tree-sitter-optional-{ordinal}"
                        ))?,
                    },
                    GrammarFixtures {
                        positive: GrammarFixture::new(
                            GrammarFixtureOrigin::UpstreamTreeSitterCorpus,
                            format!("fixtures/{ordinal}/positive.txt"),
                            format!("natural positive {}", capability.id),
                            format!("natural positive {} source\n", capability.id),
                        ),
                        negative: GrammarFixture::new(
                            GrammarFixtureOrigin::ProjectAtlasIncompleteUpstreamCase,
                            format!("fixtures/{ordinal}/negative.txt"),
                            format!("natural negative {} incomplete", capability.id),
                            format!("natural negative {} source ?\n", capability.id),
                        ),
                    },
                ))
            })
            .collect::<Result<Vec<_>, OptionalParserPackManifestError>>()?;
        grammars.sort_by(|left, right| left.language_id.cmp(&right.language_id));
        OptionalParserPackManifest::new(
            OptionalParserPackSource {
                package: OPTIONAL_GRAMMAR_CATALOG.to_string(),
                version: OPTIONAL_GRAMMAR_CATALOG_VERSION.to_string(),
                cargo_archive: OptionalParserCargoArchive {
                    sha256: Sha256Digest::new(CRATE_SHA256)?,
                    vcs_revision: SourceRevision::new(CRATE_VCS_REVISION)?,
                    path_in_vcs: OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS.to_string(),
                },
                native_release: OptionalParserNativeRelease {
                    tag: OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG.to_string(),
                    revision,
                    source_bundle_sha256: Sha256Digest::new(SOURCE_BUNDLE_SHA256)?,
                },
            },
            OptionalParserPackRuntime {
                consumer: ParserPackConsumer::ProjectAtlasParserWorker,
                projectatlas_version: OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION.to_string(),
                tree_sitter_version: "0.26.9".to_string(),
                minimum_abi: 13,
                maximum_abi: 15,
            },
            licenses,
            grammars,
        )
        .map_err(Into::into)
    }

    fn test_construction_network_denial(platform: PackPlatform) -> ParserPackNetworkDenial {
        ParserPackNetworkDenial {
            mechanism: ParserPackNetworkIsolation::for_construction(platform),
            dns_denied: true,
            direct_tcp_denied: true,
            https_denied: true,
        }
    }

    fn test_fresh_runner_network_denial(platform: PackPlatform) -> ParserPackNetworkDenial {
        ParserPackNetworkDenial {
            mechanism: ParserPackNetworkIsolation::for_fresh_runner(platform),
            dns_denied: true,
            direct_tcp_denied: true,
            https_denied: true,
        }
    }

    fn test_candidate(
        state: ParserPackCandidateSourceState,
        cargo_package_version: &str,
    ) -> Result<ParserPackCandidateIdentity, OptionalParserPackManifestError> {
        Ok(ParserPackCandidateIdentity {
            projectatlas_revision: SourceRevision::new(CRATE_VCS_REVISION)?,
            cargo_package_version: cargo_package_version.to_string(),
            intended_release_version: OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION.to_string(),
            cargo_lock_sha256: Sha256Digest::new(format!("{:064x}", 31))?,
            rustc_release: "1.88.0".to_string(),
            rustc_commit_hash: "01f6ddf7588f42ae2d7eb0a2f21d44e8e96674cf".to_string(),
            source_state: state,
        })
    }

    fn test_artifact(
        logical: &OptionalParserPackManifest,
        platform: PackPlatform,
    ) -> Result<OptionalParserPackArtifactManifest, Box<dyn Error>> {
        let mut files = vec![
            ParserPackPayloadFile {
                path: PackRelativePath::new(platform.worker_file_name())?,
                role: ParserPackPayloadRole::Worker,
                bytes: 10,
                sha256: Sha256Digest::new(format!("{:064x}", 1))?,
            },
            ParserPackPayloadFile {
                path: PackRelativePath::new("accepted-capabilities.json")?,
                role: ParserPackPayloadRole::AcceptedManifest,
                bytes: 11,
                sha256: Sha256Digest::new(format!("{:064x}", 21))?,
            },
            ParserPackPayloadFile {
                path: PackRelativePath::new("optional-parser-pack-corpus.json")?,
                role: ParserPackPayloadRole::FixtureCorpus,
                bytes: 12,
                sha256: Sha256Digest::new(format!("{:064x}", 22))?,
            },
            ParserPackPayloadFile {
                path: PackRelativePath::new("LICENSE")?,
                role: ParserPackPayloadRole::ProjectLicense,
                bytes: 13,
                sha256: Sha256Digest::new(format!("{:064x}", 4))?,
            },
            ParserPackPayloadFile {
                path: PackRelativePath::new("native-import-policy.json")?,
                role: ParserPackPayloadRole::NativeImportPolicy,
                bytes: 14,
                sha256: Sha256Digest::new(format!("{:064x}", 25))?,
            },
            ParserPackPayloadFile {
                path: PackRelativePath::new("native-audit-report.json")?,
                role: ParserPackPayloadRole::NativeAuditReport,
                bytes: 15,
                sha256: Sha256Digest::new(format!("{:064x}", 26))?,
            },
        ];
        if let Some(broker_name) = platform.containment_broker_file_name() {
            files.push(ParserPackPayloadFile {
                path: PackRelativePath::new(broker_name)?,
                role: ParserPackPayloadRole::ContainmentBroker,
                bytes: 16,
                sha256: Sha256Digest::new(format!("{:064x}", 28))?,
            });
        }
        for (index, grammar) in logical.grammars().iter().enumerate() {
            files.push(ParserPackPayloadFile {
                path: PackRelativePath::new(format!(
                    "lib/{}",
                    platform.grammar_library_file_name(&grammar.abi_export.library_stem)
                ))?,
                role: ParserPackPayloadRole::GrammarLibrary {
                    language_id: grammar.language_id.clone(),
                },
                bytes: u64::try_from(index)?.saturating_add(100),
                sha256: Sha256Digest::new(format!("{:064x}", index + 100))?,
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let measurements = ParserPackPayloadMeasurements::from_files(&files)?;
        Ok(OptionalParserPackArtifactManifest {
            schema_version: OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
            pack_id: logical.pack_id().to_string(),
            projectatlas_version: logical.runtime().projectatlas_version.clone(),
            platform,
            candidate: test_candidate(ParserPackCandidateSourceState::Dirty, "0.3.26")?,
            accepted_manifest_sha256: Sha256Digest::new(format!("{:064x}", 21))?,
            capability_set_digest: logical.capability_set_digest().clone(),
            fixture_corpus_sha256: Sha256Digest::new(format!("{:064x}", 22))?,
            source_asset: ParserPackSourceAsset {
                release_tag: logical.source().native_release.tag.clone(),
                release_revision: logical.source().native_release.revision.clone(),
                name: format!("parsers-{}.tar.zst", platform.as_str()),
                sha256: Sha256Digest::new(format!("{:064x}", 23))?,
                bytes: 1_024,
                parsers_manifest_sha256: Sha256Digest::new(format!("{:064x}", 24))?,
            },
            construction: ParserPackOfflineConstruction {
                cargo_frozen: ParserPackVerifiedControl::Verified,
                cargo_offline: ParserPackVerifiedControl::Verified,
                dependency_offline: ParserPackVerifiedControl::Verified,
                zero_embedded_grammars: ParserPackVerifiedControl::Verified,
                language_selector_absent: ParserPackVerifiedControl::Verified,
                failed_grammar_override_absent: ParserPackVerifiedControl::Verified,
                network_denial: test_construction_network_denial(platform),
            },
            native_audit: ParserPackNativeAudit {
                policy_sha256: Sha256Digest::new(format!("{:064x}", 25))?,
                report_sha256: Sha256Digest::new(format!("{:064x}", 26))?,
                audited_libraries: u32::try_from(logical.grammars().len())?,
                forbidden_imports: 0,
                unexpected_dependencies: 0,
                missing_exports: 0,
                unexpected_exports: 0,
            },
            measurements,
            files,
        })
    }

    fn test_platform_proof(
        logical: &OptionalParserPackManifest,
        platform: PackPlatform,
        ordinal: usize,
    ) -> Result<OptionalParserPackPlatformProof, Box<dyn Error>> {
        Ok(OptionalParserPackPlatformProof {
            schema_version: OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION,
            pack_id: logical.pack_id().to_string(),
            platform,
            candidate: test_candidate(
                ParserPackCandidateSourceState::Clean,
                OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION,
            )?,
            archive_name: format!("projectatlas-broad-parser-{}.tar.zst", platform.as_str()),
            archive_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 40))?,
            archive_bytes: 1_024,
            expanded_bytes: 4_096,
            artifact_manifest_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 50))?,
            accepted_manifest_sha256: Sha256Digest::new(format!("{:064x}", 21))?,
            capability_set_digest: logical.capability_set_digest().clone(),
            fixture_corpus_sha256: Sha256Digest::new(format!("{:064x}", 22))?,
            native_audit_report_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 60))?,
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

    #[test]
    fn valid_manifest_round_trips_with_complete_optional_floor() -> Result<(), Box<dyn Error>> {
        let manifest = test_manifest()?;
        manifest.validate()?;
        require(
            manifest.grammars.len() >= OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS,
            "test manifest did not cover the accepted optional grammar floor",
        )?;
        require(
            manifest
                .grammars
                .iter()
                .any(|grammar| grammar.license_record_ids.len() == 2),
            "valid dual-licensed grammar was not retained as a license-record set",
        )?;
        let encoded = serde_json::to_vec(&manifest)?;
        let decoded = OptionalParserPackManifest::from_json(&encoded)?;
        require(
            decoded == manifest,
            "validated manifest JSON did not round-trip",
        )?;
        Ok(())
    }

    #[test]
    fn external_release_json_rejects_unknown_fields() -> Result<(), Box<dyn Error>> {
        let manifest = test_manifest()?;

        let mut logical_root = serde_json::to_value(&manifest)?;
        logical_root
            .as_object_mut()
            .ok_or_else(|| io::Error::other("logical manifest is not an object"))?
            .insert("unmodeled_claim".to_string(), serde_json::Value::Bool(true));
        require(
            OptionalParserPackManifest::from_json(&serde_json::to_vec(&logical_root)?).is_err(),
            "logical manifest accepted an unknown root field",
        )?;

        let mut logical_nested = serde_json::to_value(&manifest)?;
        logical_nested["grammars"][0]["fixtures"]["positive"]
            .as_object_mut()
            .ok_or_else(|| io::Error::other("positive fixture is not an object"))?
            .insert("unmodeled_claim".to_string(), serde_json::Value::Bool(true));
        require(
            OptionalParserPackManifest::from_json(&serde_json::to_vec(&logical_nested)?).is_err(),
            "logical manifest accepted an unknown nested field",
        )?;

        let artifact = test_artifact(&manifest, PackPlatform::LinuxX86_64)?;
        let mut artifact_json = serde_json::to_value(&artifact)?;
        artifact_json["candidate"]
            .as_object_mut()
            .ok_or_else(|| io::Error::other("artifact candidate is not an object"))?
            .insert("unmodeled_claim".to_string(), serde_json::Value::Bool(true));
        require(
            serde_json::from_value::<OptionalParserPackArtifactManifest>(artifact_json).is_err(),
            "artifact manifest accepted an unknown nested field",
        )?;

        let platform_proof = test_platform_proof(&manifest, PackPlatform::LinuxX86_64, 0)?;
        let mut proof_json = serde_json::to_value(&platform_proof)?;
        proof_json
            .as_object_mut()
            .ok_or_else(|| io::Error::other("platform proof is not an object"))?
            .insert("unmodeled_claim".to_string(), serde_json::Value::Bool(true));
        require(
            serde_json::from_value::<OptionalParserPackPlatformProof>(proof_json).is_err(),
            "platform proof accepted an unknown root field",
        )?;

        let aggregate = OptionalParserPackProofAggregate {
            schema_version: OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION,
            pack_id: manifest.pack_id().to_string(),
            projectatlas_version: manifest.runtime().projectatlas_version.clone(),
            accepted_manifest_sha256: platform_proof.accepted_manifest_sha256.clone(),
            capability_set_digest: manifest.capability_set_digest().clone(),
            fixture_corpus_sha256: platform_proof.fixture_corpus_sha256.clone(),
            platforms: vec![platform_proof],
        };
        let mut aggregate_json = serde_json::to_value(aggregate)?;
        aggregate_json
            .as_object_mut()
            .ok_or_else(|| io::Error::other("proof aggregate is not an object"))?
            .insert("unmodeled_claim".to_string(), serde_json::Value::Bool(true));
        require(
            serde_json::from_value::<OptionalParserPackProofAggregate>(aggregate_json).is_err(),
            "proof aggregate accepted an unknown root field",
        )?;
        Ok(())
    }

    #[test]
    fn manifest_rejects_built_in_overlap_and_capability_shrinkage() -> Result<(), Box<dyn Error>> {
        let mut overlap = test_manifest()?;
        overlap.grammars[0].language_id = "rust".to_string();
        overlap
            .grammars
            .sort_by(|left, right| left.language_id.cmp(&right.language_id));
        require(
            matches!(
                overlap.validate(),
                Err(OptionalParserPackManifestError::BuiltInOverlap { .. })
            ),
            "default-core grammar overlap was accepted",
        )?;

        let mut too_small = test_manifest()?;
        too_small
            .grammars
            .truncate(OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS - 1);
        require(
            matches!(
                too_small.validate(),
                Err(OptionalParserPackManifestError::CountOutOfBounds {
                    field: "grammars",
                    ..
                })
            ),
            "accepted grammar floor shrinkage was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn manifest_rejects_nondeterminism_unknown_licenses_and_runtime_collisions()
    -> Result<(), Box<dyn Error>> {
        let mut unsorted = test_manifest()?;
        unsorted.grammars.swap(0, 1);
        require(
            matches!(
                unsorted.validate(),
                Err(OptionalParserPackManifestError::NotSortedUnique { field: "grammars" })
            ),
            "unsorted grammar rows were accepted",
        )?;

        let mut missing_license = test_manifest()?;
        missing_license.grammars[0].license_record_ids = vec!["missing".to_string()];
        require(
            matches!(
                missing_license.validate(),
                Err(OptionalParserPackManifestError::UnknownLicense { .. })
            ),
            "unknown license reference was accepted",
        )?;

        let mut duplicate_symbol = test_manifest()?;
        duplicate_symbol.grammars[1].abi_export.export_symbol = duplicate_symbol.grammars[0]
            .abi_export
            .export_symbol
            .clone();
        duplicate_symbol.grammars[1].capability_digest =
            duplicate_symbol.grammars[1].computed_capability_digest();
        require(
            matches!(
                duplicate_symbol.validate(),
                Err(OptionalParserPackManifestError::DuplicateRuntimeIdentity {
                    field: "export_symbol",
                    ..
                })
            ),
            "duplicate grammar export symbol was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn manifest_rejects_tampered_content_abi_and_registry_binding() -> Result<(), Box<dyn Error>> {
        let mut tampered_license = test_manifest()?;
        tampered_license.licenses[0].text.push_str("\ntampered");
        require(
            matches!(
                tampered_license.validate(),
                Err(OptionalParserPackManifestError::DigestMismatch {
                    field: "text_blake3",
                    ..
                })
            ),
            "tampered exact license text was accepted",
        )?;

        let mut invalid_abi = test_manifest()?;
        invalid_abi.grammars[0].abi_export.expected_abi = 16;
        invalid_abi.grammars[0].capability_digest =
            invalid_abi.grammars[0].computed_capability_digest();
        require(
            matches!(
                invalid_abi.validate(),
                Err(OptionalParserPackManifestError::AbiMismatch { .. })
            ),
            "grammar ABI outside the runtime window was accepted",
        )?;

        let mut wrong_fixture_role = test_manifest()?;
        wrong_fixture_role.grammars[0].fixtures.negative.origin =
            GrammarFixtureOrigin::UpstreamTreeSitterCorpus;
        wrong_fixture_role.grammars[0].capability_digest =
            wrong_fixture_role.grammars[0].computed_capability_digest();
        require(
            matches!(
                wrong_fixture_role.validate(),
                Err(OptionalParserPackManifestError::InvalidField {
                    field: "fixtures.origin",
                    ..
                })
            ),
            "a natural-positive origin was accepted for a negative fixture",
        )?;

        let mut stale_registry = test_manifest()?;
        stale_registry.registry.registry_version += 1;
        require(
            matches!(
                stale_registry.validate(),
                Err(OptionalParserPackManifestError::BindingMismatch {
                    field: "registry.registry_version",
                    ..
                })
            ),
            "stale language-registry binding was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn validated_loader_identities_reject_abbreviated_or_unsafe_values() {
        assert!(SourceRevision::new("6258abac").is_err());
        assert!(Sha256Digest::new("ABCDEF").is_err());
        assert!(GrammarExportSymbol::new("../tree_sitter_bad").is_err());
        assert!(GrammarLibraryStem::new("tree-sitter.dll").is_err());
        assert!(PackRelativePath::new("../artifact-manifest.json").is_err());
        assert!(PackRelativePath::new("lib\\tree_sitter_bad.dll").is_err());
        assert!(PackRelativePath::new("/absolute/path").is_err());
    }

    #[test]
    fn artifact_manifest_requires_exact_payload_and_closed_audit() -> Result<(), Box<dyn Error>> {
        let logical = test_manifest()?;
        let artifact = test_artifact(&logical, PackPlatform::WindowsX86_64)?;
        artifact.validate(&logical)?;
        require(
            artifact.files.iter().any(|file| {
                matches!(&file.role, ParserPackPayloadRole::ContainmentBroker)
                    && file.path.as_str() == "projectatlas-parser-containment.exe"
            }),
            "Windows artifact did not retain its runtime-containment broker",
        )?;
        require(
            usize::try_from(artifact.measurements.grammar_libraries)? == logical.grammars().len(),
            "artifact did not retain the exact accepted grammar count",
        )?;

        let mut wrong_construction_isolation = artifact.clone();
        wrong_construction_isolation
            .construction
            .network_denial
            .mechanism = ParserPackNetworkIsolation::WindowsAppContainer;
        require(
            wrong_construction_isolation.validate(&logical).is_err(),
            "Windows construction accepted its fresh-verification isolation mechanism",
        )?;
        let mut cross_platform_construction_isolation = artifact.clone();
        cross_platform_construction_isolation
            .construction
            .network_denial
            .mechanism = ParserPackNetworkIsolation::LinuxNetworkNamespace;
        require(
            cross_platform_construction_isolation
                .validate(&logical)
                .is_err(),
            "Windows construction accepted a Linux network namespace",
        )?;

        let mut recursive_manifest = artifact.clone();
        let worker = recursive_manifest
            .files
            .iter_mut()
            .find(|file| matches!(&file.role, ParserPackPayloadRole::Worker))
            .ok_or_else(|| io::Error::other("worker payload missing"))?;
        worker.path = PackRelativePath::new("artifact-manifest.json")?;
        recursive_manifest
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        require(
            recursive_manifest.validate(&logical).is_err(),
            "artifact manifest was allowed to list itself",
        )?;

        let mut missing_broker = artifact.clone();
        missing_broker
            .files
            .retain(|file| !matches!(&file.role, ParserPackPayloadRole::ContainmentBroker));
        missing_broker.measurements =
            ParserPackPayloadMeasurements::from_files(&missing_broker.files)?;
        require(
            missing_broker.validate(&logical).is_err(),
            "Windows artifact without its runtime-containment broker was accepted",
        )?;

        let mut forbidden_import = artifact.clone();
        forbidden_import.native_audit.forbidden_imports = 1;
        require(
            forbidden_import.validate(&logical).is_err(),
            "artifact with a forbidden native import was accepted",
        )?;

        let mut detached_audit_report = artifact.clone();
        let report = detached_audit_report
            .files
            .iter_mut()
            .find(|file| matches!(&file.role, ParserPackPayloadRole::NativeAuditReport))
            .ok_or_else(|| io::Error::other("native audit report payload missing"))?;
        report.sha256 = Sha256Digest::new(format!("{:064x}", 27))?;
        require(
            detached_audit_report.validate(&logical).is_err(),
            "artifact audit claim was allowed to detach from its packaged report",
        )?;

        let mut oversized_policy = artifact.clone();
        let policy = oversized_policy
            .files
            .iter_mut()
            .find(|file| matches!(&file.role, ParserPackPayloadRole::NativeImportPolicy))
            .ok_or_else(|| io::Error::other("native-import policy payload missing"))?;
        policy.bytes = OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_MAX_BYTES + 1;
        oversized_policy.measurements =
            ParserPackPayloadMeasurements::from_files(&oversized_policy.files)?;
        require(
            matches!(
                oversized_policy.validate(&logical),
                Err(OptionalParserPackManifestError::InvalidField { field: "bytes", .. })
            ),
            "oversized native-import policy was accepted",
        )?;

        let mut network_available = artifact;
        network_available.construction.network_denial.https_denied = false;
        require(
            network_available.validate(&logical).is_err(),
            "artifact constructed with reachable HTTPS was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn linux_artifact_rejects_a_windows_containment_broker() -> Result<(), Box<dyn Error>> {
        let logical = test_manifest()?;
        let mut artifact = test_artifact(&logical, PackPlatform::LinuxX86_64)?;
        for mechanism in [
            ParserPackNetworkIsolation::WindowsPrincipalFirewall,
            ParserPackNetworkIsolation::WindowsAppContainer,
        ] {
            let mut wrong_isolation = artifact.clone();
            wrong_isolation.construction.network_denial.mechanism = mechanism;
            require(
                wrong_isolation.validate(&logical).is_err(),
                "Linux construction accepted a Windows isolation mechanism",
            )?;
        }
        require(
            artifact
                .files
                .iter()
                .all(|file| !matches!(&file.role, ParserPackPayloadRole::ContainmentBroker)),
            "Linux test artifact unexpectedly contained a broker",
        )?;
        artifact.files.push(ParserPackPayloadFile {
            path: PackRelativePath::new("projectatlas-parser-containment.exe")?,
            role: ParserPackPayloadRole::ContainmentBroker,
            bytes: 16,
            sha256: Sha256Digest::new(format!("{:064x}", 28))?,
        });
        artifact
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        artifact.measurements = ParserPackPayloadMeasurements::from_files(&artifact.files)?;
        require(
            artifact.validate(&logical).is_err(),
            "Linux artifact accepted a Windows runtime-containment broker",
        )?;
        Ok(())
    }

    #[test]
    fn proof_aggregate_requires_one_clean_identical_platform_set() -> Result<(), Box<dyn Error>> {
        let logical = test_manifest()?;
        let platforms = PackPlatform::ALL
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, platform)| test_platform_proof(&logical, platform, ordinal))
            .collect::<Result<Vec<_>, _>>()?;
        let aggregate = OptionalParserPackProofAggregate {
            schema_version: OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION,
            pack_id: logical.pack_id().to_string(),
            projectatlas_version: logical.runtime().projectatlas_version.clone(),
            accepted_manifest_sha256: platforms[0].accepted_manifest_sha256.clone(),
            capability_set_digest: logical.capability_set_digest().clone(),
            fixture_corpus_sha256: platforms[0].fixture_corpus_sha256.clone(),
            platforms,
        };
        aggregate.validate(&logical)?;

        let mut wrong_fresh_isolation = aggregate.clone();
        wrong_fresh_isolation.platforms[1]
            .runner
            .network_denial
            .mechanism = ParserPackNetworkIsolation::WindowsPrincipalFirewall;
        require(
            wrong_fresh_isolation.validate(&logical).is_err(),
            "Windows fresh verification accepted its construction isolation mechanism",
        )?;
        let mut cross_platform_fresh_isolation = aggregate.clone();
        cross_platform_fresh_isolation.platforms[1]
            .runner
            .network_denial
            .mechanism = ParserPackNetworkIsolation::LinuxNetworkNamespace;
        require(
            cross_platform_fresh_isolation.validate(&logical).is_err(),
            "Windows fresh verification accepted a Linux network namespace",
        )?;
        for mechanism in [
            ParserPackNetworkIsolation::WindowsPrincipalFirewall,
            ParserPackNetworkIsolation::WindowsAppContainer,
        ] {
            let mut linux_wrong_isolation = aggregate.clone();
            linux_wrong_isolation.platforms[0]
                .runner
                .network_denial
                .mechanism = mechanism;
            require(
                linux_wrong_isolation.validate(&logical).is_err(),
                "Linux fresh verification accepted a Windows isolation mechanism",
            )?;
        }

        let mut failed_probe = aggregate.clone();
        failed_probe.platforms[0].grammars[0].worker_probe_passed = false;
        require(
            failed_probe.validate(&logical).is_err(),
            "aggregate accepted one failed grammar/platform probe",
        )?;

        let mut dirty_candidate = aggregate.clone();
        dirty_candidate.platforms[0].candidate.source_state = ParserPackCandidateSourceState::Dirty;
        require(
            dirty_candidate.validate(&logical).is_err(),
            "aggregate accepted a dirty candidate proof",
        )?;

        let mut false_overshoot = aggregate.clone();
        false_overshoot.platforms[0]
            .memory
            .maximum_observed_overshoot_bytes = Some(0);
        require(
            false_overshoot.validate(&logical).is_err(),
            "aggregate accepted an inconsistent sampled RSS overshoot",
        )?;

        let mut mismatched_memory_control = aggregate.clone();
        mismatched_memory_control.platforms[1].memory.control =
            ParserPackMemoryControl::LinuxCgroupV2;
        require(
            mismatched_memory_control.validate(&logical).is_err(),
            "aggregate accepted a memory control from another platform",
        )?;

        let mut impossible_windows_probe = aggregate.clone();
        impossible_windows_probe.platforms[1]
            .memory
            .process_limit_bytes =
            OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES.saturating_sub(1);
        require(
            impossible_windows_probe.validate(&logical).is_err(),
            "aggregate accepted a Windows probe below the broker's configured floor",
        )?;

        let mut missing_platform = aggregate;
        missing_platform.platforms.pop();
        require(
            missing_platform.validate(&logical).is_err(),
            "aggregate accepted a missing required platform",
        )?;
        Ok(())
    }
}
