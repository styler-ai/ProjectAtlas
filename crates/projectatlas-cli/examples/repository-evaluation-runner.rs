//! Dedicated executable for isolated repository-evaluation evidence capture.

#[path = "../src/bounded_process_supervisor.rs"]
mod bounded_process_supervisor;
#[path = "../src/git_process_policy.rs"]
mod git_process_policy;
#[path = "../src/repository_evaluation_runner.rs"]
mod repository_evaluation_runner;
#[path = "../src/sqlite_architecture_evaluation.rs"]
mod sqlite_architecture_evaluation;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), repository_evaluation_runner::EvaluationError> {
    repository_evaluation_runner::run_from_arguments().await
}
