//! Cooperative cancellation and failure contracts for bounded index work.

use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Cloneable cancellation signal shared by one indexing operation.
#[derive(Clone, Debug, Default)]
pub struct IndexCancellation {
    /// Atomic flag observed by every worker participating in the operation.
    cancelled: Arc<AtomicBool>,
}

impl IndexCancellation {
    /// Create an active cancellation signal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Return whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// Closed stages at which bounded indexing can stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexWorkStage {
    /// Repository entries are being discovered and filtered.
    RepositoryTraversal,
    /// Source metadata is being read and classified.
    SourceMetadata,
    /// Exact source bytes are being hashed.
    SourceHash,
    /// Source text is being staged for lexical and structural indexing.
    TextIndex,
    /// Source symbols and relationships are being parsed.
    SymbolParsing,
    /// A completed repository scan is being finalized for its caller.
    ScanFinalization,
    /// Validated staged index data is being published.
    Publication,
}

impl fmt::Display for IndexWorkStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryTraversal => "repository_traversal",
            Self::SourceMetadata => "source_metadata",
            Self::SourceHash => "source_hash",
            Self::TextIndex => "text_index",
            Self::SymbolParsing => "symbol_parsing",
            Self::ScanFinalization => "scan_finalization",
            Self::Publication => "publication",
        })
    }
}

/// Closed resources governed by indexing limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexWorkResource {
    /// Repository entries considered by a scanner.
    Entries,
    /// Total source bytes admitted for exact content hashing.
    SourceBytes,
    /// Total UTF-8 source bytes retained for one text-index publication.
    TextBytes,
    /// Authored purpose and publication-input bytes inspected by one refresh.
    PurposeBytes,
    /// Normalized legacy purpose records admitted by one refresh.
    PurposeRecords,
    /// Symbol parse results retained before sequential persistence.
    SymbolJobs,
    /// Symbol rows admitted by one index publication.
    SymbolRows,
    /// Relation rows admitted by one index publication.
    RelationRows,
    /// Retained parser-output string bytes admitted by one index publication.
    OutputBytes,
    /// Parallel workers used by one indexing operation.
    Workers,
}

impl fmt::Display for IndexWorkResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Entries => "entries",
            Self::SourceBytes => "source_bytes",
            Self::TextBytes => "text_bytes",
            Self::PurposeBytes => "purpose_bytes",
            Self::PurposeRecords => "purpose_records",
            Self::SymbolJobs => "symbol_jobs",
            Self::SymbolRows => "symbol_rows",
            Self::RelationRows => "relation_rows",
            Self::OutputBytes => "output_bytes",
            Self::Workers => "workers",
        })
    }
}

/// Typed cooperative-stop failures returned by bounded indexing work.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum IndexWorkFailure {
    /// The caller requested cancellation.
    #[error("index work was canceled during {stage}")]
    Cancelled {
        /// Stage observing cancellation.
        stage: IndexWorkStage,
    },
    /// The operation reached its shared deadline.
    #[error("index work deadline was reached during {stage}")]
    DeadlineExceeded {
        /// Stage observing the elapsed deadline.
        stage: IndexWorkStage,
    },
    /// Work would exceed one declared resource limit.
    #[error(
        "index work exceeded the {resource} limit during {stage}: observed {observed}, limit {limit}"
    )]
    ResourceLimitExceeded {
        /// Stage observing the exhausted resource.
        stage: IndexWorkStage,
        /// Resource whose limit was reached.
        resource: IndexWorkResource,
        /// Configured inclusive maximum.
        limit: u64,
        /// First observed value beyond the maximum.
        observed: u64,
    },
}

impl IndexWorkFailure {
    /// Construct a typed resource-limit failure.
    #[must_use]
    pub const fn resource_limit(
        stage: IndexWorkStage,
        resource: IndexWorkResource,
        limit: u64,
        observed: u64,
    ) -> Self {
        Self::ResourceLimitExceeded {
            stage,
            resource,
            limit,
            observed,
        }
    }
}

/// Shared cancellation and deadline boundary for one indexing operation.
#[derive(Clone, Debug)]
pub struct IndexWorkControl {
    /// Cooperative cancellation signal shared across operation workers.
    cancellation: IndexCancellation,
    /// Common operation start used to derive and report its deadline.
    started_at: Instant,
    /// Optional absolute deadline observed by every operation worker.
    deadline: Option<Instant>,
}

impl IndexWorkControl {
    /// Create a work boundary with an optional timeout from one shared start.
    #[must_use]
    pub fn new(cancellation: IndexCancellation, timeout: Option<Duration>) -> Self {
        let started_at = Instant::now();
        // An unrepresentable deadline fails closed instead of silently removing the bound.
        let deadline = timeout.map(|timeout| started_at.checked_add(timeout).unwrap_or(started_at));
        Self {
            cancellation,
            started_at,
            deadline,
        }
    }

    /// Create a work boundary with a caller-selected absolute deadline.
    #[must_use]
    pub fn with_deadline(cancellation: IndexCancellation, deadline: Instant) -> Self {
        Self {
            cancellation,
            started_at: Instant::now(),
            deadline: Some(deadline),
        }
    }

    /// Return the common start instant for this operation.
    #[must_use]
    pub fn started_at(&self) -> Instant {
        self.started_at
    }

    /// Return the common absolute deadline, when configured.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Clone this boundary while applying a maximum duration from its original start.
    #[must_use]
    pub fn with_timeout_ceiling(&self, timeout: Duration) -> Self {
        let ceiling = self
            .started_at
            .checked_add(timeout)
            .unwrap_or(self.started_at);
        Self {
            cancellation: self.cancellation.clone(),
            started_at: self.started_at,
            deadline: Some(
                self.deadline
                    .map_or(ceiling, |deadline| deadline.min(ceiling)),
            ),
        }
    }

    /// Request cooperative cancellation for every clone of this control.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Check cancellation and deadline state at one typed work stage.
    ///
    /// # Errors
    ///
    /// Returns [`IndexWorkFailure::Cancelled`] when cancellation was requested,
    /// or [`IndexWorkFailure::DeadlineExceeded`] when the deadline elapsed.
    pub fn check(&self, stage: IndexWorkStage) -> Result<(), IndexWorkFailure> {
        if self.cancellation.is_cancelled() {
            return Err(IndexWorkFailure::Cancelled { stage });
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(IndexWorkFailure::DeadlineExceeded { stage });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clones must observe one cancellation flag and one absolute deadline.
    #[test]
    fn work_control_shares_cancellation_and_deadline() {
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let worker = control.clone();
        assert_eq!(worker.started_at(), control.started_at());
        cancellation.cancel();
        assert_eq!(
            worker.check(IndexWorkStage::SourceHash),
            Err(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SourceHash,
            })
        );

        let bounded = worker.with_timeout_ceiling(Duration::from_secs(1));
        assert_eq!(bounded.started_at(), worker.started_at());
        assert!(bounded.deadline().is_some());

        let elapsed = IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
        assert_eq!(
            elapsed.check(IndexWorkStage::RepositoryTraversal),
            Err(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::RepositoryTraversal,
            })
        );

        let overflow = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::MAX));
        assert_eq!(
            overflow.check(IndexWorkStage::RepositoryTraversal),
            Err(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::RepositoryTraversal,
            })
        );
    }
}
