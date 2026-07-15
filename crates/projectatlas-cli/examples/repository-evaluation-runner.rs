//! Dedicated executable for isolated repository-evaluation evidence capture.

#[path = "../src/bounded_process_supervisor.rs"]
mod bounded_process_supervisor;
#[allow(
    dead_code,
    reason = "this evidence target uses shared Git primitives but retains its process-evidence-specific command path"
)]
#[path = "../src/git_process_policy.rs"]
mod git_process_policy;
#[path = "../src/graph_scale_plan.rs"]
mod graph_scale_plan;
#[path = "../src/repository_evaluation_runner.rs"]
mod repository_evaluation_runner;
#[path = "../src/sqlite_architecture_evaluation.rs"]
mod sqlite_architecture_evaluation;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), repository_evaluation_runner::EvaluationError> {
    repository_evaluation_runner::run_from_arguments().await
}
