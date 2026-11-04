//! Test-only attribution of native current-read work; absent from shipping runtimes.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Closed owner boundaries whose inclusive durations can overlap.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::runtime) enum Stage {
    /// Resolve the caller's source and database namespace.
    Binding,
    /// Acquire or start the process-local native observer.
    Registry,
    /// Reset coverage and drain events before exact verification.
    Clear,
    /// Discover selected policy inputs and their native namespaces.
    PolicyDiscovery,
    /// Acknowledge non-recursive native policy registrations.
    PolicyInstall,
    /// Acknowledge recursive source-root registration.
    RecursiveRebind,
    /// Build and witness policy before source verification.
    BeforePolicy,
    /// Open and assess the exact saved-source snapshot.
    ExactRead,
    /// Open the root-bound read-only database snapshot.
    StoreOpen,
    /// Walk and hash current repository source.
    SourceWalk,
    /// Compare ordered source rows and document projections with the snapshot.
    SQLiteCompare,
    /// Confirm that policy is unchanged after source verification.
    AfterPolicy,
    /// Fence the completed query against native events and policy changes.
    Acceptance,
}

thread_local! {
    /// Only the explicitly captured test thread records timings; callbacks do not.
    static TIMINGS: RefCell<Option<BTreeMap<Stage, (u64, Duration)>>> = const { RefCell::new(None) };
}

/// One explicitly enabled test scope, reset even when a test exits early.
pub(in crate::runtime) struct Capture;

impl Capture {
    /// Enable bounded attribution without allowing nested captures to erase evidence.
    pub(in crate::runtime) fn start() -> Result<Self, std::io::Error> {
        TIMINGS.with_borrow_mut(|timings| {
            if timings.is_some() {
                return Err(std::io::Error::other("read timing capture already active"));
            }
            *timings = Some(BTreeMap::new());
            Ok(Self)
        })
    }

    /// Transfer inclusive call counts and durations to the test's diagnostic output.
    pub(in crate::runtime) fn finish(self) -> BTreeMap<Stage, (u64, Duration)> {
        let timings = TIMINGS.with_borrow_mut(|timings| timings.take().unwrap_or_default());
        drop(self);
        timings
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        TIMINGS.with_borrow_mut(|timings| *timings = None);
    }
}

/// Record a single owner's duration without holding a borrow across production work.
pub(in crate::runtime) struct Span {
    /// Owner receiving this inclusive sample.
    stage: Stage,
    /// Disabled captures avoid even sampling the clock.
    started: Option<Instant>,
}

impl Stage {
    /// Start an inclusive sample only when this test thread requested attribution.
    pub(in crate::runtime) fn start(self) -> Span {
        Span {
            stage: self,
            started: TIMINGS.with_borrow(|timings| timings.is_some().then(Instant::now)),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            let elapsed = started.elapsed();
            TIMINGS.with_borrow_mut(|timings| {
                if let Some(timings) = timings {
                    let (calls, duration) = timings.entry(self.stage).or_default();
                    *calls = calls.saturating_add(1);
                    *duration = duration.saturating_add(elapsed);
                }
            });
        }
    }
}
