//! Purpose: Scan repository files and folders for `ProjectAtlas` 3.

pub mod worktree;

mod git_config;
mod policy_match;
mod regular_file;
mod source_reader;

pub use policy_match::{PolicyMatchError, PolicyMatchLimits, PolicyMatchResource, RootScanMatcher};
pub use regular_file::open_regular_file;

use blake3::Hasher;
use ignore::{DirEntry, WalkBuilder, WalkState};
use projectatlas_core::language::{
    LANGUAGE_CONTENT_DETECTION_MAX_BYTES, LanguageDetection, LanguageDetectionRequest,
    detect_language_request, language_capability,
};
use projectatlas_core::{
    CoreError, IndexCancellation, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
    IndexWorkStage, MAX_GIT_WORKTREE_REGISTRATIONS, Node, NodeKind, normalize_repo_path,
    normalized_extension, normalized_parent,
};
use source_reader::SourceRootReader;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Reserved metadata files that should not become indexed project nodes.
const RESERVED_METADATA_FILE_NAMES: &[&str] = &[".purpose"];

/// Durable `.projectatlas` inputs that are part of the project contract.
const INDEXED_PROJECTATLAS_INPUT_PATHS: &[&str] = &[
    ".projectatlas",
    ".projectatlas/config.toml",
    ".projectatlas/projectatlas-nonsource-files.toon",
    ".projectatlas/projectatlas-purpose-review.json",
];

/// Maximum worker count used even when callers request more host parallelism.
const SCAN_WORKER_SAFE_CEILING: usize = 32;
/// Default maximum repository entries considered by one scan.
const DEFAULT_SCAN_MAX_ENTRIES: u64 = 1_000_000;
/// Default maximum source bytes hashed by one scan.
const DEFAULT_SCAN_MAX_SOURCE_BYTES: u64 = 16 * 1_024 * 1_024 * 1_024;
/// Default deadline for the compatibility repository scan.
const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_mins(30);
/// Exact-byte hash read buffer size.
const HASH_BUFFER_BYTES: usize = 8_192;
/// Maximum linked-worktree `.git` pointer bytes inspected for policy discovery.
const GIT_DIRECTORY_POINTER_MAX_BYTES: u64 = 64 * 1_024;
/// Filesystem scanner errors.
#[derive(Debug, Error)]
pub enum FsError {
    /// Core normalization failed.
    #[error("{0}")]
    Core(#[from] CoreError),
    /// Filesystem operation failed.
    #[error("filesystem error for {path:?}: {source}")]
    Io {
        /// Path involved in the error.
        path: PathBuf,
        /// Source IO error.
        source: io::Error,
    },
    /// The supplied root is not a directory.
    #[error("scan root is not a directory: {0:?}")]
    RootNotDirectory(PathBuf),
    /// A Git worktree boundary could not be validated safely.
    #[error("repository boundary could not be validated for {path:?}: {source}")]
    RepositoryBoundary {
        /// Git control path involved in the boundary failure.
        path: PathBuf,
        /// Source boundary error.
        source: io::Error,
    },
    /// Cooperative indexing work was canceled or exceeded a declared bound.
    #[error("{0}")]
    IndexWork(#[from] IndexWorkFailure),
}

/// Convenient result alias for scanner operations.
pub type FsResult<T> = Result<T, FsError>;

/// Repository scanner configuration.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Additional directory names to exclude.
    pub exclude_dir_names: Vec<String>,
    /// Additional directory suffixes to exclude.
    pub exclude_dir_suffixes: Vec<String>,
    /// Repository-relative path prefixes to exclude.
    pub exclude_path_prefixes: Vec<String>,
    /// Explicit filename or extension selectors mapped to canonical language IDs.
    pub language_overrides: BTreeMap<String, String>,
    /// Whether registry-known optional languages may be assigned to scanned files.
    pub admit_optional_languages: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            exclude_dir_names: vec![
                ".git".to_string(),
                ".projectatlas".to_string(),
                ".venv".to_string(),
                "__pycache__".to_string(),
                "node_modules".to_string(),
                "dist".to_string(),
                "build".to_string(),
                "target".to_string(),
            ],
            exclude_dir_suffixes: Vec::new(),
            exclude_path_prefixes: Vec::new(),
            language_overrides: BTreeMap::new(),
            admit_optional_languages: false,
        }
    }
}

impl ScanOptions {
    /// Return whether a repository-relative slash path is excluded.
    #[must_use]
    pub fn excludes_relative_path(&self, relative_path: &str) -> bool {
        if is_indexed_projectatlas_input(relative_path) {
            return false;
        }
        has_excluded_directory_component(relative_path, self)
            || has_excluded_path_prefix(relative_path, self)
    }
}

/// Canonical selected root plus its resolved repository exclusion policy.
#[derive(Clone, Debug)]
pub struct RootScanPolicy {
    /// Canonical selected source root.
    root: PathBuf,
    /// Absolute selected spelling when it differs from the canonical root.
    selected_root_alias: Option<PathBuf>,
    /// Caller options extended with repository-derived exclusions.
    options: ScanOptions,
    /// One shared walker configuration, including compiled external ignore rules.
    walker: WalkBuilder,
}

impl RootScanPolicy {
    /// Discover bounded repository policy once for one selected scan root.
    ///
    /// # Errors
    ///
    /// Returns an error when the root is invalid, repository boundary metadata
    /// cannot be validated, or cooperative work has stopped.
    pub fn discover(
        root: &Path,
        options: &ScanOptions,
        control: &IndexWorkControl,
    ) -> FsResult<Self> {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        if root.as_os_str().is_empty() {
            return Err(FsError::RootNotDirectory(root.to_path_buf()));
        }
        let selected_root: Cow<'_, Path> = if root.is_absolute() {
            Cow::Borrowed(root)
        } else {
            let anchored = std::env::current_dir()
                .map_err(|source| FsError::Io {
                    path: root.to_path_buf(),
                    source,
                })?
                .join(root);
            // Drive-relative Windows paths need the native per-drive context.
            Cow::Owned(if anchored.is_absolute() {
                anchored
            } else {
                std::path::absolute(root).map_err(|source| FsError::Io {
                    path: root.to_path_buf(),
                    source,
                })?
            })
        };
        #[cfg(windows)]
        let selected_root: Cow<'_, Path> = Cow::Owned(
            std::path::absolute(selected_root.as_ref()).map_err(|source| FsError::Io {
                path: root.to_path_buf(),
                source,
            })?,
        );
        if !selected_root.is_dir() {
            return Err(FsError::RootNotDirectory(root.to_path_buf()));
        }
        let canonical_root = selected_root.canonicalize().map_err(|source| FsError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let selected_root_alias = (selected_root.as_ref() != canonical_root.as_path())
            .then(|| selected_root.into_owned());
        let root = canonical_root;
        let options = scan_options_for_root(&root, options, control)?;
        for ancestor in root.ancestors() {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            validate_local_ignore_inputs(ancestor, true, true)?;
        }
        let mut walker = WalkBuilder::new(&root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(false)
            .require_git(false)
            .current_dir(root.clone());
        let external = git_config::ExternalGitConfig::discover(&root, control)?;
        if let Some(path) = external.excludes {
            match fs::metadata(&path) {
                Ok(metadata) => {
                    git_config::require_regular_file(&path, &metadata)?;
                    if let Some(error) = walker.add_ignore(&path) {
                        return Err(FsError::Io {
                            path,
                            source: io::Error::other(error),
                        });
                    }
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(FsError::Io { path, source }),
            }
        }
        Ok(Self {
            root,
            selected_root_alias,
            options,
            walker,
        })
    }

    /// Return whether one repository-contained path is excluded by the
    /// effective scanner policy, including rules that match an absent leaf.
    /// Absolute paths beneath the original selected root spelling are rebased
    /// onto its canonical root without resolving target components.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be normalized or an ignore
    /// source cannot be parsed safely.
    pub fn excludes_path(&self, path: &Path) -> FsResult<bool> {
        let absolute = self.absolute_target(path);
        if should_skip_path(&self.root, &absolute, &self.options) {
            return Ok(true);
        }
        standard_ignore_excludes_path(&self.root, &absolute, &self.walker, true, true)
    }

    /// Rebase selected-root aliases without resolving target components.
    fn absolute_target(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            match self
                .selected_root_alias
                .as_deref()
                .and_then(|selected| path.strip_prefix(selected).ok())
            {
                Some(relative) => self.root.join(relative),
                None => path.to_path_buf(),
            }
        } else {
            self.root.join(path)
        }
    }
}

/// Hard repository-scan resource limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanLimits {
    /// Maximum repository entries considered, including folders.
    entries: u64,
    /// Maximum cumulative file bytes admitted for exact hashing.
    source_bytes: u64,
    /// Maximum requested scanner workers before host and safety caps.
    workers: usize,
}

impl ScanLimits {
    /// Create explicit repository-scan limits.
    #[must_use]
    pub const fn new(max_entries: u64, max_source_bytes: u64, max_workers: usize) -> Self {
        Self {
            entries: max_entries,
            source_bytes: max_source_bytes,
            workers: max_workers,
        }
    }

    /// Return the maximum repository entries considered.
    #[must_use]
    pub const fn max_entries(self) -> u64 {
        self.entries
    }

    /// Return the maximum cumulative source bytes hashed.
    #[must_use]
    pub const fn max_source_bytes(self) -> u64 {
        self.source_bytes
    }

    /// Return the requested maximum worker count.
    #[must_use]
    pub const fn max_workers(self) -> usize {
        self.workers
    }

    /// Derive the worker count from the request, host availability, and safety cap.
    #[must_use]
    pub fn effective_workers(self) -> usize {
        let available = thread::available_parallelism().map_or(1, usize::from);
        self.workers.min(available).min(SCAN_WORKER_SAFE_CEILING)
    }
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self::new(
            DEFAULT_SCAN_MAX_ENTRIES,
            DEFAULT_SCAN_MAX_SOURCE_BYTES,
            SCAN_WORKER_SAFE_CEILING,
        )
    }
}

/// Resource work completed by one successful repository scan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanWork {
    /// Repository entries admitted by the scan budget, including folders.
    pub entries: u64,
    /// Exact source bytes admitted by the scan budget while hashing files.
    pub source_bytes: u64,
}

/// Complete nodes and resource work from one successful repository scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanOutcome {
    /// Complete sorted repository nodes.
    pub nodes: Vec<Node>,
    /// Resource work consumed while producing the nodes.
    pub work: ScanWork,
}

/// Apply the operation-owned worker ceiling to the scan-specific limits.
fn effective_scan_workers(limits: ScanLimits, control: &IndexWorkControl) -> usize {
    let workers = limits.effective_workers();
    control
        .worker_ceiling()
        .map_or(workers, |ceiling| workers.min(ceiling))
}

/// Shared counters and controls for one repository scan.
#[derive(Debug)]
struct ScanBudget {
    /// Hard resource limits for the scan.
    limits: ScanLimits,
    /// Shared cancellation and deadline boundary.
    control: IndexWorkControl,
    /// Repository entries admitted so far.
    entries: AtomicU64,
    /// File bytes admitted for hashing so far.
    source_bytes: AtomicU64,
}

impl ScanBudget {
    /// Create an unused scan budget.
    fn new(limits: ScanLimits, control: IndexWorkControl) -> Self {
        Self {
            limits,
            control,
            entries: AtomicU64::new(0),
            source_bytes: AtomicU64::new(0),
        }
    }

    /// Check traversal state and admit one repository entry.
    fn claim_entry(&self) -> Result<(), IndexWorkFailure> {
        self.control.check(IndexWorkStage::RepositoryTraversal)?;
        claim_resource(
            &self.entries,
            1,
            self.limits.entries,
            IndexWorkStage::RepositoryTraversal,
            IndexWorkResource::Entries,
        )
    }

    /// Admit source bytes before starting exact content hashing.
    fn claim_source_bytes(&self, bytes: u64) -> Result<(), IndexWorkFailure> {
        claim_resource(
            &self.source_bytes,
            bytes,
            self.limits.source_bytes,
            IndexWorkStage::SourceHash,
            IndexWorkResource::SourceBytes,
        )
    }

    /// Snapshot the admitted work after a scan has completed.
    fn work(&self) -> ScanWork {
        ScanWork {
            entries: self.entries.load(Ordering::Relaxed),
            source_bytes: self.source_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Atomically claim an inclusive bounded resource amount.
fn claim_resource(
    counter: &AtomicU64,
    amount: u64,
    limit: u64,
    stage: IndexWorkStage,
    resource: IndexWorkResource,
) -> Result<(), IndexWorkFailure> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current
                .checked_add(amount)
                .filter(|observed| *observed <= limit)
        })
        .map(|_previous| ())
        .map_err(|current| {
            IndexWorkFailure::resource_limit(stage, resource, limit, current.saturating_add(amount))
        })
}

/// Scan a repository into `ProjectAtlas` nodes.
///
/// # Errors
///
/// Returns an error when the root is invalid or filesystem metadata cannot be
/// read.
pub fn scan_repo(root: &Path, options: &ScanOptions) -> FsResult<Vec<Node>> {
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_SCAN_TIMEOUT));
    scan_repo_controlled(root, options, ScanLimits::default(), &control)
}

/// Scan a repository with explicit resource and cooperative-stop controls.
///
/// The scan hashes current file bytes exactly. Cancellation, elapsed deadlines,
/// and resource failures discard all staged nodes instead of returning a partial
/// repository view.
///
/// # Errors
///
/// Returns an error when the root is invalid, filesystem metadata cannot be
/// read, cancellation or the deadline is observed, or a scan limit is exceeded.
pub fn scan_repo_controlled(
    root: &Path,
    options: &ScanOptions,
    limits: ScanLimits,
    control: &IndexWorkControl,
) -> FsResult<Vec<Node>> {
    scan_repo_controlled_with_work(root, options, limits, control).map(|outcome| outcome.nodes)
}

/// Scan a repository with explicit controls and report the admitted work.
///
/// The scan hashes current file bytes exactly. Cancellation, elapsed deadlines,
/// and resource failures discard all staged nodes and work instead of returning
/// a partial repository view.
///
/// # Errors
///
/// Returns an error when the root is invalid, filesystem metadata cannot be
/// read, cancellation or the deadline is observed, or a scan limit is exceeded.
pub fn scan_repo_controlled_with_work(
    root: &Path,
    options: &ScanOptions,
    limits: ScanLimits,
    control: &IndexWorkControl,
) -> FsResult<ScanOutcome> {
    let policy = RootScanPolicy::discover(root, options, control)?;
    let root = policy.root;
    let source_reader = SourceRootReader::open(&root)?;
    let options = policy.options;
    let mut builder = policy.walker;
    let effective_workers = effective_scan_workers(limits, control);
    if effective_workers == 0 {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::RepositoryTraversal,
            IndexWorkResource::Workers,
            0,
            1,
        )
        .into());
    }
    builder.threads(effective_workers);

    let nodes = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let budget = Arc::new(ScanBudget::new(limits, control.clone()));
    let filter_errors = Arc::clone(&errors);
    let filter_root = root.clone();
    let filter_options = options.clone();
    let filter_control = control.clone();
    let policy_failed = Arc::new(AtomicBool::new(false));
    let filter_failed = Arc::clone(&policy_failed);
    // The ignore worker opens directory rules before calling its visitor. The
    // admission filter runs before enqueueing a child directory, so guard here.
    builder.filter_entry(move |entry| {
        if filter_failed.load(Ordering::Acquire) {
            return false;
        }
        if should_skip_path(&filter_root, entry.path(), &filter_options) {
            return false;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
            return true;
        }
        let validation = filter_control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(FsError::from)
            .and_then(|()| validate_local_ignore_inputs(entry.path(), true, true));
        if let Err(error) = validation {
            // A wide directory of rejected children retains only the first
            // policy error, then stops further policy I/O and queued work.
            if !filter_failed.swap(true, Ordering::AcqRel) {
                push_error(&filter_errors, error);
            }
            return false;
        }
        true
    });
    builder.build_parallel().run(|| {
        let source_reader = &source_reader;
        let root = root.clone();
        let options = options.clone();
        let nodes = Arc::clone(&nodes);
        let errors = Arc::clone(&errors);
        let budget = Arc::clone(&budget);
        let policy_failed = Arc::clone(&policy_failed);
        Box::new(move |result| {
            if policy_failed.load(Ordering::Acquire) {
                return WalkState::Quit;
            }
            if let Err(error) = budget.claim_entry() {
                push_error(&errors, error.into());
                return WalkState::Quit;
            }
            let entry = match result {
                Ok(entry) => entry,
                Err(error) => {
                    push_error(
                        &errors,
                        FsError::Io {
                            path: root.clone(),
                            source: io::Error::other(error.to_string()),
                        },
                    );
                    return WalkState::Quit;
                }
            };
            let path = entry.path();
            if should_skip_path(&root, path, &options) {
                return skip_entry_state(&entry);
            }
            match scanned_node(&root, path, &options, &budget, source_reader) {
                Ok(Some(node)) => {
                    if let Ok(mut guard) = nodes.lock() {
                        guard.push(node);
                        WalkState::Continue
                    } else {
                        push_error(&errors, lock_error(&root));
                        WalkState::Quit
                    }
                }
                Ok(None) => WalkState::Continue,
                Err(error) => {
                    push_error(&errors, error);
                    WalkState::Quit
                }
            }
        })
    });
    // The builder retains its admission closure after worker shutdown.
    drop(builder);
    let errors = Arc::try_unwrap(errors)
        .map_err(|_remaining| state_error(&root, "parallel scanner error state still shared"))?;
    let mut errors = errors.into_inner().map_err(|source| {
        state_error(
            &root,
            &format!("parallel scanner error state lock failed: {source}"),
        )
    })?;
    if let Some(error) = errors.pop() {
        return Err(error);
    }
    control.check(IndexWorkStage::ScanFinalization)?;
    let nodes = Arc::try_unwrap(nodes)
        .map_err(|_remaining| state_error(&root, "parallel scanner node state still shared"))?;
    let mut nodes = nodes.into_inner().map_err(|source| {
        state_error(
            &root,
            &format!("parallel scanner node state lock failed: {source}"),
        )
    })?;
    nodes.sort_by(|left, right| left.path.cmp(&right.path));
    control.check(IndexWorkStage::ScanFinalization)?;
    Ok(ScanOutcome {
        nodes,
        work: budget.work(),
    })
}

/// Scan one path into a `ProjectAtlas` node when it is indexable.
///
/// # Errors
///
/// Returns an error when root canonicalization or metadata reads fail.
pub fn scan_path(root: &Path, path: &Path, options: &ScanOptions) -> FsResult<Option<Node>> {
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_SCAN_TIMEOUT));
    scan_path_controlled(root, path, options, ScanLimits::default(), &control)
}

/// Scan one path with explicit resource and cooperative-stop controls.
///
/// # Errors
///
/// Returns an error when root canonicalization or metadata reads fail,
/// cancellation or the deadline is observed, or a scan limit is exceeded.
pub fn scan_path_controlled(
    root: &Path,
    path: &Path,
    options: &ScanOptions,
    limits: ScanLimits,
    control: &IndexWorkControl,
) -> FsResult<Option<Node>> {
    let policy = RootScanPolicy::discover(root, options, control)?;
    scan_path_with_policy_controlled(&policy, path, limits, control)
}

/// Scan one path using repository policy already resolved for the selected root.
///
/// # Errors
///
/// Returns an error when path canonicalization or metadata reads fail,
/// cancellation or the deadline is observed, or a scan limit is exceeded.
pub fn scan_path_with_policy_controlled(
    policy: &RootScanPolicy,
    path: &Path,
    limits: ScanLimits,
    control: &IndexWorkControl,
) -> FsResult<Option<Node>> {
    let budget = ScanBudget::new(limits, control.clone());
    budget.claim_entry()?;
    let root = &policy.root;
    let options = &policy.options;
    #[cfg(windows)]
    let native_path = if path.is_absolute() {
        Cow::Owned(std::path::absolute(path).map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?)
    } else {
        Cow::Borrowed(path)
    };
    #[cfg(windows)]
    let path = native_path.as_ref();
    let absolute = policy.absolute_target(path);
    if !absolute.exists() {
        control.check(IndexWorkStage::ScanFinalization)?;
        return Ok(None);
    }
    if path_has_symlink_component(root, &absolute, control)? {
        control.check(IndexWorkStage::ScanFinalization)?;
        return Ok(None);
    }
    let Some(absolute) = direct_source_path(root, &absolute) else {
        control.check(IndexWorkStage::ScanFinalization)?;
        return Ok(None);
    };
    if policy.excludes_path(&absolute)? {
        control.check(IndexWorkStage::ScanFinalization)?;
        return Ok(None);
    }
    let source_reader = SourceRootReader::open(root)?;
    let node = scanned_node(root, &absolute, options, &budget, &source_reader)?;
    control.check(IndexWorkStage::ScanFinalization)?;
    Ok(node)
}

/// Normalize direct in-root components after unresolved symlink inspection.
fn direct_source_path(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let relative = absolute.strip_prefix(root).ok()?;
    let mut direct = root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => direct.push(name),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if direct != root => {
                direct.pop();
            }
            _ => return None,
        }
    }
    Some(direct)
}

/// Return whether any path component below root is a symlink.
fn path_has_symlink_component(
    root: &Path,
    absolute: &Path,
    control: &IndexWorkControl,
) -> FsResult<bool> {
    let Ok(relative) = absolute.strip_prefix(root) else {
        return Ok(true);
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        control.check(IndexWorkStage::SourceMetadata)?;
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map_err(|source| FsError::Io {
                path: current.clone(),
                source,
            })?
            .file_type()
            .is_symlink()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return whether repository `.gitignore` rules exclude a path.
///
/// This helper is for single-path refreshes. Full repository scans use
/// `ignore::WalkBuilder` directly.
///
/// # Errors
///
/// Returns an error if the root cannot be canonicalized or a discovered
/// `.gitignore` file cannot be parsed.
pub fn gitignore_excludes_path(root: &Path, path: &Path) -> FsResult<bool> {
    let input_root = root;
    let root = root.canonicalize().map_err(|source| FsError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let absolute = if path.is_absolute() {
        if let Ok(relative) = path.strip_prefix(input_root) {
            root.join(relative)
        } else if let Ok(relative) = path.strip_prefix(&root) {
            root.join(relative)
        } else {
            path.to_path_buf()
        }
    } else {
        root.join(path)
    };
    let absolute = if absolute.exists() {
        absolute.canonicalize().map_err(|source| FsError::Io {
            path: absolute.clone(),
            source,
        })?
    } else {
        absolute
    };
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(true)
        .git_exclude(false)
        .git_global(false)
        .require_git(false)
        .current_dir(root.clone());
    standard_ignore_excludes_path(&root, &absolute, &builder, false, false)
}

/// Keep non-repeatable streams/devices out of the canonical ignore parser.
/// Missing inputs and symlinks to regular files retain the engine's normal semantics.
fn validate_local_ignore_inputs(
    directory: &Path,
    plain_ignore: bool,
    git_exclude: bool,
) -> FsResult<()> {
    visit_local_ignore_inputs(directory, plain_ignore, git_exclude, |path| {
        validate_optional_policy_file(path).map(|_bytes| ())
    })
}

/// Enumerate only enabled policy inputs inside an admitted directory.
fn visit_local_ignore_inputs<E: From<FsError>>(
    directory: &Path,
    plain_ignore: bool,
    git_exclude: bool,
    mut visit: impl FnMut(&Path) -> Result<(), E>,
) -> Result<(), E> {
    match fs::metadata(directory) {
        Ok(metadata) if !metadata.is_dir() => return Ok(()),
        Ok(_) => {}
        Err(source)
            if matches!(
                source.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(());
        }
        Err(source) => {
            return Err(FsError::Io {
                path: directory.to_path_buf(),
                source,
            }
            .into());
        }
    }
    if plain_ignore {
        visit(&directory.join(".ignore"))?;
    }
    visit(&directory.join(".gitignore"))?;
    if git_exclude && let Some(common) = common_git_directory(directory)? {
        visit(&common.join("info/exclude"))?;
    }
    Ok(())
}

/// Refuse a present unsupported policy file before a library can block opening it.
fn validate_optional_policy_file(path: &Path) -> FsResult<u64> {
    match fs::metadata(path) {
        Ok(metadata) => {
            git_config::require_regular_file(path, &metadata)?;
            Ok(metadata.len())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(source) => Err(FsError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Reuse the traversal engine's hierarchy, parent pruning and exclusion precedence.
fn standard_ignore_excludes_path(
    root: &Path,
    path: &Path,
    builder: &WalkBuilder,
    plain_ignore: bool,
    git_exclude: bool,
) -> FsResult<bool> {
    let relative = normalize_repo_path(root, path)?;
    if relative == "." || relative.split('/').any(|component| component == "..") {
        return Ok(false);
    }
    for ancestor in root.ancestors() {
        validate_local_ignore_inputs(ancestor, plain_ignore, git_exclude)?;
    }
    // A fresh lazy matcher observes local policy edits without retaining a growing
    // cache in the shared root policy. Its external matchers reuse the builder.
    let mut matcher = builder
        .build_matchers()
        .into_iter()
        .next()
        .ok_or_else(|| FsError::Io {
            path: root.to_path_buf(),
            source: io::Error::other("source policy has no selected ignore root"),
        })?;
    // Match directories using guarded parents before examining their rules.
    // A pruned parent's own policy inputs must never be opened or inspected.
    let relative = Path::new(&relative);
    let mut parents = relative
        .ancestors()
        .skip(1)
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    parents.reverse();
    for parent in parents {
        let (matched, error) = matcher.matched_with_errors(parent, true);
        if let Some(error) = error {
            return Err(FsError::Io {
                path: root.join(parent),
                source: io::Error::other(error),
            });
        }
        if matched.is_ignore() {
            return Ok(true);
        }
        validate_local_ignore_inputs(&root.join(parent), plain_ignore, git_exclude)?;
    }
    let (matched, error) = matcher.matched_with_errors(
        relative,
        path.metadata().is_ok_and(|metadata| metadata.is_dir()),
    );
    if let Some(error) = error {
        return Err(FsError::Io {
            path: path.to_path_buf(),
            source: io::Error::other(error),
        });
    }
    Ok(matched.is_ignore())
}

/// Resolve the global Git excludes file selected by the same ignore engine as scans.
///
/// The returned path may not exist yet. Callers that retain a source-policy
/// witness should therefore preserve the path and its absent/present state.
#[must_use]
pub fn git_global_excludes_path() -> Option<PathBuf> {
    let root = std::env::current_dir().ok()?;
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_SCAN_TIMEOUT));
    git_config::ExternalGitConfig::discover(&root, &control)
        .ok()?
        .excludes
}

/// Return bounded external inputs that can change the standard scan ignore policy.
///
/// Root-contained nested `.gitignore` and `.ignore` files are covered by the
/// recursive source observer. This inventory adds ancestor rules, repository
/// excludes, linked-worktree metadata, and the global Git configuration inputs
/// used by [`WalkBuilder`]. Paths are retained even when absent so creation is
/// visible to a process-local policy witness.
///
/// # Errors
///
/// Returns an error when the root cannot be canonicalized or a linked-worktree
/// pointer cannot be inspected within its declared bound.
pub fn source_selection_policy_paths(root: &Path) -> FsResult<Vec<PathBuf>> {
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_SCAN_TIMEOUT));
    source_selection_policy_paths_controlled(root, &control)
}

/// Return bounded external source-selection inputs under one shared work control.
///
/// # Errors
///
/// Returns an error when policy discovery is canceled, exceeds its deadline or
/// registration bound, or cannot validate a linked-worktree pointer.
pub fn source_selection_policy_paths_controlled(
    root: &Path,
    control: &IndexWorkControl,
) -> FsResult<Vec<PathBuf>> {
    control.check(IndexWorkStage::RepositoryTraversal)?;
    let root = root.canonicalize().map_err(|source| FsError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut paths = BTreeSet::new();
    for ancestor in root.ancestors() {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        paths.insert(ancestor.join(".gitignore"));
        paths.insert(ancestor.join(".ignore"));
        let git = ancestor.join(".git");
        paths.insert(git.clone());
        match fs::metadata(&git) {
            Ok(metadata) if metadata.is_dir() => {
                paths.insert(git.join("info").join("exclude"));
            }
            Ok(metadata) if metadata.is_file() => {
                let directory = worktree::read_prefixed_pointer(&git, "gitdir:")
                    .map_err(git_pointer_io_error)?;
                let directory = if directory.is_absolute() {
                    directory
                } else {
                    ancestor.join(&directory)
                };
                paths.insert(directory.join("info").join("exclude"));
                let common_dir_pointer = directory.join("commondir");
                paths.insert(common_dir_pointer.clone());
                if let Some(common_dir) =
                    read_git_directory_pointer(&common_dir_pointer, &directory)?
                {
                    paths.insert(common_dir.join("info").join("exclude"));
                }
            }
            Ok(_metadata) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(FsError::Io { path: git, source });
            }
        }
    }
    let external = git_config::ExternalGitConfig::discover(&root, control)?;
    paths.extend(external.sources);
    paths.extend(external.excludes);
    if let Some(common_git_dir) = common_git_directory(&root)? {
        let registrations = common_git_dir.join("worktrees");
        paths.insert(registrations.clone());
        match fs::read_dir(&registrations) {
            Ok(entries) => {
                for (index, entry) in entries.enumerate() {
                    check_registered_worktree(control, index)?;
                    let entry = entry.map_err(|source| FsError::RepositoryBoundary {
                        path: registrations.clone(),
                        source,
                    })?;
                    paths.insert(entry.path().join("gitdir"));
                }
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(FsError::RepositoryBoundary {
                    path: registrations,
                    source,
                });
            }
        }
    }
    Ok(paths.into_iter().collect())
}

/// Read one bounded Git directory pointer relative to its containing directory.
fn read_git_directory_pointer(path: &Path, base: &Path) -> FsResult<Option<PathBuf>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FsError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let value = worktree::read_plain_pointer(path).map_err(git_pointer_io_error)?;
    Ok(Some(if value.is_absolute() {
        value
    } else {
        base.join(value)
    }))
}

/// Adapt one structural pointer failure for scanner policy callers.
fn git_pointer_io_error(issue: worktree::GitStructureIssue) -> FsError {
    let (path, source) = issue.into_io_error();
    FsError::Io { path, source }
}

/// Adapt one structural pointer failure for repository-boundary callers.
fn git_pointer_boundary_error(issue: worktree::GitStructureIssue) -> FsError {
    let (path, source) = issue.into_io_error();
    FsError::RepositoryBoundary { path, source }
}

/// Add registered in-root sibling worktrees to the existing prefix policy.
fn scan_options_for_root(
    root: &Path,
    options: &ScanOptions,
    control: &IndexWorkControl,
) -> FsResult<ScanOptions> {
    let mut options = options.clone();
    options
        .exclude_path_prefixes
        .extend(linked_worktree_excluded_prefixes(root, control)?);
    Ok(options)
}

/// Return registered sibling worktrees physically nested beneath the selected root.
fn linked_worktree_excluded_prefixes(
    root: &Path,
    control: &IndexWorkControl,
) -> FsResult<Vec<String>> {
    let Some(common_git_dir) = common_git_directory(root)? else {
        return Ok(Vec::new());
    };
    let registrations = common_git_dir.join("worktrees");
    let entries = match fs::read_dir(&registrations) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: registrations,
                source,
            });
        }
    };
    let mut prefixes = BTreeSet::new();
    for (index, entry) in entries.enumerate() {
        check_registered_worktree(control, index)?;
        let entry = entry.map_err(|source| FsError::RepositoryBoundary {
            path: registrations.clone(),
            source,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| FsError::RepositoryBoundary {
                path: entry.path(),
                source,
            })?;
        if !file_type.is_dir() {
            return Err(FsError::RepositoryBoundary {
                path: entry.path(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "registered worktree metadata entry is not a directory",
                ),
            });
        }
        let gitdir_path = entry.path().join("gitdir");
        let git_control_path = read_repository_boundary_pointer(
            &gitdir_path,
            &entry.path(),
            "registered worktree gitdir",
        )?
        .ok_or_else(|| FsError::RepositoryBoundary {
            path: gitdir_path.clone(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "registered worktree gitdir is missing",
            ),
        })?;
        let git_control_path =
            git_control_path
                .canonicalize()
                .map_err(|source| FsError::RepositoryBoundary {
                    path: gitdir_path.clone(),
                    source,
                })?;
        let git_control_metadata =
            fs::metadata(&git_control_path).map_err(|source| FsError::RepositoryBoundary {
                path: gitdir_path.clone(),
                source,
            })?;
        if !git_control_metadata.is_file()
            || git_control_path.file_name().and_then(|name| name.to_str()) != Some(".git")
        {
            return Err(FsError::RepositoryBoundary {
                path: gitdir_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "registered worktree gitdir does not address a .git control file",
                ),
            });
        }
        let worktree_root = git_control_path
            .parent()
            .ok_or_else(|| FsError::RepositoryBoundary {
                path: gitdir_path.clone(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "registered worktree gitdir has no checkout parent",
                ),
            })?
            .canonicalize()
            .map_err(|source| FsError::RepositoryBoundary {
                path: gitdir_path,
                source,
            })?;
        if common_git_directory(&worktree_root)?.as_ref() != Some(&common_git_dir) {
            return Err(FsError::RepositoryBoundary {
                path: git_control_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "registered worktree does not resolve to the selected common Git directory",
                ),
            });
        }
        if worktree_root != root && worktree_root.starts_with(root) {
            let prefix = normalize_repo_path(root, &worktree_root).map_err(FsError::Core)?;
            if prefix != "." {
                prefixes.insert(prefix);
            }
        }
    }
    Ok(prefixes.into_iter().collect())
}

/// Admit one bounded registered-worktree policy entry.
fn check_registered_worktree(control: &IndexWorkControl, index: usize) -> FsResult<()> {
    control.check(IndexWorkStage::RepositoryTraversal)?;
    let observed = index.saturating_add(1);
    if observed > MAX_GIT_WORKTREE_REGISTRATIONS {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::RepositoryTraversal,
            IndexWorkResource::Entries,
            u64::try_from(MAX_GIT_WORKTREE_REGISTRATIONS).unwrap_or(u64::MAX),
            u64::try_from(observed).unwrap_or(u64::MAX),
        )
        .into());
    }
    Ok(())
}

/// Resolve the common Git directory for a repository or linked-worktree root.
fn common_git_directory(root: &Path) -> FsResult<Option<PathBuf>> {
    let git = root.join(".git");
    let metadata = match fs::metadata(&git) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FsError::RepositoryBoundary { path: git, source });
        }
    };
    if metadata.is_dir() {
        return git
            .canonicalize()
            .map(Some)
            .map_err(|source| FsError::RepositoryBoundary { path: git, source });
    }
    if !metadata.is_file() {
        return Err(FsError::RepositoryBoundary {
            path: git,
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "repository .git control path is neither a file nor a directory",
            ),
        });
    }
    let git_dir =
        worktree::read_prefixed_pointer(&git, "gitdir:").map_err(git_pointer_boundary_error)?;
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        root.join(&git_dir)
    };
    let git_dir = git_dir
        .canonicalize()
        .map_err(|source| FsError::RepositoryBoundary {
            path: git.clone(),
            source,
        })?;
    let common_dir_pointer = git_dir.join("commondir");
    let common_dir = read_repository_boundary_pointer(
        &common_dir_pointer,
        &git_dir,
        "linked-worktree commondir",
    )?
    .unwrap_or(git_dir);
    common_dir
        .canonicalize()
        .map(Some)
        .map_err(|source| FsError::RepositoryBoundary {
            path: common_dir_pointer,
            source,
        })
}

/// Read a Git boundary pointer while preserving a typed boundary failure.
fn read_repository_boundary_pointer(
    path: &Path,
    base: &Path,
    description: &str,
) -> FsResult<Option<PathBuf>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_file() {
        return Err(FsError::RepositoryBoundary {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is not a file"),
            ),
        });
    }
    match read_git_directory_pointer(path, base) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => Err(FsError::RepositoryBoundary {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is empty"),
            ),
        }),
        Err(FsError::Io { path, source }) => Err(FsError::RepositoryBoundary { path, source }),
        Err(other) => Err(other),
    }
}

/// Resolve the current user's home directory without adding a platform helper dependency.
fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Return the correct walker state for a skipped entry.
fn skip_entry_state(entry: &DirEntry) -> WalkState {
    if entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        WalkState::Skip
    } else {
        WalkState::Continue
    }
}

/// Convert one walker entry into an indexed node.
fn scanned_node(
    root: &Path,
    path: &Path,
    options: &ScanOptions,
    budget: &ScanBudget,
    source_reader: &SourceRootReader,
) -> FsResult<Option<Node>> {
    budget.control.check(IndexWorkStage::SourceMetadata)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Ok(None);
    }
    if metadata.is_dir() {
        return folder_node(root, path).map(Some);
    }
    if metadata.is_file() {
        return file_node(root, path, &metadata, options, budget, source_reader).map(Some);
    }
    Ok(None)
}

/// Push a scanner error through the shared parallel error channel.
fn push_error(errors: &Arc<Mutex<Vec<FsError>>>, error: FsError) {
    if let Ok(mut guard) = errors.lock() {
        guard.push(error);
    }
}

/// Build a lock poisoning error.
fn lock_error(root: &Path) -> FsError {
    state_error(root, "parallel scanner state lock failed")
}

/// Build a scanner state error.
fn state_error(root: &Path, message: &str) -> FsError {
    FsError::Io {
        path: root.to_path_buf(),
        source: io::Error::other(message.to_string()),
    }
}

/// Return whether a repository-relative path should be skipped.
fn should_skip_path(root: &Path, path: &Path, options: &ScanOptions) -> bool {
    match normalize_repo_path(root, path) {
        Ok(relative) => {
            relative != "."
                && (options.excludes_relative_path(&relative) || is_reserved_metadata_file(path))
        }
        Err(_) => true,
    }
}

/// Return whether a repository-relative slash path contains an excluded directory.
fn has_excluded_directory_component(relative_path: &str, options: &ScanOptions) -> bool {
    relative_path.split('/').any(|name| {
        options
            .exclude_dir_names
            .iter()
            .any(|excluded| excluded == name)
            || options
                .exclude_dir_suffixes
                .iter()
                .any(|suffix| !suffix.is_empty() && name.ends_with(suffix))
    })
}

/// Return whether a repository-relative slash path starts with an excluded prefix.
fn has_excluded_path_prefix(relative_path: &str, options: &ScanOptions) -> bool {
    options.exclude_path_prefixes.iter().any(|prefix| {
        let prefix = prefix.replace('\\', "/");
        let prefix = prefix.trim_matches('/');
        !prefix.is_empty()
            && (relative_path == prefix
                || relative_path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/')))
    })
}

/// Return whether a ProjectAtlas-local metadata path should remain indexable.
fn is_indexed_projectatlas_input(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    INDEXED_PROJECTATLAS_INPUT_PATHS.contains(&normalized)
}

/// Return whether a path is a reserved metadata file.
fn is_reserved_metadata_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        RESERVED_METADATA_FILE_NAMES.contains(&name.as_ref())
    })
}

/// Build a folder node from filesystem metadata.
fn folder_node(root: &Path, path: &Path) -> FsResult<Node> {
    let normalized = normalize_repo_path(root, path)?;
    Ok(Node {
        parent_path: normalized_parent(&normalized),
        path: normalized,
        kind: NodeKind::Folder,
        extension: None,
        language: None,
        size_bytes: None,
        mtime_ns: None,
        content_hash: None,
    })
}

/// Build a file node from filesystem metadata and content hash.
fn file_node(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
    options: &ScanOptions,
    budget: &ScanBudget,
    source_reader: &SourceRootReader,
) -> FsResult<Node> {
    let normalized = normalize_repo_path(root, path)?;
    let extension = normalized_extension(path);
    budget.control.check(IndexWorkStage::SourceHash)?;
    let explicit_override = explicit_language_override(
        &normalized,
        extension.as_deref(),
        &options.language_overrides,
    );
    let preliminary_language = admitted_scan_language(
        detect_language_request(LanguageDetectionRequest {
            path: &normalized,
            extension: extension.as_deref(),
            explicit_override,
            content_prefix: None,
        })
        .map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, source),
        })?,
        options.admit_optional_languages,
    );
    let (hashed, metadata) = hash_file(
        path,
        metadata,
        budget,
        preliminary_language.is_none(),
        source_reader,
    )?;
    let language = if let Some(detected) = preliminary_language {
        Some(detected.language.to_string())
    } else {
        admitted_scan_language(
            detect_language_request(LanguageDetectionRequest {
                path: "",
                extension: None,
                explicit_override: None,
                content_prefix: hashed.content_prefix.as_deref(),
            })
            .map_err(|source| FsError::Io {
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidInput, source),
            })?,
            options.admit_optional_languages,
        )
        .map(|detected| detected.language.to_string())
    };
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(system_time_to_ns)
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX));
    Ok(Node {
        parent_path: normalized_parent(&normalized),
        path: normalized,
        kind: NodeKind::File,
        extension,
        language,
        size_bytes: Some(hashed.size_bytes),
        mtime_ns,
        content_hash: Some(hashed.digest),
    })
}

/// Apply effective scan admission without weakening core registry recognition.
fn admitted_scan_language(
    detected: Option<LanguageDetection>,
    admit_optional_languages: bool,
) -> Option<LanguageDetection> {
    detected.filter(|detected| {
        admit_optional_languages
            || language_capability(detected.language)
                .is_none_or(|capability| capability.optional_pack.is_none())
    })
}

/// Select one configured explicit override before built-in detector rules.
/// Exact filenames take precedence over the longest matching extension selector.
#[must_use]
pub fn explicit_language_override<'a>(
    path: &str,
    extension: Option<&str>,
    overrides: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    if overrides.is_empty() {
        return None;
    }
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if let Some(language) = overrides.get(file_name) {
        return Some(language);
    }
    let lower_file_name = file_name.to_ascii_lowercase();
    overrides
        .iter()
        .filter(|(selector, _)| selector.starts_with('.'))
        .filter(|(selector, _)| {
            lower_file_name.ends_with(selector.as_str())
                || extension.is_some_and(|extension| extension.eq_ignore_ascii_case(selector))
        })
        .max_by_key(|(selector, _)| selector.len())
        .map(|(_, language)| language.as_str())
}

/// Exact hash plus the bounded prefix already observed by the same source read.
#[derive(Debug)]
struct HashedFile {
    /// BLAKE3 digest of every byte.
    digest: String,
    /// Exact source byte count.
    size_bytes: u64,
    /// Prefix retained only for bounded language detection.
    content_prefix: Option<Vec<u8>>,
}

/// Hash a file with BLAKE3 for stale-purpose detection.
fn hash_file(
    path: &Path,
    admitted_metadata: &fs::Metadata,
    budget: &ScanBudget,
    retain_content_prefix: bool,
    source_reader: &SourceRootReader,
) -> FsResult<(HashedFile, fs::Metadata)> {
    budget.control.check(IndexWorkStage::SourceHash)?;
    let (file, metadata) = source_reader.open_file(path, admitted_metadata, &budget.control)?;
    let hashed = hash_reader(path, file, budget, retain_content_prefix)?;
    Ok((hashed, metadata))
}

/// Hash every byte from a reader while observing cancellation and deadline state.
fn hash_reader(
    path: &Path,
    mut reader: impl Read,
    budget: &ScanBudget,
    retain_content_prefix: bool,
) -> FsResult<HashedFile> {
    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    let mut size_bytes = 0_u64;
    let mut content_prefix =
        retain_content_prefix.then(|| Vec::with_capacity(LANGUAGE_CONTENT_DETECTION_MAX_BYTES));
    loop {
        budget.control.check(IndexWorkStage::SourceHash)?;
        let count = reader.read(&mut buffer).map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        budget.claim_source_bytes(count as u64)?;
        size_bytes = size_bytes.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
        if let Some(content_prefix) = &mut content_prefix {
            let retained =
                LANGUAGE_CONTENT_DETECTION_MAX_BYTES.saturating_sub(content_prefix.len());
            content_prefix.extend_from_slice(&buffer[..count.min(retained)]);
        }
    }
    budget.control.check(IndexWorkStage::SourceHash)?;
    Ok(HashedFile {
        digest: hasher.finalize().to_hex().to_string(),
        size_bytes,
        content_prefix,
    })
}

/// Convert a system timestamp into nanoseconds since the Unix epoch.
fn system_time_to_ns(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;
    use std::process::Command;
    use std::time::Instant;

    #[cfg(any(unix, windows))]
    #[test]
    fn source_hash_refuses_indirection_substituted_after_metadata() -> Result<(), Box<dyn Error>> {
        let mut admitted = Vec::new();
        for ancestor in [false, true] {
            for target_in_root in [false, true] {
                for same_file_object in [false, true] {
                    let temp = tempfile::tempdir()?;
                    let root = temp.path().join("repo");
                    let directory = root.join("src");
                    fs::create_dir_all(&directory)?;
                    let source = directory.join("source.rs");
                    let original = b"fn inside_source() {}\n";
                    fs::write(&source, original)?;
                    let metadata = fs::symlink_metadata(&source)?;
                    let source_reader = SourceRootReader::open(&root)?;
                    let target = if target_in_root {
                        if ancestor {
                            root.join("other")
                        } else {
                            directory.join("other")
                        }
                    } else {
                        temp.path().join("outside")
                    };
                    fs::create_dir(&target)?;
                    let redirected = b"fn redirected_source() {}\n";
                    if same_file_object {
                        fs::hard_link(&source, target.join("source.rs"))?;
                    } else {
                        fs::write(target.join("source.rs"), redirected)?;
                    }
                    let budget = ScanBudget::new(
                        ScanLimits::default(),
                        IndexWorkControl::new(IndexCancellation::new(), None),
                    );
                    require(
                        file_node(
                            &root,
                            &source,
                            &metadata,
                            &ScanOptions::default(),
                            &budget,
                            &source_reader,
                        )?
                        .content_hash
                            == Some(blake3::hash(original).to_hex().to_string()),
                        "regular source fixture was not admitted before substitution",
                    )?;
                    if ancestor {
                        fs::rename(&directory, temp.path().join("saved-directory"))?;
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(
                            if target_in_root {
                                Path::new("other")
                            } else {
                                &target
                            },
                            &directory,
                        )?;
                        #[cfg(windows)]
                        std::os::windows::fs::symlink_dir(
                            if target_in_root {
                                Path::new("other")
                            } else {
                                &target
                            },
                            &directory,
                        )?;
                    } else {
                        fs::rename(&source, temp.path().join("saved.rs"))?;
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(
                            if target_in_root {
                                PathBuf::from("other/source.rs")
                            } else {
                                target.join("source.rs")
                            },
                            &source,
                        )?;
                        #[cfg(windows)]
                        std::os::windows::fs::symlink_file(
                            if target_in_root {
                                PathBuf::from("other/source.rs")
                            } else {
                                target.join("source.rs")
                            },
                            &source,
                        )?;
                    }
                    if let Ok(node) = file_node(
                        &root,
                        &source,
                        &metadata,
                        &ScanOptions::default(),
                        &budget,
                        &source_reader,
                    ) {
                        admitted.push(format!(
                        "ancestor={ancestor}, target_in_root={target_in_root}, same_file_object={same_file_object}, redirected_digest={}",
                        node.content_hash == Some(blake3::hash(redirected).to_hex().to_string())
                    ));
                    }
                }
            }
        }
        if !admitted.is_empty() {
            return Err(io::Error::other(format!(
                "source indirection crossed its metadata admission: {admitted:?}"
            ))
            .into());
        }
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn targeted_scan_skips_inside_root_symlink_ancestors() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let direct = root.join("direct");
        fs::create_dir_all(&direct)?;
        fs::write(direct.join("source.rs"), "fn direct_source() {}\n")?;
        let alias = root.join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&direct, &alias)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&direct, &alias)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        for path in [&alias, &alias.join("source.rs")] {
            require(
                scan_path_with_policy_controlled(&policy, path, ScanLimits::default(), &control)?
                    .is_none(),
                "targeted scan canonicalized a source alias into an admitted direct path",
            )?;
        }
        require(
            scan_path_with_policy_controlled(
                &policy,
                &direct.join("source.rs"),
                ScanLimits::default(),
                &control,
            )?
            .is_some(),
            "targeted scan lost the direct source alongside its excluded alias",
        )?;
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn targeted_scan_keeps_selected_alias_and_direct_parent_components()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("nested"))?;
        fs::write(root.join("source.rs"), "fn admitted_source() {}\n")?;
        let alias = temp.path().join("selected");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, &alias)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&root, &alias)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&alias, &ScanOptions::default(), &control)?;
        for path in [
            alias.join("source.rs"),
            alias.join("nested/../source.rs"),
            PathBuf::from("nested/../source.rs"),
            root.canonicalize()?.join("source.rs"),
        ] {
            let node =
                scan_path_with_policy_controlled(&policy, &path, ScanLimits::default(), &control)?;
            require(
                node.is_some_and(|node| node.path == "source.rs"),
                "selected root alias or ordinary parent traversal changed the direct source",
            )?;
        }
        // A symlink followed by .. is still indirection, even if lexical cleanup
        // would erase the component and land on a permitted source.
        let indirect = root.join("indirect");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("nested"), &indirect)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(root.join("nested"), &indirect)?;
        let indirect_parent = scan_path_with_policy_controlled(
            &policy,
            &alias.join("indirect/../source.rs"),
            ScanLimits::default(),
            &control,
        )?;
        #[cfg(unix)]
        require(
            indirect_parent.is_none(),
            "parent cleanup erased source indirection",
        )?;
        #[cfg(windows)]
        {
            require(
                indirect_parent.is_some_and(|node| node.path == "source.rs"),
                "native Windows parent normalization changed the selected direct source",
            )?;
            let missing_parent = scan_path_with_policy_controlled(
                &policy,
                &alias.join("missing/../source.rs"),
                ScanLimits::default(),
                &control,
            )?;
            require(
                missing_parent.is_some_and(|node| node.path == "source.rs"),
                "native Windows missing-parent normalization changed the direct source",
            )?;
        }
        Ok(())
    }

    #[test]
    fn source_node_uses_opened_file_metadata_for_modified_content() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let path = root.join("source.rs");
        fs::write(&path, b"fn before_edit() {}\n")?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000))?;
        let original_metadata = fs::symlink_metadata(&path)?;
        let edited = b"fn after_edit() {}\n";
        fs::write(&path, edited)?;
        let opened_metadata = fs::metadata(&path)?;
        require(
            original_metadata.modified()? != opened_metadata.modified()?,
            "source timestamp fixture did not change",
        )?;
        let budget = ScanBudget::new(
            ScanLimits::default(),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );
        let reader = SourceRootReader::open(root)?;
        let node = file_node(
            root,
            &path,
            &original_metadata,
            &ScanOptions::default(),
            &budget,
            &reader,
        )?;
        let expected_mtime = system_time_to_ns(opened_metadata.modified()?)
            .map(|value| i64::try_from(value).unwrap_or(i64::MAX));
        require(
            node.content_hash == Some(blake3::hash(edited).to_hex().to_string())
                && node.mtime_ns == expected_mtime,
            "source node paired edited bytes with pre-open metadata",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_hash_refuses_atomic_replacement_after_metadata_and_accepts_retry()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let path = root.join("source.rs");
        fs::write(&path, b"fn before_save() {}\n")?;
        let original_metadata = fs::symlink_metadata(&path)?;
        let reader = SourceRootReader::open(root)?;
        let saved = b"fn after_save() {}\n";
        let pending = root.join("pending.rs");
        fs::write(&pending, saved)?;
        fs::rename(&pending, &path)?;
        let budget = ScanBudget::new(
            ScanLimits::default(),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );
        require(
            matches!(
                file_node(
                    root,
                    &path,
                    &original_metadata,
                    &ScanOptions::default(),
                    &budget,
                    &reader
                ),
                Err(FsError::Io { .. })
            ),
            "source replacement between metadata and open was not refused",
        )?;
        require(
            file_node(
                root,
                &path,
                &fs::symlink_metadata(&path)?,
                &ScanOptions::default(),
                &budget,
                &reader,
            )?
            .content_hash
                == Some(blake3::hash(saved).to_hex().to_string()),
            "fresh metadata retry lost the ordinary editor save",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_hash_refuses_walker_metadata_from_a_replaced_root() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let path = root.join("source.rs");
        let original = b"fn selected_root_source() {}\n";
        fs::write(&path, original)?;
        let reader = SourceRootReader::open(&root)?;
        let budget = ScanBudget::new(
            ScanLimits::default(),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );
        let options = ScanOptions::default();
        let original_metadata = fs::symlink_metadata(&path)?;
        let selected = temp.path().join("selected-root");
        fs::rename(&root, &selected)?;
        fs::create_dir(&root)?;
        fs::write(&path, b"fn replacement_root_source() {}\n")?;
        let replacement_metadata = fs::symlink_metadata(&path)?;
        // Model the actual path walker seeing replacement B after the source
        // reader retained selected root A. Refuse before pairing B's metadata
        // with A's bytes, even if the original pathname is restored afterward.
        let refused = file_node(
            &root,
            &path,
            &replacement_metadata,
            &options,
            &budget,
            &reader,
        );
        fs::rename(&root, temp.path().join("replacement-root"))?;
        fs::rename(&selected, &root)?;
        require(
            refused.is_err(),
            "walker metadata from a replacement root labeled selected-root bytes",
        )?;
        require(
            file_node(&root, &path, &original_metadata, &options, &budget, &reader)?.content_hash
                == Some(blake3::hash(original).to_hex().to_string()),
            "restored selected root lost direct source admission",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_hash_refuses_fifo_substituted_after_metadata() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let source = root.join("source.rs");
        fs::write(&source, "fn saved() {}\n")?;
        let metadata = fs::metadata(&source)?;
        fs::rename(&source, temp.path().join("saved.rs"))?;
        nix::unistd::mkfifo(
            &source,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )?;
        let checked = source.clone();
        let source_reader = SourceRootReader::open(&root)?;
        crate::regular_file::verify_fifo_refusal(&source, move || {
            let budget = ScanBudget::new(
                ScanLimits::default(),
                IndexWorkControl::new(IndexCancellation::new(), None),
            );
            matches!(
                file_node(&root, &checked, &metadata, &ScanOptions::default(), &budget, &source_reader),
                Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput
            )
        })
    }

    /// Reader that requests cancellation after yielding its first non-empty chunk.
    struct CancelAfterFirstChunk<R> {
        /// Wrapped source reader.
        inner: R,
        /// Signal shared with the work control under test.
        cancellation: IndexCancellation,
        /// Whether cancellation was already requested.
        canceled: bool,
    }

    impl<R: Read> Read for CancelAfterFirstChunk<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.inner.read(buffer)?;
            if count > 0 && !self.canceled {
                self.cancellation.cancel();
                self.canceled = true;
            }
            Ok(count)
        }
    }

    /// Run one Git command inside a test repository.
    fn run_git(repo: &Path, arguments: &[&str]) -> Result<(), Box<dyn Error>> {
        let output = Command::new("git")
            .current_dir(repo)
            .args(arguments)
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "git {arguments:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }

    #[test]
    fn controlled_scan_refuses_precancelled_work() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("source.rs"), "fn source() {}\n")?;
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);

        let result = scan_repo_controlled(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::default(),
            &control,
        );
        require(
            matches!(
                result,
                Err(FsError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal,
                }))
            ),
            "pre-canceled repository scan did not return typed cancellation",
        )?;

        let expired = IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
        let deadline_result = scan_repo_controlled(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::default(),
            &expired,
        );
        require(
            matches!(
                deadline_result,
                Err(FsError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::RepositoryTraversal,
                }))
            ),
            "expired repository scan did not return the typed deadline",
        )?;
        Ok(())
    }

    #[test]
    fn registered_worktree_inventory_is_controlled_and_bounded() {
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let canceled = check_registered_worktree(&IndexWorkControl::new(cancellation, None), 0);
        assert!(matches!(
            canceled,
            Err(FsError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::RepositoryTraversal
            }))
        ));

        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        assert!(check_registered_worktree(&control, MAX_GIT_WORKTREE_REGISTRATIONS - 1).is_ok());
        assert!(matches!(
            check_registered_worktree(&control, MAX_GIT_WORKTREE_REGISTRATIONS),
            Err(FsError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::RepositoryTraversal,
                    resource: IndexWorkResource::Entries,
                    limit,
                    observed
                }
            )) if limit == MAX_GIT_WORKTREE_REGISTRATIONS as u64
                && observed == MAX_GIT_WORKTREE_REGISTRATIONS as u64 + 1
        ));
    }

    #[test]
    fn controlled_scan_enforces_entry_and_byte_limits_without_partial_results()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("source.rs"), "four")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let entry_result = scan_repo_controlled(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::new(1, 64, 1),
            &control,
        );
        require(
            matches!(
                entry_result,
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded {
                        resource: IndexWorkResource::Entries,
                        ..
                    }
                ))
            ),
            "entry-bounded scan did not return the typed entry limit",
        )?;

        let byte_result = scan_repo_controlled(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::new(8, 3, 1),
            &control,
        );
        require(
            matches!(
                byte_result,
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded {
                        resource: IndexWorkResource::SourceBytes,
                        ..
                    }
                ))
            ),
            "byte-bounded scan did not return the typed source-byte limit",
        )?;

        let host_workers = thread::available_parallelism().map_or(1, usize::from);
        let bounded_workers = ScanLimits::new(8, 64, usize::MAX).effective_workers();
        require(
            bounded_workers == host_workers.min(SCAN_WORKER_SAFE_CEILING),
            "effective workers did not honor host availability and the safety cap",
        )?;
        let operation_bounded_workers = effective_scan_workers(
            ScanLimits::new(8, 64, usize::MAX),
            &control.with_worker_ceiling(1),
        );
        require(
            operation_bounded_workers == 1,
            "operation-owned worker ceiling did not reach the repository scanner",
        )?;

        let worker_result = scan_repo_controlled(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::new(8, 64, 0),
            &control,
        );
        require(
            matches!(
                worker_result,
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded {
                        resource: IndexWorkResource::Workers,
                        ..
                    }
                ))
            ),
            "zero-worker scan did not return the typed worker limit",
        )?;
        Ok(())
    }

    #[test]
    fn controlled_scan_reports_admitted_work() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("source.rs"), "four")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let outcome = scan_repo_controlled_with_work(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::new(8, 64, 1),
            &control,
        )?;

        require_path(&outcome.nodes, ".")?;
        require_path(&outcome.nodes, "source.rs")?;
        require(
            outcome.work
                == ScanWork {
                    entries: 2,
                    source_bytes: 4,
                },
            "scan work did not match the admitted entry and source-byte counters",
        )?;
        Ok(())
    }

    #[test]
    fn source_policy_inventory_covers_absent_rules_and_linked_worktrees()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let worktree_git_dir = temp
            .path()
            .join("git-metadata")
            .join("worktrees")
            .join("repo");
        let common_git_dir = temp.path().join("git-metadata");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&worktree_git_dir)?;
        fs::create_dir_all(common_git_dir.join("info"))?;
        fs::write(
            repo.join(".git"),
            format!("gitdir: {}\n", worktree_git_dir.display()),
        )?;
        fs::write(
            worktree_git_dir.join("commondir"),
            format!("{}\n", common_git_dir.display()),
        )?;
        fs::write(common_git_dir.join("info").join("exclude"), "ignored.rs\n")?;

        let canonical_repo = repo.canonicalize()?;
        let canonical_common_git_dir = common_git_dir.canonicalize()?;
        let canonical_worktree_git_dir = worktree_git_dir.canonicalize()?;
        let paths = source_selection_policy_paths(&repo)?;

        require(
            paths.contains(&canonical_repo.join(".ignore")),
            "policy inventory omitted the possibly absent root .ignore",
        )?;
        require(
            paths.contains(&canonical_repo.join(".gitignore")),
            "policy inventory omitted the possibly absent root .gitignore",
        )?;
        require(
            paths.contains(&canonical_repo.join(".git")),
            "policy inventory omitted the linked-worktree pointer",
        )?;
        require(
            paths.contains(&worktree_git_dir.join("commondir")),
            "policy inventory omitted the linked-worktree commondir pointer",
        )?;
        require(
            paths.contains(&common_git_dir.join("info").join("exclude")),
            "policy inventory omitted the common Git exclude file",
        )?;
        require(
            paths.contains(&canonical_common_git_dir.join("worktrees")),
            "policy inventory omitted the registered-worktree directory",
        )?;
        require(
            paths.contains(&canonical_worktree_git_dir.join("gitdir")),
            "policy inventory omitted the registered-worktree root pointer",
        )?;
        Ok(())
    }

    #[test]
    fn scan_excludes_registered_in_root_worktree_without_an_ignore_rule()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir(&repo)?;
        run_git(&repo, &["init"])?;
        run_git(&repo, &["config", "user.name", "ProjectAtlas Test"])?;
        run_git(
            &repo,
            &["config", "user.email", "projectatlas@example.invalid"],
        )?;
        fs::create_dir(repo.join("src"))?;
        fs::write(repo.join("src").join("main.rs"), "fn main_checkout() {}\n")?;
        run_git(&repo, &["add", "."])?;
        run_git(&repo, &["commit", "-m", "fixture"])?;

        let linked = repo.join("linked-checkout");
        let output = Command::new("git")
            .current_dir(&repo)
            .args(["worktree", "add", "-b", "linked-branch"])
            .arg(&linked)
            .output()?;
        require(
            output.status.success(),
            &format!(
                "git worktree add failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        fs::write(
            linked.join("src").join("branch_only.rs"),
            "fn linked_branch_only() {}\n",
        )?;

        let nested_repo = repo.join("vendor").join("unrelated");
        fs::create_dir_all(nested_repo.join("src"))?;
        run_git(&nested_repo, &["init"])?;
        fs::write(
            nested_repo.join("src").join("lib.rs"),
            "fn unrelated_nested_repo() {}\n",
        )?;
        require(
            !repo.join(".gitignore").exists(),
            "fixture unexpectedly depended on a worktree-container ignore rule",
        )?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        require_path(&nodes, "src/main.rs")?;
        reject_path(&nodes, "linked-checkout")?;
        reject_path(&nodes, "linked-checkout/src/branch_only.rs")?;
        require_path(&nodes, "vendor/unrelated/src/lib.rs")?;

        let single = scan_path(
            &repo,
            &linked.join("src").join("branch_only.rs"),
            &ScanOptions::default(),
        )?;
        require(
            single.is_none(),
            "single-path refresh crossed a registered sibling worktree boundary",
        )?;
        Ok(())
    }

    #[test]
    fn scan_fails_typed_when_registered_worktree_boundary_is_unreadable()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let registration = repo.join(".git").join("worktrees").join("missing");
        fs::create_dir_all(&registration)?;
        fs::write(
            registration.join("gitdir"),
            repo.join("missing-worktree")
                .join(".git")
                .display()
                .to_string(),
        )?;
        fs::write(repo.join("source.rs"), "fn source() {}\n")?;

        let result = scan_repo(&repo, &ScanOptions::default());
        require(
            matches!(result, Err(FsError::RepositoryBoundary { .. })),
            "uncertain registered worktree boundary did not fail with the typed error",
        )?;
        Ok(())
    }

    #[test]
    fn exact_hash_loop_observes_work_bounds() {
        let exact_budget = ScanBudget::new(
            ScanLimits::new(8, (HASH_BUFFER_BYTES * 2) as u64, 1),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );
        let exact_source = vec![3_u8; HASH_BUFFER_BYTES + 17];
        let exact_digest = blake3::hash(&exact_source).to_hex().to_string();
        let exact = hash_reader(
            Path::new("exact-source.rs"),
            io::Cursor::new(exact_source),
            &exact_budget,
            false,
        );
        assert!(matches!(
            exact,
            Ok(HashedFile {
                digest,
                size_bytes,
                content_prefix,
            }) if digest == exact_digest
                && size_bytes == (HASH_BUFFER_BYTES + 17) as u64
                && content_prefix.is_none()
        ));

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let budget = ScanBudget::new(ScanLimits::default(), control);
        let reader = CancelAfterFirstChunk {
            inner: io::Cursor::new(vec![7_u8; HASH_BUFFER_BYTES * 2]),
            cancellation,
            canceled: false,
        };

        let result = hash_reader(Path::new("source.rs"), reader, &budget, false);
        assert!(matches!(
            result,
            Err(FsError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SourceHash,
            }))
        ));

        let expired_budget = ScanBudget::new(
            ScanLimits::default(),
            IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now()),
        );
        let expired = hash_reader(
            Path::new("deadline-source.rs"),
            io::Cursor::new(vec![7_u8; HASH_BUFFER_BYTES * 2]),
            &expired_budget,
            false,
        );
        assert!(matches!(
            expired,
            Err(FsError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::SourceHash,
            }))
        ));

        let byte_budget = ScanBudget::new(
            ScanLimits::new(8, HASH_BUFFER_BYTES as u64, 1),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );
        let oversized = hash_reader(
            Path::new("growing-source.rs"),
            io::Cursor::new(vec![7_u8; HASH_BUFFER_BYTES * 2]),
            &byte_budget,
            false,
        );
        assert!(matches!(
            oversized,
            Err(FsError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::SourceHash,
                    resource: IndexWorkResource::SourceBytes,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn classified_hash_retains_no_content_prefix() -> Result<(), Box<dyn Error>> {
        let preliminary_language =
            detect_language_request(LanguageDetectionRequest::new("source.rs", Some(".rs")))?;
        let budget = ScanBudget::new(
            ScanLimits::new(8, 64, 1),
            IndexWorkControl::new(IndexCancellation::new(), None),
        );

        let hashed = hash_reader(
            Path::new("source.rs"),
            io::Cursor::new(b"fn main() {}\n"),
            &budget,
            preliminary_language.is_none(),
        )?;

        require(
            preliminary_language.map(|detected| detected.language) == Some("rust"),
            "preliminary extension classification did not select Rust",
        )?;
        require(
            hashed.content_prefix.is_none(),
            "classified hash retained a content prefix",
        )?;
        Ok(())
    }

    #[test]
    fn scans_files_and_folders() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let src = temp.path().join("src");
        fs::create_dir(&src)?;
        fs::write(src.join("main.rs"), "fn main() {}\n")?;
        fs::write(src.join(".purpose"), "Rust source folder\n")?;

        let nodes = scan_repo(temp.path(), &ScanOptions::default())?;
        require_path(&nodes, ".")?;
        require_path(&nodes, "src")?;
        require_path(&nodes, "src/main.rs")?;
        reject_path(&nodes, "src/.purpose")?;
        Ok(())
    }

    #[test]
    fn scan_uses_explicit_language_override_before_builtin_filename_rules()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("Cargo.toml"), "#!/usr/bin/env node\n")?;
        let mut options = ScanOptions::default();
        options
            .language_overrides
            .insert(".toml".to_string(), "python".to_string());

        let nodes = scan_repo(temp.path(), &options)?;
        let cargo = nodes
            .iter()
            .find(|node| node.path == "Cargo.toml")
            .ok_or_else(|| io::Error::other("Cargo.toml was not scanned"))?;
        require(
            cargo.language.as_deref() == Some("python"),
            "explicit language override did not win",
        )?;
        Ok(())
    }

    #[test]
    fn invalid_language_override_fails_before_source_hashing() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let source_path = temp.path().join("source.rs");
        fs::write(&source_path, "fn main() {}\n")?;
        let mut options = ScanOptions::default();
        options
            .language_overrides
            .insert(".rs".to_string(), "missing-language".to_string());

        for _ in 0..2 {
            let result = scan_path_controlled(
                temp.path(),
                &source_path,
                &options,
                ScanLimits::new(8, 0, 1),
                &IndexWorkControl::new(IndexCancellation::new(), None),
            );
            match result {
                Err(FsError::Io { source, .. }) => {
                    require(
                        source.kind() == io::ErrorKind::InvalidInput,
                        "invalid override did not return InvalidInput",
                    )?;
                    require(
                        source.to_string()
                            == "unknown explicit language override \"missing-language\"",
                        "invalid override diagnostic was not deterministic",
                    )?;
                }
                other => {
                    return Err(io::Error::other(format!("unexpected result: {other:?}")).into());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn scan_detects_bounded_shebang_from_the_existing_hash_read() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(
            temp.path().join("tool"),
            "#!/usr/bin/env python\nprint('atlas')\n",
        )?;

        let nodes = scan_repo(temp.path(), &ScanOptions::default())?;
        let tool = nodes
            .iter()
            .find(|node| node.path == "tool")
            .ok_or_else(|| io::Error::other("extensionless tool was not scanned"))?;
        require(
            tool.language.as_deref() == Some("python"),
            "extensionless shebang was not detected from the retained prefix",
        )?;
        Ok(())
    }

    #[test]
    fn default_scan_keeps_optional_catalog_recognition_inactive() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let optional_path = temp.path().join("report.awk");
        fs::write(&optional_path, "{ print $1 }\n")?;
        fs::write(temp.path().join("main.rs"), "fn main() {}\n")?;

        let recognized =
            detect_language_request(LanguageDetectionRequest::new("report.awk", Some(".awk")))?;
        require(
            recognized.map(|detected| detected.language) == Some("awk"),
            "optional AWK recognition was removed from the core catalog",
        )?;

        let nodes = scan_repo(temp.path(), &ScanOptions::default())?;
        let optional = nodes
            .iter()
            .find(|node| node.path == "report.awk")
            .ok_or_else(|| io::Error::other("optional source was not scanned"))?;
        require(
            optional.language.is_none(),
            "default scan admitted an inactive optional language",
        )?;
        let built_in = nodes
            .iter()
            .find(|node| node.path == "main.rs")
            .ok_or_else(|| io::Error::other("built-in Rust source was not scanned"))?;
        require(
            built_in.language.as_deref() == Some("rust"),
            "optional-language admission changed built-in recognition",
        )?;

        let refreshed = scan_path(temp.path(), &optional_path, &ScanOptions::default())?
            .ok_or_else(|| io::Error::other("optional source was not refreshed"))?;
        require(
            refreshed.language.is_none(),
            "single-path refresh admitted an inactive optional language",
        )?;
        Ok(())
    }

    #[test]
    fn enabled_scan_admits_optional_language_for_full_and_single_path_scans()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let optional_path = temp.path().join("report.awk");
        fs::write(&optional_path, "{ print $1 }\n")?;
        let options = ScanOptions {
            admit_optional_languages: true,
            ..ScanOptions::default()
        };

        let nodes = scan_repo(temp.path(), &options)?;
        let optional = nodes
            .iter()
            .find(|node| node.path == "report.awk")
            .ok_or_else(|| io::Error::other("optional source was not scanned"))?;
        require(
            optional.language.as_deref() == Some("awk"),
            "enabled scan did not admit the optional AWK language",
        )?;

        let refreshed = scan_path(temp.path(), &optional_path, &options)?
            .ok_or_else(|| io::Error::other("optional source was not refreshed"))?;
        require(
            refreshed.language.as_deref() == Some("awk"),
            "enabled single-path refresh did not admit the optional AWK language",
        )?;
        Ok(())
    }

    #[test]
    fn optional_override_respects_admission_and_rejected_extension_uses_content_fallback()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("report.txt"), "{ print $1 }\n")?;
        fs::write(
            temp.path().join("tool.awk"),
            "#!/usr/bin/env python\nprint('atlas')\n",
        )?;
        let mut options = ScanOptions::default();
        options
            .language_overrides
            .insert(".txt".to_string(), "awk".to_string());

        let nodes = scan_repo(temp.path(), &options)?;
        let overridden = nodes
            .iter()
            .find(|node| node.path == "report.txt")
            .ok_or_else(|| io::Error::other("overridden source was not scanned"))?;
        require(
            overridden.language.is_none(),
            "explicit override bypassed optional-language admission",
        )?;
        let content_detected = nodes
            .iter()
            .find(|node| node.path == "tool.awk")
            .ok_or_else(|| io::Error::other("content-detected source was not scanned"))?;
        require(
            content_detected.language.as_deref() == Some("python"),
            "rejected optional extension did not fall through to built-in content detection",
        )?;

        options.admit_optional_languages = true;
        let nodes = scan_repo(temp.path(), &options)?;
        let overridden = nodes
            .iter()
            .find(|node| node.path == "report.txt")
            .ok_or_else(|| io::Error::other("enabled overridden source was not scanned"))?;
        require(
            overridden.language.as_deref() == Some("awk"),
            "enabled scan did not admit an explicit optional-language override",
        )?;
        Ok(())
    }

    #[test]
    fn default_scan_uses_gitignore_for_local_state() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("local-agent-state").join("rules").join("memory"))?;
        fs::create_dir(repo.join("src"))?;
        fs::write(
            repo.join("local-agent-state")
                .join("rules")
                .join("memory")
                .join("activeContext.md"),
            "private local agent state\n",
        )?;
        fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;
        fs::write(repo.join(".gitignore"), "local-agent-state/\n")?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        reject_path(&nodes, "local-agent-state")?;
        reject_path(&nodes, "local-agent-state/rules/memory/activeContext.md")?;
        require_path(&nodes, "src/main.rs")?;
        Ok(())
    }

    #[test]
    fn scans_repo_under_excluded_named_parent() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("target").join("repo");
        let src = repo.join("src");
        fs::create_dir_all(&src)?;
        fs::write(src.join("main.rs"), "fn main() {}\n")?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        require_path(&nodes, ".")?;
        require_path(&nodes, "src")?;
        require_path(&nodes, "src/main.rs")?;
        Ok(())
    }

    #[test]
    fn excludes_configured_path_prefix_without_hiding_same_named_source()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("docs").join("api"))?;
        fs::create_dir_all(repo.join("src").join("api"))?;
        fs::write(
            repo.join("docs").join("api").join("generated.rs"),
            "fn generated() {}\n",
        )?;
        fs::write(
            repo.join("src").join("api").join("live.rs"),
            "fn live() {}\n",
        )?;
        let options = ScanOptions {
            exclude_path_prefixes: vec!["docs\\api".to_string()],
            ..ScanOptions::default()
        };

        let nodes = scan_repo(&repo, &options)?;
        reject_path(&nodes, "docs/api")?;
        reject_path(&nodes, "docs/api/generated.rs")?;
        require_path(&nodes, "docs")?;
        require_path(&nodes, "src/api")?;
        require_path(&nodes, "src/api/live.rs")?;
        Ok(())
    }

    #[test]
    fn excludes_configured_directory_suffixes_for_full_and_single_path_scans()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("vendor.egg-info"))?;
        fs::create_dir_all(repo.join("src").join("live"))?;
        fs::write(repo.join("vendor.egg-info").join("PKG-INFO"), "metadata\n")?;
        fs::write(
            repo.join("src").join("live").join("main.rs"),
            "fn main() {}\n",
        )?;
        let options = ScanOptions {
            exclude_dir_suffixes: vec![".egg-info".to_string()],
            ..ScanOptions::default()
        };

        let nodes = scan_repo(&repo, &options)?;
        reject_path(&nodes, "vendor.egg-info")?;
        reject_path(&nodes, "vendor.egg-info/PKG-INFO")?;
        require_path(&nodes, "src/live/main.rs")?;

        let single = scan_path(
            &repo,
            &repo.join("vendor.egg-info").join("PKG-INFO"),
            &options,
        )?;
        if single.is_some() {
            return Err(
                io::Error::other("single-path refresh indexed suffix-excluded file").into(),
            );
        }
        Ok(())
    }

    #[test]
    fn default_scan_indexes_durable_projectatlas_inputs_only() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let projectatlas = repo.join(".projectatlas");
        fs::create_dir_all(&projectatlas)?;
        fs::write(
            projectatlas.join("config.toml"),
            "[project]\nroot = \".\"\n",
        )?;
        fs::write(
            projectatlas.join("projectatlas-nonsource-files.toon"),
            "nonsource_files[]:\n",
        )?;
        fs::write(
            projectatlas.join("projectatlas-purpose-review.json"),
            "{\"items\":[]}\n",
        )?;
        fs::write(projectatlas.join("projectatlas.db"), b"sqlite bytes")?;
        fs::write(projectatlas.join("projectatlas.toon"), "generated map\n")?;
        fs::write(projectatlas.join("projectatlas.mcp.json"), "{}\n")?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        require_path(&nodes, ".projectatlas")?;
        require_path(&nodes, ".projectatlas/config.toml")?;
        require_path(&nodes, ".projectatlas/projectatlas-nonsource-files.toon")?;
        require_path(&nodes, ".projectatlas/projectatlas-purpose-review.json")?;
        reject_path(&nodes, ".projectatlas/projectatlas.db")?;
        reject_path(&nodes, ".projectatlas/projectatlas.toon")?;
        reject_path(&nodes, ".projectatlas/projectatlas.mcp.json")?;
        Ok(())
    }

    #[test]
    fn scan_inherits_gitignore_for_ignored_directories() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("local-state").join("memory"))?;
        fs::create_dir(repo.join("src"))?;
        fs::write(repo.join(".gitignore"), "local-state/\n")?;
        fs::write(
            repo.join("local-state").join("memory").join("notes.md"),
            "local ignored notes\n",
        )?;
        fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        reject_path(&nodes, "local-state")?;
        reject_path(&nodes, "local-state/memory/notes.md")?;
        require_path(&nodes, "src/main.rs")?;
        Ok(())
    }

    #[test]
    fn scan_path_inherits_gitignore_for_single_path_refresh() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("local-state").join("memory"))?;
        fs::create_dir(repo.join("src"))?;
        fs::write(repo.join(".gitignore"), "local-state/\n")?;
        fs::write(
            repo.join("local-state").join("memory").join("notes.md"),
            "local ignored notes\n",
        )?;
        fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;

        let ignored = scan_path(
            &repo,
            &repo.join("local-state").join("memory").join("notes.md"),
            &ScanOptions::default(),
        )?;
        let indexed = scan_path(
            &repo,
            &repo.join("src").join("main.rs"),
            &ScanOptions::default(),
        )?;
        if ignored.is_some() {
            return Err(io::Error::other("single-path refresh indexed ignored state").into());
        }
        if indexed.is_none() {
            return Err(io::Error::other("single-path refresh skipped indexed source").into());
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn nonregular_ignore_inputs_refuse_before_open_and_preserve_pruned_subtrees()
    -> Result<(), Box<dyn Error>> {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join(".git/info"))?;
        let options = ScanOptions::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        for relative in [".ignore", ".gitignore", ".git/info/exclude"] {
            let input = root.join(relative);
            mkfifo(&input, Mode::S_IRUSR | Mode::S_IWUSR)?;
            if !matches!(RootScanPolicy::discover(&root, &options, &control),
                Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput)
            {
                return Err(io::Error::other("nonregular root policy input was accepted").into());
            }
            fs::remove_file(input)?;
        }
        let nested = root.join("nested");
        fs::create_dir(&nested)?;
        let input = nested.join(".ignore");
        let policy = RootScanPolicy::discover(&root, &options, &control)?;
        mkfifo(&input, Mode::S_IRUSR | Mode::S_IWUSR)?;
        if !matches!(policy.excludes_path(&nested.join("absent.rs")),
            Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput)
        {
            return Err(io::Error::other("targeted nested policy input was accepted").into());
        }
        // The legacy Git-only helper must not inspect the disabled .ignore family.
        if gitignore_excludes_path(&root, &nested.join("absent.rs"))? {
            return Err(
                io::Error::other("legacy Git-only policy changed its admitted family").into(),
            );
        }
        fs::write(root.join(".gitignore"), "nested/\n")?;
        if !policy.excludes_path(&nested.join("absent.rs"))? {
            return Err(io::Error::other("targeted ignored parent was reopened").into());
        }
        let nodes = scan_repo(&root, &options)?;
        reject_path(&nodes, "nested")?;
        fs::remove_file(input)?;
        fs::write(root.join(".gitignore"), "")?;
        let regular = temp.path().join("regular-policy");
        fs::write(&regular, "blocked.rs\n")?;
        symlink(&regular, nested.join(".ignore"))?;
        fs::write(nested.join("blocked.rs"), "pub fn blocked() {}")?;
        fs::write(nested.join("kept.rs"), "pub fn kept() {}")?;
        let policy = RootScanPolicy::discover(&root, &options, &control)?;
        if !policy.excludes_path(&nested.join("blocked.rs"))? {
            return Err(io::Error::other("regular policy symlink was not applied").into());
        }
        let nodes = scan_repo(&root, &options)?;
        reject_path(&nodes, "nested/blocked.rs")?;
        require_path(&nodes, "nested/kept.rs")?;
        // An absent or non-directory parent is not a selected policy directory.
        fs::write(root.join("ordinary.rs"), "pub fn ordinary() {}")?;
        policy.excludes_path(&root.join("ordinary.rs/absent.rs"))?;
        Ok(())
    }

    #[test]
    fn selected_root_spelling_keeps_targeted_policy_and_scans_consistent()
    -> Result<(), Box<dyn Error>> {
        fn write_policy_root(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
            let root = directory.join("repo");
            fs::create_dir(&root)?;
            fs::create_dir(directory.join("step"))?;
            fs::write(root.join(".gitignore"), "blocked.rs\nmissing.md\n")?;
            fs::write(root.join("kept.rs"), "pub fn kept() {}")?;
            fs::write(root.join("blocked.rs"), "pub fn blocked() {}")?;
            Ok(root)
        }

        let cwd = std::env::current_dir()?;
        let temp = tempfile::tempdir_in(&cwd)?;
        let directory = temp.path().canonicalize()?;
        let root = write_policy_root(&directory)?;
        // Keep relative-root coverage under CWD even when a long Windows CWD
        // requires verbatim spelling. The Win32 parent case uses its own temp root.
        #[cfg(windows)]
        let alias_temp = tempfile::tempdir()?;
        #[cfg(windows)]
        let alias_directory = alias_temp.path().canonicalize()?;
        #[cfg(not(windows))]
        let alias_directory = directory;
        #[cfg(windows)]
        let alias_root = write_policy_root(&alias_directory)?;
        #[cfg(not(windows))]
        let alias_root = root.clone();
        // A Win32 spelling retains `..`; joining onto a verbatim path would
        // normalize it away and silently miss the alias branch on Windows.
        #[cfg(windows)]
        let directory_spelling = PathBuf::from(projectatlas_core::lossless_native_path_display(
            &alias_directory,
        )?);
        #[cfg(not(windows))]
        let directory_spelling = alias_directory;
        #[cfg(windows)]
        require(
            directory_spelling.is_absolute()
                && matches!(directory_spelling.components().next(), Some(std::path::Component::Prefix(prefix))
                    if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(..))),
            "root parent fixture requires an ordinary Win32 base",
        )?;
        let spelling = directory_spelling.join("step").join("..").join("repo");
        require(
            spelling != alias_root,
            "fixture did not retain a distinct root spelling",
        )?;
        let relative = PathBuf::from(
            temp.path()
                .file_name()
                .ok_or_else(|| io::Error::other("relative fixture has no directory name"))?,
        )
        .join("step")
        .join("..")
        .join("repo");
        let absolute_relative = cwd.join(&relative);
        let selections = [
            (root.clone(), root.clone(), root.clone()),
            (alias_root, spelling.clone(), spelling),
            (root, relative, absolute_relative),
        ];
        let options = ScanOptions::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        require(
            matches!(RootScanPolicy::discover(Path::new(""), &options, &control), Err(FsError::RootNotDirectory(path)) if path.as_os_str().is_empty()),
            "empty root selected the working directory",
        )?;
        require(
            RootScanPolicy::discover(Path::new("."), &options, &control)?.root
                == cwd.canonicalize()?,
            "explicit current-directory root changed its identity",
        )?;
        for (root, selected, absolute) in selections {
            let canonical_nodes = scan_repo(&root, &options)?;
            let policy = RootScanPolicy::discover(&selected, &options, &control)?;
            require(
                policy.root == root,
                "selected spelling changed canonical root identity",
            )?;
            require(
                scan_repo(&selected, &options)? == canonical_nodes,
                "selected spelling changed canonical source rows",
            )?;
            for prefix in [&root, &absolute] {
                require(
                    !policy.excludes_path(&prefix.join("kept.rs"))?,
                    "selected root spelling hid an admitted file",
                )?;
                require(
                    policy.excludes_path(&prefix.join("blocked.rs"))?,
                    "selected root spelling lost a local exclusion",
                )?;
                require(
                    policy.excludes_path(&prefix.join("missing.md"))?,
                    "selected root spelling lost an absent exclusion",
                )?;
                require(
                    !policy.excludes_path(&prefix.join("absent.rs"))?,
                    "selected root spelling hid an ordinary absent target",
                )?;
            }
            require(
                !policy.excludes_path(Path::new("kept.rs"))?,
                "relative target was not anchored at the selected root",
            )?;
            require(
                policy.excludes_path(Path::new("missing.md"))?,
                "relative absent exclusion was lost",
            )?;
            require(
                scan_path_with_policy_controlled(
                    &policy,
                    &absolute.join("kept.rs"),
                    ScanLimits::default(),
                    &control,
                )?
                .is_some(),
                "existing selected-root target was not scanned",
            )?;
            require(
                scan_path_with_policy_controlled(
                    &policy,
                    &absolute.join("absent.rs"),
                    ScanLimits::default(),
                    &control,
                )?
                .is_none(),
                "absent selected-root target became a source node",
            )?;
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn selected_root_alias_keeps_nested_policy_admission() -> Result<(), Box<dyn Error>> {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let alias = temp.path().join("selected");
        fs::create_dir_all(root.join("nested"))?;
        fs::write(root.join("kept.rs"), "pub fn kept() {}")?;
        symlink(&root, &alias)?;
        let options = ScanOptions::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&alias, &options, &control)?;
        require(
            !policy.excludes_path(&alias.join("kept.rs"))?,
            "selected symlink root hid an admitted file",
        )?;
        let input = root.join("nested/.ignore");
        mkfifo(&input, Mode::S_IRUSR | Mode::S_IWUSR)?;
        for target in [
            alias.join("nested/absent.rs"),
            root.canonicalize()?.join("nested/absent.rs"),
            PathBuf::from("nested/absent.rs"),
        ] {
            require(
                matches!(policy.excludes_path(&target), Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput),
                "selected-root spelling bypassed nested policy admission",
            )?;
        }
        fs::write(root.join(".gitignore"), "nested/\n")?;
        require(
            policy.excludes_path(&alias.join("nested/absent.rs"))?,
            "selected-root alias reopened a pruned policy directory",
        )?;
        let outside = temp.path().join("selected-other");
        fs::create_dir(&outside)?;
        mkfifo(&outside.join(".ignore"), Mode::S_IRUSR | Mode::S_IWUSR)?;
        require(
            policy.excludes_path(&outside.join("absent.rs"))?,
            "unselected outside path entered target policy",
        )?;
        Ok(())
    }

    #[test]
    fn root_scan_policy_classifies_absent_standard_and_atlas_ignores() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".git").join("info"))?;
        fs::write(repo.join(".gitignore"), "generated/\n")?;
        fs::write(repo.join(".ignore"), "drafts/\n")?;
        fs::write(
            repo.join(".git").join("info").join("exclude"),
            "private.txt\n",
        )?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&repo, &ScanOptions::default(), &control)?;

        require(
            policy.excludes_path(&repo.join("generated").join("missing.md"))?,
            "absent .gitignore target was not excluded",
        )?;
        require(
            policy.excludes_path(&repo.join("drafts").join("missing.md"))?,
            "absent .ignore target was not excluded",
        )?;
        require(
            policy.excludes_path(&repo.join("private.txt"))?,
            "absent Git info/exclude target was not excluded",
        )?;
        require(
            policy.excludes_path(&repo.join(".projectatlas").join("state.db"))?,
            "Atlas-specific excluded prefix was not retained",
        )?;
        Ok(())
    }

    #[test]
    fn inherited_ignore_policy_matches_full_and_targeted_subroots() -> Result<(), Box<dyn Error>> {
        let mut failures = Vec::new();
        for source in [".ignore", ".gitignore", ".git/info/exclude"] {
            let temp = tempfile::tempdir()?;
            let repo = temp.path().join("repo");
            let selected = repo.join("sub");
            fs::create_dir_all(&selected)?;
            run_git(&repo, &["init", "--quiet"])?;
            fs::write(repo.join(source), "sub/hidden.rs\n")?;
            fs::write(selected.join("hidden.rs"), "fn hidden() {}\n")?;
            fs::write(selected.join("keep.rs"), "fn keep() {}\n")?;
            let nodes = scan_repo(&selected, &ScanOptions::default())?;
            require_path(&nodes, "keep.rs")?;
            let traversed = nodes.iter().any(|node| node.path == "hidden.rs");
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            let policy = RootScanPolicy::discover(&selected, &ScanOptions::default(), &control)?;
            let excluded = policy.excludes_path(&selected.join("hidden.rs"))?;
            if traversed || !excluded {
                failures.push(format!(
                    "{source}: full admitted={traversed}, targeted excluded={excluded}"
                ));
            }
        }
        require(
            failures.is_empty(),
            &format!("inherited policy disagreed: {failures:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn ignored_parent_cannot_be_reopened_by_a_leaf_negation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path();
        fs::create_dir(repo.join("private"))?;
        fs::write(repo.join(".gitignore"), "private/\n!private/hidden.rs\n")?;
        fs::write(repo.join("private/hidden.rs"), "fn hidden() {}\n")?;
        let nodes = scan_repo(repo, &ScanOptions::default())?;
        require(
            !nodes.iter().any(|node| node.path == "private/hidden.rs"),
            "full scan admitted a leaf below an excluded parent",
        )?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(repo, &ScanOptions::default(), &control)?;
        require(
            policy.excludes_path(&repo.join("private/hidden.rs"))?,
            "targeted policy reopened an excluded parent",
        )?;
        Ok(())
    }

    #[test]
    fn scan_path_skips_symlinked_files_before_canonicalizing() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir(&repo)?;
        let outside = temp.path().join("outside.txt");
        let link = repo.join("linked.txt");
        fs::write(&outside, "outside secret\n")?;
        if !create_file_symlink(&outside, &link)? {
            return Ok(());
        }

        let indexed = scan_path(&repo, &link, &ScanOptions::default())?;
        if indexed.is_some() {
            return Err(io::Error::other("single-path refresh indexed a symlink").into());
        }
        Ok(())
    }

    #[test]
    fn scan_path_skips_symlinked_ancestor_before_canonicalizing() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let outside = temp.path().join("outside");
        fs::create_dir(&repo)?;
        fs::create_dir(&outside)?;
        fs::write(outside.join("secret.rs"), "fn secret() {}\n")?;
        let link = repo.join("linked");
        if !create_dir_symlink(&outside, &link)? {
            return Ok(());
        }

        let indexed = scan_path(&repo, &link.join("secret.rs"), &ScanOptions::default())?;
        if indexed.is_some() {
            return Err(
                io::Error::other("single-path refresh indexed through a symlinked folder").into(),
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_files() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir(&repo)?;
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, "outside secret\n")?;
        symlink(&outside, repo.join("linked.txt"))?;

        let nodes = scan_repo(&repo, &ScanOptions::default())?;
        reject_path(&nodes, "linked.txt")?;
        Ok(())
    }

    /// Create a file symlink for tests, returning false when the host forbids it.
    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }

    /// Create a file symlink for tests, returning false when the host forbids it.
    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        match std::os::windows::fs::symlink_file(target, link) {
            Ok(()) => Ok(true),
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Create a directory symlink for tests, returning false when the host forbids it.
    #[cfg(unix)]
    fn create_dir_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }

    /// Create a directory symlink for tests, returning false when the host forbids it.
    #[cfg(windows)]
    fn create_dir_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(true),
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Require a scanned node path to exist.
    fn require_path(nodes: &[Node], expected: &str) -> Result<(), Box<dyn Error>> {
        if nodes.iter().any(|node| node.path == expected) {
            Ok(())
        } else {
            Err(io::Error::other(format!("missing scanned path {expected}")).into())
        }
    }

    /// Require a scanned node path not to exist.
    fn reject_path(nodes: &[Node], rejected: &str) -> Result<(), Box<dyn Error>> {
        if nodes.iter().any(|node| node.path == rejected) {
            Err(io::Error::other(format!("unexpected scanned path {rejected}")).into())
        } else {
            Ok(())
        }
    }

    /// Require a test condition without panicking from a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }
}
