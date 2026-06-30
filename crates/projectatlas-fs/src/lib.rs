//! Purpose: Scan repository files and folders for `ProjectAtlas` 3.

use blake3::Hasher;
use ignore::{DirEntry, WalkBuilder, WalkState, gitignore::GitignoreBuilder};
use projectatlas_core::language::detect_language_for_path;
use projectatlas_core::{
    CoreError, Node, NodeKind, normalize_repo_path, normalized_extension, normalized_parent,
};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
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

/// Scan a repository into `ProjectAtlas` nodes.
///
/// # Errors
///
/// Returns an error when the root is invalid or filesystem metadata cannot be
/// read.
pub fn scan_repo(root: &Path, options: &ScanOptions) -> FsResult<Vec<Node>> {
    if !root.is_dir() {
        return Err(FsError::RootNotDirectory(root.to_path_buf()));
    }
    let root = root.canonicalize().map_err(|source| FsError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false);
    builder.threads(0);

    let nodes = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    builder.build_parallel().run(|| {
        let root = root.clone();
        let options = options.clone();
        let nodes = Arc::clone(&nodes);
        let errors = Arc::clone(&errors);
        Box::new(move |result| {
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
            match scanned_node(&root, path) {
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
    let nodes = Arc::try_unwrap(nodes)
        .map_err(|_remaining| state_error(&root, "parallel scanner node state still shared"))?;
    let mut nodes = nodes.into_inner().map_err(|source| {
        state_error(
            &root,
            &format!("parallel scanner node state lock failed: {source}"),
        )
    })?;
    nodes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(nodes)
}

/// Scan one path into a `ProjectAtlas` node when it is indexable.
///
/// # Errors
///
/// Returns an error when root canonicalization or metadata reads fail.
pub fn scan_path(root: &Path, path: &Path, options: &ScanOptions) -> FsResult<Option<Node>> {
    let root = root.canonicalize().map_err(|source| FsError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if !absolute.exists() {
        return Ok(None);
    }
    let symlink_checked_absolute = path_for_symlink_component_check(&absolute)?;
    if path_has_symlink_component(&root, &symlink_checked_absolute)? {
        return Ok(None);
    }
    let absolute = symlink_checked_absolute
        .canonicalize()
        .map_err(|source| FsError::Io {
            path: symlink_checked_absolute.clone(),
            source,
        })?;
    if !absolute.starts_with(&root) {
        return Ok(None);
    }
    if gitignore_excludes_path(&root, &absolute)? || should_skip_path(&root, &absolute, options) {
        return Ok(None);
    }
    scanned_node(&root, &absolute)
}

/// Return a path with a canonical parent but the leaf component preserved.
fn path_for_symlink_component_check(absolute: &Path) -> FsResult<PathBuf> {
    if absolute.is_dir() {
        return absolute.canonicalize().map_err(|source| FsError::Io {
            path: absolute.to_path_buf(),
            source,
        });
    }
    let Some(parent) = absolute.parent() else {
        return Ok(absolute.to_path_buf());
    };
    let parent = parent.canonicalize().map_err(|source| FsError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    if let Some(file_name) = absolute.file_name() {
        Ok(parent.join(file_name))
    } else {
        Ok(parent)
    }
}

/// Return whether any path component below root is a symlink.
fn path_has_symlink_component(root: &Path, absolute: &Path) -> FsResult<bool> {
    let Ok(relative) = absolute.strip_prefix(root) else {
        return Ok(true);
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
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
    let relative = normalize_repo_path(&root, &absolute)?;
    if relative == "." || relative.split('/').any(|component| component == "..") {
        return Ok(false);
    }
    let is_dir = absolute.metadata().is_ok_and(|metadata| metadata.is_dir());
    let target_dir = if is_dir {
        absolute.as_path()
    } else {
        absolute.parent().unwrap_or(root.as_path())
    };
    let mut ignored = false;
    for directory in gitignore_search_dirs(&root, target_dir) {
        let gitignore_path = directory.join(".gitignore");
        if !gitignore_path.exists() {
            continue;
        }
        let mut builder = GitignoreBuilder::new(&directory);
        if let Some(error) = builder.add(&gitignore_path) {
            return Err(FsError::Io {
                path: gitignore_path,
                source: io::Error::other(error.to_string()),
            });
        }
        let matcher = builder.build().map_err(|error| FsError::Io {
            path: directory.join(".gitignore"),
            source: io::Error::other(error.to_string()),
        })?;
        let matched = matcher.matched_path_or_any_parents(&absolute, is_dir);
        if matched.is_ignore() {
            ignored = true;
        } else if matched.is_whitelist() {
            ignored = false;
        }
    }
    Ok(ignored)
}

/// Return directories whose `.gitignore` files can affect a target path.
fn gitignore_search_dirs(root: &Path, target_dir: &Path) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut current = target_dir;
    loop {
        directories.push(current.to_path_buf());
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    directories.reverse();
    directories
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
fn scanned_node(root: &Path, path: &Path) -> FsResult<Option<Node>> {
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
        return file_node(root, path, &metadata).map(Some);
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
fn file_node(root: &Path, path: &Path, metadata: &fs::Metadata) -> FsResult<Node> {
    let normalized = normalize_repo_path(root, path)?;
    let extension = normalized_extension(path);
    let language = detect_language_for_path(&normalized, extension.as_deref());
    let hash = hash_file(path)?;
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
        size_bytes: Some(metadata.len()),
        mtime_ns,
        content_hash: Some(hash),
    })
}

/// Hash a file with BLAKE3 for stale-purpose detection.
fn hash_file(path: &Path) -> FsResult<String> {
    let mut file = fs::File::open(path).map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
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
}
