//! Select cooperative indexing deadlines from evidence already required by the operation.

use super::{MAX_INDEX_WORK_TIMEOUT, SymbolBuildOptions};
use projectatlas_core::{IndexWorkControl, Node, NodeKind};
use std::time::Duration;

/// Source work whose count and byte total are known without another filesystem walk.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SourceWork {
    /// Files and folders admitted by the owning pipeline.
    entries: u64,
    /// Current source bytes, excluding folder metadata.
    bytes: u64,
}

impl SourceWork {
    /// Count the existing inventory; absent file sizes make the estimate unknown.
    fn from_nodes(nodes: &[Node]) -> Option<Self> {
        nodes.iter().try_fold(Self::default(), Self::include)
    }

    /// Add one already-read persisted source record without retaining another inventory.
    pub(super) fn include(mut self, node: &Node) -> Option<Self> {
        self.entries = self.entries.checked_add(1)?;
        if node.kind == NodeKind::File {
            self.bytes = self.bytes.checked_add(node.size_bytes?)?;
        }
        Some(self)
    }

    /// Headroom for one exact traversal/hash pass over this inventory.
    fn discovery_seconds(self) -> u64 {
        self.entries
            .div_ceil(4_096)
            .saturating_add(self.bytes.div_ceil(64 * 1_024 * 1_024))
    }
}

/// Select publication work from admitted source and the complete validation inventory.
pub(super) fn planned_timeout(
    options: &SymbolBuildOptions,
    admitted: &[Node],
    verification: &[Node],
    unestimated_removals: bool,
) -> Duration {
    if options.timeout.is_some() || unestimated_removals {
        return MAX_INDEX_WORK_TIMEOUT;
    }
    let (Some(admitted), Some(verification)) = (
        SourceWork::from_nodes(admitted),
        SourceWork::from_nodes(verification),
    ) else {
        return MAX_INDEX_WORK_TIMEOUT;
    };
    Duration::from_secs(
        30_u64
            .saturating_add(admitted.entries.div_ceil(32))
            .saturating_add(admitted.bytes.div_ceil(4_096))
            .saturating_add(verification.discovery_seconds().saturating_mul(2))
            .min(MAX_INDEX_WORK_TIMEOUT.as_secs()),
    )
}

/// Retain the unknown-work ceiling unless an existing read proves non-shrinking replacement.
pub(super) fn replacement_timeout(
    options: &SymbolBuildOptions,
    admitted: &[Node],
    previous: Option<SourceWork>,
) -> Duration {
    let non_shrinking =
        previous
            .zip(SourceWork::from_nodes(admitted))
            .is_some_and(|(previous, current)| {
                current.entries >= previous.entries && current.bytes >= previous.bytes
            });
    planned_timeout(options, admitted, admitted, !non_shrinking)
}

/// Add measured old/new closure work after staging, retaining unknown-work headroom.
pub(super) fn staged_timeout(
    options: &SymbolBuildOptions,
    source_timeout: Duration,
    symbol_bytes: u64,
    graph_bytes: Option<u64>,
) -> Duration {
    if options.timeout.is_some() {
        return MAX_INDEX_WORK_TIMEOUT;
    }
    let Some(bytes) = graph_bytes.and_then(|bytes| bytes.checked_add(symbol_bytes)) else {
        return MAX_INDEX_WORK_TIMEOUT;
    };
    source_timeout.max(Duration::from_secs(
        30_u64
            .saturating_add(bytes.div_ceil(4_096))
            .min(MAX_INDEX_WORK_TIMEOUT.as_secs()),
    ))
}

/// Bound an unchanged result by measured discovery, preserving the operation's clock.
pub(super) fn unchanged_control(
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
    discovered_entries: u64,
    discovered_bytes: u64,
) -> IndexWorkControl {
    let timeout = if options.timeout.is_some() {
        MAX_INDEX_WORK_TIMEOUT
    } else {
        Duration::from_secs(
            5_u64
                .saturating_add(
                    SourceWork {
                        entries: discovered_entries,
                        bytes: discovered_bytes,
                    }
                    .discovery_seconds(),
                )
                .min(MAX_INDEX_WORK_TIMEOUT.as_secs()),
        )
    };
    control.with_timeout_ceiling(timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::{IndexCancellation, IndexWorkStage};

    fn file(size: Option<u64>) -> Node {
        Node {
            path: "lib.rs".to_string(),
            kind: NodeKind::File,
            parent_path: None,
            extension: None,
            language: None,
            size_bytes: size,
            mtime_ns: None,
            content_hash: None,
        }
    }

    #[test]
    fn estimates_distinguish_work_and_fail_closed_on_unknown_sizes() {
        let options = SymbolBuildOptions::new(1_024, Some(1), None);
        let small = [file(Some(1))];
        assert_eq!(
            planned_timeout(&options, &small, &small, false),
            Duration::from_secs(36)
        );
        let reference = vec![file(Some(730)); 1_024];
        assert_eq!(
            planned_timeout(&options, &reference, &reference, false),
            Duration::from_secs(249)
        );
        assert_eq!(
            planned_timeout(&options, &small, &reference, false),
            Duration::from_secs(36)
        );
        for unknown in [
            [file(None)].as_slice(),
            [file(Some(u64::MAX)), file(Some(1))].as_slice(),
        ] {
            assert_eq!(
                planned_timeout(&options, unknown, &small, false),
                MAX_INDEX_WORK_TIMEOUT
            );
        }
        assert_eq!(
            planned_timeout(&options, &[file(Some(u64::MAX))], &small, false),
            MAX_INDEX_WORK_TIMEOUT
        );
    }

    #[test]
    fn removed_or_unknown_prior_work_retains_the_maximum_budget() {
        let options = SymbolBuildOptions::new(1_024, Some(1), None);
        let small = [file(Some(1))];
        let large = [file(Some(1_024 * 1_024))];
        assert_eq!(
            planned_timeout(&options, &small, &small, true),
            MAX_INDEX_WORK_TIMEOUT
        );
        for previous in [None, SourceWork::from_nodes(&large)] {
            assert_eq!(
                replacement_timeout(&options, &small, previous),
                MAX_INDEX_WORK_TIMEOUT
            );
        }
        assert_eq!(
            replacement_timeout(&options, &small, SourceWork::from_nodes(&small)),
            planned_timeout(&options, &small, &small, false)
        );
        assert_eq!(
            replacement_timeout(&options, &[], SourceWork::from_nodes(&small)),
            MAX_INDEX_WORK_TIMEOUT
        );
    }

    #[test]
    fn measured_old_and_new_closure_work_retains_dense_headroom() {
        let options = SymbolBuildOptions::new(1_024, Some(1), None);
        let source = Duration::from_secs(36);
        assert_eq!(staged_timeout(&options, source, 0, Some(0)), source);
        assert_eq!(
            staged_timeout(&options, source, 1, Some(1_048_576)),
            Duration::from_secs(287)
        );
        for graph in [None, Some(u64::MAX)] {
            assert_eq!(
                staged_timeout(&options, source, 1, graph),
                MAX_INDEX_WORK_TIMEOUT
            );
        }
        assert_eq!(
            staged_timeout(&options, MAX_INDEX_WORK_TIMEOUT, 0, Some(0)),
            MAX_INDEX_WORK_TIMEOUT
        );
    }

    #[test]
    fn explicit_limits_keep_precedence_and_automatic_children_share_clock_and_cancel() {
        let nodes = [file(Some(1))];
        for seconds in [0, 1, 1_800, 1_801, u64::MAX] {
            let options = SymbolBuildOptions::new(1_024, Some(1), Some(seconds));
            let parent = super::super::index_work_control(&options);
            assert_eq!(
                planned_timeout(&options, &nodes, &nodes, false),
                MAX_INDEX_WORK_TIMEOUT
            );
            let child = unchanged_control(&options, &parent, 0, 0);
            assert_eq!(parent.deadline(), child.deadline());
            assert_eq!(parent.started_at(), child.started_at());
        }
        let options = SymbolBuildOptions::new(1_024, Some(1), None);
        let parent = IndexWorkControl::new(IndexCancellation::new(), Some(MAX_INDEX_WORK_TIMEOUT))
            .with_worker_ceiling(2);
        let child = unchanged_control(&options, &parent, 0, 0);
        assert_eq!(
            child
                .deadline()
                .map(|end| end.duration_since(parent.started_at())),
            Some(Duration::from_secs(5))
        );
        let cold = unchanged_control(&options, &parent, 4_097, 64 * 1_024 * 1_024 + 1);
        assert_eq!(
            cold.deadline()
                .map(|end| end.duration_since(parent.started_at())),
            Some(Duration::from_secs(9))
        );
        assert_eq!(parent.started_at(), child.started_at());
        assert_eq!(child.worker_ceiling(), Some(2));
        assert_eq!(parent.consume_purpose_bytes(10, 4), Ok(4));
        assert_eq!(child.consume_purpose_bytes(10, 5), Ok(9));
        assert!(parent.consume_purpose_bytes(10, 2).is_err());
        parent.cancel();
        assert!(child.check(IndexWorkStage::Publication).is_err());
    }
}
