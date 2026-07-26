//! Private Linux descriptor handoff shared by the parser supervisor and worker.

/// Exact Linux serve operation.
pub(crate) const SERVE_ARGUMENT: &str = "--serve";
/// Artifact-manifest descriptor flag.
pub(crate) const ARTIFACT_FD_ARGUMENT: &str = "--artifact-fd";
/// Accepted-manifest descriptor flag.
pub(crate) const ACCEPTED_FD_ARGUMENT: &str = "--accepted-fd";
/// Native-import-policy descriptor flag.
pub(crate) const POLICY_FD_ARGUMENT: &str = "--policy-fd";
/// Selected-grammar descriptor flag.
pub(crate) const GRAMMAR_FD_ARGUMENT: &str = "--grammar-fd";
