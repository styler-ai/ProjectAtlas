//! Purpose: Scan repository files and folders for `ProjectAtlas` 3.

use blake3::Hasher;
use ignore::{DirEntry, WalkBuilder, WalkState};
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
        }
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
    builder.hidden(false).git_ignore(true).git_exclude(true);
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
    if should_skip_path(&root, &absolute, options) || !absolute.exists() {
        return Ok(None);
    }
    scanned_node(&root, &absolute)
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
                && (has_excluded_directory_component(&relative, options)
                    || is_reserved_metadata_file(path))
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
    })
}

/// Return whether a path is a reserved metadata file.
fn is_reserved_metadata_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        RESERVED_METADATA_FILE_NAMES
            .iter()
            .any(|reserved| *reserved == name.as_ref())
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
