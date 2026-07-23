//! Typed, project-qualified repository graph contracts.

use crate::symbols::{RelationKind, SymbolKind};
use crate::{CoreError, IndexGeneration, validated_repo_file_key, validated_repo_node_key};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU32;
use std::path::Path;
use thiserror::Error;

/// Canonical identity namespace for repository graph entities.
const ENTITY_KEY_DOMAIN: &str = "projectatlas.graph.entity.v1";
/// Canonical identity namespace for logical repository relationships.
const RELATION_KEY_DOMAIN: &str = "projectatlas.graph.relation.v1";
/// Canonical identity namespace for repository resolution keys.
const RESOLUTION_KEY_DOMAIN: &str = "projectatlas.graph.resolution.v1";
/// Largest accepted identity component in bytes.
const MAX_IDENTITY_BYTES: usize = 4_096;

/// Failure while constructing or reconciling typed graph contracts.
#[derive(Debug, Error)]
pub enum GraphContractError {
    /// A project instance identifier was malformed or used the zero sentinel.
    #[error("invalid project instance identifier: {reason}")]
    InvalidProjectInstanceId {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
    /// An identity component was blank, padded, too large, or contained control data.
    #[error("invalid graph identity text: {reason}")]
    InvalidIdentityText {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
    /// An existing repository path validator rejected a graph path.
    #[error(transparent)]
    InvalidRepositoryPath(#[from] CoreError),
    /// A persisted stable key digest did not match its canonical identity.
    #[error("stable graph key digest does not match its canonical identity")]
    InvalidStableKeyDigest,
    /// A persisted canonical resolution-key domain was not supported.
    #[error("unsupported canonical resolution-key domain")]
    InvalidResolutionKeyDomain,
    /// Two distinct canonical identities claimed the same compact key.
    #[error("stable graph key collision for digest {digest}")]
    StableKeyCollision {
        /// Compact digest that mapped to conflicting canonical material.
        digest: String,
    },
    /// A persisted entity key did not retain its project-qualified prefix.
    #[error("stable entity key is not qualified by its declared project")]
    ProjectQualificationMismatch,
    /// A canonical resolution key and its graph owner belonged to different projects.
    #[error("canonical resolution key belongs to a different project than its graph owner")]
    ResolutionKeyOwnerMismatch,
    /// A resolved relationship crossed project identity without federation.
    #[error("resolved graph relation target belongs to another project")]
    CrossProjectRelation,
    /// Related graph records did not belong to one complete publication.
    #[error("graph generation mismatch for {context}")]
    GenerationMismatch {
        /// Record relationship whose generations disagreed.
        context: &'static str,
    },
    /// A derived graph record claimed the pre-publication zero generation.
    #[error("graph records require a complete nonzero publication generation")]
    InvalidGeneration,
    /// A resolution retained a target or selector that did not identify one entity.
    #[error("invalid graph relation resolution: {reason}")]
    InvalidResolution {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
    /// A source span used an invalid line or ordering.
    #[error("invalid source span: {reason}")]
    InvalidSourceSpan {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
    /// Coverage state and counts disagreed.
    #[error("invalid graph coverage: {reason}")]
    InvalidCoverage {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
    /// A query budget was zero or exceeded the absolute product ceiling.
    #[error("invalid graph limits: {reason}")]
    InvalidLimits {
        /// Stable explanation suitable for diagnostics.
        reason: &'static str,
    },
}

/// Stable identity of one `ProjectAtlas` index across supported moves and upgrades.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectInstanceId([u8; 16]);

impl ProjectInstanceId {
    /// Construct a project identity from its durable 16-byte representation.
    ///
    /// # Errors
    ///
    /// Returns an error for the all-zero sentinel.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, GraphContractError> {
        if bytes == [0; 16] {
            return Err(GraphContractError::InvalidProjectInstanceId {
                reason: "the zero identifier is reserved",
            });
        }
        Ok(Self(bytes))
    }

    /// Return the durable binary representation.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }

    /// Return the canonical lowercase hexadecimal representation.
    #[must_use]
    pub fn as_hex(self) -> String {
        encode_hex(&self.0)
    }
}

impl fmt::Display for ProjectInstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.as_hex())
    }
}

impl TryFrom<&str> for ProjectInstanceId {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let compact = match value.len() {
            32 => value.to_string(),
            36 if value.as_bytes().get(8) == Some(&b'-')
                && value.as_bytes().get(13) == Some(&b'-')
                && value.as_bytes().get(18) == Some(&b'-')
                && value.as_bytes().get(23) == Some(&b'-') =>
            {
                value
                    .chars()
                    .filter(|character| *character != '-')
                    .collect()
            }
            _ => {
                return Err(GraphContractError::InvalidProjectInstanceId {
                    reason: "expected 32 hexadecimal digits or a hyphenated UUID",
                });
            }
        };
        if compact.len() != 32 {
            return Err(GraphContractError::InvalidProjectInstanceId {
                reason: "hyphenated identifier must contain exactly 32 hexadecimal digits",
            });
        }
        let mut bytes = [0_u8; 16];
        for (index, pair) in compact.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex(pair[0]).ok_or(GraphContractError::InvalidProjectInstanceId {
                reason: "identifier contains a non-hexadecimal digit",
            })?;
            let low = decode_hex(pair[1]).ok_or(GraphContractError::InvalidProjectInstanceId {
                reason: "identifier contains a non-hexadecimal digit",
            })?;
            bytes[index] = (high << 4) | low;
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

impl Serialize for ProjectInstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.as_hex())
    }
}

impl<'de> Deserialize<'de> for ProjectInstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

/// Validated non-empty, unpadded identity component used by graph selectors.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GraphIdentityText(String);

impl GraphIdentityText {
    /// Validate one graph identity component.
    ///
    /// # Errors
    ///
    /// Returns an error for blank, padded, oversized, or control-bearing text.
    pub fn new(value: impl Into<String>) -> Result<Self, GraphContractError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(GraphContractError::InvalidIdentityText {
                reason: "identity text must not be empty",
            });
        }
        if value.trim() != value {
            return Err(GraphContractError::InvalidIdentityText {
                reason: "identity text must not contain surrounding whitespace",
            });
        }
        if value.len() > MAX_IDENTITY_BYTES {
            return Err(GraphContractError::InvalidIdentityText {
                reason: "identity text exceeds the byte limit",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(GraphContractError::InvalidIdentityText {
                reason: "identity text contains control characters",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the validated value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GraphIdentityText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for GraphIdentityText {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for GraphIdentityText {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<GraphIdentityText> for String {
    fn from(value: GraphIdentityText) -> Self {
        value.0
    }
}

/// Normalized repository-relative node path, including the project root `.`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepositoryNodePath(String);

impl RepositoryNodePath {
    /// Validate and normalize a repository node path through the shared validator.
    ///
    /// # Errors
    ///
    /// Returns an error for absolute, parent-traversing, empty, or non-UTF-8 paths.
    pub fn new(path: &Path) -> Result<Self, GraphContractError> {
        Ok(Self(validated_repo_node_key(path)?))
    }

    /// Borrow the normalized slash-separated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepositoryNodePath {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(Path::new(&value))
    }
}

impl From<RepositoryNodePath> for String {
    fn from(value: RepositoryNodePath) -> Self {
        value.0
    }
}

/// Normalized repository-relative file path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepositoryFilePath(String);

impl RepositoryFilePath {
    /// Validate and normalize a repository file path through the shared validator.
    ///
    /// # Errors
    ///
    /// Returns an error for root, absolute, parent-traversing, empty, or non-UTF-8 paths.
    pub fn new(path: &Path) -> Result<Self, GraphContractError> {
        Ok(Self(validated_repo_file_key(path)?))
    }

    /// Borrow the normalized slash-separated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepositoryFilePath {
    type Error = GraphContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(Path::new(&value))
    }
}

impl From<RepositoryFilePath> for String {
    fn from(value: RepositoryFilePath) -> Self {
        value.0
    }
}

/// Package identity scoped by manager, name, and owning manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PackageSelector {
    /// Package ecosystem or manifest family.
    pub manager: GraphIdentityText,
    /// Package name as declared by its manifest.
    pub name: GraphIdentityText,
    /// Repository-local manifest that owns the package.
    pub manifest: RepositoryFilePath,
}

/// Declaration identity stable across source line movement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SymbolSelector {
    /// Repository-local source file containing the declaration.
    pub file: RepositoryFilePath,
    /// Declaration name.
    pub name: GraphIdentityText,
    /// Existing `ProjectAtlas` symbol kind.
    pub kind: SymbolKind,
    /// Optional containing symbol or namespace.
    pub parent: Option<GraphIdentityText>,
    /// Normalized declaration signature that distinguishes overloads.
    pub signature: GraphIdentityText,
}

/// External identity retained without fabricating a local target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExternalSelector {
    /// External namespace such as a package ecosystem or protocol.
    pub system: GraphIdentityText,
    /// Identity within the external namespace.
    pub identity: GraphIdentityText,
}

/// Closed entity selectors owned by the repository graph domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntitySelector {
    /// The selected project instance itself.
    Project,
    /// One repository folder.
    Folder {
        /// Normalized repository-relative folder path.
        path: RepositoryNodePath,
    },
    /// One repository file.
    File {
        /// Normalized repository-relative file path.
        path: RepositoryFilePath,
    },
    /// One package owned by a manifest.
    Package {
        /// Typed package identity.
        package: PackageSelector,
    },
    /// One declaration stable across line movement.
    Symbol {
        /// Typed declaration identity.
        symbol: SymbolSelector,
    },
    /// One target outside the selected local project.
    External {
        /// Typed external identity.
        external: ExternalSelector,
    },
}

/// Persisted digest plus the canonical material needed to detect collisions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StableKeyWire", into = "StableKeyWire")]
struct StableKey {
    /// Lowercase BLAKE3 digest.
    digest: String,
    /// Unambiguous canonical identity that produced the digest.
    canonical_identity: String,
}

impl StableKey {
    /// Derive a compact key while retaining its canonical collision witness.
    fn new(canonical_identity: String) -> Self {
        let digest = blake3::hash(canonical_identity.as_bytes())
            .to_hex()
            .as_str()
            .to_owned();
        Self {
            digest,
            canonical_identity,
        }
    }

    /// Validate persisted key material.
    fn from_persisted(
        digest: &str,
        canonical_identity: String,
    ) -> Result<Self, GraphContractError> {
        let key = Self::new(canonical_identity);
        if key.digest != digest {
            return Err(GraphContractError::InvalidStableKeyDigest);
        }
        Ok(key)
    }

    /// Compare compact keys without silently accepting a digest collision.
    fn reconcile(&self, other: &Self) -> Result<bool, GraphContractError> {
        if self.digest != other.digest {
            return Ok(false);
        }
        if self.canonical_identity != other.canonical_identity {
            return Err(GraphContractError::StableKeyCollision {
                digest: self.digest.clone(),
            });
        }
        Ok(true)
    }

    /// Decode the validated digest into its compact binary representation.
    fn digest_bytes(&self) -> Result<[u8; 32], GraphContractError> {
        let Ok(digest) = blake3::Hash::from_hex(&self.digest) else {
            return Err(GraphContractError::InvalidStableKeyDigest);
        };
        Ok(*digest.as_bytes())
    }
}

/// Serializable stable-key representation with validation on input.
#[derive(Deserialize, Serialize)]
struct StableKeyWire {
    /// Lowercase BLAKE3 digest.
    digest: String,
    /// Canonical collision witness.
    canonical_identity: String,
}

impl TryFrom<StableKeyWire> for StableKey {
    type Error = GraphContractError;

    fn try_from(value: StableKeyWire) -> Result<Self, Self::Error> {
        Self::from_persisted(&value.digest, value.canonical_identity)
    }
}

impl From<StableKey> for StableKeyWire {
    fn from(value: StableKey) -> Self {
        Self {
            digest: value.digest,
            canonical_identity: value.canonical_identity,
        }
    }
}

/// Stable project-qualified key for one graph entity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GraphEntityKeyWire", into = "GraphEntityKeyWire")]
pub struct GraphEntityKey {
    /// Project instance that owns this key.
    project: ProjectInstanceId,
    /// Compact digest and retained canonical identity.
    stable: StableKey,
}

impl GraphEntityKey {
    /// Derive a stable key from a project and typed selector.
    #[must_use]
    pub fn new(project: ProjectInstanceId, selector: &EntitySelector) -> Self {
        let canonical_identity = entity_canonical_identity(project, selector);
        Self {
            project,
            stable: StableKey::new(canonical_identity),
        }
    }

    /// Return the owning project instance.
    #[must_use]
    pub const fn project(&self) -> ProjectInstanceId {
        self.project
    }

    /// Borrow the lowercase compact digest.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.stable.digest
    }

    /// Return the compact binary digest used by normalized persistence.
    ///
    /// # Errors
    ///
    /// Returns [`GraphContractError::InvalidStableKeyDigest`] if the private key
    /// invariant was violated by incompatible persisted input.
    pub fn digest_bytes(&self) -> Result<[u8; 32], GraphContractError> {
        self.stable.digest_bytes()
    }

    /// Borrow the canonical identity retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &str {
        &self.stable.canonical_identity
    }

    /// Determine whether two keys identify the same entity, failing on collision.
    ///
    /// # Errors
    ///
    /// Returns [`GraphContractError::StableKeyCollision`] when equal digests retain
    /// different canonical identities.
    pub fn reconcile(&self, other: &Self) -> Result<bool, GraphContractError> {
        if self.project != other.project {
            return Ok(false);
        }
        self.stable.reconcile(&other.stable)
    }
}

/// Validated serialized entity-key representation.
#[derive(Deserialize, Serialize)]
struct GraphEntityKeyWire {
    /// Owning project instance.
    project: ProjectInstanceId,
    /// Stable key material.
    stable: StableKey,
}

impl TryFrom<GraphEntityKeyWire> for GraphEntityKey {
    type Error = GraphContractError;

    fn try_from(value: GraphEntityKeyWire) -> Result<Self, Self::Error> {
        let prefix = entity_project_prefix(value.project);
        if !has_canonical_prefix(&value.stable.canonical_identity, &prefix) {
            return Err(GraphContractError::ProjectQualificationMismatch);
        }
        Ok(Self {
            project: value.project,
            stable: value.stable,
        })
    }
}

impl From<GraphEntityKey> for GraphEntityKeyWire {
    fn from(value: GraphEntityKey) -> Self {
        Self {
            project: value.project,
            stable: value.stable,
        }
    }
}

/// One typed graph entity at a complete index generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GraphEntity {
    /// Stable project-qualified key.
    key: GraphEntityKey,
    /// Typed identity material used to derive the key.
    selector: EntitySelector,
    /// Complete generation containing this entity.
    generation: IndexGeneration,
}

impl GraphEntity {
    /// Construct an entity and derive its stable key.
    ///
    /// # Errors
    ///
    /// Returns an error for the pre-publication zero generation.
    pub fn new(
        project: ProjectInstanceId,
        selector: EntitySelector,
        generation: IndexGeneration,
    ) -> Result<Self, GraphContractError> {
        if generation == IndexGeneration::ZERO {
            return Err(GraphContractError::InvalidGeneration);
        }
        let key = GraphEntityKey::new(project, &selector);
        Ok(Self {
            key,
            selector,
            generation,
        })
    }

    /// Borrow the stable project-qualified key.
    #[must_use]
    pub const fn key(&self) -> &GraphEntityKey {
        &self.key
    }

    /// Borrow the typed identity selector.
    #[must_use]
    pub const fn selector(&self) -> &EntitySelector {
        &self.selector
    }

    /// Return the complete generation containing this entity.
    #[must_use]
    pub const fn generation(&self) -> IndexGeneration {
        self.generation
    }
}

/// Additive graph-only relation families.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtendedRelationKind {
    /// A source references a declaration or resource.
    References,
    /// A test exercises a source target.
    Tests,
    /// A route or protocol entry reaches a handler.
    RoutesTo,
    /// Configuration selects or controls a target.
    Configures,
    /// A source performs a bounded static read.
    Reads,
    /// A source performs a bounded static write.
    Writes,
}

/// Relation family that preserves the existing exhaustive legacy enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "scope", content = "value", rename_all = "snake_case")]
pub enum GraphRelationKind {
    /// Existing `ProjectAtlas` relation value and payload spelling.
    Legacy(RelationKind),
    /// Additive repository-wide graph relation.
    Extended(ExtendedRelationKind),
}

impl GraphRelationKind {
    /// Wrap one legacy relation without changing its enum.
    #[must_use]
    pub const fn from_legacy(kind: RelationKind) -> Self {
        Self::Legacy(kind)
    }

    /// Return the old projection when this is an existing relation family.
    #[must_use]
    pub const fn legacy_kind(self) -> Option<RelationKind> {
        match self {
            Self::Legacy(kind) => Some(kind),
            Self::Extended(_) => None,
        }
    }

    /// Return the stable canonical relation spelling.
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::Legacy(RelationKind::Contains) => "legacy:contains",
            Self::Legacy(RelationKind::Imports) => "legacy:imports",
            Self::Legacy(RelationKind::Calls) => "legacy:calls",
            Self::Legacy(RelationKind::DependsOn) => "legacy:depends-on",
            Self::Extended(ExtendedRelationKind::References) => "extended:references",
            Self::Extended(ExtendedRelationKind::Tests) => "extended:tests",
            Self::Extended(ExtendedRelationKind::RoutesTo) => "extended:routes-to",
            Self::Extended(ExtendedRelationKind::Configures) => "extended:configures",
            Self::Extended(ExtendedRelationKind::Reads) => "extended:reads",
            Self::Extended(ExtendedRelationKind::Writes) => "extended:writes",
        }
    }
}

/// One exact source range using one-based lines and zero-based columns.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SourceSpanWire", into = "SourceSpanWire")]
pub struct SourceSpan {
    /// First one-based source line.
    start_line: u32,
    /// First zero-based source column.
    start_column: u32,
    /// Last one-based source line.
    end_line: u32,
    /// Exclusive zero-based end column.
    end_column: u32,
}

impl SourceSpan {
    /// Validate a non-reversed source range.
    ///
    /// # Errors
    ///
    /// Returns an error for zero lines or an end before the start.
    pub fn new(
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Result<Self, GraphContractError> {
        if start_line == 0 || end_line == 0 {
            return Err(GraphContractError::InvalidSourceSpan {
                reason: "source lines are one-based",
            });
        }
        if (end_line, end_column) < (start_line, start_column) {
            return Err(GraphContractError::InvalidSourceSpan {
                reason: "source span end precedes its start",
            });
        }
        Ok(Self {
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }

    /// Return the first one-based line.
    #[must_use]
    pub const fn start_line(self) -> u32 {
        self.start_line
    }

    /// Return the first zero-based column.
    #[must_use]
    pub const fn start_column(self) -> u32 {
        self.start_column
    }

    /// Return the last one-based line.
    #[must_use]
    pub const fn end_line(self) -> u32 {
        self.end_line
    }

    /// Return the exclusive zero-based end column.
    #[must_use]
    pub const fn end_column(self) -> u32 {
        self.end_column
    }
}

/// Source-span wire shape validated during deserialization.
#[derive(Deserialize, Serialize)]
struct SourceSpanWire {
    /// First one-based line.
    start_line: u32,
    /// First zero-based column.
    start_column: u32,
    /// Last one-based line.
    end_line: u32,
    /// Exclusive zero-based end column.
    end_column: u32,
}

impl TryFrom<SourceSpanWire> for SourceSpan {
    type Error = GraphContractError;

    fn try_from(value: SourceSpanWire) -> Result<Self, Self::Error> {
        Self::new(
            value.start_line,
            value.start_column,
            value.end_line,
            value.end_column,
        )
    }
}

impl From<SourceSpan> for SourceSpanWire {
    fn from(value: SourceSpan) -> Self {
        Self {
            start_line: value.start_line,
            start_column: value.start_column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}

/// Exact local selectors reusable by summary, relation, and source-slice adapters.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReusableTargetSelector {
    /// Select one repository folder.
    Folder {
        /// Exact normalized folder path.
        folder: RepositoryNodePath,
    },
    /// Select one repository file.
    File {
        /// Exact normalized file path.
        file: RepositoryFilePath,
    },
    /// Select one package through its exact manifest-owned identity.
    Package {
        /// Exact package identity whose manifest is directly reusable by file calls.
        package: PackageSelector,
    },
    /// Select one declaration without depending on its current line.
    Symbol {
        /// Stable declaration identity.
        symbol: SymbolSelector,
    },
}

impl ReusableTargetSelector {
    /// Derive the exact reusable selector for one navigable local entity.
    fn for_entity(target: &GraphEntity) -> Result<Self, GraphContractError> {
        match &target.selector {
            EntitySelector::Project => Err(GraphContractError::InvalidResolution {
                reason: "the project aggregate is not a direct source target",
            }),
            EntitySelector::Folder { path } => Ok(Self::Folder {
                folder: path.clone(),
            }),
            EntitySelector::File { path } => Ok(Self::File { file: path.clone() }),
            EntitySelector::Package { package } => Ok(Self::Package {
                package: package.clone(),
            }),
            EntitySelector::Symbol { symbol } => Ok(Self::Symbol {
                symbol: symbol.clone(),
            }),
            EntitySelector::External { .. } => Err(GraphContractError::InvalidResolution {
                reason: "external entities do not have a local reusable selector",
            }),
        }
    }

    /// Return the entity identity selected by this navigation target.
    fn entity_selector(&self) -> EntitySelector {
        match self {
            Self::Folder { folder } => EntitySelector::Folder {
                path: folder.clone(),
            },
            Self::File { file } => EntitySelector::File { path: file.clone() },
            Self::Package { package } => EntitySelector::Package {
                package: package.clone(),
            },
            Self::Symbol { symbol } => EntitySelector::Symbol {
                symbol: symbol.clone(),
            },
        }
    }
}

/// Relationship resolution state and optional exact local jump target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RelationResolution {
    /// Exactly one local entity was resolved.
    Resolved {
        /// Stable target entity.
        target: GraphEntityKey,
        /// Exact selector accepted by a later summary, relation, or slice adapter.
        selector: ReusableTargetSelector,
        /// Complete generation containing the resolved target.
        generation: IndexGeneration,
    },
    /// More than one statically valid target remains.
    Ambiguous {
        /// Original normalized reference text.
        reference: GraphIdentityText,
        /// Number of retained valid candidates before result limits.
        candidates: NonZeroU32,
    },
    /// No supported static target was found.
    Unresolved {
        /// Original normalized reference text.
        reference: GraphIdentityText,
    },
    /// The target is intentionally outside the selected local project.
    External {
        /// Project-qualified external entity key.
        target: GraphEntityKey,
        /// Typed external identity retained for fail-closed validation.
        external: ExternalSelector,
        /// Complete generation containing the external entity record.
        generation: IndexGeneration,
    },
}

impl RelationResolution {
    /// Construct one exact local resolution from a graph entity.
    ///
    /// # Errors
    ///
    /// Returns an error when the target is not a navigable local entity.
    pub fn resolved(target: &GraphEntity) -> Result<Self, GraphContractError> {
        Ok(Self::Resolved {
            target: target.key.clone(),
            selector: ReusableTargetSelector::for_entity(target)?,
            generation: target.generation,
        })
    }

    /// Construct one external resolution from a graph entity record.
    ///
    /// # Errors
    ///
    /// Returns an error when the target is not an external entity record.
    pub fn external(target: &GraphEntity) -> Result<Self, GraphContractError> {
        let EntitySelector::External { external } = &target.selector else {
            return Err(GraphContractError::InvalidResolution {
                reason: "external resolution requires an external entity",
            });
        };
        Ok(Self::External {
            target: target.key.clone(),
            external: external.clone(),
            generation: target.generation,
        })
    }

    /// Return the exact traversable local target, if one exists.
    #[must_use]
    pub const fn resolved_target(&self) -> Option<&GraphEntityKey> {
        match self {
            Self::Resolved { target, .. } => Some(target),
            Self::Ambiguous { .. } | Self::Unresolved { .. } | Self::External { .. } => None,
        }
    }

    /// Return any project-qualified target key retained by the resolution.
    const fn target_key(&self) -> Option<&GraphEntityKey> {
        match self {
            Self::Resolved { target, .. } | Self::External { target, .. } => Some(target),
            Self::Ambiguous { .. } | Self::Unresolved { .. } => None,
        }
    }

    /// Return the generation of a retained target entity, when one exists.
    const fn target_generation(&self) -> Option<IndexGeneration> {
        match self {
            Self::Resolved { generation, .. } | Self::External { generation, .. } => {
                Some(*generation)
            }
            Self::Ambiguous { .. } | Self::Unresolved { .. } => None,
        }
    }

    /// Append stable resolution material to a canonical relation identity.
    fn append_canonical(&self, canonical: &mut String) {
        match self {
            Self::Resolved { target, .. } => {
                append_canonical_field(canonical, "resolved");
                append_canonical_field(canonical, target.canonical_identity());
            }
            Self::Ambiguous { reference, .. } => {
                append_canonical_field(canonical, "ambiguous");
                append_canonical_field(canonical, reference.as_str());
            }
            Self::Unresolved { reference } => {
                append_canonical_field(canonical, "unresolved");
                append_canonical_field(canonical, reference.as_str());
            }
            Self::External { target, .. } => {
                append_canonical_field(canonical, "external");
                append_canonical_field(canonical, target.canonical_identity());
            }
        }
    }
}

/// Coarse confidence that avoids unsupported numeric precision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceClass {
    /// Direct language or manifest semantics establish the fact.
    Exact,
    /// Strong deterministic evidence supports the fact.
    High,
    /// Multiple conservative signals support the fact.
    Medium,
    /// The fact is useful only as a weak candidate.
    Low,
}

/// Whether the producer observed all supported facts for its bounded scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// Every supported fact in the scope was considered.
    Complete,
    /// A declared limit or unsupported region omitted some facts.
    Partial,
}

/// Stable project-qualified key for a deduplicated logical relation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LogicalRelationKeyWire", into = "LogicalRelationKeyWire")]
pub struct LogicalRelationKey {
    /// Project instance that owns this relation.
    project: ProjectInstanceId,
    /// Compact digest and retained canonical identity.
    stable: StableKey,
}

/// Validated serialized logical-relation key representation.
#[derive(Deserialize, Serialize)]
struct LogicalRelationKeyWire {
    /// Owning project instance.
    project: ProjectInstanceId,
    /// Stable relation-key material.
    stable: StableKey,
}

impl TryFrom<LogicalRelationKeyWire> for LogicalRelationKey {
    type Error = GraphContractError;

    fn try_from(value: LogicalRelationKeyWire) -> Result<Self, Self::Error> {
        let prefix = relation_project_prefix(value.project);
        if !has_canonical_prefix(&value.stable.canonical_identity, &prefix) {
            return Err(GraphContractError::ProjectQualificationMismatch);
        }
        Ok(Self {
            project: value.project,
            stable: value.stable,
        })
    }
}

impl From<LogicalRelationKey> for LogicalRelationKeyWire {
    fn from(value: LogicalRelationKey) -> Self {
        Self {
            project: value.project,
            stable: value.stable,
        }
    }
}

impl LogicalRelationKey {
    /// Return the owning project instance.
    #[must_use]
    pub const fn project(&self) -> ProjectInstanceId {
        self.project
    }

    /// Borrow the lowercase compact digest.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.stable.digest
    }

    /// Return the compact binary digest used by normalized persistence.
    ///
    /// # Errors
    ///
    /// Returns [`GraphContractError::InvalidStableKeyDigest`] if the private key
    /// invariant was violated by incompatible persisted input.
    pub fn digest_bytes(&self) -> Result<[u8; 32], GraphContractError> {
        self.stable.digest_bytes()
    }

    /// Borrow the canonical identity retained for collision detection.
    #[must_use]
    pub fn canonical_identity(&self) -> &str {
        &self.stable.canonical_identity
    }

    /// Determine whether two keys identify the same logical relation.
    ///
    /// # Errors
    ///
    /// Returns [`GraphContractError::StableKeyCollision`] for conflicting
    /// canonical identities with the same digest.
    pub fn reconcile(&self, other: &Self) -> Result<bool, GraphContractError> {
        if self.project != other.project {
            return Ok(false);
        }
        self.stable.reconcile(&other.stable)
    }
}

/// Closed target-identity families used by canonical resolution keys.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionKeyDomain {
    /// A declaration, value, type, or other named source symbol.
    Declaration,
    /// A source module, namespace, or importable file identity.
    Module,
    /// A package or manifest dependency identity.
    Package,
}

impl ResolutionKeyDomain {
    /// Return the stable `SQLite` and wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Module => "module",
            Self::Package => "package",
        }
    }
}

impl TryFrom<&str> for ResolutionKeyDomain {
    type Error = GraphContractError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "declaration" => Ok(Self::Declaration),
            "module" => Ok(Self::Module),
            "package" => Ok(Self::Package),
            _ => Err(GraphContractError::InvalidResolutionKeyDomain),
        }
    }
}

/// Project-qualified canonical identity used for export and dependency resolution.
///
/// The fixed digest is the indexed hot-path value. The canonical identity remains
/// alongside it as the collision witness and includes every identity-affecting
/// provider, language, package, scope, relation-family, and target field.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(
    try_from = "CanonicalResolutionKeyWire",
    into = "CanonicalResolutionKeyWire"
)]
pub struct CanonicalResolutionKey {
    /// Project instance whose resolver namespace owns this key.
    project: ProjectInstanceId,
    /// Closed resolver family.
    domain: ResolutionKeyDomain,
    /// Fixed compact key used by `SQLite` indexes.
    digest: [u8; 32],
    /// Canonical collision witness.
    canonical_identity: String,
}

impl CanonicalResolutionKey {
    /// Construct a deterministic canonical resolution key.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        project: ProjectInstanceId,
        domain: ResolutionKeyDomain,
        provider: &GraphIdentityText,
        language: &GraphIdentityText,
        package: Option<&GraphIdentityText>,
        scope: Option<&GraphIdentityText>,
        relation: Option<GraphRelationKind>,
        identity: &GraphIdentityText,
    ) -> Self {
        let mut canonical_identity = resolution_project_prefix(project, domain);
        append_canonical_field(&mut canonical_identity, provider.as_str());
        append_canonical_field(&mut canonical_identity, language.as_str());
        append_optional_canonical_field(&mut canonical_identity, package);
        append_optional_canonical_field(&mut canonical_identity, scope);
        append_optional_raw_canonical_field(
            &mut canonical_identity,
            relation.map(GraphRelationKind::canonical_name),
        );
        append_canonical_field(&mut canonical_identity, identity.as_str());
        let digest = *blake3::hash(canonical_identity.as_bytes()).as_bytes();
        Self {
            project,
            domain,
            digest,
            canonical_identity,
        }
    }

    /// Reconstruct and validate persisted canonical key material.
    ///
    /// # Errors
    ///
    /// Returns an error when the witness is not qualified by the declared project
    /// and domain or its digest does not match.
    pub fn from_persisted(
        project: ProjectInstanceId,
        domain: ResolutionKeyDomain,
        digest: [u8; 32],
        canonical_identity: String,
    ) -> Result<Self, GraphContractError> {
        let prefix = resolution_project_prefix(project, domain);
        if !has_canonical_prefix(&canonical_identity, &prefix) {
            return Err(GraphContractError::ProjectQualificationMismatch);
        }
        if *blake3::hash(canonical_identity.as_bytes()).as_bytes() != digest {
            return Err(GraphContractError::InvalidStableKeyDigest);
        }
        Ok(Self {
            project,
            domain,
            digest,
            canonical_identity,
        })
    }

    /// Return the owning project instance.
    #[must_use]
    pub const fn project(&self) -> ProjectInstanceId {
        self.project
    }

    /// Return the closed resolver domain.
    #[must_use]
    pub const fn domain(&self) -> ResolutionKeyDomain {
        self.domain
    }

    /// Return the fixed compact digest used by normalized persistence.
    #[must_use]
    pub const fn digest_bytes(&self) -> [u8; 32] {
        self.digest
    }

    /// Borrow the canonical collision witness.
    #[must_use]
    pub fn canonical_identity(&self) -> &str {
        &self.canonical_identity
    }

    /// Compare compact keys without silently accepting a digest collision.
    ///
    /// # Errors
    ///
    /// Returns [`GraphContractError::StableKeyCollision`] when equal compact keys
    /// retain different canonical material.
    pub fn reconcile(&self, other: &Self) -> Result<bool, GraphContractError> {
        if self.project != other.project || self.domain != other.domain {
            return Ok(false);
        }
        if self.digest != other.digest {
            return Ok(false);
        }
        if self.canonical_identity != other.canonical_identity {
            return Err(GraphContractError::StableKeyCollision {
                digest: encode_hex(&self.digest),
            });
        }
        Ok(true)
    }
}

/// Validated serialized canonical resolution-key representation.
#[derive(Deserialize, Serialize)]
struct CanonicalResolutionKeyWire {
    /// Owning project instance.
    project: ProjectInstanceId,
    /// Closed resolver family.
    domain: ResolutionKeyDomain,
    /// Fixed compact digest.
    digest: [u8; 32],
    /// Canonical collision witness.
    canonical_identity: String,
}

impl TryFrom<CanonicalResolutionKeyWire> for CanonicalResolutionKey {
    type Error = GraphContractError;

    fn try_from(value: CanonicalResolutionKeyWire) -> Result<Self, Self::Error> {
        Self::from_persisted(
            value.project,
            value.domain,
            value.digest,
            value.canonical_identity,
        )
    }
}

impl From<CanonicalResolutionKey> for CanonicalResolutionKeyWire {
    fn from(value: CanonicalResolutionKey) -> Self {
        Self {
            project: value.project,
            domain: value.domain,
            digest: value.digest,
            canonical_identity: value.canonical_identity,
        }
    }
}

/// One exported canonical key bound to its owning graph entity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EntityResolutionKey {
    /// Entity that exports the canonical identity.
    entity: GraphEntityKey,
    /// Canonical resolver identity exported by the entity.
    key: CanonicalResolutionKey,
}

impl EntityResolutionKey {
    /// Bind one canonical key to its exported entity.
    ///
    /// # Errors
    ///
    /// Returns an error when the entity and key belong to different projects.
    pub fn new(
        entity: GraphEntityKey,
        key: CanonicalResolutionKey,
    ) -> Result<Self, GraphContractError> {
        if entity.project() != key.project() {
            return Err(GraphContractError::ResolutionKeyOwnerMismatch);
        }
        Ok(Self { entity, key })
    }

    /// Borrow the owning entity key.
    #[must_use]
    pub const fn entity(&self) -> &GraphEntityKey {
        &self.entity
    }

    /// Borrow the canonical resolver key.
    #[must_use]
    pub const fn key(&self) -> &CanonicalResolutionKey {
        &self.key
    }
}

/// One canonical dependency identity bound to its owning logical relation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationDependencyKey {
    /// Logical relation that depends on the canonical identity.
    relation: LogicalRelationKey,
    /// Canonical identity used to select candidate exports.
    key: CanonicalResolutionKey,
}

impl RelationDependencyKey {
    /// Bind one dependency identity to its logical relation.
    ///
    /// # Errors
    ///
    /// Returns an error when the relation and key belong to different projects.
    pub fn new(
        relation: LogicalRelationKey,
        key: CanonicalResolutionKey,
    ) -> Result<Self, GraphContractError> {
        if relation.project() != key.project() {
            return Err(GraphContractError::ResolutionKeyOwnerMismatch);
        }
        Ok(Self { relation, key })
    }

    /// Borrow the owning logical-relation key.
    #[must_use]
    pub const fn relation(&self) -> &LogicalRelationKey {
        &self.relation
    }

    /// Borrow the canonical resolver key.
    #[must_use]
    pub const fn key(&self) -> &CanonicalResolutionKey {
        &self.key
    }
}

/// One deduplicated source-kind-target relationship at a complete generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogicalRelation {
    /// Stable key independent of individual source occurrences.
    key: LogicalRelationKey,
    /// Source entity.
    source: GraphEntityKey,
    /// Typed legacy or additive family.
    kind: GraphRelationKind,
    /// Resolution state and optional exact local target.
    resolution: RelationResolution,
    /// Coarse trust class.
    confidence: ConfidenceClass,
    /// Producer completeness for this relation scope.
    completeness: Completeness,
    /// Complete generation containing the relation.
    generation: IndexGeneration,
}

impl LogicalRelation {
    /// Construct one logical relation and derive its stable key.
    ///
    /// # Errors
    ///
    /// Returns an error when the source or target belongs to another generation,
    /// a retained target belongs to another project, a reusable selector names a
    /// different entity, or the relation claims the zero generation.
    pub fn new(
        source: &GraphEntity,
        kind: GraphRelationKind,
        resolution: RelationResolution,
        confidence: ConfidenceClass,
        completeness: Completeness,
        generation: IndexGeneration,
    ) -> Result<Self, GraphContractError> {
        if generation == IndexGeneration::ZERO {
            return Err(GraphContractError::InvalidGeneration);
        }
        if source.generation != generation {
            return Err(GraphContractError::GenerationMismatch {
                context: "logical relation source",
            });
        }
        if resolution
            .target_key()
            .is_some_and(|target| target.project() != source.key.project())
        {
            return Err(GraphContractError::CrossProjectRelation);
        }
        if resolution
            .target_generation()
            .is_some_and(|target_generation| target_generation != generation)
        {
            return Err(GraphContractError::GenerationMismatch {
                context: "logical relation target",
            });
        }
        let selected = match &resolution {
            RelationResolution::Resolved {
                target, selector, ..
            } => Some((target, selector.entity_selector())),
            RelationResolution::External {
                target, external, ..
            } => Some((
                target,
                EntitySelector::External {
                    external: external.clone(),
                },
            )),
            RelationResolution::Ambiguous { .. } | RelationResolution::Unresolved { .. } => None,
        };
        if let Some((target, selector)) = selected {
            let selected = GraphEntityKey::new(target.project(), &selector);
            if !target.reconcile(&selected)? {
                return Err(GraphContractError::InvalidResolution {
                    reason: "resolution selector does not identify its retained target",
                });
            }
        }
        let mut canonical = relation_project_prefix(source.key.project());
        append_canonical_field(&mut canonical, source.key.canonical_identity());
        append_canonical_field(&mut canonical, kind.canonical_name());
        resolution.append_canonical(&mut canonical);
        let key = LogicalRelationKey {
            project: source.key.project(),
            stable: StableKey::new(canonical),
        };
        Ok(Self {
            key,
            source: source.key.clone(),
            kind,
            resolution,
            confidence,
            completeness,
            generation,
        })
    }

    /// Borrow the stable logical-relation key.
    #[must_use]
    pub const fn key(&self) -> &LogicalRelationKey {
        &self.key
    }

    /// Borrow the source entity key.
    #[must_use]
    pub const fn source(&self) -> &GraphEntityKey {
        &self.source
    }

    /// Return the typed relation family.
    #[must_use]
    pub const fn kind(&self) -> GraphRelationKind {
        self.kind
    }

    /// Borrow the relation resolution state.
    #[must_use]
    pub const fn resolution(&self) -> &RelationResolution {
        &self.resolution
    }

    /// Return the coarse relation confidence.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceClass {
        self.confidence
    }

    /// Return the producer completeness for this relation.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// Return the complete generation containing this relation.
    #[must_use]
    pub const fn generation(&self) -> IndexGeneration {
        self.generation
    }
}

/// One exact source occurrence supporting a logical relationship.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationOccurrence {
    /// Deduplicated logical relation supported by this occurrence.
    relation: LogicalRelationKey,
    /// Repository-local file containing the evidence.
    file: RepositoryFilePath,
    /// Exact supporting source range.
    span: SourceSpan,
    /// Complete generation containing the occurrence.
    generation: IndexGeneration,
}

impl RelationOccurrence {
    /// Construct one source occurrence for a logical relation.
    ///
    /// # Errors
    ///
    /// Returns an error when the occurrence and relation generations differ.
    pub fn new(
        relation: &LogicalRelation,
        file: RepositoryFilePath,
        span: SourceSpan,
        generation: IndexGeneration,
    ) -> Result<Self, GraphContractError> {
        if relation.generation != generation {
            return Err(GraphContractError::GenerationMismatch {
                context: "relation occurrence",
            });
        }
        Ok(Self {
            relation: relation.key.clone(),
            file,
            span,
            generation,
        })
    }

    /// Borrow the supported logical-relation key.
    #[must_use]
    pub const fn relation(&self) -> &LogicalRelationKey {
        &self.relation
    }

    /// Borrow the repository-local source file.
    #[must_use]
    pub const fn file(&self) -> &RepositoryFilePath {
        &self.file
    }

    /// Return the exact supporting source span.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Return the complete generation containing this occurrence.
    #[must_use]
    pub const fn generation(&self) -> IndexGeneration {
        self.generation
    }
}

/// Coverage lifecycle state for one indexed graph scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageState {
    /// Every supported item was covered.
    Complete,
    /// Some supported items were covered and some were omitted.
    Partial,
    /// Extraction failed for the whole scope.
    Failed,
    /// Configuration intentionally excluded the scope.
    Ignored,
    /// A declared size or work limit excluded the scope.
    Oversized,
    /// Trust policy isolated the scope.
    Quarantined,
    /// Previously derived coverage no longer matches current source state.
    Stale,
}

/// Indexed scope whose graph coverage is being reported.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoverageScope {
    /// The selected project as a whole.
    Project,
    /// One repository path.
    Path {
        /// Normalized repository-relative path.
        path: RepositoryNodePath,
    },
}

/// Absolute product limit that a coverage or query row reached.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphLimitKind {
    /// Result-row limit.
    Rows,
    /// Unique or active node limit.
    Nodes,
    /// Inspected logical-edge limit.
    Edges,
    /// Per-relation source-occurrence limit.
    Occurrences,
    /// Node-simple visited-state limit.
    Visited,
    /// Decoded or retained intermediate-memory byte limit.
    IntermediateBytes,
    /// Elapsed request deadline.
    Deadline,
    /// Traversal-depth limit.
    Depth,
    /// Encoded-output byte limit.
    OutputBytes,
}

/// Coverage report with counts consistent with its lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoverageRecord {
    /// Covered project or path.
    scope: CoverageScope,
    /// Optional relation family when the row is relation-specific.
    relation: Option<GraphRelationKind>,
    /// Coverage lifecycle state.
    state: CoverageState,
    /// Supported items in the scope.
    total: u64,
    /// Successfully covered items.
    covered: u64,
    /// Omitted, failed, or untrusted items.
    omitted: u64,
    /// Complete generation associated with this row.
    generation: IndexGeneration,
    /// Actionable non-complete explanation.
    reason: Option<GraphIdentityText>,
    /// Product limit reached, when applicable.
    reached_limit: Option<GraphLimitKind>,
}

impl CoverageRecord {
    /// Construct a coverage row and enforce count/state consistency.
    ///
    /// # Errors
    ///
    /// Returns an error when counts overflow, contradict the selected state, omit
    /// a required reason, or claim the pre-publication zero generation.
    pub fn new(
        scope: CoverageScope,
        relation: Option<GraphRelationKind>,
        state: CoverageState,
        covered: u64,
        omitted: u64,
        generation: IndexGeneration,
        reason: Option<GraphIdentityText>,
        reached_limit: Option<GraphLimitKind>,
    ) -> Result<Self, GraphContractError> {
        if generation == IndexGeneration::ZERO {
            return Err(GraphContractError::InvalidGeneration);
        }
        let total = covered
            .checked_add(omitted)
            .ok_or(GraphContractError::InvalidCoverage {
                reason: "coverage counts overflow",
            })?;
        let valid_counts = match state {
            CoverageState::Complete => omitted == 0,
            CoverageState::Partial => covered > 0 && omitted > 0,
            CoverageState::Failed
            | CoverageState::Ignored
            | CoverageState::Oversized
            | CoverageState::Quarantined
            | CoverageState::Stale => covered == 0 && omitted > 0,
        };
        if !valid_counts {
            return Err(GraphContractError::InvalidCoverage {
                reason: "coverage state contradicts covered and omitted counts",
            });
        }
        if state == CoverageState::Complete && (reason.is_some() || reached_limit.is_some()) {
            return Err(GraphContractError::InvalidCoverage {
                reason: "complete coverage cannot report an omission reason or limit",
            });
        }
        if state != CoverageState::Complete && reason.is_none() {
            return Err(GraphContractError::InvalidCoverage {
                reason: "non-complete coverage requires an actionable reason",
            });
        }
        Ok(Self {
            scope,
            relation,
            state,
            total,
            covered,
            omitted,
            generation,
            reason,
            reached_limit,
        })
    }

    /// Borrow the covered project or path scope.
    #[must_use]
    pub const fn scope(&self) -> &CoverageScope {
        &self.scope
    }

    /// Return the optional relation family covered by this row.
    #[must_use]
    pub const fn relation(&self) -> Option<GraphRelationKind> {
        self.relation
    }

    /// Return the coverage lifecycle state.
    #[must_use]
    pub const fn state(&self) -> CoverageState {
        self.state
    }

    /// Return the total number of supported items in scope.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Return the number of successfully covered items.
    #[must_use]
    pub const fn covered(&self) -> u64 {
        self.covered
    }

    /// Return the number of omitted, failed, or untrusted items.
    #[must_use]
    pub const fn omitted(&self) -> u64 {
        self.omitted
    }

    /// Return the complete generation associated with this row.
    #[must_use]
    pub const fn generation(&self) -> IndexGeneration {
        self.generation
    }

    /// Borrow the actionable non-complete explanation, when present.
    #[must_use]
    pub const fn reason(&self) -> Option<&GraphIdentityText> {
        self.reason.as_ref()
    }

    /// Return the product limit reached, when applicable.
    #[must_use]
    pub const fn reached_limit(&self) -> Option<GraphLimitKind> {
        self.reached_limit
    }
}

/// Hard bounded retrieval limits accepted by repository graph consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GraphLimitsWire", into = "GraphLimitsWire")]
pub struct GraphLimits {
    /// Maximum returned logical rows.
    rows: NonZeroU32,
    /// Maximum returned occurrences for one logical relation.
    occurrences: NonZeroU32,
    /// Maximum traversal depth.
    depth: NonZeroU32,
    /// Maximum encoded result bytes.
    output_bytes: NonZeroU32,
}

impl GraphLimits {
    /// Absolute row ceiling for any one graph request.
    pub const MAX_ROWS: u32 = 10_000;
    /// Absolute occurrence ceiling for any one logical relation.
    pub const MAX_OCCURRENCES: u32 = 1_024;
    /// Absolute traversal-depth ceiling.
    pub const MAX_DEPTH: u32 = 64;
    /// Absolute encoded-output ceiling.
    pub const MAX_OUTPUT_BYTES: u32 = 16 * 1_024 * 1_024;

    /// Validate result-defining hard limits.
    ///
    /// # Errors
    ///
    /// Returns an error for zero values or values above absolute ceilings.
    pub fn new(
        rows: u32,
        occurrences: u32,
        depth: u32,
        output_bytes: u32,
    ) -> Result<Self, GraphContractError> {
        if rows == 0 || rows > Self::MAX_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "row limit is zero or above the absolute ceiling",
            });
        }
        if occurrences == 0 || occurrences > Self::MAX_OCCURRENCES {
            return Err(GraphContractError::InvalidLimits {
                reason: "occurrence limit is zero or above the absolute ceiling",
            });
        }
        if depth == 0 || depth > Self::MAX_DEPTH {
            return Err(GraphContractError::InvalidLimits {
                reason: "depth limit is zero or above the absolute ceiling",
            });
        }
        if output_bytes == 0 || output_bytes > Self::MAX_OUTPUT_BYTES {
            return Err(GraphContractError::InvalidLimits {
                reason: "output limit is zero or above the absolute ceiling",
            });
        }
        Ok(Self {
            rows: NonZeroU32::new(rows).ok_or(GraphContractError::InvalidLimits {
                reason: "row limit must be nonzero",
            })?,
            occurrences: NonZeroU32::new(occurrences).ok_or(GraphContractError::InvalidLimits {
                reason: "occurrence limit must be nonzero",
            })?,
            depth: NonZeroU32::new(depth).ok_or(GraphContractError::InvalidLimits {
                reason: "depth limit must be nonzero",
            })?,
            output_bytes: NonZeroU32::new(output_bytes).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "output limit must be nonzero",
                },
            )?,
        })
    }

    /// Return the logical-row limit.
    #[must_use]
    pub const fn rows(self) -> u32 {
        self.rows.get()
    }

    /// Return the per-relation occurrence limit.
    #[must_use]
    pub const fn occurrences(self) -> u32 {
        self.occurrences.get()
    }

    /// Return the traversal-depth limit.
    #[must_use]
    pub const fn depth(self) -> u32 {
        self.depth.get()
    }

    /// Return the encoded-output byte limit.
    #[must_use]
    pub const fn output_bytes(self) -> u32 {
        self.output_bytes.get()
    }
}

/// Graph-limit wire shape validated during deserialization.
#[derive(Deserialize, Serialize)]
struct GraphLimitsWire {
    /// Logical-row limit.
    rows: u32,
    /// Per-relation occurrence limit.
    occurrences: u32,
    /// Traversal-depth limit.
    depth: u32,
    /// Encoded-output byte limit.
    output_bytes: u32,
}

impl TryFrom<GraphLimitsWire> for GraphLimits {
    type Error = GraphContractError;

    fn try_from(value: GraphLimitsWire) -> Result<Self, Self::Error> {
        Self::new(
            value.rows,
            value.occurrences,
            value.depth,
            value.output_bytes,
        )
    }
}

impl From<GraphLimits> for GraphLimitsWire {
    fn from(value: GraphLimits) -> Self {
        Self {
            rows: value.rows(),
            occurrences: value.occurrences(),
            depth: value.depth(),
            output_bytes: value.output_bytes(),
        }
    }
}

/// Start a canonical identity with a stable domain and project prefix.
fn project_canonical_prefix(domain: &str, project: ProjectInstanceId) -> String {
    let mut canonical = domain.to_string();
    append_canonical_field(&mut canonical, &project.as_hex());
    canonical
}

/// Return the canonical prefix required for one entity key.
fn entity_project_prefix(project: ProjectInstanceId) -> String {
    project_canonical_prefix(ENTITY_KEY_DOMAIN, project)
}

/// Return the canonical prefix required for one relation key.
fn relation_project_prefix(project: ProjectInstanceId) -> String {
    project_canonical_prefix(RELATION_KEY_DOMAIN, project)
}

/// Return the canonical prefix required for one resolution key.
fn resolution_project_prefix(project: ProjectInstanceId, domain: ResolutionKeyDomain) -> String {
    let mut canonical = project_canonical_prefix(RESOLUTION_KEY_DOMAIN, project);
    append_canonical_field(&mut canonical, domain.as_str());
    canonical
}

/// Return whether canonical material continues after one complete field prefix.
fn has_canonical_prefix(canonical: &str, prefix: &str) -> bool {
    canonical
        .strip_prefix(prefix)
        .is_some_and(|remainder| remainder.starts_with('|'))
}

/// Encode a typed selector into unambiguous project-qualified material.
fn entity_canonical_identity(project: ProjectInstanceId, selector: &EntitySelector) -> String {
    let mut canonical = entity_project_prefix(project);
    match selector {
        EntitySelector::Project => append_canonical_field(&mut canonical, "project"),
        EntitySelector::Folder { path } => {
            append_canonical_field(&mut canonical, "folder");
            append_canonical_field(&mut canonical, path.as_str());
        }
        EntitySelector::File { path } => {
            append_canonical_field(&mut canonical, "file");
            append_canonical_field(&mut canonical, path.as_str());
        }
        EntitySelector::Package { package } => {
            append_canonical_field(&mut canonical, "package");
            append_canonical_field(&mut canonical, package.manager.as_str());
            append_canonical_field(&mut canonical, package.name.as_str());
            append_canonical_field(&mut canonical, package.manifest.as_str());
        }
        EntitySelector::Symbol { symbol } => {
            append_canonical_field(&mut canonical, "symbol");
            append_canonical_field(&mut canonical, symbol.file.as_str());
            append_canonical_field(&mut canonical, symbol.name.as_str());
            append_canonical_field(&mut canonical, &symbol.kind.to_string());
            append_optional_canonical_field(&mut canonical, symbol.parent.as_ref());
            append_canonical_field(&mut canonical, symbol.signature.as_str());
        }
        EntitySelector::External { external } => {
            append_canonical_field(&mut canonical, "external");
            append_canonical_field(&mut canonical, external.system.as_str());
            append_canonical_field(&mut canonical, external.identity.as_str());
        }
    }
    canonical
}

/// Append one byte-length-prefixed field to canonical identity material.
fn append_canonical_field(canonical: &mut String, value: &str) {
    canonical.push('|');
    canonical.push_str(&value.len().to_string());
    canonical.push(':');
    canonical.push_str(value);
}

/// Append an optional identity field without conflating absent and empty values.
fn append_optional_canonical_field(canonical: &mut String, value: Option<&GraphIdentityText>) {
    match value {
        Some(value) => {
            append_canonical_field(canonical, "some");
            append_canonical_field(canonical, value.as_str());
        }
        None => append_canonical_field(canonical, "none"),
    }
}

/// Append optional validated contract text without conflating absent and empty values.
fn append_optional_raw_canonical_field(canonical: &mut String, value: Option<&str>) {
    match value {
        Some(value) => {
            append_canonical_field(canonical, "some");
            append_canonical_field(canonical, value);
        }
        None => append_canonical_field(canonical, "none"),
    }
}

/// Encode bytes as lowercase hexadecimal without another dependency.
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Decode one ASCII hexadecimal digit.
const fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
        CoverageState, EntityResolutionKey, EntitySelector, ExtendedRelationKind, ExternalSelector,
        GraphContractError, GraphEntity, GraphEntityKey, GraphIdentityText, GraphLimitKind,
        GraphLimits, GraphRelationKind, LogicalRelation, PackageSelector, ProjectInstanceId,
        RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath,
        RepositoryNodePath, ResolutionKeyDomain, ReusableTargetSelector, SourceSpan, StableKey,
        SymbolSelector,
    };
    use crate::IndexGeneration;
    use crate::symbols::{RelationKind, SymbolKind};
    use std::io;
    use std::num::NonZeroU32;
    use std::path::Path;

    /// Return a stable nonzero project identity for unit tests.
    fn project() -> Result<ProjectInstanceId, GraphContractError> {
        ProjectInstanceId::try_from("00112233445566778899aabbccddeeff")
    }

    /// Return one stable function selector for unit tests.
    fn symbol_selector(path: &str) -> Result<SymbolSelector, GraphContractError> {
        Ok(SymbolSelector {
            file: RepositoryFilePath::new(Path::new(path))?,
            name: GraphIdentityText::new("répond")?,
            kind: SymbolKind::Function,
            parent: Some(GraphIdentityText::new("Service")?),
            signature: GraphIdentityText::new("fn répond(input: &str)")?,
        })
    }

    /// Return an ordinary test error when a behavior condition is false.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    #[test]
    fn project_identity_validates_and_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let id = ProjectInstanceId::try_from("00112233-4455-6677-8899-AABBCCDDEEFF")?;
        require(
            id.as_hex() == "00112233445566778899aabbccddeeff",
            "project identity did not normalize to lowercase hexadecimal",
        )?;
        let encoded = serde_json::to_string(&id)?;
        require(
            encoded == "\"00112233445566778899aabbccddeeff\"",
            "project identity serialized with an unstable shape",
        )?;
        require(
            serde_json::from_str::<ProjectInstanceId>(&encoded)? == id,
            "project identity did not round-trip",
        )?;
        require(
            ProjectInstanceId::try_from("00000000000000000000000000000000").is_err(),
            "zero project identity was accepted",
        )?;
        require(
            ProjectInstanceId::try_from("not-an-id").is_err(),
            "malformed project identity was accepted",
        )?;
        require(
            ProjectInstanceId::try_from("00112233-4455-6677-8899-AABBCCDDEEF-").is_err(),
            "hyphenated project identity with missing hexadecimal data was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn entity_keys_are_stable_line_independent_and_project_scoped()
    -> Result<(), Box<dyn std::error::Error>> {
        let selector = EntitySelector::Symbol {
            symbol: symbol_selector("src\\service.rs")?,
        };
        let first = GraphEntity::new(project()?, selector.clone(), IndexGeneration::new(4))?;
        let rescanned = GraphEntity::new(project()?, selector, IndexGeneration::new(5))?;
        require(
            first.key().reconcile(rescanned.key())?,
            "unchanged selector did not retain identity",
        )?;
        require(
            first.key().digest() == rescanned.key().digest(),
            "unchanged selector changed its compact key",
        )?;
        require(
            first.key().digest_bytes()?
                == *blake3::hash(first.key().canonical_identity().as_bytes()).as_bytes(),
            "binary persistence key did not match the validated digest",
        )?;

        let other_project = ProjectInstanceId::try_from("10112233445566778899aabbccddeeff")?;
        let other = GraphEntityKey::new(other_project, rescanned.selector());
        require(
            !first.key().reconcile(&other)?,
            "independent projects shared an entity identity",
        )?;
        require(
            first.key().digest() != other.digest(),
            "independent projects shared a compact entity key",
        )?;
        Ok(())
    }

    #[test]
    fn stable_key_collisions_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let selector = EntitySelector::File {
            path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
        };
        let first = GraphEntityKey::new(project()?, &selector);
        let conflicting = GraphEntityKey {
            project: first.project,
            stable: StableKey {
                digest: first.stable.digest.clone(),
                canonical_identity: "different canonical identity".to_string(),
            },
        };
        require(
            matches!(
                first.reconcile(&conflicting),
                Err(GraphContractError::StableKeyCollision { .. })
            ),
            "conflicting canonical identity did not fail closed",
        )?;

        let mut serialized = serde_json::to_value(&first)?;
        serialized["stable"]["canonical_identity"] = serde_json::json!("tampered");
        require(
            serde_json::from_value::<GraphEntityKey>(serialized).is_err(),
            "tampered stable-key material was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn canonical_resolution_keys_are_stable_qualified_and_collision_checked()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = GraphIdentityText::new("tree-sitter")?;
        let language = GraphIdentityText::new("rust")?;
        let package = GraphIdentityText::new("auth")?;
        let scope = GraphIdentityText::new("crate")?;
        let identity = GraphIdentityText::new("répond")?;
        let key = CanonicalResolutionKey::new(
            project()?,
            ResolutionKeyDomain::Declaration,
            &provider,
            &language,
            Some(&package),
            Some(&scope),
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            &identity,
        );
        let repeated = CanonicalResolutionKey::new(
            project()?,
            ResolutionKeyDomain::Declaration,
            &provider,
            &language,
            Some(&package),
            Some(&scope),
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            &identity,
        );
        require(
            key.reconcile(&repeated)?,
            "equal canonical resolver input changed identity",
        )?;
        require(
            key.digest_bytes() == *blake3::hash(key.canonical_identity().as_bytes()).as_bytes(),
            "fixed resolver digest did not match its collision witness",
        )?;
        let persisted = CanonicalResolutionKey::from_persisted(
            key.project(),
            key.domain(),
            key.digest_bytes(),
            key.canonical_identity().to_string(),
        )?;
        require(
            key.reconcile(&persisted)?,
            "validated persisted resolver key changed identity",
        )?;

        let other_project = ProjectInstanceId::try_from("10112233445566778899aabbccddeeff")?;
        let case_distinct = GraphIdentityText::new("Répond")?;
        let variants = [
            CanonicalResolutionKey::new(
                other_project,
                ResolutionKeyDomain::Declaration,
                &provider,
                &language,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Module,
                &provider,
                &language,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &GraphIdentityText::new("manifest")?,
                &language,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &provider,
                &GraphIdentityText::new("typescript")?,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &provider,
                &language,
                Some(&GraphIdentityText::new("billing")?),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &provider,
                &language,
                Some(&package),
                Some(&GraphIdentityText::new("module")?),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &provider,
                &language,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Imports)),
                &identity,
            ),
            CanonicalResolutionKey::new(
                project()?,
                ResolutionKeyDomain::Declaration,
                &provider,
                &language,
                Some(&package),
                Some(&scope),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &case_distinct,
            ),
        ];
        for variant in variants {
            require(
                !key.reconcile(&variant)?,
                "identity-affecting resolver dimension was ignored",
            )?;
        }
        require(
            matches!(
                ResolutionKeyDomain::try_from("declaration"),
                Ok(ResolutionKeyDomain::Declaration)
            ) && ResolutionKeyDomain::try_from("unknown").is_err(),
            "closed resolver-domain persistence accepted an unsupported value",
        )?;

        let conflicting = CanonicalResolutionKey {
            canonical_identity: "different canonical identity".to_string(),
            ..key
        };
        require(
            matches!(
                key.reconcile(&conflicting),
                Err(GraphContractError::StableKeyCollision { .. })
            ),
            "equal resolver digest with a different witness did not fail closed",
        )?;
        let mut serialized = serde_json::to_value(&key)?;
        serialized["canonical_identity"] = serde_json::json!("tampered");
        require(
            serde_json::from_value::<CanonicalResolutionKey>(serialized).is_err(),
            "tampered persisted resolver key was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn resolution_key_bindings_preserve_dependency_identity_across_states()
    -> Result<(), Box<dyn std::error::Error>> {
        let generation = IndexGeneration::new(7);
        let source = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/caller.rs")?,
            },
            generation,
        )?;
        let target = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/service.rs")?,
            },
            generation,
        )?;
        let dependency = CanonicalResolutionKey::new(
            project()?,
            ResolutionKeyDomain::Declaration,
            &GraphIdentityText::new("tree-sitter")?,
            &GraphIdentityText::new("rust")?,
            None,
            None,
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            &GraphIdentityText::new("répond")?,
        );
        let export = EntityResolutionKey::new(target.key().clone(), dependency.clone())?;
        require(
            export.key().reconcile(&dependency)?,
            "export binding changed its canonical resolver identity",
        )?;

        let relations = [
            LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&target)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::Ambiguous {
                    reference: GraphIdentityText::new("répond")?,
                    candidates: NonZeroU32::new(2).ok_or("nonzero candidate fixture")?,
                },
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new("répond")?,
                },
                ConfidenceClass::Low,
                Completeness::Complete,
                generation,
            )?,
        ];
        for relation in relations {
            let binding = RelationDependencyKey::new(relation.key().clone(), dependency.clone())?;
            require(
                binding.key().reconcile(&dependency)?,
                "resolution state changed the retained dependency identity",
            )?;
        }

        let foreign = CanonicalResolutionKey::new(
            ProjectInstanceId::try_from("10112233445566778899aabbccddeeff")?,
            ResolutionKeyDomain::Declaration,
            &GraphIdentityText::new("tree-sitter")?,
            &GraphIdentityText::new("rust")?,
            None,
            None,
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            &GraphIdentityText::new("répond")?,
        );
        require(
            matches!(
                EntityResolutionKey::new(target.key().clone(), foreign),
                Err(GraphContractError::ResolutionKeyOwnerMismatch)
            ),
            "cross-project resolver binding was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn selectors_normalize_slashes_and_preserve_unicode_and_case()
    -> Result<(), Box<dyn std::error::Error>> {
        let upper = EntitySelector::Symbol {
            symbol: symbol_selector("Src\\Über.rs")?,
        };
        let lower = EntitySelector::Symbol {
            symbol: symbol_selector("src/Über.rs")?,
        };
        let encoded = serde_json::to_string(&upper)?;
        let decoded: EntitySelector = serde_json::from_str(&encoded)?;
        require(decoded == upper, "Unicode selector did not round-trip")?;
        require(
            encoded.contains("Src/Über.rs"),
            "selector changed Unicode or path case",
        )?;
        require(
            GraphEntityKey::new(project()?, &upper) != GraphEntityKey::new(project()?, &lower),
            "case-distinct repository paths collapsed",
        )?;

        let slash_variant = EntitySelector::File {
            path: RepositoryFilePath::new(Path::new("src\\lib.rs"))?,
        };
        let normalized = EntitySelector::File {
            path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
        };
        require(
            GraphEntityKey::new(project()?, &slash_variant)
                == GraphEntityKey::new(project()?, &normalized),
            "slash variants did not normalize to one entity",
        )?;
        Ok(())
    }

    #[test]
    fn invalid_selector_inputs_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        require(
            RepositoryFilePath::new(Path::new("../secret.rs")).is_err(),
            "parent traversal was accepted",
        )?;
        require(
            RepositoryFilePath::new(Path::new(".")).is_err(),
            "project root was accepted as a file",
        )?;
        require(
            RepositoryNodePath::new(Path::new("C:/repo")).is_err(),
            "absolute Windows path was accepted",
        )?;
        require(
            GraphIdentityText::new(" ").is_err(),
            "blank identity text was accepted",
        )?;
        require(
            GraphIdentityText::new(" padded").is_err()
                && GraphIdentityText::new("padded ").is_err(),
            "surrounding identity whitespace was accepted",
        )?;
        require(
            GraphIdentityText::new("bad\nidentity").is_err(),
            "control-bearing identity text was accepted",
        )?;
        require(
            GraphIdentityText::new("x".repeat(super::MAX_IDENTITY_BYTES + 1)).is_err(),
            "oversized identity text was accepted",
        )?;
        require(
            SourceSpan::new(0, 0, 1, 0).is_err(),
            "zero source line was accepted",
        )?;
        require(
            SourceSpan::new(4, 2, 3, 9).is_err(),
            "reversed source span was accepted",
        )?;
        require(
            GraphEntity::new(
                project()?,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                },
                IndexGeneration::ZERO,
            )
            .is_err(),
            "pre-publication generation was accepted for a graph entity",
        )?;

        let package = EntitySelector::Package {
            package: PackageSelector {
                manager: GraphIdentityText::new("cargo")?,
                name: GraphIdentityText::new("projectatlas-core")?,
                manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
            },
        };
        require(
            package == serde_json::from_str(&serde_json::to_string(&package)?)?,
            "package selector did not round-trip",
        )?;
        Ok(())
    }

    #[test]
    fn logical_relations_deduplicate_distinct_source_occurrences()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/caller.rs")?,
            },
            IndexGeneration::new(7),
        )?;
        let target_symbol = symbol_selector("src/target.rs")?;
        let target = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: target_symbol,
            },
            IndexGeneration::new(7),
        )?;
        let relation = LogicalRelation::new(
            &source,
            GraphRelationKind::from_legacy(RelationKind::Calls),
            RelationResolution::resolved(&target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            IndexGeneration::new(7),
        )?;
        let first = RelationOccurrence::new(
            &relation,
            RepositoryFilePath::new(Path::new("src/caller.rs"))?,
            SourceSpan::new(10, 4, 10, 18)?,
            IndexGeneration::new(7),
        )?;
        let second = RelationOccurrence::new(
            &relation,
            RepositoryFilePath::new(Path::new("src/caller.rs"))?,
            SourceSpan::new(20, 4, 20, 18)?,
            IndexGeneration::new(7),
        )?;
        require(
            first.relation() == second.relation(),
            "occurrences did not retain one logical relation",
        )?;
        require(
            first.span() != second.span(),
            "distinct relation occurrences lost their source spans",
        )?;
        Ok(())
    }

    #[test]
    fn relation_records_reject_inconsistent_identity_resolution_and_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/caller.rs")?,
            },
            IndexGeneration::new(3),
        )?;
        let target_symbol = symbol_selector("src/target.rs")?;
        let target = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: target_symbol.clone(),
            },
            IndexGeneration::new(4),
        )?;
        require(
            matches!(
                LogicalRelation::new(
                    &source,
                    GraphRelationKind::from_legacy(RelationKind::Calls),
                    RelationResolution::Unresolved {
                        reference: GraphIdentityText::new("missing")?,
                    },
                    ConfidenceClass::Low,
                    Completeness::Complete,
                    IndexGeneration::new(4),
                ),
                Err(GraphContractError::GenerationMismatch { .. })
            ),
            "mixed source generation was accepted",
        )?;
        require(
            matches!(
                LogicalRelation::new(
                    &source,
                    GraphRelationKind::from_legacy(RelationKind::Calls),
                    RelationResolution::resolved(&target)?,
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    IndexGeneration::new(3),
                ),
                Err(GraphContractError::GenerationMismatch { .. })
            ),
            "mixed target generation was accepted",
        )?;
        let target = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: target_symbol,
            },
            IndexGeneration::new(3),
        )?;
        let relation = LogicalRelation::new(
            &source,
            GraphRelationKind::from_legacy(RelationKind::Calls),
            RelationResolution::resolved(&target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            IndexGeneration::new(3),
        )?;
        let encoded = serde_json::to_string(relation.key())?;
        let decoded = serde_json::from_str::<super::LogicalRelationKey>(&encoded)?;
        require(
            relation.key().reconcile(&decoded)?,
            "validated logical relation key did not round-trip",
        )?;
        require(
            matches!(
                RelationOccurrence::new(
                    &relation,
                    RepositoryFilePath::new(Path::new("src/caller.rs"))?,
                    SourceSpan::new(2, 0, 2, 8)?,
                    IndexGeneration::new(4),
                ),
                Err(GraphContractError::GenerationMismatch { .. })
            ),
            "mixed occurrence generation was accepted",
        )?;
        require(
            matches!(
                LogicalRelation::new(
                    &source,
                    GraphRelationKind::from_legacy(RelationKind::Calls),
                    RelationResolution::Resolved {
                        target: target.key().clone(),
                        selector: ReusableTargetSelector::Symbol {
                            symbol: symbol_selector("src/wrong.rs")?,
                        },
                        generation: IndexGeneration::new(3),
                    },
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    IndexGeneration::new(3),
                ),
                Err(GraphContractError::InvalidResolution { .. })
            ),
            "selector for an unrelated entity was accepted",
        )?;

        let other_project_target = GraphEntity::new(
            ProjectInstanceId::try_from("10112233445566778899aabbccddeeff")?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/target.rs")?,
            },
            IndexGeneration::new(3),
        )?;
        require(
            matches!(
                LogicalRelation::new(
                    &source,
                    GraphRelationKind::from_legacy(RelationKind::Calls),
                    RelationResolution::resolved(&other_project_target)?,
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    IndexGeneration::new(3),
                ),
                Err(GraphContractError::CrossProjectRelation)
            ),
            "cross-project relation was accepted without federation",
        )?;

        let external_selector = ExternalSelector {
            system: GraphIdentityText::new("cargo")?,
            identity: GraphIdentityText::new("serde")?,
        };
        let external_target = GraphEntity::new(
            project()?,
            EntitySelector::External {
                external: external_selector.clone(),
            },
            IndexGeneration::new(3),
        )?;
        LogicalRelation::new(
            &source,
            GraphRelationKind::from_legacy(RelationKind::DependsOn),
            RelationResolution::external(&external_target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            IndexGeneration::new(3),
        )?;
        require(
            matches!(
                LogicalRelation::new(
                    &source,
                    GraphRelationKind::from_legacy(RelationKind::DependsOn),
                    RelationResolution::External {
                        target: external_target.key().clone(),
                        external: ExternalSelector {
                            system: external_selector.system,
                            identity: GraphIdentityText::new("different")?,
                        },
                        generation: IndexGeneration::new(3),
                    },
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    IndexGeneration::new(3),
                ),
                Err(GraphContractError::InvalidResolution { .. })
            ),
            "mismatched external identity was accepted",
        )?;

        let mut serialized = serde_json::to_value(relation.key())?;
        serialized["stable"]["canonical_identity"] =
            serde_json::json!(source.key().canonical_identity());
        serialized["stable"]["digest"] = serde_json::json!(
            blake3::hash(source.key().canonical_identity().as_bytes())
                .to_hex()
                .as_str()
        );
        require(
            serde_json::from_value::<super::LogicalRelationKey>(serialized).is_err(),
            "entity namespace was accepted as a relation key",
        )?;

        let file_key = GraphEntityKey::new(
            project()?,
            &EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
            },
        );
        let mut serialized = serde_json::to_value(&file_key)?;
        let malformed = format!("{}suffix", super::entity_project_prefix(project()?));
        serialized["stable"]["canonical_identity"] = serde_json::json!(&malformed);
        serialized["stable"]["digest"] =
            serde_json::json!(blake3::hash(malformed.as_bytes()).to_hex().as_str());
        require(
            serde_json::from_value::<GraphEntityKey>(serialized).is_err(),
            "partial project prefix was accepted as typed entity identity",
        )?;
        Ok(())
    }

    #[test]
    fn ambiguous_candidate_counts_do_not_change_logical_relation_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = GraphEntity::new(
            project()?,
            EntitySelector::Symbol {
                symbol: symbol_selector("src/caller.rs")?,
            },
            IndexGeneration::new(11),
        )?;
        let relation = |candidates| {
            LogicalRelation::new(
                &source,
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                RelationResolution::Ambiguous {
                    reference: GraphIdentityText::new("handler")?,
                    candidates: NonZeroU32::new(candidates).ok_or(
                        GraphContractError::InvalidCoverage {
                            reason: "test candidate count must be nonzero",
                        },
                    )?,
                },
                ConfidenceClass::High,
                Completeness::Complete,
                IndexGeneration::new(11),
            )
        };
        let first = relation(2)?;
        let second = relation(3)?;
        require(
            first.key().reconcile(second.key())?,
            "candidate-count metadata changed logical relation identity",
        )?;
        Ok(())
    }

    #[test]
    fn relation_resolution_and_target_selectors_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let selector = ReusableTargetSelector::Symbol {
            symbol: symbol_selector("src/lib.rs")?,
        };
        let encoded = serde_json::to_string(&selector)?;
        require(
            serde_json::from_str::<ReusableTargetSelector>(&encoded)? == selector,
            "exact target selector did not round-trip",
        )?;

        let cases = [
            (
                EntitySelector::Folder {
                    path: RepositoryNodePath::new(Path::new("src"))?,
                },
                ReusableTargetSelector::Folder {
                    folder: RepositoryNodePath::new(Path::new("src"))?,
                },
            ),
            (
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                },
                ReusableTargetSelector::File {
                    file: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                },
            ),
            (
                EntitySelector::Package {
                    package: PackageSelector {
                        manager: GraphIdentityText::new("cargo")?,
                        name: GraphIdentityText::new("projectatlas-core")?,
                        manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
                    },
                },
                ReusableTargetSelector::Package {
                    package: PackageSelector {
                        manager: GraphIdentityText::new("cargo")?,
                        name: GraphIdentityText::new("projectatlas-core")?,
                        manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
                    },
                },
            ),
            (
                EntitySelector::Symbol {
                    symbol: symbol_selector("src/lib.rs")?,
                },
                selector,
            ),
        ];
        for (entity_selector, expected) in cases {
            let entity = GraphEntity::new(project()?, entity_selector, IndexGeneration::new(5))?;
            require(
                ReusableTargetSelector::for_entity(&entity)? == expected,
                "entity-derived reusable selector drifted",
            )?;
        }

        let project_entity =
            GraphEntity::new(project()?, EntitySelector::Project, IndexGeneration::new(5))?;
        require(
            RelationResolution::resolved(&project_entity).is_err(),
            "project aggregate was exposed as a direct source target",
        )?;
        let external_entity = GraphEntity::new(
            project()?,
            EntitySelector::External {
                external: ExternalSelector {
                    system: GraphIdentityText::new("cargo")?,
                    identity: GraphIdentityText::new("serde")?,
                },
            },
            IndexGeneration::new(5),
        )?;
        let external = RelationResolution::external(&external_entity)?;
        require(
            external.resolved_target().is_none(),
            "external resolution exposed a traversable local target",
        )?;
        require(
            RelationResolution::external(&project_entity).is_err(),
            "non-external entity was accepted as an external resolution",
        )?;

        let unresolved = RelationResolution::Unresolved {
            reference: GraphIdentityText::new("missing::target")?,
        };
        require(
            unresolved.resolved_target().is_none(),
            "unresolved relation exposed a traversable target",
        )?;
        let ambiguous = RelationResolution::Ambiguous {
            reference: GraphIdentityText::new("handler")?,
            candidates: NonZeroU32::new(2).ok_or("candidate count must be nonzero")?,
        };
        require(
            ambiguous.resolved_target().is_none(),
            "ambiguous relation exposed a traversable target",
        )?;
        Ok(())
    }

    #[test]
    fn legacy_relation_projection_remains_compatible() {
        for kind in [
            RelationKind::Contains,
            RelationKind::Imports,
            RelationKind::Calls,
            RelationKind::DependsOn,
        ] {
            assert_eq!(
                GraphRelationKind::from_legacy(kind).legacy_kind(),
                Some(kind)
            );
        }
        assert_eq!(
            GraphRelationKind::Extended(ExtendedRelationKind::Tests).legacy_kind(),
            None
        );
    }

    #[test]
    fn coverage_state_enforces_consistent_counts_and_reasons()
    -> Result<(), Box<dyn std::error::Error>> {
        let complete = CoverageRecord::new(
            CoverageScope::Project,
            None,
            CoverageState::Complete,
            8,
            0,
            IndexGeneration::new(9),
            None,
            None,
        )?;
        require(complete.total() == 8, "complete coverage total drifted")?;

        let partial = CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src"))?,
            },
            Some(GraphRelationKind::Extended(
                ExtendedRelationKind::References,
            )),
            CoverageState::Partial,
            5,
            3,
            IndexGeneration::new(9),
            Some(GraphIdentityText::new("parser limit reached")?),
            Some(GraphLimitKind::Rows),
        )?;
        require(
            (partial.covered(), partial.omitted(), partial.total()) == (5, 3, 8),
            "partial coverage counts are inconsistent",
        )?;

        require(
            CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Complete,
                7,
                1,
                IndexGeneration::new(9),
                None,
                None,
            )
            .is_err(),
            "complete coverage accepted omitted rows",
        )?;
        require(
            CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Failed,
                0,
                8,
                IndexGeneration::new(9),
                None,
                None,
            )
            .is_err(),
            "failed coverage accepted a missing reason",
        )?;
        require(
            CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Complete,
                1,
                0,
                IndexGeneration::ZERO,
                None,
                None,
            )
            .is_err(),
            "pre-publication coverage generation was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn graph_limits_are_nonzero_bounded_and_validated_on_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let limits = GraphLimits::new(100, 20, 4, 64 * 1_024)?;
        let encoded = serde_json::to_string(&limits)?;
        require(
            serde_json::from_str::<GraphLimits>(&encoded)? == limits,
            "graph limits did not round-trip",
        )?;
        require(
            GraphLimits::new(0, 20, 4, 1024).is_err(),
            "zero graph row limit was accepted",
        )?;
        require(
            GraphLimits::new(GraphLimits::MAX_ROWS + 1, 20, 4, 1024).is_err(),
            "graph row limit exceeded its hard ceiling",
        )?;
        require(
            serde_json::from_str::<GraphLimits>(
                r#"{"rows":1,"occurrences":1,"depth":65,"output_bytes":1}"#,
            )
            .is_err(),
            "deserialization bypassed graph depth limits",
        )?;
        Ok(())
    }
}
