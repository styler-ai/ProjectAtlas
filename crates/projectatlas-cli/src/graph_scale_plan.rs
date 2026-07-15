//! Closed manifest-owned repository-graph scale and resource plan.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Exact declared active typed entity scale.
const DECLARED_ENTITY_COUNT: usize = 1_000_000;
/// Exact declared active logical relation scale.
const DECLARED_RELATION_COUNT: usize = 3_000_000;
/// Exact preregistered process sampling interval for claim-bearing evidence.
const DECLARED_PROCESS_SAMPLE_INTERVAL_MS: u64 = 100;
/// Slowest sampling interval accepted for exploratory process evidence.
const MAX_PROCESS_SAMPLE_INTERVAL_MS: u64 = 1_000;

/// Failures in the closed graph-scale plan contract.
#[derive(Debug, Error)]
#[error("graph-scale policy failed: {0}")]
pub(super) struct GraphScalePlanError(String);

/// Closed manifest-owned graph scale and resource gate plan.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GraphScalePlan {
    /// Plan schema version.
    pub(super) schema_version: u32,
    /// Number of bounded file graphs generated and persisted.
    pub(super) graph_count: usize,
    /// Declaration entities generated per file graph.
    pub(super) declarations_per_graph: usize,
    /// Normal call relations emitted per declaration.
    pub(super) call_edges_per_declaration: usize,
    /// Additional distinct calls emitted per file graph to make the declared edge total exact.
    pub(super) extra_call_edges_per_graph: usize,
    /// Fixed worker count used by graph construction.
    pub(super) worker_count: usize,
    /// Maximum completed file graphs retained before sequential persistence.
    pub(super) completed_graphs_per_batch: usize,
    /// Untimed query warmups for each boundary.
    pub(super) query_warmups: usize,
    /// Timed repetitions for each warm query boundary.
    pub(super) query_repetitions: usize,
    /// Maximum adjacency rows decoded per service step.
    pub(super) query_limit: u32,
    /// Hard supervisor deadline for the same-executable workload child.
    pub(super) workload_timeout_seconds: u64,
    /// Maximum retained bytes for each workload child output stream.
    pub(super) process_output_limit_bytes: usize,
    /// Interval between supervised process-group resident-memory samples.
    pub(super) process_sample_interval_ms: u64,
    /// Structural resource ceilings and floors for this implementation checkpoint.
    pub(super) resource_gates: GraphScaleResourceGates,
}

/// Manifest-owned structural resource gates portable enough for implementation CI.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GraphScaleResourceGates {
    /// Maximum Rust global-allocator requests per declared logical graph fact.
    pub(super) max_rust_allocator_requests_per_logical_fact: f64,
    /// Maximum sampled aggregate resident bytes for the supervised process group.
    pub(super) max_process_group_resident_bytes: u64,
    /// Minimum staged logical graph facts per second.
    pub(super) min_staging_logical_facts_per_second: u64,
    /// Maximum retained derived database and sidecar bytes per logical graph fact.
    pub(super) max_derived_storage_bytes_per_logical_fact: u64,
}

impl GraphScalePlan {
    /// Validate the exact accepted million-entity and three-million-relation plan.
    pub(super) fn validate_declared(&self) -> Result<(), GraphScalePlanError> {
        self.validate_shape()?;
        require(
            self.expected_entities()? == DECLARED_ENTITY_COUNT,
            "graph scale must declare exactly 1,000,000 active entities",
        )?;
        require(
            self.expected_relations()? == DECLARED_RELATION_COUNT,
            "graph scale must declare exactly 3,000,000 active relations",
        )?;
        require(
            self.query_warmups >= 100 && self.query_repetitions >= 1_000,
            "full graph scale requires at least 100 warmups and 1,000 measured requests per query cell",
        )?;
        require(
            self.process_sample_interval_ms == DECLARED_PROCESS_SAMPLE_INTERVAL_MS,
            "full graph scale requires the preregistered 100 ms process sample interval",
        )
    }

    /// Validate bounded worker, query, and resource-gate choices independent of fixture size.
    pub(super) fn validate_shape(&self) -> Result<(), GraphScalePlanError> {
        require(
            self.schema_version == 1,
            "graph scale schema version drifted",
        )?;
        require(self.graph_count > 0, "graph count must be positive")?;
        require(
            self.declarations_per_graph >= 8,
            "each graph needs at least eight declarations for distinct query edges",
        )?;
        require(
            self.call_edges_per_declaration > 0,
            "call edges per declaration must be positive",
        )?;
        require(self.worker_count > 0, "worker count must be positive")?;
        require(
            self.completed_graphs_per_batch > 0
                && self.completed_graphs_per_batch <= self.worker_count,
            "completed graph batches must be positive and no wider than the worker pool",
        )?;
        require(
            self.query_warmups > 0 && self.query_repetitions >= 20,
            "warm query samples need positive warmups and at least twenty repetitions",
        )?;
        require(
            self.query_limit >= 2,
            "query limit must retain at least two calls",
        )?;
        require(
            self.workload_timeout_seconds > 0,
            "workload timeout must be positive",
        )?;
        require(
            self.process_output_limit_bytes > 0,
            "process output limit must be positive",
        )?;
        require(
            (1..=MAX_PROCESS_SAMPLE_INTERVAL_MS).contains(&self.process_sample_interval_ms),
            "process sample interval must be between 1 and 1,000 milliseconds",
        )?;
        require(
            self.resource_gates
                .max_rust_allocator_requests_per_logical_fact
                .is_finite()
                && self
                    .resource_gates
                    .max_rust_allocator_requests_per_logical_fact
                    > 0.0,
            "Rust allocator request gate must be finite and positive",
        )?;
        require(
            self.resource_gates.max_process_group_resident_bytes > 0
                && self.resource_gates.min_staging_logical_facts_per_second > 0
                && self
                    .resource_gates
                    .max_derived_storage_bytes_per_logical_fact
                    > 0,
            "resource gates must be positive",
        )?;
        let _entities = self.expected_entities()?;
        let _relations = self.expected_relations()?;
        Ok(())
    }

    /// Compute active entities, including one file entity per bounded graph.
    pub(super) fn expected_entities(&self) -> Result<usize, GraphScalePlanError> {
        let entities_per_graph = self
            .declarations_per_graph
            .checked_add(1)
            .ok_or_else(|| GraphScalePlanError("entity count overflowed".into()))?;
        self.graph_count
            .checked_mul(entities_per_graph)
            .ok_or_else(|| GraphScalePlanError("entity count overflowed".into()))
    }

    /// Compute active logical call relations from normal and exact remainder calls.
    pub(super) fn expected_relations(&self) -> Result<usize, GraphScalePlanError> {
        let normal_calls = self
            .declarations_per_graph
            .checked_mul(self.call_edges_per_declaration)
            .ok_or_else(|| GraphScalePlanError("call count overflowed".into()))?;
        let per_graph = normal_calls
            .checked_add(self.extra_call_edges_per_graph)
            .ok_or_else(|| GraphScalePlanError("relation count overflowed".into()))?;
        self.graph_count
            .checked_mul(per_graph)
            .ok_or_else(|| GraphScalePlanError("relation count overflowed".into()))
    }
}

/// Return a typed plan error when one invariant is false.
fn require(condition: bool, message: impl Into<String>) -> Result<(), GraphScalePlanError> {
    if condition {
        Ok(())
    } else {
        Err(GraphScalePlanError(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared_plan() -> GraphScalePlan {
        GraphScalePlan {
            schema_version: 1,
            graph_count: 500,
            declarations_per_graph: 1_999,
            call_edges_per_declaration: 3,
            extra_call_edges_per_graph: 3,
            worker_count: 8,
            completed_graphs_per_batch: 8,
            query_warmups: 100,
            query_repetitions: 1_000,
            query_limit: 8,
            workload_timeout_seconds: 5_400,
            process_output_limit_bytes: 8 * 1024 * 1024,
            process_sample_interval_ms: DECLARED_PROCESS_SAMPLE_INTERVAL_MS,
            resource_gates: GraphScaleResourceGates {
                max_rust_allocator_requests_per_logical_fact: 64.0,
                max_process_group_resident_bytes: 17_179_869_184,
                min_staging_logical_facts_per_second: 1_000,
                max_derived_storage_bytes_per_logical_fact: 8_192,
            },
        }
    }

    #[test]
    fn task_arri_ut_arri_4_23_graph_scale_plan_enforces_sampling_and_overflow() {
        let mut plan = declared_plan();
        assert!(plan.validate_declared().is_ok());

        plan.process_sample_interval_ms = 250;
        assert!(plan.validate_shape().is_ok());
        assert!(plan.validate_declared().is_err());

        for invalid in [0, MAX_PROCESS_SAMPLE_INTERVAL_MS + 1] {
            plan.process_sample_interval_ms = invalid;
            assert!(plan.validate_shape().is_err());
        }

        plan.process_sample_interval_ms = DECLARED_PROCESS_SAMPLE_INTERVAL_MS;
        plan.process_output_limit_bytes = 0;
        assert!(plan.validate_shape().is_err());

        plan.graph_count = 1;
        plan.declarations_per_graph = usize::MAX;
        assert!(plan.expected_entities().is_err());
    }
}
