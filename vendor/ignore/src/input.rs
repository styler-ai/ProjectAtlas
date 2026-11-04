//! Optional policy-file I/O supplied by the owner of a walk.

use std::{
    fmt,
    io::{self, Read},
    path::Path,
};

/// Semantic role of one input consumed by the canonical ignore engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum IgnoreFileKind {
    /// Global, repository or hierarchical ignore rules.
    Rules,
    /// A linked checkout's `.git` pointer.
    GitDirectory,
    /// A linked checkout's `commondir` pointer.
    CommonDirectory,
}

/// Operation-owned access to the inputs consumed by ignore matching.
///
/// The default walker uses ordinary filesystem I/O. An explicit source may
/// meter reads and retain evidence of the bytes actually parsed. It must
/// preserve the selected path's filesystem and missing-file semantics.
/// Shared sources can be called concurrently by parallel walkers. Cached
/// matchers reuse the original read; they do not issue another notification.
/// Automatic global Git configuration selection is outside this I/O seam:
/// callers observing that selection should disable automatic global discovery
/// and add its explicitly selected rule file instead.
pub trait IgnoreFileSource: fmt::Debug + Send + Sync {
    /// Open the selected file for the canonical parser.
    ///
    /// # Errors
    ///
    /// Errors retain the parser's existing path/line and partial-error handling.
    fn open(&self, path: &Path, kind: IgnoreFileKind) -> io::Result<Box<dyn Read + Send>>;

    /// Observe an absent candidate skipped by a directory/existence optimization.
    ///
    /// This records the absence used by the matcher, without opening a path
    /// that may have appeared since the directory was read.
    ///
    /// # Errors
    ///
    /// Returns an operation-owned observation or resource error.
    fn absent(&self, path: &Path, kind: IgnoreFileKind) -> io::Result<()>;
}
