//! Dedicated executable for reproducible calibration evidence capture.

#[path = "../src/bounded_process_supervisor.rs"]
mod bounded_process_supervisor;
#[path = "../src/calibration_evidence_runner.rs"]
mod calibration_evidence_runner;
#[path = "../src/git_process_policy.rs"]
mod git_process_policy;

#[tokio::main]
async fn main() -> Result<(), calibration_evidence_runner::CalibrationError> {
    calibration_evidence_runner::run_from_arguments().await
}
