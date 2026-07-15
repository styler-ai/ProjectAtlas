//! Dedicated executable for manifest-bound repository-graph scale evidence.

use stats_alloc::{INSTRUMENTED_SYSTEM, StatsAlloc};
use std::alloc::System;

/// Instrumented allocator used only by the graph-scale evidence executable.
#[global_allocator]
static GRAPH_SCALE_ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[allow(
    dead_code,
    reason = "this standalone target includes the shared supervisor only for its command policy"
)]
#[path = "../src/bounded_process_supervisor.rs"]
mod bounded_process_supervisor;
#[path = "../src/git_process_policy.rs"]
mod git_process_policy;
#[path = "../src/graph_scale_evaluation.rs"]
mod graph_scale_evaluation;
#[path = "../src/graph_scale_plan.rs"]
mod graph_scale_plan;
#[path = "../src/graph_scale_process.rs"]
mod graph_scale_process;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), graph_scale_evaluation::GraphScaleEvaluationError> {
    let command = graph_scale_evaluation::parse_arguments(std::env::args_os().skip(1).collect())?;
    graph_scale_evaluation::run(command).await
}
