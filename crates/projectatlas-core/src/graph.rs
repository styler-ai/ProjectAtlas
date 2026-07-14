//! Typed repository-graph identities, provenance, coverage, and publication limits.

use crate::CoreError;
#[cfg(test)]
use crate::budget::{BudgetEnforcement, DefaultCoreBudgets};
use crate::budget::{DefaultCoreBudget, DefaultCoreBudgetKind};
use crate::symbols::{ParserKind, RelationKind};
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

/// Encoding version used by newly created graph identities.
pub const GRAPH_KEY_ENCODING_VERSION: u16 = 1;

/// Maximum byte length accepted for one canonical identity scalar.
const MAX_IDENTITY_TEXT_BYTES: usize = 4_096;

/// Maximum normalized repository-path byte length accepted in graph identities.
const MAX_REPOSITORY_PATH_BYTES: usize = 16_384;

/// `BLAKE3` derivation context for entity identities.
const ENTITY_KEY_CONTEXT: &str = "projectatlas.graph.entity-key.v1";

/// `BLAKE3` derivation context for logical relation identities.
const LOGICAL_EDGE_KEY_CONTEXT: &str = "projectatlas.graph.logical-edge-key.v1";

/// `BLAKE3` derivation context for evidence-occurrence identities.
const EVIDENCE_KEY_CONTEXT: &str = "projectatlas.graph.evidence-occurrence-key.v1";

/// `BLAKE3` derivation context for non-traversable resolution occurrences.
const RESOLUTION_OCCURRENCE_KEY_CONTEXT: &str = "projectatlas.graph.resolution-occurrence-key.v1";

/// `BLAKE3` derivation context for content-anchored evidence spans.
const SPAN_FINGERPRINT_CONTEXT: &str = "projectatlas.graph.span-fingerprint.v1";

/// Errors raised while constructing typed graph contracts.
#[derive(Debug, Error)]
pub enum GraphContractError {
    /// A project instance identifier was not a nonzero 128-bit hexadecimal value.
    #[error("invalid project instance identifier: {reason}")]
    InvalidProjectInstanceId {
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// An identity scalar was empty, noncanonical, or too large.
    #[error("invalid {field}: {reason}")]
    InvalidIdentityText {
        /// Name of the invalid identity field.
        field: &'static str,
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// A repository path failed the shared `ProjectAtlas` path contract.
    #[error("invalid graph repository path: {source}")]
    InvalidRepositoryPath {
        /// Original core path-validation error.
        source: CoreError,
    },
    /// A source span has invalid byte or line bounds.
    #[error("invalid source span: {reason}")]
    InvalidSourceSpan {
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// A candidate set exceeds the hard candidate bound.
    #[error("resolution returned {actual} candidates; maximum is {maximum}")]
    CandidateLimitExceeded {
        /// Number of candidates supplied by the resolver.
        actual: usize,
        /// Maximum accepted number of retained candidates.
        maximum: usize,
    },
    /// Candidate counters or completeness disagree with retained candidates.
    #[error("invalid resolution candidate set: {reason}")]
    InvalidCandidateSet {
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// A persisted relation record would violate logical-edge invariants.
    #[error("invalid graph relation record: {reason}")]
    InvalidRelationRecord {
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// An internal target belongs to another independently initialized project.
    #[error(
        "internal graph target belongs to project {target_project}, not source project {source_project}"
    )]
    CrossProjectInternalTarget {
        /// Project owning the source entity.
        source_project: ProjectInstanceId,
        /// Project owning the rejected internal target.
        target_project: ProjectInstanceId,
    },
    /// Internal evidence belongs to another independently initialized project.
    #[error(
        "graph evidence belongs to project {origin_project}, not source project {source_project}"
    )]
    CrossProjectEvidenceOrigin {
        /// Project owning the source entity.
        source_project: ProjectInstanceId,
        /// Project owning the rejected evidence origin.
        origin_project: ProjectInstanceId,
    },
    /// A coverage record claims an internally inconsistent complete state.
    #[error("invalid graph coverage record: {reason}")]
    InvalidCoverage {
        /// Stable validation reason suitable for diagnostics.
        reason: &'static str,
    },
    /// Advancing a publication epoch would overflow its persistent scalar.
    #[error("index epoch cannot advance beyond u64::MAX")]
    IndexEpochOverflow,
}

/// Persistent random identity for one independently initialized `ProjectAtlas` database.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectInstanceId([u8; 16]);

impl ProjectInstanceId {
    /// Construct an instance identifier from 16 random bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for the all-zero value, which is reserved as invalid.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, GraphContractError> {
        if bytes == [0; 16] {
            return Err(GraphContractError::InvalidProjectInstanceId {
                reason: "the all-zero value is reserved",
            });
        }
        Ok(Self(bytes))
    }

    /// Return the canonical 16-byte identifier.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Display for ProjectInstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ProjectInstanceId {
    type Err = GraphContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

impl TryFrom<&str> for ProjectInstanceId {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() != 32 {
            return Err(GraphContractError::InvalidProjectInstanceId {
                reason: "expected exactly 32 hexadecimal characters",
            });
        }

        let mut bytes = [0_u8; 16];
        for (slot, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
            let [high, low] = pair else {
                return Err(GraphContractError::InvalidProjectInstanceId {
                    reason: "expected complete hexadecimal byte pairs",
                });
            };
            let high =
                decode_hex_nibble(*high).ok_or(GraphContractError::InvalidProjectInstanceId {
                    reason: "identifier contains a non-hexadecimal character",
                })?;
            let low =
                decode_hex_nibble(*low).ok_or(GraphContractError::InvalidProjectInstanceId {
                    reason: "identifier contains a non-hexadecimal character",
                })?;
            *slot = (high << 4) | low;
        }
        Self::from_bytes(bytes)
    }
}

impl TryFrom<String> for ProjectInstanceId {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectInstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Decode one ASCII hexadecimal digit.
fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Validated nonempty text used inside canonical graph identities.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IdentityText(String);

impl IdentityText {
    /// Return the validated scalar.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for IdentityText {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for IdentityText {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_identity_text("identity text", &value)?;
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for IdentityText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Validate a bounded canonical identity scalar.
fn validate_identity_text(field: &'static str, value: &str) -> Result<(), GraphContractError> {
    if value.is_empty() {
        return Err(GraphContractError::InvalidIdentityText {
            field,
            reason: "a value is required",
        });
    }
    if value.trim() != value {
        return Err(GraphContractError::InvalidIdentityText {
            field,
            reason: "leading or trailing whitespace is not canonical",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(GraphContractError::InvalidIdentityText {
            field,
            reason: "control characters are not allowed",
        });
    }
    if value.len() > MAX_IDENTITY_TEXT_BYTES {
        return Err(GraphContractError::InvalidIdentityText {
            field,
            reason: "value exceeds the identity-text byte limit",
        });
    }
    Ok(())
}

/// Canonical slash-separated repository-relative graph path.
///
/// Case and Unicode codepoint sequences are preserved exactly so two distinct
/// Git paths are never collapsed by platform-specific folding or normalization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryPath(String);

impl RepositoryPath {
    /// Return the normalized repository path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for RepositoryPath {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for RepositoryPath {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let normalized = crate::validated_repo_node_key(Path::new(&value))
            .map_err(|source| GraphContractError::InvalidRepositoryPath { source })?;
        validate_repository_path_length(&normalized)?;
        Ok(Self(normalized))
    }
}

/// Canonical slash-separated repository-relative file path.
///
/// Case and Unicode codepoint sequences are preserved exactly so two distinct
/// Git paths are never collapsed by platform-specific folding or normalization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryFilePath(String);

impl RepositoryFilePath {
    /// Return the normalized repository file path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for RepositoryFilePath {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for RepositoryFilePath {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let normalized = crate::validated_repo_file_key(Path::new(&value))
            .map_err(|source| GraphContractError::InvalidRepositoryPath { source })?;
        validate_repository_path_length(&normalized)?;
        Ok(Self(normalized))
    }
}

impl<'de> Deserialize<'de> for RepositoryFilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Enforce the graph identity bound after shared path normalization.
fn validate_repository_path_length(path: &str) -> Result<(), GraphContractError> {
    if path.len() > MAX_REPOSITORY_PATH_BYTES {
        return Err(GraphContractError::InvalidIdentityText {
            field: "repository path",
            reason: "path exceeds the graph identity byte limit",
        });
    }
    Ok(())
}

impl<'de> Deserialize<'de> for RepositoryPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Namespaced identity for a package, protocol endpoint, or other external target.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
pub struct ExternalIdentity {
    /// Namespace such as an ecosystem, protocol, or provider identifier.
    pub namespace: IdentityText,
    /// Canonical identity inside the namespace.
    pub value: IdentityText,
}

/// Stable categories of entities in the repository graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum GraphEntityKind {
    /// The indexed project instance.
    Repository,
    /// A repository folder.
    Folder,
    /// A repository file.
    File,
    /// A package or distribution identity.
    Package,
    /// A language module or namespace.
    Module,
    /// A declared code or manifest symbol.
    Declaration,
    /// A source-level reference occurrence identity.
    Reference,
    /// A protocol endpoint.
    Endpoint,
    /// A route identity.
    Route,
    /// A message channel or topic.
    Channel,
    /// A configuration identity.
    Configuration,
    /// An environment identity.
    Environment,
    /// An infrastructure identity.
    Infrastructure,
    /// A test identity.
    Test,
    /// An identity outside the indexed project.
    External,
}

impl GraphEntityKind {
    /// Return the stable canonical and serialized spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Folder => "folder",
            Self::File => "file",
            Self::Package => "package",
            Self::Module => "module",
            Self::Declaration => "declaration",
            Self::Reference => "reference",
            Self::Endpoint => "endpoint",
            Self::Route => "route",
            Self::Channel => "channel",
            Self::Configuration => "configuration",
            Self::Environment => "environment",
            Self::Infrastructure => "infrastructure",
            Self::Test => "test",
            Self::External => "external",
        }
    }
}

/// Kind-specific canonical selector used to construct an [`EntityKey`].
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum EntitySelector {
    /// The project instance itself.
    Repository,
    /// A folder selected by normalized repository path.
    Folder {
        /// Normalized repository path.
        path: RepositoryPath,
    },
    /// A file selected by normalized repository path.
    File {
        /// Normalized repository path.
        path: RepositoryFilePath,
    },
    /// A package selected by namespaced package identity.
    Package {
        /// Ecosystem-qualified package identity.
        identity: ExternalIdentity,
    },
    /// A module selected by path and qualified name.
    Module {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Fully qualified module name.
        qualified_name: IdentityText,
    },
    /// A declaration selected without mutable source coordinates.
    Declaration {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Fully qualified declaration name, including scope.
        qualified_name: IdentityText,
        /// Optional normalized signature or overload discriminator.
        signature: Option<IdentityText>,
    },
    /// A reference identity selected by its content-stable discriminator.
    Reference {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Fully qualified source identity when known.
        qualified_name: IdentityText,
        /// Content-stable occurrence discriminator.
        discriminator: IdentityText,
    },
    /// A protocol endpoint selected by namespaced identity.
    Endpoint {
        /// Protocol-qualified endpoint identity.
        identity: ExternalIdentity,
    },
    /// A route selected by namespaced identity.
    Route {
        /// Protocol-qualified route identity.
        identity: ExternalIdentity,
    },
    /// A message channel selected by namespaced identity.
    Channel {
        /// Broker- or protocol-qualified channel identity.
        identity: ExternalIdentity,
    },
    /// A configuration entry selected by path and optional qualified name.
    Configuration {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Optional qualified configuration identity.
        qualified_name: Option<IdentityText>,
    },
    /// An environment item selected by canonical name.
    Environment {
        /// Canonical environment identity.
        identity: IdentityText,
    },
    /// An infrastructure item selected by path and qualified name.
    Infrastructure {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Qualified infrastructure identity.
        qualified_name: IdentityText,
    },
    /// A test selected by path and qualified name.
    Test {
        /// Owning normalized repository path.
        path: RepositoryFilePath,
        /// Qualified test identity.
        qualified_name: IdentityText,
    },
    /// An out-of-project entity selected by namespaced identity.
    External {
        /// Canonical external identity.
        identity: ExternalIdentity,
    },
}

impl EntitySelector {
    /// Return the graph entity category owned by this selector.
    #[must_use]
    pub const fn entity_kind(&self) -> GraphEntityKind {
        match self {
            Self::Repository => GraphEntityKind::Repository,
            Self::Folder { .. } => GraphEntityKind::Folder,
            Self::File { .. } => GraphEntityKind::File,
            Self::Package { .. } => GraphEntityKind::Package,
            Self::Module { .. } => GraphEntityKind::Module,
            Self::Declaration { .. } => GraphEntityKind::Declaration,
            Self::Reference { .. } => GraphEntityKind::Reference,
            Self::Endpoint { .. } => GraphEntityKind::Endpoint,
            Self::Route { .. } => GraphEntityKind::Route,
            Self::Channel { .. } => GraphEntityKind::Channel,
            Self::Configuration { .. } => GraphEntityKind::Configuration,
            Self::Environment { .. } => GraphEntityKind::Environment,
            Self::Infrastructure { .. } => GraphEntityKind::Infrastructure,
            Self::Test { .. } => GraphEntityKind::Test,
            Self::External { .. } => GraphEntityKind::External,
        }
    }

    /// Append the kind-specific identity fields in their versioned canonical order.
    fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.field(self.entity_kind().as_str().as_bytes());
        match self {
            Self::Repository => {}
            Self::Folder { path } => {
                encoder.field(path.as_str().as_bytes());
            }
            Self::File { path } => {
                encoder.field(path.as_str().as_bytes());
            }
            Self::Package { identity }
            | Self::Endpoint { identity }
            | Self::Route { identity }
            | Self::Channel { identity }
            | Self::External { identity } => encode_external_identity(identity, encoder),
            Self::Module {
                path,
                qualified_name,
            }
            | Self::Infrastructure {
                path,
                qualified_name,
            }
            | Self::Test {
                path,
                qualified_name,
            } => {
                encoder.field(path.as_str().as_bytes());
                encoder.field(qualified_name.as_str().as_bytes());
            }
            Self::Declaration {
                path,
                qualified_name,
                signature,
            } => {
                encoder.field(path.as_str().as_bytes());
                encoder.field(qualified_name.as_str().as_bytes());
                encode_optional_text(signature.as_ref(), encoder);
            }
            Self::Reference {
                path,
                qualified_name,
                discriminator,
            } => {
                encoder.field(path.as_str().as_bytes());
                encoder.field(qualified_name.as_str().as_bytes());
                encoder.field(discriminator.as_str().as_bytes());
            }
            Self::Configuration {
                path,
                qualified_name,
            } => {
                encoder.field(path.as_str().as_bytes());
                encode_optional_text(qualified_name.as_ref(), encoder);
            }
            Self::Environment { identity } => encoder.field(identity.as_str().as_bytes()),
        }
    }
}

/// Append an optional identity scalar with an explicit presence field.
fn encode_optional_text(value: Option<&IdentityText>, encoder: &mut CanonicalEncoder) {
    match value {
        Some(value) => {
            encoder.field(&[1]);
            encoder.field(value.as_str().as_bytes());
        }
        None => encoder.field(&[0]),
    }
}

/// Append a namespaced external identity in canonical order.
fn encode_external_identity(identity: &ExternalIdentity, encoder: &mut CanonicalEncoder) {
    encoder.field(identity.namespace.as_str().as_bytes());
    encoder.field(identity.value.as_str().as_bytes());
}

/// Length-prefixing encoder used by every stable graph key family.
struct CanonicalEncoder {
    /// Accumulated versioned canonical identity bytes.
    bytes: Vec<u8>,
}

impl CanonicalEncoder {
    /// Start a canonical identity with its fixed-width encoding version.
    fn new() -> Self {
        Self {
            bytes: GRAPH_KEY_ENCODING_VERSION.to_be_bytes().to_vec(),
        }
    }

    /// Append one length-prefixed canonical field.
    fn field(&mut self, value: &[u8]) {
        let length = value.len() as u64;
        self.bytes.extend_from_slice(&length.to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    /// Finish the canonical identity byte sequence.
    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Shared internal material for one typed stable key.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct KeyMaterial {
    /// Stable encoding version.
    encoding_version: u16,
    /// Domain-separated digest of the canonical identity.
    digest: [u8; 32],
    /// Canonical material retained for collision checks.
    canonical_identity: Vec<u8>,
}

impl KeyMaterial {
    /// Construct current-version key material for one key domain.
    fn new(context: &'static str, canonical_identity: Vec<u8>) -> Self {
        let digest = derive_digest(context, &canonical_identity);
        Self {
            encoding_version: GRAPH_KEY_ENCODING_VERSION,
            digest,
            canonical_identity,
        }
    }
}

/// Compute a domain-separated `BLAKE3` digest.
fn derive_digest(context: &'static str, canonical_identity: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    hasher.update(canonical_identity);
    *hasher.finalize().as_bytes()
}

/// Stable versioned identity for one graph entity.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EntityKey {
    /// Project instance owning this entity.
    project: ProjectInstanceId,
    /// Versioned digest and collision material.
    material: KeyMaterial,
}

impl EntityKey {
    /// Derive an entity key from an explicit project identity and kind-specific selector.
    #[must_use]
    pub fn new(project: ProjectInstanceId, selector: &EntitySelector) -> Self {
        let mut encoder = CanonicalEncoder::new();
        encoder.field(&project.as_bytes());
        selector.encode(&mut encoder);
        Self {
            project,
            material: KeyMaterial::new(ENTITY_KEY_CONTEXT, encoder.finish()),
        }
    }

    /// Return the independently initialized project owning this entity.
    #[must_use]
    pub const fn project(&self) -> ProjectInstanceId {
        self.project
    }

    /// Return the stable encoding version.
    #[must_use]
    pub const fn encoding_version(&self) -> u16 {
        self.material.encoding_version
    }

    /// Return the domain-separated digest used for indexed lookup.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.material.digest
    }

    /// Return canonical identity material retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &[u8] {
        &self.material.canonical_identity
    }
}

/// Accepted versioned repository-graph relation families.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum GraphRelationKind {
    /// One declaration calls another target.
    Calls,
    /// One entity communicates through a topic or channel.
    Channel,
    /// Two entities change together in version-control history.
    CoChanges,
    /// One entity configures another.
    Configures,
    /// One entity contains another.
    Contains,
    /// A call-scoped federated match relates entities from explicit repositories.
    CrossRepository,
    /// One entity declares another.
    Declares,
    /// One package, module, or manifest depends on another.
    DependsOn,
    /// One infrastructure entity deploys another.
    Deploys,
    /// One module or package exports another entity.
    Exports,
    /// One entity generates another.
    Generates,
    /// One declaration implements an interface or trait contract.
    Implements,
    /// One source imports or includes another module.
    Imports,
    /// One declaration inherits from another.
    Inherits,
    /// One declaration overrides another declaration.
    Overrides,
    /// One entity reads from a configuration, environment, or data source.
    Reads,
    /// One source occurrence references another entity.
    References,
    /// One client or handler participates in an HTTP route.
    Routes,
    /// One client or handler participates in a typed RPC boundary.
    Rpc,
    /// Two entities are inferred to be structurally or semantically similar.
    Similar,
    /// One test entity exercises another entity.
    Tests,
    /// One entity writes to a configuration, environment, or data target.
    Writes,
}

impl GraphRelationKind {
    /// Complete accepted relation-family inventory in stable serialized order.
    pub const ALL: [Self; 22] = [
        Self::Calls,
        Self::Channel,
        Self::CoChanges,
        Self::Configures,
        Self::Contains,
        Self::CrossRepository,
        Self::Declares,
        Self::DependsOn,
        Self::Deploys,
        Self::Exports,
        Self::Generates,
        Self::Implements,
        Self::Imports,
        Self::Inherits,
        Self::Overrides,
        Self::Reads,
        Self::References,
        Self::Routes,
        Self::Rpc,
        Self::Similar,
        Self::Tests,
        Self::Writes,
    ];

    /// Return the stable canonical and serialized spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::Channel => "channel",
            Self::CoChanges => "co-changes",
            Self::Configures => "configures",
            Self::Contains => "contains",
            Self::CrossRepository => "cross-repository",
            Self::Declares => "declares",
            Self::DependsOn => "depends-on",
            Self::Deploys => "deploys",
            Self::Exports => "exports",
            Self::Generates => "generates",
            Self::Implements => "implements",
            Self::Imports => "imports",
            Self::Inherits => "inherits",
            Self::Overrides => "overrides",
            Self::Reads => "reads",
            Self::References => "references",
            Self::Routes => "routes",
            Self::Rpc => "rpc",
            Self::Similar => "similar",
            Self::Tests => "tests",
            Self::Writes => "writes",
        }
    }

    /// Return the source-compatible legacy relation kind.
    #[must_use]
    pub const fn legacy_kind(self) -> Option<RelationKind> {
        match self {
            Self::Contains => Some(RelationKind::Contains),
            Self::Imports => Some(RelationKind::Imports),
            Self::Calls => Some(RelationKind::Calls),
            Self::DependsOn => Some(RelationKind::DependsOn),
            Self::Channel
            | Self::CoChanges
            | Self::Configures
            | Self::CrossRepository
            | Self::Declares
            | Self::Deploys
            | Self::Exports
            | Self::Generates
            | Self::Implements
            | Self::Inherits
            | Self::Overrides
            | Self::Reads
            | Self::References
            | Self::Routes
            | Self::Rpc
            | Self::Similar
            | Self::Tests
            | Self::Writes => None,
        }
    }
}

impl From<RelationKind> for GraphRelationKind {
    fn from(value: RelationKind) -> Self {
        match value {
            RelationKind::Contains => Self::Contains,
            RelationKind::Imports => Self::Imports,
            RelationKind::Calls => Self::Calls,
            RelationKind::DependsOn => Self::DependsOn,
        }
    }
}

/// Typed target of a resolved internal or external relation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ResolutionTarget {
    /// An entity in a `ProjectAtlas` graph.
    Internal(EntityKey),
    /// A canonical identity outside the indexed graph.
    External(ExternalIdentity),
}

impl ResolutionTarget {
    /// Return whether this target is internal or external.
    #[must_use]
    pub const fn scope(&self) -> TargetScope {
        match self {
            Self::Internal(_) => TargetScope::Internal,
            Self::External(_) => TargetScope::External,
        }
    }

    /// Reject internal targets owned by another independent project instance.
    fn validate_for_source(&self, source: &EntityKey) -> Result<(), GraphContractError> {
        if let Self::Internal(target) = self
            && source.project() != target.project()
        {
            return Err(GraphContractError::CrossProjectInternalTarget {
                source_project: source.project(),
                target_project: target.project(),
            });
        }
        Ok(())
    }

    /// Compare targets by their canonical identity for deterministic storage.
    fn canonical_cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Internal(left), Self::Internal(right)) => {
                left.canonical_identity().cmp(right.canonical_identity())
            }
            (Self::Internal(_), Self::External(_)) => Ordering::Less,
            (Self::External(_), Self::Internal(_)) => Ordering::Greater,
            (Self::External(left), Self::External(right)) => left
                .namespace
                .cmp(&right.namespace)
                .then_with(|| left.value.cmp(&right.value)),
        }
    }

    /// Append this target in canonical logical-edge order.
    fn encode(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Internal(key) => {
                encoder.field(b"internal");
                encoder.field(key.canonical_identity());
            }
            Self::External(identity) => {
                encoder.field(b"external");
                encode_external_identity(identity, encoder);
            }
        }
    }
}

/// Stable versioned identity for one traversable logical relation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LogicalEdgeKey(KeyMaterial);

impl LogicalEdgeKey {
    /// Derive an edge identity without mutable source spans or line coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error when an internal target belongs to another project.
    pub fn new(
        source: &EntityKey,
        target: &ResolutionTarget,
        kind: GraphRelationKind,
    ) -> Result<Self, GraphContractError> {
        target.validate_for_source(source)?;
        let mut encoder = CanonicalEncoder::new();
        encoder.field(source.canonical_identity());
        target.encode(&mut encoder);
        encoder.field(kind.as_str().as_bytes());
        Ok(Self(KeyMaterial::new(
            LOGICAL_EDGE_KEY_CONTEXT,
            encoder.finish(),
        )))
    }

    /// Return the stable encoding version.
    #[must_use]
    pub const fn encoding_version(&self) -> u16 {
        self.0.encoding_version
    }

    /// Return the domain-separated digest used for indexed lookup.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.0.digest
    }

    /// Return canonical identity material retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &[u8] {
        &self.0.canonical_identity
    }
}

/// Content-anchored fingerprint used in evidence occurrence identities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
pub struct ContentSpanFingerprint([u8; 32]);

impl ContentSpanFingerprint {
    /// Fingerprint normalized evidence bytes without using line coordinates.
    #[must_use]
    pub fn from_content(content: &[u8]) -> Self {
        Self(derive_digest(SPAN_FINGERPRINT_CONTEXT, content))
    }

    /// Return the fingerprint bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Origin identity for a graph evidence occurrence.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceOrigin {
    /// Evidence originates from an indexed graph entity.
    Entity {
        /// Stable entity identity.
        key: EntityKey,
    },
    /// Evidence originates from a project-owned repository path.
    RepositoryPath {
        /// Project instance owning the path.
        project: ProjectInstanceId,
        /// Normalized repository path.
        path: RepositoryPath,
    },
    /// Evidence originates outside the indexed project.
    External {
        /// Namespaced external origin identity.
        identity: ExternalIdentity,
    },
}

impl EvidenceOrigin {
    /// Reject internal origins owned by another independent project instance.
    fn validate_for_source(&self, source: &EntityKey) -> Result<(), GraphContractError> {
        let origin_project = match self {
            Self::Entity { key } => Some(key.project()),
            Self::RepositoryPath { project, .. } => Some(*project),
            Self::External { .. } => None,
        };
        if let Some(origin_project) = origin_project
            && origin_project != source.project()
        {
            return Err(GraphContractError::CrossProjectEvidenceOrigin {
                source_project: source.project(),
                origin_project,
            });
        }
        Ok(())
    }

    /// Append this origin in canonical evidence-key order.
    fn encode(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Entity { key } => {
                encoder.field(b"entity");
                encoder.field(key.canonical_identity());
            }
            Self::RepositoryPath { project, path } => {
                encoder.field(b"repository-path");
                encoder.field(&project.as_bytes());
                encoder.field(path.as_str().as_bytes());
            }
            Self::External { identity } => {
                encoder.field(b"external");
                encode_external_identity(identity, encoder);
            }
        }
    }
}

/// Versioned resolver identity recorded with semantic evidence.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
pub struct ResolverIdentity {
    /// Stable resolver or provider name.
    pub name: IdentityText,
    /// Stable resolver implementation or rule-set version.
    pub version: IdentityText,
}

/// Stable versioned identity for one logical edge's source occurrence.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EvidenceOccurrenceKey(KeyMaterial);

impl EvidenceOccurrenceKey {
    /// Derive an evidence identity from content-stable occurrence inputs.
    #[must_use]
    fn new(
        edge: &LogicalEdgeKey,
        origin: &EvidenceOrigin,
        resolver: &ResolverIdentity,
        span_fingerprint: ContentSpanFingerprint,
        occurrence_discriminator: u32,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new();
        encoder.field(edge.canonical_identity());
        origin.encode(&mut encoder);
        encoder.field(resolver.name.as_str().as_bytes());
        encoder.field(resolver.version.as_str().as_bytes());
        encoder.field(&span_fingerprint.as_bytes());
        encoder.field(&occurrence_discriminator.to_be_bytes());
        Self(KeyMaterial::new(EVIDENCE_KEY_CONTEXT, encoder.finish()))
    }

    /// Return the stable encoding version.
    #[must_use]
    pub const fn encoding_version(&self) -> u16 {
        self.0.encoding_version
    }

    /// Return the domain-separated digest used for indexed lookup.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.0.digest
    }

    /// Return canonical identity material retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &[u8] {
        &self.0.canonical_identity
    }
}

/// One-based source coordinates and zero-based byte bounds for evidence display.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    /// Zero-based inclusive byte offset.
    start_byte: u64,
    /// Zero-based exclusive byte offset.
    end_byte: u64,
    /// One-based inclusive start line.
    start_line: u32,
    /// One-based inclusive end line.
    end_line: u32,
}

impl SourceSpan {
    /// Construct validated source coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error when byte bounds are reversed, either line is zero, or
    /// line bounds are reversed.
    pub fn new(
        start_byte: u64,
        end_byte: u64,
        start_line: u32,
        end_line: u32,
    ) -> Result<Self, GraphContractError> {
        if end_byte < start_byte {
            return Err(GraphContractError::InvalidSourceSpan {
                reason: "end byte precedes start byte",
            });
        }
        if start_line == 0 || end_line == 0 {
            return Err(GraphContractError::InvalidSourceSpan {
                reason: "line coordinates are one-based",
            });
        }
        if end_line < start_line {
            return Err(GraphContractError::InvalidSourceSpan {
                reason: "end line precedes start line",
            });
        }
        Ok(Self {
            start_byte,
            end_byte,
            start_line,
            end_line,
        })
    }

    /// Return the zero-based inclusive byte offset.
    #[must_use]
    pub const fn start_byte(self) -> u64 {
        self.start_byte
    }

    /// Return the zero-based exclusive byte offset.
    #[must_use]
    pub const fn end_byte(self) -> u64 {
        self.end_byte
    }

    /// Return the one-based inclusive start line.
    #[must_use]
    pub const fn start_line(self) -> u32 {
        self.start_line
    }

    /// Return the one-based inclusive end line.
    #[must_use]
    pub const fn end_line(self) -> u32 {
        self.end_line
    }
}

/// Resolution state of a semantic reference occurrence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionStatus {
    /// Exactly one accepted target was resolved.
    Resolved,
    /// More than one viable bounded candidate remains.
    Ambiguous,
    /// No accepted target was resolved.
    Unresolved,
}

/// Scope of a resolved target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetScope {
    /// Target is an indexed graph entity.
    Internal,
    /// Target is outside the indexed graph.
    External,
}

/// How relation evidence was established.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    /// Evidence comes directly from syntax, manifests, or explicit metadata.
    Direct,
    /// Evidence comes from a documented deterministic inference.
    Inferred,
}

/// Independent completeness state for bounded facts or result sets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Completeness {
    /// All facts in the declared scope are present.
    Complete,
    /// The producer could establish only part of the declared scope.
    Partial,
    /// A bounded result omitted known facts after reaching a limit.
    Truncated,
}

/// Finite confidence class for graph resolution and inference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfidenceClass {
    /// Weak evidence; consumers should present substantial uncertainty.
    Low,
    /// Useful evidence with meaningful unresolved uncertainty.
    Medium,
    /// Strong evidence with a narrowly bounded chance of error.
    High,
    /// Deterministic identity evidence under the declared resolver contract.
    Exact,
}

/// One bounded viable target retained for ambiguous resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionCandidate {
    /// Candidate target identity.
    pub target: ResolutionTarget,
    /// Candidate-specific confidence.
    pub confidence: ConfidenceClass,
    /// Optional bounded explanation or rejection context.
    pub explanation: Option<IdentityText>,
}

/// Bounded candidate page stored with an ambiguous resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSet {
    /// Retained candidates in deterministic resolver order.
    candidates: Vec<ResolutionCandidate>,
    /// Total viable candidates before the retention limit.
    total: u32,
    /// Completeness of the retained candidate page.
    completeness: Completeness,
}

impl CandidateSet {
    /// Construct a reconciled bounded candidate set.
    ///
    /// # Errors
    ///
    /// Returns an error when retained candidates exceed the hard bound, totals
    /// disagree, or completeness contradicts the counters.
    pub fn new(
        candidates: Vec<ResolutionCandidate>,
        total: u32,
        completeness: Completeness,
    ) -> Result<Self, GraphContractError> {
        Self::new_with_limit(
            candidates,
            total,
            completeness,
            DefaultCoreBudgetKind::ResolutionCandidates.default_budget(),
        )
    }

    /// Construct a candidate set under one configured typed retention limit.
    ///
    /// # Errors
    ///
    /// Returns an error when `limit` controls another resource or candidate
    /// counters, uniqueness, or completeness are inconsistent.
    pub fn new_with_limit(
        candidates: Vec<ResolutionCandidate>,
        total: u32,
        completeness: Completeness,
        limit: DefaultCoreBudget,
    ) -> Result<Self, GraphContractError> {
        if limit.kind() != DefaultCoreBudgetKind::ResolutionCandidates {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "candidate retention requires a resolution-candidates limit",
            });
        }
        // The typed hard ceiling is 64, below usize::MAX on every Rust target.
        let maximum = limit.value() as usize;
        Self::new_with_maximum(candidates, total, completeness, maximum)
    }

    /// Validate and canonically order candidates under the selected maximum.
    fn new_with_maximum(
        mut candidates: Vec<ResolutionCandidate>,
        total: u32,
        completeness: Completeness,
        maximum: usize,
    ) -> Result<Self, GraphContractError> {
        if total < 2 {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "ambiguous resolution requires at least two viable targets",
            });
        }
        if candidates.is_empty() {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "ambiguous resolution must retain at least one candidate",
            });
        }
        if candidates.len() > maximum {
            return Err(GraphContractError::CandidateLimitExceeded {
                actual: candidates.len(),
                maximum,
            });
        }
        let returned = candidates.len() as u32;
        if returned > total {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "retained candidates exceed the reported total",
            });
        }
        if completeness == Completeness::Complete && returned != total {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "complete candidate set must retain the reported total",
            });
        }
        if completeness == Completeness::Truncated && returned >= total {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "truncated candidate set must omit at least one candidate",
            });
        }
        candidates.sort_by(|left, right| left.target.canonical_cmp(&right.target));
        if candidates
            .windows(2)
            .any(|pair| pair[0].target == pair[1].target)
        {
            return Err(GraphContractError::InvalidCandidateSet {
                reason: "retained candidate target identities must be unique",
            });
        }
        Ok(Self {
            candidates,
            total,
            completeness,
        })
    }

    /// Return retained candidates in deterministic canonical target order.
    #[must_use]
    pub fn candidates(&self) -> &[ResolutionCandidate] {
        &self.candidates
    }

    /// Return the total viable candidate count.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Return candidate-page completeness.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }
}

/// Structural parser family that produced a graph fact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum ParserOriginKind {
    /// A trusted tree-sitter parser produced the fact.
    TreeSitter,
    /// A structured manifest parser produced the fact.
    Manifest,
    /// A deterministic structural adapter produced the fact.
    Structural,
    /// A conservative fallback parser produced the fact.
    Fallback,
    /// A separately installed and supervised parser pack produced the fact.
    ParserPack,
}

impl From<ParserKind> for ParserOriginKind {
    fn from(value: ParserKind) -> Self {
        match value {
            ParserKind::TreeSitter => Self::TreeSitter,
            ParserKind::Manifest => Self::Manifest,
            ParserKind::Structural => Self::Structural,
            ParserKind::Fallback => Self::Fallback,
        }
    }
}

/// Versioned parser provenance recorded with graph facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ParserOrigin {
    /// Parser family and containment boundary.
    pub kind: ParserOriginKind,
    /// Stable parser, adapter, or capability identifier.
    pub identity: IdentityText,
    /// Stable parser or adapter version.
    pub version: IdentityText,
}

/// Non-traversable resolution state retained without fabricating a target edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionOccurrenceState {
    /// Multiple viable bounded targets remain.
    Ambiguous {
        /// Reconciled bounded candidate set.
        candidates: CandidateSet,
    },
    /// No viable target was established.
    Unresolved {
        /// Optional bounded diagnostic reason.
        reason: Option<IdentityText>,
    },
}

impl ResolutionOccurrenceState {
    /// Return the non-traversable resolution status.
    #[must_use]
    pub const fn status(&self) -> ResolutionStatus {
        match self {
            Self::Ambiguous { .. } => ResolutionStatus::Ambiguous,
            Self::Unresolved { .. } => ResolutionStatus::Unresolved,
        }
    }

    /// Return bounded candidates only for an ambiguous occurrence.
    #[must_use]
    pub const fn candidates(&self) -> Option<&CandidateSet> {
        match self {
            Self::Ambiguous { candidates } => Some(candidates),
            Self::Unresolved { .. } => None,
        }
    }

    /// Return the bounded reason only for an unresolved occurrence.
    #[must_use]
    pub const fn reason(&self) -> Option<&IdentityText> {
        match self {
            Self::Ambiguous { .. } => None,
            Self::Unresolved { reason } => reason.as_ref(),
        }
    }
}

/// Stable identity for one ambiguous or unresolved source occurrence.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ResolutionOccurrenceKey(KeyMaterial);

impl ResolutionOccurrenceKey {
    /// Derive a non-traversable occurrence identity without a target or logical edge.
    fn new(
        source: &EntityKey,
        kind: GraphRelationKind,
        origin: &EvidenceOrigin,
        resolver: &ResolverIdentity,
        span_fingerprint: ContentSpanFingerprint,
        occurrence_discriminator: u32,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new();
        encoder.field(source.canonical_identity());
        encoder.field(kind.as_str().as_bytes());
        origin.encode(&mut encoder);
        encoder.field(resolver.name.as_str().as_bytes());
        encoder.field(resolver.version.as_str().as_bytes());
        encoder.field(&span_fingerprint.as_bytes());
        encoder.field(&occurrence_discriminator.to_be_bytes());
        Self(KeyMaterial::new(
            RESOLUTION_OCCURRENCE_KEY_CONTEXT,
            encoder.finish(),
        ))
    }

    /// Return the stable encoding version.
    #[must_use]
    pub const fn encoding_version(&self) -> u16 {
        self.0.encoding_version
    }

    /// Return the domain-separated digest used for indexed lookup.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.0.digest
    }

    /// Return canonical identity material retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &[u8] {
        &self.0.canonical_identity
    }
}

/// First-class ambiguous or unresolved occurrence that is never traversable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionOccurrenceRecord {
    /// Stable identity derived without a target or logical edge.
    key: ResolutionOccurrenceKey,
    /// Source entity containing the occurrence.
    source: EntityKey,
    /// Accepted legacy-compatible relation kind.
    kind: GraphRelationKind,
    /// Typed evidence origin.
    origin: EvidenceOrigin,
    /// Mutable display coordinates excluded from stable identity.
    span: Option<SourceSpan>,
    /// Resolver that produced the occurrence.
    resolver: ResolverIdentity,
    /// Content-anchored source fingerprint.
    span_fingerprint: ContentSpanFingerprint,
    /// Stable discriminator for equal occurrences in the same content span.
    occurrence_discriminator: u32,
    /// Ambiguous candidates or unresolved reason.
    state: ResolutionOccurrenceState,
    /// Direct or inferred evidence class.
    evidence_class: EvidenceClass,
    /// Finite occurrence confidence.
    confidence: ConfidenceClass,
    /// Fact completeness independent of resolution status.
    completeness: Completeness,
    /// Parser provenance that produced the occurrence.
    parser_origin: ParserOrigin,
}

impl ResolutionOccurrenceRecord {
    /// Construct a non-traversable occurrence with a target-free stable identity.
    ///
    /// # Errors
    ///
    /// Returns an error when internal evidence or an ambiguous candidate belongs
    /// to another project.
    pub fn new(
        source: EntityKey,
        kind: GraphRelationKind,
        origin: EvidenceOrigin,
        span: Option<SourceSpan>,
        resolver: ResolverIdentity,
        span_fingerprint: ContentSpanFingerprint,
        occurrence_discriminator: u32,
        state: ResolutionOccurrenceState,
        evidence_class: EvidenceClass,
        confidence: ConfidenceClass,
        completeness: Completeness,
        parser_origin: ParserOrigin,
    ) -> Result<Self, GraphContractError> {
        origin.validate_for_source(&source)?;
        if let ResolutionOccurrenceState::Ambiguous { candidates } = &state {
            for candidate in candidates.candidates() {
                candidate.target.validate_for_source(&source)?;
            }
        }
        let key = ResolutionOccurrenceKey::new(
            &source,
            kind,
            &origin,
            &resolver,
            span_fingerprint,
            occurrence_discriminator,
        );
        Ok(Self {
            key,
            source,
            kind,
            origin,
            span,
            resolver,
            span_fingerprint,
            occurrence_discriminator,
            state,
            evidence_class,
            confidence,
            completeness,
            parser_origin,
        })
    }

    /// Return the stable target-free occurrence identity.
    #[must_use]
    pub const fn key(&self) -> &ResolutionOccurrenceKey {
        &self.key
    }

    /// Return the source entity containing the occurrence.
    #[must_use]
    pub const fn source(&self) -> &EntityKey {
        &self.source
    }

    /// Return the accepted relation kind.
    #[must_use]
    pub const fn kind(&self) -> GraphRelationKind {
        self.kind
    }

    /// Return the typed occurrence origin.
    #[must_use]
    pub const fn origin(&self) -> &EvidenceOrigin {
        &self.origin
    }

    /// Return optional source coordinates used only for display.
    #[must_use]
    pub const fn span(&self) -> Option<SourceSpan> {
        self.span
    }

    /// Return the resolver that produced the occurrence.
    #[must_use]
    pub const fn resolver(&self) -> &ResolverIdentity {
        &self.resolver
    }

    /// Return the content-anchored source fingerprint.
    #[must_use]
    pub const fn span_fingerprint(&self) -> ContentSpanFingerprint {
        self.span_fingerprint
    }

    /// Return the stable same-span occurrence discriminator.
    #[must_use]
    pub const fn occurrence_discriminator(&self) -> u32 {
        self.occurrence_discriminator
    }

    /// Return the ambiguous or unresolved state.
    #[must_use]
    pub const fn state(&self) -> &ResolutionOccurrenceState {
        &self.state
    }

    /// Return whether the occurrence evidence is direct or inferred.
    #[must_use]
    pub const fn evidence_class(&self) -> EvidenceClass {
        self.evidence_class
    }

    /// Return the finite occurrence confidence.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceClass {
        self.confidence
    }

    /// Return occurrence completeness independently of resolution status.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// Return parser provenance for the occurrence.
    #[must_use]
    pub const fn parser_origin(&self) -> &ParserOrigin {
        &self.parser_origin
    }
}

/// Generation-neutral evidence occurrence attached to one exact logical relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationEvidence {
    /// Logical edge owning this occurrence.
    edge: LogicalEdgeKey,
    /// Stable occurrence identity.
    key: EvidenceOccurrenceKey,
    /// Typed origin identity.
    origin: EvidenceOrigin,
    /// Mutable display coordinates excluded from logical identity.
    span: Option<SourceSpan>,
    /// Resolver that produced the evidence.
    resolver: ResolverIdentity,
    /// Direct or inferred evidence class.
    evidence_class: EvidenceClass,
    /// Finite confidence class.
    confidence: ConfidenceClass,
    /// Fact completeness independent of resolution status.
    completeness: Completeness,
    /// Optional bounded human-facing explanation.
    explanation: Option<IdentityText>,
}

impl RelationEvidence {
    /// Construct evidence whose occurrence key is derived from its exact inputs.
    #[must_use]
    pub fn new(
        edge: &LogicalEdgeKey,
        origin: EvidenceOrigin,
        span: Option<SourceSpan>,
        resolver: ResolverIdentity,
        span_fingerprint: ContentSpanFingerprint,
        occurrence_discriminator: u32,
        evidence_class: EvidenceClass,
        confidence: ConfidenceClass,
        completeness: Completeness,
        explanation: Option<IdentityText>,
    ) -> Self {
        let key = EvidenceOccurrenceKey::new(
            edge,
            &origin,
            &resolver,
            span_fingerprint,
            occurrence_discriminator,
        );
        Self {
            edge: edge.clone(),
            key,
            origin,
            span,
            resolver,
            evidence_class,
            confidence,
            completeness,
            explanation,
        }
    }

    /// Return the exact logical edge owning this evidence.
    #[must_use]
    pub const fn edge(&self) -> &LogicalEdgeKey {
        &self.edge
    }

    /// Return the stable occurrence identity.
    #[must_use]
    pub const fn key(&self) -> &EvidenceOccurrenceKey {
        &self.key
    }

    /// Return the typed evidence origin.
    #[must_use]
    pub const fn origin(&self) -> &EvidenceOrigin {
        &self.origin
    }

    /// Return optional source coordinates used only for display.
    #[must_use]
    pub const fn span(&self) -> Option<SourceSpan> {
        self.span
    }

    /// Return the resolver that produced this evidence.
    #[must_use]
    pub const fn resolver(&self) -> &ResolverIdentity {
        &self.resolver
    }

    /// Return whether this evidence is direct or inferred.
    #[must_use]
    pub const fn evidence_class(&self) -> EvidenceClass {
        self.evidence_class
    }

    /// Return the finite evidence confidence class.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceClass {
        self.confidence
    }

    /// Return the evidence completeness independently of resolution status.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// Return the optional bounded explanation.
    #[must_use]
    pub const fn explanation(&self) -> Option<&IdentityText> {
        self.explanation.as_ref()
    }
}

/// Exactly one of the two physical structural derived-data slots.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StructuralSlot {
    /// First structural slot.
    A,
    /// Second structural slot.
    B,
}

impl StructuralSlot {
    /// Return the inactive counterpart of this slot.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

/// Monotonically increasing structural publication epoch.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct IndexEpoch(u64);

impl IndexEpoch {
    /// Initial epoch before the first derived-data publication.
    pub const INITIAL: Self = Self(0);

    /// Construct an epoch from its persistent scalar.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the persistent epoch scalar.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Return the next monotonically increasing epoch.
    ///
    /// # Errors
    ///
    /// Returns an error when the current epoch is `u64::MAX`.
    pub fn next(self) -> Result<Self, GraphContractError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(GraphContractError::IndexEpochOverflow)
    }
}

/// Atomically observed active structural slot and epoch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct PublicationState {
    /// Active structural slot visible to normal readers.
    pub active_slot: StructuralSlot,
    /// Active monotonically increasing structural epoch.
    pub active_epoch: IndexEpoch,
}

impl PublicationState {
    /// Return the state after a full publication flips to the inactive slot.
    ///
    /// # Errors
    ///
    /// Returns an error when the active epoch cannot advance.
    pub fn next_full(self) -> Result<Self, GraphContractError> {
        Ok(Self {
            active_slot: self.active_slot.other(),
            active_epoch: self.active_epoch.next()?,
        })
    }

    /// Return the state after an incremental publication advances in place.
    ///
    /// # Errors
    ///
    /// Returns an error when the active epoch cannot advance.
    pub fn next_incremental(self) -> Result<Self, GraphContractError> {
        Ok(Self {
            active_slot: self.active_slot,
            active_epoch: self.active_epoch.next()?,
        })
    }
}

/// Persistence-owned slot and freshness metadata attached after extraction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuralRowStamp {
    /// Structural slot receiving the row.
    slot: StructuralSlot,
    /// Last structural epoch that changed the row.
    last_changed_epoch: IndexEpoch,
}

impl StructuralRowStamp {
    /// Construct a persistence stamp independently from extracted graph facts.
    #[must_use]
    pub const fn new(slot: StructuralSlot, last_changed_epoch: IndexEpoch) -> Self {
        Self {
            slot,
            last_changed_epoch,
        }
    }

    /// Return the structural slot receiving the row.
    #[must_use]
    pub const fn slot(self) -> StructuralSlot {
        self.slot
    }

    /// Return the row's freshness epoch.
    #[must_use]
    pub const fn last_changed_epoch(self) -> IndexEpoch {
        self.last_changed_epoch
    }
}

/// Generation-neutral extracted graph entity with identity derived from its selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphEntityFact {
    /// Stable entity identity derived from the project and selector.
    key: EntityKey,
    /// Entity kind derived from the selector.
    kind: GraphEntityKind,
    /// Kind-specific canonical selector.
    selector: EntitySelector,
    /// Parser provenance that produced the entity.
    parser_origin: ParserOrigin,
}

impl GraphEntityFact {
    /// Construct an extracted entity fact with matching stable key and kind.
    #[must_use]
    pub fn new(
        project: ProjectInstanceId,
        selector: EntitySelector,
        parser_origin: ParserOrigin,
    ) -> Self {
        Self {
            key: EntityKey::new(project, &selector),
            kind: selector.entity_kind(),
            selector,
            parser_origin,
        }
    }

    /// Return the stable entity identity.
    #[must_use]
    pub const fn key(&self) -> &EntityKey {
        &self.key
    }

    /// Return the entity kind derived from the selector.
    #[must_use]
    pub const fn kind(&self) -> GraphEntityKind {
        self.kind
    }

    /// Return the canonical entity selector.
    #[must_use]
    pub const fn selector(&self) -> &EntitySelector {
        &self.selector
    }

    /// Return the parser provenance that produced the entity.
    #[must_use]
    pub const fn parser_origin(&self) -> &ParserOrigin {
        &self.parser_origin
    }
}

/// Generation-neutral resolved logical relation with one or more evidence occurrences.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalRelationRecord {
    /// Stable logical-edge identity derived from the record fields.
    key: LogicalEdgeKey,
    /// Stable source entity identity.
    source: EntityKey,
    /// Accepted legacy-compatible relation kind.
    kind: GraphRelationKind,
    /// Resolved internal or external target.
    target: ResolutionTarget,
    /// Confidence in the accepted target.
    confidence: ConfidenceClass,
    /// Checked evidence occurrences bound to this exact logical edge.
    evidence: Vec<RelationEvidence>,
    /// Parser provenance that produced the relation.
    parser_origin: ParserOrigin,
}

impl LogicalRelationRecord {
    /// Construct a resolved relation whose key and evidence occurrences agree.
    ///
    /// # Errors
    ///
    /// Returns an error for cross-project internal data, missing or duplicate
    /// evidence, or evidence derived from another logical edge.
    pub fn new(
        source: EntityKey,
        kind: GraphRelationKind,
        target: ResolutionTarget,
        confidence: ConfidenceClass,
        evidence: Vec<RelationEvidence>,
        parser_origin: ParserOrigin,
    ) -> Result<Self, GraphContractError> {
        let key = LogicalEdgeKey::new(&source, &target, kind)?;
        if evidence.is_empty() {
            return Err(GraphContractError::InvalidRelationRecord {
                reason: "a resolved logical relation requires at least one evidence occurrence",
            });
        }
        if evidence.iter().any(|occurrence| occurrence.edge() != &key) {
            return Err(GraphContractError::InvalidRelationRecord {
                reason: "relation evidence belongs to a different logical edge",
            });
        }
        for occurrence in &evidence {
            occurrence.origin().validate_for_source(&source)?;
        }
        if evidence.iter().enumerate().any(|(index, occurrence)| {
            evidence[..index]
                .iter()
                .any(|retained| retained.key() == occurrence.key())
        }) {
            return Err(GraphContractError::InvalidRelationRecord {
                reason: "logical relation evidence occurrence identities must be unique",
            });
        }
        Ok(Self {
            key,
            source,
            kind,
            target,
            confidence,
            evidence,
            parser_origin,
        })
    }

    /// Return the stable logical-edge identity.
    #[must_use]
    pub const fn key(&self) -> &LogicalEdgeKey {
        &self.key
    }

    /// Return the stable source entity identity.
    #[must_use]
    pub const fn source(&self) -> &EntityKey {
        &self.source
    }

    /// Return the accepted relation kind.
    #[must_use]
    pub const fn kind(&self) -> GraphRelationKind {
        self.kind
    }

    /// Return the resolved internal or external target.
    #[must_use]
    pub const fn target(&self) -> &ResolutionTarget {
        &self.target
    }

    /// Return confidence in the accepted target.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceClass {
        self.confidence
    }

    /// Return all evidence occurrences bound to this exact logical edge.
    #[must_use]
    pub fn evidence(&self) -> &[RelationEvidence] {
        &self.evidence
    }

    /// Return parser provenance for the relation.
    #[must_use]
    pub const fn parser_origin(&self) -> &ParserOrigin {
        &self.parser_origin
    }
}

/// Persisted coverage state for a structural graph scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageState {
    /// The declared scope completed without omissions.
    Complete,
    /// The declared scope produced usable but incomplete facts.
    Partial,
    /// The declared scope failed and produced no ready claim.
    Failed,
    /// The declared scope was intentionally ignored by accepted policy.
    Ignored,
    /// An input exceeded a structural size or count limit.
    Oversized,
    /// An unsafe or repeatedly failing input was isolated.
    Quarantined,
    /// Previously derived facts are known to require refresh.
    Stale,
}

impl CoverageState {
    /// Complete accepted coverage-state inventory in stable serialized order.
    pub const ALL: [Self; 7] = [
        Self::Complete,
        Self::Partial,
        Self::Failed,
        Self::Ignored,
        Self::Oversized,
        Self::Quarantined,
        Self::Stale,
    ];

    /// Return the stable canonical and serialized spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Ignored => "ignored",
            Self::Oversized => "oversized",
            Self::Quarantined => "quarantined",
            Self::Stale => "stale",
        }
    }
}

/// Closed structural scope at which graph coverage is reconciled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoverageScope {
    /// Coverage for the complete repository publication.
    Repository,
    /// Coverage for one normalized repository file.
    File {
        /// File whose structural facts are covered.
        path: RepositoryFilePath,
    },
    /// Coverage for one named structural pass over a file.
    Pass {
        /// File processed by the pass.
        path: RepositoryFilePath,
        /// Stable bounded pass identity.
        pass: IdentityText,
    },
    /// Coverage for one accepted relation kind emitted from a file.
    Relation {
        /// File owning the relation scope.
        path: RepositoryFilePath,
        /// Accepted relation kind covered by the record.
        relation: GraphRelationKind,
    },
}

/// Known or unavailable count of facts omitted from one coverage scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OmittedFactCount {
    /// The producer established an exact omitted fact count.
    Known(u64),
    /// The producer established incompleteness but could not count all omissions.
    Unknown,
}

impl OmittedFactCount {
    /// Return the exact omitted count when it is known.
    #[must_use]
    pub const fn known(self) -> Option<u64> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }

    /// Return whether the count proves that no facts were omitted.
    const fn is_known_zero(self) -> bool {
        matches!(self, Self::Known(0))
    }
}

/// Reconciled coverage counters and provenance for one structural scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRecord {
    /// Structural scope reconciled by this record.
    scope: CoverageScope,
    /// Persisted coverage state.
    state: CoverageState,
    /// Number of facts retained for the scope.
    produced: u64,
    /// Exact or unavailable number of facts omitted from the scope.
    omitted: OmittedFactCount,
    /// Structural limit reached, when applicable.
    reached_limit: Option<DefaultCoreBudgetKind>,
    /// Bounded explanation for non-complete coverage.
    reason: Option<IdentityText>,
    /// Structural slot containing this coverage record.
    slot: StructuralSlot,
    /// Last structural epoch that changed this coverage record.
    last_changed_epoch: IndexEpoch,
}

impl CoverageRecord {
    /// Construct a reconciled coverage record.
    ///
    /// # Errors
    ///
    /// Returns an error when counters and diagnostic fields contradict the
    /// selected coverage state.
    pub fn new(
        scope: CoverageScope,
        state: CoverageState,
        produced: u64,
        omitted: OmittedFactCount,
        reached_limit: Option<DefaultCoreBudgetKind>,
        reason: Option<IdentityText>,
        slot: StructuralSlot,
        last_changed_epoch: IndexEpoch,
    ) -> Result<Self, GraphContractError> {
        match state {
            CoverageState::Complete => {
                if !omitted.is_known_zero() || reached_limit.is_some() || reason.is_some() {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "complete coverage cannot report omissions, a reached limit, or a reason",
                    });
                }
            }
            CoverageState::Partial => {
                if omitted.is_known_zero() || reason.is_none() {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "partial coverage must omit facts and include a reason",
                    });
                }
            }
            CoverageState::Failed => {
                if produced != 0 || reason.is_none() {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "failed coverage must produce no ready facts and include a reason",
                    });
                }
            }
            CoverageState::Ignored => {
                if produced != 0
                    || !omitted.is_known_zero()
                    || reached_limit.is_some()
                    || reason.is_none()
                {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "ignored coverage must have zero counters, no reached limit, and a policy reason",
                    });
                }
            }
            CoverageState::Oversized => {
                if produced != 0
                    || omitted.is_known_zero()
                    || reached_limit.is_none()
                    || reason.is_none()
                {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "oversized coverage must omit facts, produce none, identify the reached limit, and include a reason",
                    });
                }
            }
            CoverageState::Quarantined => {
                if produced != 0 || reached_limit.is_some() || reason.is_none() {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "quarantined coverage must produce no facts, report no limit, and include a reason",
                    });
                }
            }
            CoverageState::Stale => {
                if reached_limit.is_some() || reason.is_none() {
                    return Err(GraphContractError::InvalidCoverage {
                        reason: "stale coverage must report no reached limit and include a refresh reason",
                    });
                }
            }
        }
        Ok(Self {
            scope,
            state,
            produced,
            omitted,
            reached_limit,
            reason,
            slot,
            last_changed_epoch,
        })
    }

    /// Return the structural scope reconciled by this record.
    #[must_use]
    pub const fn scope(&self) -> &CoverageScope {
        &self.scope
    }

    /// Return the persisted coverage state.
    #[must_use]
    pub const fn state(&self) -> CoverageState {
        self.state
    }

    /// Return the number of retained facts.
    #[must_use]
    pub const fn produced(&self) -> u64 {
        self.produced
    }

    /// Return the exact or unavailable number of omitted facts.
    #[must_use]
    pub const fn omitted(&self) -> OmittedFactCount {
        self.omitted
    }

    /// Return the structural limit reached, when known.
    #[must_use]
    pub const fn reached_limit(&self) -> Option<DefaultCoreBudgetKind> {
        self.reached_limit
    }

    /// Return the bounded non-complete explanation.
    #[must_use]
    pub const fn reason(&self) -> Option<&IdentityText> {
        self.reason.as_ref()
    }

    /// Return the structural slot containing this record.
    #[must_use]
    pub const fn slot(&self) -> StructuralSlot {
        self.slot
    }

    /// Return the last epoch that changed this record.
    #[must_use]
    pub const fn last_changed_epoch(&self) -> IndexEpoch {
        self.last_changed_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the fixed test project identity.
    fn project() -> Result<ProjectInstanceId, GraphContractError> {
        ProjectInstanceId::try_from("00112233445566778899aabbccddeeff")
    }

    /// Build validated identity text for concise tests.
    fn text(value: &str) -> Result<IdentityText, GraphContractError> {
        IdentityText::try_from(value)
    }

    /// Build a declaration selector for identity tests.
    fn declaration(
        qualified_name: &str,
        signature: &str,
    ) -> Result<EntitySelector, GraphContractError> {
        Ok(EntitySelector::Declaration {
            path: RepositoryFilePath::try_from("src/lib.rs")?,
            qualified_name: text(qualified_name)?,
            signature: Some(text(signature)?),
        })
    }

    /// Build fixed parser provenance for graph record tests.
    fn parser_origin() -> Result<ParserOrigin, GraphContractError> {
        Ok(ParserOrigin {
            kind: ParserOriginKind::TreeSitter,
            identity: text("rust")?,
            version: text("1")?,
        })
    }

    /// Return a test error instead of asserting inside a fallible test.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(message.into())
        }
    }

    /// Stable entity bytes and digest are frozen by the versioned encoding.
    #[test]
    fn entity_key_has_stable_canonical_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let key = EntityKey::new(project()?, &declaration("crate::run", "fn(i32)->bool")?);
        let expected = [
            0, 1, 0, 0, 0, 0, 0, 0, 0, 16, 0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187,
            204, 221, 238, 255, 0, 0, 0, 0, 0, 0, 0, 11, 100, 101, 99, 108, 97, 114, 97, 116, 105,
            111, 110, 0, 0, 0, 0, 0, 0, 0, 10, 115, 114, 99, 47, 108, 105, 98, 46, 114, 115, 0, 0,
            0, 0, 0, 0, 0, 10, 99, 114, 97, 116, 101, 58, 58, 114, 117, 110, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 0, 0, 0, 0, 0, 0, 0, 13, 102, 110, 40, 105, 51, 50, 41, 45, 62, 98, 111, 111,
            108,
        ];
        require(
            key.encoding_version() == GRAPH_KEY_ENCODING_VERSION,
            "entity key encoding version drifted",
        )?;
        require(
            key.canonical_identity() == expected,
            "entity key canonical bytes drifted",
        )?;
        require(
            key.digest()
                == &[
                    29, 36, 108, 80, 22, 101, 185, 47, 68, 230, 222, 44, 101, 197, 71, 213, 66,
                    157, 194, 166, 52, 239, 186, 134, 189, 67, 82, 57, 201, 200, 135, 97,
                ],
            "entity key digest drifted",
        )?;
        Ok(())
    }

    /// Entity identity excludes mutable lines, spans, and formatting.
    #[test]
    fn declaration_identity_is_line_and_format_independent()
    -> Result<(), Box<dyn std::error::Error>> {
        let selector = declaration("crate::run", "fn(i32)->bool")?;
        let before = EntityKey::new(project()?, &selector);
        let after = EntityKey::new(project()?, &selector);
        let old_span = SourceSpan::new(10, 40, 2, 3)?;
        let moved_span = SourceSpan::new(1_000, 1_070, 80, 84)?;

        require(old_span != moved_span, "test spans must differ")?;
        require(before == after, "line movement changed stable identity")?;
        Ok(())
    }

    /// Qualified scope and signature distinguish duplicate short names and overloads.
    #[test]
    fn declaration_identity_distinguishes_scopes_and_overloads()
    -> Result<(), Box<dyn std::error::Error>> {
        let project = project()?;
        let left = EntityKey::new(project, &declaration("left::run", "fn(i32)")?);
        let right = EntityKey::new(project, &declaration("right::run", "fn(i32)")?);
        let overload = EntityKey::new(project, &declaration("left::run", "fn(&str)")?);
        let independent_project = ProjectInstanceId::try_from("ffeeddccbbaa99887766554433221100")?;
        let clone_key = EntityKey::new(independent_project, &declaration("left::run", "fn(i32)")?);

        require(left != right, "scope did not disambiguate declaration")?;
        require(left != overload, "signature did not disambiguate overload")?;
        require(
            left != clone_key,
            "independent project identities collapsed",
        )?;
        Ok(())
    }

    /// Public graph enums keep stable kebab-case serialized spellings.
    #[test]
    fn graph_enums_have_stable_serialization() -> Result<(), Box<dyn std::error::Error>> {
        require(
            serde_json::to_string(&GraphRelationKind::DependsOn)? == "\"depends-on\"",
            "graph relation serialization drifted",
        )?;
        require(
            serde_json::to_string(&GraphEntityKind::Infrastructure)? == "\"infrastructure\"",
            "graph entity serialization drifted",
        )?;
        require(
            serde_json::to_string(&ResolutionStatus::Unresolved)? == "\"unresolved\"",
            "resolution serialization drifted",
        )?;
        require(
            serde_json::to_string(&StructuralSlot::A)? == "\"a\"",
            "structural slot serialization drifted",
        )?;
        require(
            serde_json::to_string(&EvidenceClass::Inferred)? == "\"inferred\"",
            "evidence class serialization drifted",
        )?;
        require(
            serde_json::to_string(&ConfidenceClass::Exact)? == "\"exact\"",
            "confidence serialization drifted",
        )?;
        require(
            serde_json::to_string(&Completeness::Truncated)? == "\"truncated\"",
            "completeness serialization drifted",
        )?;
        require(
            serde_json::to_string(&CoverageState::Quarantined)? == "\"quarantined\"",
            "coverage serialization drifted",
        )?;
        require(
            serde_json::to_string(&ParserOriginKind::ParserPack)? == "\"parser-pack\"",
            "parser origin serialization drifted",
        )?;
        Ok(())
    }

    /// Invalid IDs, identity text, paths, spans, and limits fail validation.
    #[test]
    fn invalid_identity_and_limit_inputs_are_rejected() {
        assert!(ProjectInstanceId::try_from("00").is_err());
        assert!(ProjectInstanceId::from_bytes([0; 16]).is_err());
        assert!(IdentityText::try_from(" trailing ").is_err());
        assert!(RepositoryPath::try_from("../outside.rs").is_err());
        assert!(SourceSpan::new(4, 3, 1, 1).is_err());
        assert!(DefaultCoreBudget::new(DefaultCoreBudgetKind::WorkerCount, 0,).is_err());
        assert!(DefaultCoreBudget::new(DefaultCoreBudgetKind::WorkerCount, 17,).is_err());
    }

    /// File paths normalize syntax while preserving Git path codepoints and case.
    #[test]
    fn repository_file_paths_have_explicit_canonicalization_and_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let canonical = RepositoryFilePath::try_from("src/Grüße.rs")?;
        let dotted = RepositoryFilePath::try_from("./src/./Grüße.rs")?;
        let separated = RepositoryFilePath::try_from("src\\Grüße.rs")?;
        let lower_case = RepositoryFilePath::try_from("src/grüße.rs")?;
        let composed = RepositoryFilePath::try_from("src/café.rs")?;
        let decomposed = RepositoryFilePath::try_from("src/cafe\u{301}.rs")?;
        let cjk = RepositoryFilePath::try_from("src/索引.rs")?;
        let emoji = RepositoryFilePath::try_from("src/atlas-🗺.rs")?;
        let restored = RepositoryFilePath::try_from(canonical.as_str())?;

        require(canonical == dotted, "dot component changed file identity")?;
        require(canonical == separated, "separator changed file identity")?;
        require(
            canonical == restored,
            "validated file path did not round-trip",
        )?;
        require(
            canonical != lower_case,
            "case-sensitive repository identities collapsed",
        )?;
        require(
            composed != decomposed,
            "distinct Git path codepoint sequences were normalized together",
        )?;
        require(
            RepositoryFilePath::try_from(cjk.as_str())? == cjk
                && RepositoryFilePath::try_from(emoji.as_str())? == emoji,
            "Unicode repository paths did not round-trip",
        )?;
        let maximum_path = format!(
            "{}.rs",
            "a".repeat(MAX_REPOSITORY_PATH_BYTES.saturating_sub(3))
        );
        let oversized_path = format!(
            "{}.rs",
            "a".repeat(MAX_REPOSITORY_PATH_BYTES.saturating_sub(2))
        );
        require(
            RepositoryFilePath::try_from(maximum_path).is_ok(),
            "maximum-length repository path was rejected",
        )?;
        require(
            RepositoryFilePath::try_from(oversized_path).is_err(),
            "oversized repository path was accepted",
        )?;
        require(
            RepositoryPath::try_from(".")?.as_str() == ".",
            "repository root path was rejected",
        )?;
        require(
            RepositoryFilePath::try_from(".").is_err(),
            "repository root was accepted as a file",
        )?;
        require(
            ProjectInstanceId::try_from("00112233445566778899AABBCCDDEEFF")?.to_string()
                == "00112233445566778899aabbccddeeff",
            "project identifier did not render canonically",
        )?;
        Ok(())
    }

    /// File-owned selectors reject root paths that remain valid for folders.
    #[test]
    fn file_owned_selectors_cannot_encode_repository_root() -> Result<(), Box<dyn std::error::Error>>
    {
        let folder = EntitySelector::Folder {
            path: RepositoryPath::try_from(".")?,
        };
        require(
            folder.entity_kind() == GraphEntityKind::Folder,
            "root folder selector changed kind",
        )?;
        require(
            RepositoryFilePath::try_from("././").is_err(),
            "noncanonical root was accepted as a file",
        )?;
        Ok(())
    }

    /// Full publication flips slots while incremental publication remains in place.
    #[test]
    fn publication_state_has_two_slot_epoch_semantics() -> Result<(), Box<dyn std::error::Error>> {
        let initial = PublicationState {
            active_slot: StructuralSlot::A,
            active_epoch: IndexEpoch::new(7),
        };
        let full = initial.next_full()?;
        let incremental = full.next_incremental()?;

        require(
            full.active_slot == StructuralSlot::B,
            "full publication did not switch slots",
        )?;
        require(
            full.active_epoch == IndexEpoch::new(8),
            "full publication did not advance epoch",
        )?;
        require(
            incremental.active_slot == StructuralSlot::B,
            "incremental publication changed slots",
        )?;
        require(
            incremental.active_epoch == IndexEpoch::new(9),
            "incremental publication did not advance epoch",
        )?;
        require(
            IndexEpoch::new(u64::MAX).next().is_err(),
            "epoch overflow was accepted",
        )?;
        Ok(())
    }

    /// Rich relation kinds preserve all four exhaustive legacy variants.
    #[test]
    fn graph_relations_preserve_legacy_compatibility() {
        let legacy = [
            RelationKind::Contains,
            RelationKind::Imports,
            RelationKind::Calls,
            RelationKind::DependsOn,
        ];
        for relation in legacy {
            let graph = GraphRelationKind::from(relation);
            assert_eq!(graph.legacy_kind(), Some(relation));
            assert_eq!(graph.as_str(), relation.to_string());
        }
    }

    /// Logical edges deduplicate call sites while evidence keys retain occurrences.
    #[test]
    fn edge_and_evidence_identity_separate_relation_from_occurrence()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = EntityKey::new(project()?, &declaration("crate::source", "fn()")?);
        let target_key = EntityKey::new(project()?, &declaration("crate::target", "fn()")?);
        let target = ResolutionTarget::Internal(target_key);
        let edge = LogicalEdgeKey::new(&source, &target, GraphRelationKind::Calls)?;
        let resolver = ResolverIdentity {
            name: text("rust-name-resolution")?,
            version: text("1")?,
        };
        let origin = EvidenceOrigin::Entity {
            key: source.clone(),
        };
        let fingerprint = ContentSpanFingerprint::from_content(b"target()");
        let first = EvidenceOccurrenceKey::new(&edge, &origin, &resolver, fingerprint, 0);
        let second = EvidenceOccurrenceKey::new(&edge, &origin, &resolver, fingerprint, 1);
        let repeated = LogicalEdgeKey::new(&source, &target, GraphRelationKind::Calls)?;

        require(edge == repeated, "logical edge identity was not stable")?;
        require(first != second, "evidence occurrences collapsed")?;
        require(
            edge.digest() != first.digest(),
            "edge and evidence domains were not separated",
        )?;
        require(
            (
                edge.digest(),
                first.digest(),
                edge.canonical_identity().len(),
                first.canonical_identity().len(),
            ) == (
                &[
                    210, 95, 32, 5, 100, 159, 154, 117, 191, 12, 9, 251, 227, 82, 66, 12, 233, 13,
                    40, 49, 64, 106, 171, 23, 248, 72, 255, 97, 13, 245, 103, 14,
                ],
                &[
                    49, 38, 97, 24, 102, 82, 188, 230, 126, 245, 133, 88, 1, 176, 177, 106, 2, 112,
                    136, 30, 200, 27, 159, 131, 1, 33, 127, 24, 214, 176, 228, 99,
                ],
                257,
                483,
            ),
            "logical-edge and evidence key golden contract drifted",
        )?;
        Ok(())
    }

    /// Extraction facts are unstamped and one resolved edge owns multiple occurrences.
    #[test]
    fn resolved_graph_facts_are_generation_neutral_and_project_scoped()
    -> Result<(), Box<dyn std::error::Error>> {
        let project = project()?;
        let source_selector = declaration("crate::source", "fn()")?;
        let source_fact = GraphEntityFact::new(project, source_selector.clone(), parser_origin()?);
        let source = EntityKey::new(project, &source_selector);
        let target = ResolutionTarget::Internal(EntityKey::new(
            project,
            &declaration("crate::target", "fn()")?,
        ));
        let edge = LogicalEdgeKey::new(&source, &target, GraphRelationKind::Calls)?;
        let resolver = ResolverIdentity {
            name: text("rust-name-resolution")?,
            version: text("1")?,
        };
        let origin = EvidenceOrigin::Entity {
            key: source.clone(),
        };
        let fingerprint = ContentSpanFingerprint::from_content(b"target()");
        let first = RelationEvidence::new(
            &edge,
            origin.clone(),
            Some(SourceSpan::new(10, 18, 2, 2)?),
            resolver.clone(),
            fingerprint,
            0,
            EvidenceClass::Direct,
            ConfidenceClass::Exact,
            Completeness::Complete,
            None,
        );
        let second = RelationEvidence::new(
            &edge,
            origin,
            Some(SourceSpan::new(30, 38, 4, 4)?),
            resolver,
            fingerprint,
            1,
            EvidenceClass::Direct,
            ConfidenceClass::High,
            Completeness::Complete,
            None,
        );
        let relation = LogicalRelationRecord::new(
            source.clone(),
            GraphRelationKind::Calls,
            target.clone(),
            ConfidenceClass::Exact,
            vec![first.clone(), second],
            parser_origin()?,
        )?;
        let stamp = StructuralRowStamp::new(StructuralSlot::A, IndexEpoch::new(7));

        require(source_fact.key() == &source, "entity fact key drifted")?;
        require(
            source_fact.kind() == GraphEntityKind::Declaration,
            "entity fact kind drifted",
        )?;
        require(relation.evidence().len() == 2, "logical evidence collapsed")?;
        require(relation.key() == &edge, "logical relation key drifted")?;
        require(
            stamp.slot() == StructuralSlot::A && stamp.last_changed_epoch() == IndexEpoch::new(7),
            "persistence stamp drifted",
        )?;
        require(
            LogicalRelationRecord::new(
                source.clone(),
                GraphRelationKind::Calls,
                target,
                ConfidenceClass::Exact,
                Vec::new(),
                parser_origin()?,
            )
            .is_err(),
            "resolved relation without evidence was accepted",
        )?;

        let foreign_project = ProjectInstanceId::try_from("ffeeddccbbaa99887766554433221100")?;
        let foreign_target = ResolutionTarget::Internal(EntityKey::new(
            foreign_project,
            &declaration("crate::target", "fn()")?,
        ));
        require(
            LogicalRelationRecord::new(
                source,
                GraphRelationKind::Calls,
                foreign_target,
                ConfidenceClass::Exact,
                vec![first],
                parser_origin()?,
            )
            .is_err(),
            "cross-project internal target was accepted",
        )?;
        Ok(())
    }

    /// Relation records reject foreign internal evidence and duplicate occurrences.
    #[test]
    fn relation_records_enforce_evidence_project_and_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let project = project()?;
        let foreign_project = ProjectInstanceId::try_from("ffeeddccbbaa99887766554433221100")?;
        let source = EntityKey::new(project, &declaration("crate::source", "fn()")?);
        let target = ResolutionTarget::Internal(EntityKey::new(
            project,
            &declaration("crate::target", "fn()")?,
        ));
        let edge = LogicalEdgeKey::new(&source, &target, GraphRelationKind::Calls)?;
        let resolver = ResolverIdentity {
            name: text("rust-name-resolution")?,
            version: text("1")?,
        };
        let fingerprint = ContentSpanFingerprint::from_content(b"target()");
        let evidence_for = |origin, discriminator| {
            RelationEvidence::new(
                &edge,
                origin,
                None,
                resolver.clone(),
                fingerprint,
                discriminator,
                EvidenceClass::Direct,
                ConfidenceClass::Exact,
                Completeness::Complete,
                None,
            )
        };
        let local = evidence_for(
            EvidenceOrigin::Entity {
                key: source.clone(),
            },
            0,
        );
        require(
            LogicalRelationRecord::new(
                source.clone(),
                GraphRelationKind::Calls,
                target.clone(),
                ConfidenceClass::Exact,
                vec![local.clone(), local],
                parser_origin()?,
            )
            .is_err(),
            "duplicate evidence occurrence identity was accepted",
        )?;

        let foreign_entity_origin = EvidenceOrigin::Entity {
            key: EntityKey::new(foreign_project, &declaration("crate::foreign", "fn()")?),
        };
        require(
            LogicalRelationRecord::new(
                source.clone(),
                GraphRelationKind::Calls,
                target.clone(),
                ConfidenceClass::Exact,
                vec![evidence_for(foreign_entity_origin.clone(), 1)],
                parser_origin()?,
            )
            .is_err(),
            "foreign entity evidence origin was accepted",
        )?;
        require(
            LogicalRelationRecord::new(
                source.clone(),
                GraphRelationKind::Calls,
                target.clone(),
                ConfidenceClass::Exact,
                vec![evidence_for(
                    EvidenceOrigin::RepositoryPath {
                        project: foreign_project,
                        path: RepositoryPath::try_from("src/lib.rs")?,
                    },
                    2,
                )],
                parser_origin()?,
            )
            .is_err(),
            "foreign repository-path evidence origin was accepted",
        )?;
        require(
            LogicalRelationRecord::new(
                source.clone(),
                GraphRelationKind::Calls,
                target,
                ConfidenceClass::Exact,
                vec![evidence_for(
                    EvidenceOrigin::External {
                        identity: ExternalIdentity {
                            namespace: text("protocol")?,
                            value: text("service")?,
                        },
                    },
                    3,
                )],
                parser_origin()?,
            )
            .is_ok(),
            "typed external evidence origin was rejected",
        )?;
        require(
            ResolutionOccurrenceRecord::new(
                source,
                GraphRelationKind::Calls,
                foreign_entity_origin,
                None,
                resolver,
                fingerprint,
                0,
                ResolutionOccurrenceState::Unresolved {
                    reason: Some(text("unresolved")?),
                },
                EvidenceClass::Direct,
                ConfidenceClass::Low,
                Completeness::Complete,
                parser_origin()?,
            )
            .is_err(),
            "foreign non-traversable evidence origin was accepted",
        )?;
        Ok(())
    }

    /// Non-traversable state metadata cannot churn source-occurrence identity.
    #[test]
    fn non_traversable_occurrences_never_fabricate_logical_edges()
    -> Result<(), Box<dyn std::error::Error>> {
        let project = project()?;
        let source = EntityKey::new(project, &declaration("crate::source", "fn()")?);
        let first_target = ResolutionTarget::Internal(EntityKey::new(
            project,
            &declaration("crate::first", "fn()")?,
        ));
        let second_target = ResolutionTarget::Internal(EntityKey::new(
            project,
            &declaration("crate::second", "fn()")?,
        ));
        let candidates = CandidateSet::new(
            vec![
                ResolutionCandidate {
                    target: first_target.clone(),
                    confidence: ConfidenceClass::High,
                    explanation: None,
                },
                ResolutionCandidate {
                    target: second_target.clone(),
                    confidence: ConfidenceClass::Medium,
                    explanation: None,
                },
            ],
            2,
            Completeness::Complete,
        )?;
        let origin = EvidenceOrigin::Entity {
            key: source.clone(),
        };
        let resolver = ResolverIdentity {
            name: text("rust-name-resolution")?,
            version: text("1")?,
        };
        let fingerprint = ContentSpanFingerprint::from_content(b"dynamic()");
        let ambiguous = ResolutionOccurrenceRecord::new(
            source.clone(),
            GraphRelationKind::Calls,
            origin.clone(),
            None,
            resolver.clone(),
            fingerprint,
            0,
            ResolutionOccurrenceState::Ambiguous { candidates },
            EvidenceClass::Direct,
            ConfidenceClass::Medium,
            Completeness::Complete,
            parser_origin()?,
        )?;
        let alternate_candidates = CandidateSet::new(
            vec![
                ResolutionCandidate {
                    target: second_target,
                    confidence: ConfidenceClass::Exact,
                    explanation: Some(IdentityText::try_from("x".repeat(MAX_IDENTITY_TEXT_BYTES))?),
                },
                ResolutionCandidate {
                    target: first_target,
                    confidence: ConfidenceClass::Low,
                    explanation: Some(text("different diagnostic")?),
                },
            ],
            2,
            Completeness::Complete,
        )?;
        let alternate_metadata = ResolutionOccurrenceRecord::new(
            source.clone(),
            GraphRelationKind::Calls,
            origin.clone(),
            None,
            resolver.clone(),
            fingerprint,
            0,
            ResolutionOccurrenceState::Ambiguous {
                candidates: alternate_candidates,
            },
            EvidenceClass::Inferred,
            ConfidenceClass::Low,
            Completeness::Partial,
            parser_origin()?,
        )?;
        let next_occurrence = ResolutionOccurrenceRecord::new(
            source.clone(),
            GraphRelationKind::Calls,
            origin.clone(),
            None,
            resolver.clone(),
            fingerprint,
            1,
            ambiguous.state().clone(),
            EvidenceClass::Direct,
            ConfidenceClass::Medium,
            Completeness::Complete,
            parser_origin()?,
        )?;
        let unresolved = ResolutionOccurrenceRecord::new(
            source.clone(),
            GraphRelationKind::Calls,
            origin.clone(),
            None,
            resolver.clone(),
            fingerprint,
            0,
            ResolutionOccurrenceState::Unresolved {
                reason: Some(text("dynamic dispatch")?),
            },
            EvidenceClass::Direct,
            ConfidenceClass::Low,
            Completeness::Complete,
            parser_origin()?,
        )?;

        require(
            ambiguous.state().status() == ResolutionStatus::Ambiguous,
            "ambiguous occurrence status drifted",
        )?;
        require(
            unresolved.state().status() == ResolutionStatus::Unresolved,
            "unresolved occurrence status drifted",
        )?;
        require(
            ambiguous.key() == unresolved.key(),
            "resolution state metadata changed source-occurrence identity",
        )?;
        require(
            ambiguous.key() == alternate_metadata.key()
                && ambiguous.key().canonical_identity().len()
                    == alternate_metadata.key().canonical_identity().len(),
            "candidate ordering or metadata changed source-occurrence identity",
        )?;
        require(
            ambiguous.key() != next_occurrence.key(),
            "occurrence discriminator did not change stable identity",
        )?;
        require(
            (
                ambiguous.key().digest(),
                ambiguous.key().canonical_identity().len(),
            ) == (
                &[
                    238, 34, 200, 210, 96, 41, 240, 167, 103, 86, 42, 199, 207, 89, 50, 13, 99, 87,
                    144, 251, 242, 166, 59, 241, 142, 121, 179, 204, 73, 161, 250, 117,
                ],
                344,
            ),
            "resolution occurrence key golden contract drifted",
        )?;
        let foreign_project = ProjectInstanceId::try_from("ffeeddccbbaa99887766554433221100")?;
        let cross_project_candidates = CandidateSet::new(
            vec![
                ResolutionCandidate {
                    target: ResolutionTarget::Internal(EntityKey::new(
                        project,
                        &declaration("crate::local", "fn()")?,
                    )),
                    confidence: ConfidenceClass::Medium,
                    explanation: None,
                },
                ResolutionCandidate {
                    target: ResolutionTarget::Internal(EntityKey::new(
                        foreign_project,
                        &declaration("crate::foreign", "fn()")?,
                    )),
                    confidence: ConfidenceClass::Medium,
                    explanation: None,
                },
            ],
            2,
            Completeness::Complete,
        )?;
        require(
            ResolutionOccurrenceRecord::new(
                source,
                GraphRelationKind::Calls,
                origin,
                None,
                resolver,
                fingerprint,
                0,
                ResolutionOccurrenceState::Ambiguous {
                    candidates: cross_project_candidates,
                },
                EvidenceClass::Direct,
                ConfidenceClass::Medium,
                Completeness::Complete,
                parser_origin()?,
            )
            .is_err(),
            "cross-project ambiguous candidate was accepted",
        )?;
        Ok(())
    }

    /// Stable keys are reconstructed from validated domain inputs rather than wire bytes.
    #[test]
    fn stable_key_reconstruction_uses_canonical_domain_inputs()
    -> Result<(), Box<dyn std::error::Error>> {
        let selector = declaration("crate::run", "fn()")?;
        let first = EntityKey::new(project()?, &selector);
        let reconstructed = EntityKey::new(project()?, &selector);

        require(
            first == reconstructed,
            "canonical key reconstruction drifted",
        )?;
        require(
            first.project() == project()?
                && !first.canonical_identity().is_empty()
                && first.encoding_version() == GRAPH_KEY_ENCODING_VERSION,
            "stable key ownership or collision material drifted",
        )?;
        Ok(())
    }

    /// Candidate bounds and traversal semantics remain orthogonal to confidence.
    #[test]
    fn resolution_candidates_are_bounded_and_non_traversable()
    -> Result<(), Box<dyn std::error::Error>> {
        let target = ResolutionTarget::Internal(EntityKey::new(
            project()?,
            &declaration("crate::run", "fn()")?,
        ));
        let candidate = ResolutionCandidate {
            target,
            confidence: ConfidenceClass::High,
            explanation: None,
        };
        let other = ResolutionCandidate {
            target: ResolutionTarget::Internal(EntityKey::new(
                project()?,
                &declaration("crate::other", "fn()")?,
            )),
            confidence: ConfidenceClass::Medium,
            explanation: None,
        };
        let candidates = CandidateSet::new(vec![candidate.clone()], 2, Completeness::Truncated)?;
        let state = ResolutionOccurrenceState::Ambiguous { candidates };
        let canonical = CandidateSet::new(
            vec![candidate.clone(), other.clone()],
            2,
            Completeness::Complete,
        )?;
        let reversed = CandidateSet::new(
            vec![other.clone(), candidate.clone()],
            2,
            Completeness::Complete,
        )?;

        require(
            state.status() == ResolutionStatus::Ambiguous,
            "ambiguous resolution status drifted",
        )?;
        require(
            state.candidates().is_some() && state.reason().is_none(),
            "ambiguous resolution exposed an invalid state view",
        )?;
        require(
            CandidateSet::new(Vec::new(), 1, Completeness::Complete).is_err(),
            "single-target ambiguous set was accepted",
        )?;
        require(
            CandidateSet::new(Vec::new(), 2, Completeness::Truncated).is_err(),
            "empty ambiguous candidate page was accepted",
        )?;
        require(
            canonical == reversed,
            "candidate storage order depended on resolver insertion order",
        )?;
        let configured_limit =
            DefaultCoreBudget::new(DefaultCoreBudgetKind::ResolutionCandidates, 1)?;
        require(
            CandidateSet::new_with_limit(
                vec![candidate.clone()],
                2,
                Completeness::Truncated,
                configured_limit,
            )
            .is_ok(),
            "configured candidate limit rejected a bounded page",
        )?;
        require(
            CandidateSet::new_with_limit(
                vec![candidate.clone(), other],
                2,
                Completeness::Complete,
                configured_limit,
            )
            .is_err(),
            "configured candidate limit accepted an oversized page",
        )?;
        require(
            CandidateSet::new_with_limit(
                vec![candidate.clone()],
                2,
                Completeness::Truncated,
                DefaultCoreBudget::new(DefaultCoreBudgetKind::WorkerCount, 1)?,
            )
            .is_err(),
            "non-candidate structural limit was accepted for candidates",
        )?;
        require(
            CandidateSet::new(
                vec![candidate.clone(), candidate],
                2,
                Completeness::Complete,
            )
            .is_err(),
            "duplicate target identities were accepted",
        )?;
        Ok(())
    }

    /// Coverage and default-core budget inventories reconcile their hard contracts.
    #[test]
    fn coverage_and_default_core_budgets_are_reconciled() -> Result<(), Box<dyn std::error::Error>>
    {
        let limits =
            DefaultCoreBudgets::default().with_budget(DefaultCoreBudgetKind::WorkerCount, 8)?;
        require(
            limits.as_slice().len() == DefaultCoreBudgetKind::ALL.len(),
            "default-core budget inventory count drifted",
        )?;
        require(
            DefaultCoreBudgetKind::ALL
                .iter()
                .zip(DefaultCoreBudgets::default().as_slice())
                .all(|(kind, limit)| {
                    *kind == limit.kind()
                        && kind.hard_ceiling() == limit.value()
                        && limit.enforcement() == kind.default_enforcement()
                }),
            "default enforcement drifted from the budget inventory",
        )?;
        let worker_budget = limits.get(DefaultCoreBudgetKind::WorkerCount);
        require(
            worker_budget.kind() == DefaultCoreBudgetKind::WorkerCount
                && worker_budget.value() == 8
                && worker_budget.enforcement() == BudgetEnforcement::Advisory,
            "default-core budget replacement failed",
        )?;
        require(
            CoverageRecord::new(
                CoverageScope::Repository,
                CoverageState::Complete,
                2,
                OmittedFactCount::Known(1),
                Some(DefaultCoreBudgetKind::RelationsPerFile),
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            )
            .is_err(),
            "inconsistent complete coverage was accepted",
        )?;
        Ok(())
    }

    /// Every coverage state enforces its own counter and diagnostic contract.
    #[test]
    fn coverage_states_reject_inconsistent_records() -> Result<(), Box<dyn std::error::Error>> {
        let scope = CoverageScope::File {
            path: RepositoryFilePath::try_from("src/lib.rs")?,
        };
        let reason = text("bounded diagnostic")?;
        let valid = [
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Complete,
                2,
                OmittedFactCount::Known(0),
                None,
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                2,
                OmittedFactCount::Unknown,
                Some(DefaultCoreBudgetKind::RelationsPerFile),
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                2,
                OmittedFactCount::Known(1),
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Failed,
                0,
                OmittedFactCount::Unknown,
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Ignored,
                0,
                OmittedFactCount::Known(0),
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Oversized,
                0,
                OmittedFactCount::Known(1),
                Some(DefaultCoreBudgetKind::SourceFileBytes),
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Quarantined,
                0,
                OmittedFactCount::Unknown,
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Stale,
                2,
                OmittedFactCount::Known(0),
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
        ];
        require(
            valid.iter().all(Result::is_ok),
            "a valid state-specific coverage record was rejected",
        )?;

        let invalid = [
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Complete,
                1,
                OmittedFactCount::Known(1),
                None,
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                1,
                OmittedFactCount::Known(0),
                None,
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                1,
                OmittedFactCount::Unknown,
                Some(DefaultCoreBudgetKind::RelationsPerFile),
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                1,
                OmittedFactCount::Known(1),
                None,
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Partial,
                1,
                OmittedFactCount::Known(0),
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Failed,
                1,
                OmittedFactCount::Known(0),
                None,
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Ignored,
                0,
                OmittedFactCount::Known(0),
                Some(DefaultCoreBudgetKind::SourceFileBytes),
                Some(reason.clone()),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Oversized,
                0,
                OmittedFactCount::Known(0),
                Some(DefaultCoreBudgetKind::SourceFileBytes),
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Oversized,
                0,
                OmittedFactCount::Unknown,
                Some(DefaultCoreBudgetKind::SourceFileBytes),
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope.clone(),
                CoverageState::Quarantined,
                0,
                OmittedFactCount::Known(0),
                Some(DefaultCoreBudgetKind::SourceFileBytes),
                Some(reason),
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
            CoverageRecord::new(
                scope,
                CoverageState::Stale,
                1,
                OmittedFactCount::Known(0),
                None,
                None,
                StructuralSlot::A,
                IndexEpoch::new(1),
            ),
        ];
        require(
            invalid.iter().all(Result::is_err),
            "an inconsistent state-specific coverage record was accepted",
        )?;
        require(
            OmittedFactCount::Known(3).known() == Some(3)
                && OmittedFactCount::Unknown.known().is_none(),
            "known and unknown omission counts collapsed",
        )?;
        Ok(())
    }
}
