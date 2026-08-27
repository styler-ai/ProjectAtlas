//! Purpose: Define `ProjectAtlas` 3 core domain models and shared helpers.

pub mod graph;
pub mod health;
pub mod index_work;
pub mod language;
pub mod optional_parser_pack;
pub mod optional_parser_protocol;
pub mod outline;
pub mod project_root;
pub mod relation_capabilities;
pub mod support_catalog;
pub mod symbols;
pub mod telemetry;
pub mod toon;

pub use index_work::{
    IndexCancellation, IndexWorkControl, IndexWorkFailure, IndexWorkResource, IndexWorkStage,
};
pub use project_root::CanonicalProjectRoot;

/// Maximum Git worktree registrations admitted for one repository.
pub const MAX_GIT_WORKTREE_REGISTRATIONS: usize = 1_024;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf, StripPrefixError};
use thiserror::Error;

/// Core error type for `ProjectAtlas` domain operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A project root does not satisfy the native absolute-path contract.
    #[error("invalid canonical project root {path:?}: {reason}")]
    InvalidCanonicalProjectRoot {
        /// Path rejected before it became a native identity.
        path: PathBuf,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// Canonicalization of a project root failed.
    #[error("could not canonicalize project root {path:?}: {source}")]
    CanonicalProjectRootIo {
        /// Path passed to the native canonicalizer.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// A persisted native-root codec value is not supported or lossless.
    #[error("invalid canonical project-root codec value: {reason}")]
    CanonicalProjectRootCodec {
        /// Stable decoding failure.
        reason: &'static str,
    },
    /// A path could not be represented relative to the repository root.
    #[error("path is outside the repository root: {path}")]
    PathOutsideRoot {
        /// Path that failed normalization.
        path: PathBuf,
        /// Original path-strip error.
        source: StripPrefixError,
    },
    /// A path contains non-UTF-8 data and cannot be stored in the index.
    #[error("path is not valid UTF-8: {path:?}")]
    NonUtf8Path {
        /// Path that could not be converted to UTF-8.
        path: PathBuf,
    },
    /// A user supplied path is not a safe repository-relative file key.
    #[error("path {path:?} must be a project-relative indexed file path: {reason}")]
    InvalidRepositoryPath {
        /// Invalid path.
        path: PathBuf,
        /// Human-readable validation reason.
        reason: &'static str,
    },
}

/// Convenient result alias for `ProjectAtlas` core operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Monotonic identity of one completely published derived index.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IndexGeneration(u64);

impl IndexGeneration {
    /// Generation before the first complete publication.
    pub const ZERO: Self = Self(0);

    /// Construct a generation from its durable integer representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the durable integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advance to the next complete publication generation.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl fmt::Display for IndexGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// File or folder node kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// Directory node.
    Folder,
    /// File node.
    File,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Folder => formatter.write_str("folder"),
            Self::File => formatter.write_str("file"),
        }
    }
}

impl NodeKind {
    /// Parse a database string into a node kind.
    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// Status for purpose metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PurposeStatus {
    /// No purpose exists for this node yet.
    Missing,
    /// A generated or heuristic purpose exists but has not been approved.
    Suggested,
    /// A purpose has been explicitly approved.
    Approved,
    /// A legacy or explicitly flagged accepted purpose awaits explicit review.
    ///
    /// Normal source, hash, summary, symbol, and graph changes never create
    /// this state or demote an approved purpose.
    Stale,
}

impl fmt::Display for PurposeStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PurposeStatus {
    /// Return the stable database and payload value for this purpose status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Suggested => "suggested",
            Self::Approved => "approved",
            Self::Stale => "stale",
        }
    }

    /// Parse a database string into a purpose status.
    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            value if value == Self::Missing.as_str() => Some(Self::Missing),
            value if value == Self::Suggested.as_str() => Some(Self::Suggested),
            value if value == Self::Approved.as_str() => Some(Self::Approved),
            value if value == Self::Stale.as_str() => Some(Self::Stale),
            _ => None,
        }
    }
}

/// Source for purpose metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PurposeSource {
    /// No source is known yet.
    Missing,
    /// Imported from legacy metadata such as `.purpose` or Purpose headers.
    Imported,
    /// Generated by a heuristic.
    Generated,
    /// Explicitly set by an agent after inspecting enough context.
    Agent,
}

impl fmt::Display for PurposeSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PurposeSource {
    /// Return the stable database and payload value for this purpose source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Imported => "imported",
            Self::Generated => "generated",
            Self::Agent => "agent",
        }
    }
}

/// Agent-facing priority for purpose curation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PurposeReviewPriority {
    /// Curate during the default folder-first queue.
    High,
    /// Skip unless broad file-purpose cleanup was explicitly requested.
    Low,
}

impl fmt::Display for PurposeReviewPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => formatter.write_str("high"),
            Self::Low => formatter.write_str("low"),
        }
    }
}

/// Review signal used by agent-facing purpose curation queues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PurposeReviewSignal {
    /// Priority shown to the agent.
    pub priority: PurposeReviewPriority,
    /// Stable reason string explaining why the path is queued.
    pub reason: &'static str,
}

/// Repository node stored in the `ProjectAtlas` index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Node {
    /// Repository-relative path using forward slashes.
    pub path: String,
    /// File or folder kind.
    pub kind: NodeKind,
    /// Parent path using forward slashes.
    pub parent_path: Option<String>,
    /// File extension, including the dot.
    pub extension: Option<String>,
    /// Detected language or file family.
    pub language: Option<String>,
    /// File size in bytes.
    pub size_bytes: Option<u64>,
    /// File modification timestamp in nanoseconds since Unix epoch.
    pub mtime_ns: Option<i64>,
    /// BLAKE3 hash for file content.
    pub content_hash: Option<String>,
}

/// Purpose metadata attached to a node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Purpose {
    /// Repository-relative node path.
    pub path: String,
    /// Purpose one-liner.
    pub purpose: Option<String>,
    /// Purpose source.
    pub source: PurposeSource,
    /// Purpose lifecycle status.
    pub status: PurposeStatus,
}

impl Purpose {
    /// Return whether this purpose was explicitly approved by an agent.
    #[must_use]
    pub fn agent_reviewed(&self) -> bool {
        self.status == PurposeStatus::Approved && self.source == PurposeSource::Agent
    }
}

/// Return the purpose-curation review signal for an indexed node.
#[must_use]
pub fn purpose_review_signal(node: &Node, purpose: &Purpose) -> PurposeReviewSignal {
    if node.kind == NodeKind::Folder {
        return PurposeReviewSignal {
            priority: PurposeReviewPriority::High,
            reason: "folder_navigation",
        };
    }

    if node.kind == NodeKind::File
        && purpose.status == PurposeStatus::Stale
        && purpose.source == PurposeSource::Agent
        && is_high_impact_file_path(&node.path)
    {
        return PurposeReviewSignal {
            priority: PurposeReviewPriority::High,
            reason: "stale_agent_reviewed_file",
        };
    }

    if node.kind == NodeKind::File && is_high_impact_file_path(&node.path) {
        return PurposeReviewSignal {
            priority: PurposeReviewPriority::High,
            reason: "high_impact_file",
        };
    }

    if node.kind == NodeKind::File && purpose.status == PurposeStatus::Suggested {
        return PurposeReviewSignal {
            priority: PurposeReviewPriority::Low,
            reason: "generated_file_suggestion",
        };
    }

    PurposeReviewSignal {
        priority: PurposeReviewPriority::Low,
        reason: "selective_file_review",
    }
}

/// Return whether a file path is important enough for default purpose curation.
#[must_use]
pub fn is_high_impact_file_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    HIGH_IMPACT_FILE_NAMES.contains(&file_name)
        || HIGH_IMPACT_PATH_PREFIXES
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
        || HIGH_IMPACT_PATH_SEGMENTS
            .iter()
            .any(|segment| normalized.contains(segment))
}

/// File names that belong in default file-purpose curation.
pub const HIGH_IMPACT_FILE_NAMES: &[&str] = &[
    "cargo.toml",
    "package.json",
    "pyproject.toml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "gradle.properties",
    "dockerfile",
    "makefile",
    "justfile",
    "main.rs",
    "lib.rs",
    "mod.rs",
    "main.py",
    "app.py",
    "server.py",
    "index.ts",
    "main.ts",
    "server.ts",
    "app.ts",
    "index.tsx",
    "app.tsx",
];

/// Path prefixes that belong in default file-purpose curation.
pub const HIGH_IMPACT_PATH_PREFIXES: &[&str] = &[".github/workflows/"];

/// Path segments that belong in default file-purpose curation.
pub const HIGH_IMPACT_PATH_SEGMENTS: &[&str] = &["/migrations/", "/routes/", "/commands/", "/mcp"];

/// Legacy stored source value used by older approved human-curated purposes.
pub const LEGACY_HUMAN_PURPOSE_SOURCE: &str = "human";

/// Stored purpose source values that represent reviewed agent-owned purposes.
pub const AGENT_REVIEWED_SOURCE_VALUES: &[&str] =
    &[PurposeSource::Agent.as_str(), LEGACY_HUMAN_PURPOSE_SOURCE];

/// A node with attached purpose state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexedNode {
    /// Node metadata.
    pub node: Node,
    /// Purpose metadata.
    pub purpose: Purpose,
    /// One-line observed content summary for this node.
    pub summary: Option<String>,
}

/// Compact deterministic reasons used by agent-facing repository ranking.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RankedReasonCode {
    /// The normalized query exactly selected the repository path.
    ExactPath,
    /// The normalized query exactly selected the final path component.
    ExactName,
    /// An agent-approved responsibility purpose matched the query.
    ReviewedPurpose,
    /// Repository path text contributed weaker lexical evidence.
    Path,
    /// Observed summary text contributed weaker lexical evidence.
    Summary,
    /// An indexed symbol contributed weaker lexical evidence.
    Symbol,
    /// Persisted source text contributed weaker lexical evidence.
    IndexedText,
    /// A conventional source or test counterpart was present.
    PairedFile,
    /// Current package/dependency context contributed graph evidence.
    GraphPackage,
    /// Current import context contributed graph evidence.
    GraphImport,
    /// Current call context contributed graph evidence.
    GraphCall,
    /// Current reference context contributed graph evidence.
    GraphReference,
    /// Current test context contributed graph evidence.
    GraphTest,
    /// Current route context contributed graph evidence.
    GraphRoute,
    /// Current configuration context contributed graph evidence.
    GraphConfig,
}

/// Closed connection families exposed by folder and file navigation rows.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RankedConnectionKind {
    /// Package or manifest dependency context.
    Package,
    /// Source import context.
    Import,
    /// Static call context.
    Call,
    /// Static reference context.
    Reference,
    /// Test-to-source context.
    Test,
    /// Route or protocol context.
    Route,
    /// Configuration context.
    Config,
}

/// Direction of one sampled relationship relative to the ranked node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RankedConnectionDirection {
    /// The ranked node owns the relation source.
    Outbound,
    /// The ranked node owns the resolved relation target.
    Inbound,
}

/// Compact typed target for a sampled ranked-node connection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RankedConnectionTarget {
    /// A repository-local file or declaration.
    Local {
        /// Exact repository-relative source path.
        path: String,
        /// Declaration name when the target is a symbol.
        symbol: Option<String>,
    },
    /// A manifest-owned package identity.
    Package {
        /// Package ecosystem or manifest family.
        manager: String,
        /// Package name declared by the manifest.
        name: String,
        /// Exact repository-relative owning manifest.
        manifest: String,
    },
    /// A typed target outside the selected repository.
    External {
        /// External namespace.
        system: String,
        /// Identity inside the external namespace.
        identity: String,
    },
    /// A static reference that could not be resolved uniquely.
    Unresolved {
        /// Bounded reference identity retained by graph persistence.
        reference: String,
    },
}

/// One bounded high-value connection sampled for a ranked node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RankedConnection {
    /// Closed relationship family.
    pub kind: RankedConnectionKind,
    /// Direction relative to the ranked node.
    pub direction: RankedConnectionDirection,
    /// Typed compact target or source at the other end.
    pub target: RankedConnectionTarget,
}

/// Bounded count metadata for one connection family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RankedConnectionCount {
    /// Closed relationship family.
    pub kind: RankedConnectionKind,
    /// Number of validated rows observed inside the family bound.
    pub count: usize,
    /// Whether at least one additional row exists for this family.
    pub truncated: bool,
}

/// Existing navigation capability recommended after one ranked row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationNextCapability {
    /// Narrow a selected folder to indexed files.
    Files,
    /// Inspect one selected file summary.
    Summary,
    /// Inspect detailed typed relations after a connection sample truncates.
    Relations,
    /// Inspect bounded structural or coverage health for the selected path.
    Health,
}

/// Directly reusable next navigation call for one ranked row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NavigationNextCall {
    /// Existing capability to invoke next.
    pub capability: NavigationNextCapability,
    /// Exact repository-relative path accepted by that capability.
    pub path: String,
}

/// A ranked node with concise evidence for why it was selected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RankedNode {
    /// Selected indexed node.
    pub node: IndexedNode,
    /// Bounded human-readable ranking signals.
    pub reasons: Vec<String>,
    /// Bounded stable ranking signals for programmatic consumers.
    pub reason_codes: Vec<RankedReasonCode>,
    /// Sparse stable-order connection counts.
    pub connection_counts: Vec<RankedConnectionCount>,
    /// Bounded high-value current connection sample.
    pub connections: Vec<RankedConnection>,
    /// Whether the bounded sample omitted any validated relation through family or global overflow.
    pub connections_truncated: bool,
    /// Existing navigation capability recommended after this row.
    pub next_call: NavigationNextCall,
}

/// Overview returned by startup/overview commands.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Overview {
    /// Number of indexed files.
    pub files: usize,
    /// Number of indexed folders.
    pub folders: usize,
    /// Number of missing purpose entries.
    pub missing_purposes: usize,
    /// Number of stale purpose entries.
    pub stale_purposes: usize,
    /// Number of approved purpose entries.
    pub approved_purposes: usize,
    /// Number of suggested purpose entries.
    pub suggested_purposes: usize,
}

/// Convert an absolute path into a stable repository-relative slash path.
///
/// # Errors
///
/// Returns an error when `path` is outside `root` or cannot be represented as
/// UTF-8.
pub fn normalize_repo_path(root: &Path, path: &Path) -> CoreResult<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|source| CoreError::PathOutsideRoot {
            path: path.to_path_buf(),
            source,
        })?;
    if relative.as_os_str().is_empty() {
        return Ok(".".to_string());
    }
    let as_str = relative.to_str().ok_or_else(|| CoreError::NonUtf8Path {
        path: relative.to_path_buf(),
    })?;
    Ok(as_str.replace('\\', "/"))
}

/// Normalize a native filesystem path for stable diagnostics and metadata.
///
/// On Windows, the returned path uses forward slashes, strips extended path
/// prefixes such as `\\?\`, and converts extended UNC paths to
/// `//server/share` form. On Unix, native backslashes are preserved because
/// they are valid filename characters rather than separators. This helper is
/// for legacy compatibility metadata and agent-facing output; use
/// [`CanonicalProjectRoot`] for persisted project identity and `Path`/`PathBuf`
/// for host filesystem access.
#[must_use]
pub fn normalize_native_path_display(path: impl AsRef<Path>) -> String {
    normalize_native_path_display_str(&path.as_ref().to_string_lossy())
}

/// Normalize a native filesystem path string for stable diagnostics and metadata.
///
/// This string-oriented variant exists for values read from metadata or tests
/// before they are converted back into a platform `Path`.
#[must_use]
pub fn normalize_native_path_display_str(path: &str) -> String {
    #[cfg(windows)]
    {
        let normalized = path.replace('\\', "/");
        if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
            format!("//{rest}")
        } else if let Some(rest) = normalized.strip_prefix("//?/") {
            rest.to_string()
        } else {
            normalized
        }
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

/// Return a lossless UTF-8 display projection for a native path.
///
/// Windows extended prefixes are normalized only when the conversion keeps
/// the path absolute and does not discard Win32 verbatim semantics. A native
/// path that cannot be represented as UTF-8 returns [`CoreError::NonUtf8Path`]
/// instead of a replacement-character path that could select a different
/// filesystem object.
///
/// # Errors
///
/// Returns [`CoreError::NonUtf8Path`] when `path` contains native data that is
/// not losslessly representable as UTF-8.
pub fn lossless_native_path_display(path: &Path) -> CoreResult<String> {
    let original = path.to_str().ok_or_else(|| CoreError::NonUtf8Path {
        path: path.to_path_buf(),
    })?;
    let normalized = normalize_native_path_display_str(original);
    if Path::new(&normalized).is_absolute() && !windows_verbatim_semantics_require_prefix(path) {
        Ok(normalized)
    } else {
        Ok(original.to_owned())
    }
}

#[cfg(windows)]
/// Preserve the extended prefix when a component relies on Win32 verbatim semantics.
fn windows_verbatim_semantics_require_prefix(path: &Path) -> bool {
    use std::path::Component;

    let Some(value) = path.to_str() else {
        return true;
    };
    if !value.starts_with("\\\\?\\") {
        return false;
    }

    // A project root is immediately extended with ProjectAtlas children.
    // Preserve the namespace before the ordinary Win32 limit is reached so
    // that the child path remains usable without changing its semantics.
    let normalized = normalize_native_path_display_str(value);
    let project_atlas_suffix_units = r"\.projectatlas\projectatlas.db".encode_utf16().count();
    if normalized
        .encode_utf16()
        .count()
        .saturating_add(project_atlas_suffix_units)
        >= 260
    {
        return true;
    }

    path.components().any(|component| {
        let Component::Normal(component) = component else {
            return false;
        };
        let Some(component) = component.to_str() else {
            return true;
        };
        if component.ends_with(['.', ' ']) {
            return true;
        }
        let name = component
            .split_once('.')
            .map_or(component, |(stem, _)| stem);
        let upper = name.to_ascii_uppercase();
        matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || matches!(
                upper.as_str(),
                "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
            )
            || (upper.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && upper.as_bytes()[3].is_ascii_digit()
                && upper.as_bytes()[3] != b'0')
    })
}

#[cfg(not(windows))]
/// Unix and fallback hosts do not assign Win32 verbatim semantics to paths.
fn windows_verbatim_semantics_require_prefix(_path: &Path) -> bool {
    false
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
///
/// # Errors
///
/// Returns an error when `file` is absolute, uses a Windows drive prefix,
/// contains parent traversal, is empty, or cannot be represented as UTF-8.
pub fn validated_repo_file_key(file: &Path) -> CoreResult<String> {
    let key = validated_repo_node_key(file)?;
    if key == "." {
        return Err(CoreError::InvalidRepositoryPath {
            path: file.to_path_buf(),
            reason: "a file path is required",
        });
    }
    Ok(key)
}

/// Normalize and validate a user-supplied path as a repository-relative node key.
///
/// Unlike [`validated_repo_file_key`], this accepts `.` for the repository root
/// folder so purpose metadata can be set on either folders or files.
///
/// # Errors
///
/// Returns an error when `file` is absolute, uses a Windows drive prefix,
/// contains parent traversal, is empty, or cannot be represented as UTF-8.
pub fn validated_repo_node_key(file: &Path) -> CoreResult<String> {
    let raw = file
        .to_str()
        .ok_or_else(|| CoreError::NonUtf8Path {
            path: file.to_path_buf(),
        })?
        .replace('\\', "/");
    if raw.trim().is_empty() {
        return Err(CoreError::InvalidRepositoryPath {
            path: file.to_path_buf(),
            reason: "a path is required",
        });
    }
    if raw.starts_with('/') || raw.starts_with("//") || has_windows_drive_prefix(&raw) {
        return Err(CoreError::InvalidRepositoryPath {
            path: file.to_path_buf(),
            reason: "absolute paths are not allowed",
        });
    }
    let mut parts = Vec::new();
    for component in raw.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return Err(CoreError::InvalidRepositoryPath {
                    path: file.to_path_buf(),
                    reason: "parent traversal is not allowed",
                });
            }
            part => parts.push(part.to_string()),
        }
    }
    if parts.is_empty() {
        return Ok(".".to_string());
    }
    Ok(parts.join("/"))
}

/// Convert a stable slash-separated repository key into a native path.
#[must_use]
pub fn repo_path_to_native(path: &str) -> PathBuf {
    path.split('/').fold(PathBuf::new(), |mut native, part| {
        native.push(part);
        native
    })
}

/// Normalize a repository-relative path prefix used by query filters.
///
/// This helper accepts `.` and empty prefixes because filter callers often use
/// them to mean the repository root. Exact file reads should still use
/// [`validated_repo_file_key`] so absolute paths and traversal are rejected.
#[must_use]
pub fn normalize_repo_path_prefix(value: &str) -> String {
    let normalized = value
        .replace('\\', "/")
        .trim()
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string();
    if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    }
}

/// Return whether normalized text starts with a Windows drive prefix.
fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

/// Return the parent path for a normalized repository path.
#[must_use]
pub fn normalized_parent(path: &str) -> Option<String> {
    if path == "." {
        return None;
    }
    let parent = Path::new(path).parent()?;
    if parent.as_os_str().is_empty() {
        Some(".".to_string())
    } else {
        Some(parent.to_string_lossy().replace('\\', "/"))
    }
}

/// Return a normalized extension for indexing.
#[must_use]
pub fn normalized_extension(path: &Path) -> Option<String> {
    language::normalized_language_extension(path)
}

#[cfg(test)]
mod tests {
    use super::{
        Node, NodeKind, Purpose, PurposeReviewPriority, PurposeSource, PurposeStatus,
        is_high_impact_file_path, normalize_native_path_display_str, normalize_repo_path_prefix,
        normalized_parent, purpose_review_signal, repo_path_to_native, validated_repo_file_key,
        validated_repo_node_key,
    };
    use std::io;
    use std::path::Path;

    #[test]
    fn validated_repo_file_key_normalizes_safe_relative_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        require_eq(
            &validated_repo_file_key(Path::new("src\\main.rs"))?,
            "src/main.rs",
        )?;
        require_eq(
            &validated_repo_file_key(Path::new("./src/lib.rs"))?,
            "src/lib.rs",
        )?;
        Ok(())
    }

    #[test]
    fn validated_repo_file_key_rejects_absolute_and_parent_paths() {
        assert!(validated_repo_file_key(Path::new("../secret.rs")).is_err());
        assert!(validated_repo_file_key(Path::new("C:/secret.rs")).is_err());
        assert!(validated_repo_file_key(Path::new("/secret.rs")).is_err());
        assert!(validated_repo_file_key(Path::new(".")).is_err());
    }

    #[test]
    fn validated_repo_node_key_accepts_root_and_relative_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        require_eq(&validated_repo_node_key(Path::new("."))?, ".")?;
        require_eq(&validated_repo_node_key(Path::new("./src"))?, "src")?;
        require_eq(
            &validated_repo_node_key(Path::new("src\\main.rs"))?,
            "src/main.rs",
        )?;
        Ok(())
    }

    #[test]
    fn validated_repo_node_key_rejects_empty_paths() {
        assert!(validated_repo_node_key(Path::new("")).is_err());
        assert!(validated_repo_node_key(Path::new("   ")).is_err());
    }

    #[test]
    fn repo_path_to_native_builds_platform_path_components() {
        assert_eq!(
            repo_path_to_native("src/main.rs"),
            Path::new("src").join("main.rs")
        );
    }

    #[test]
    fn normalize_repo_path_prefix_accepts_root_and_slashes() {
        assert_eq!(normalize_repo_path_prefix(""), ".");
        assert_eq!(normalize_repo_path_prefix("."), ".");
        assert_eq!(normalize_repo_path_prefix(".\\docs\\api\\"), "docs/api");
        assert_eq!(normalize_repo_path_prefix("./src/lib"), "src/lib");
    }

    #[test]
    fn purpose_review_signal_is_folder_first_and_file_selective() {
        let folder = test_node("src", NodeKind::Folder);
        let file = test_node("src/helper.rs", NodeKind::File);
        let build_file = test_node("build.gradle.kts", NodeKind::File);
        let suggested = Purpose {
            path: "src/helper.rs".to_string(),
            purpose: Some("Generated helper suggestion".to_string()),
            source: PurposeSource::Generated,
            status: PurposeStatus::Suggested,
        };
        let approved = Purpose {
            path: "src".to_string(),
            purpose: Some("Rust source folder".to_string()),
            source: PurposeSource::Agent,
            status: PurposeStatus::Approved,
        };
        let stale = Purpose {
            path: "src/helper.rs".to_string(),
            purpose: Some("Reviewed helper implementation".to_string()),
            source: PurposeSource::Agent,
            status: PurposeStatus::Stale,
        };

        let folder_signal = purpose_review_signal(&folder, &approved);
        assert_eq!(folder_signal.priority, PurposeReviewPriority::High);
        assert_eq!(folder_signal.reason, "folder_navigation");

        let file_signal = purpose_review_signal(&file, &suggested);
        assert_eq!(file_signal.priority, PurposeReviewPriority::Low);
        assert_eq!(file_signal.reason, "generated_file_suggestion");

        let build_signal = purpose_review_signal(&build_file, &suggested);
        assert_eq!(build_signal.priority, PurposeReviewPriority::High);
        assert_eq!(build_signal.reason, "high_impact_file");

        let low_stale_signal = purpose_review_signal(&file, &stale);
        assert_eq!(low_stale_signal.priority, PurposeReviewPriority::Low);
        assert_eq!(low_stale_signal.reason, "selective_file_review");

        let high_stale_signal = purpose_review_signal(&build_file, &stale);
        assert_eq!(high_stale_signal.priority, PurposeReviewPriority::High);
        assert_eq!(high_stale_signal.reason, "stale_agent_reviewed_file");
        assert!(is_high_impact_file_path(".github/workflows/release.yml"));
    }

    #[cfg(windows)]
    #[test]
    fn native_path_display_removes_windows_extended_prefixes() {
        assert_eq!(
            normalize_native_path_display_str(r"\\?\C:\repo\.projectatlas\projectatlas.db"),
            "C:/repo/.projectatlas/projectatlas.db"
        );
        assert_eq!(
            normalize_native_path_display_str(r"\\?\UNC\server\share\repo"),
            "//server/share/repo"
        );
        assert_eq!(
            normalize_native_path_display_str("/home/user/repo"), // projectatlas: path-fixture
            "/home/user/repo"                                     // projectatlas: path-fixture
        );
        assert_eq!(
            normalize_native_path_display_str("src\\main.rs"),
            "src/main.rs"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn native_path_display_preserves_unix_backslashes() {
        let path = r"/tmp/repo\name";
        assert_eq!(normalize_native_path_display_str(path), path);
        assert_eq!(super::normalize_native_path_display(Path::new(path)), path);
    }

    fn require_eq(left: &str, right: &str) -> Result<(), Box<dyn std::error::Error>> {
        if left == right {
            Ok(())
        } else {
            Err(io::Error::other(format!("expected {right:?}, found {left:?}")).into())
        }
    }

    fn test_node(path: &str, kind: NodeKind) -> Node {
        Node {
            path: path.to_string(),
            kind,
            parent_path: normalized_parent(path),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: None,
            content_hash: None,
        }
    }
}
