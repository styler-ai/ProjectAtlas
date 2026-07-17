//! Focused drift checks for the versioned v0.4 repository-intelligence contracts.

use super::Cli;
use super::mcp::ProjectAtlasMcpServer;
use clap::{Command as ClapCommand, CommandFactory};
use projectatlas_core::budget::{BudgetEnforcement, DefaultCoreBudgetKind, DefaultCoreBudgets};
use regex::Regex;
use rmcp::{ClientHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use syn::visit::Visit as _;

/// Versioned machine-readable contract artifact.
const CONTRACT: &str = include_str!(
    "../../../docs/benchmarks/projectatlas-v0.4-repository-intelligence-contracts.json"
);
/// Generated compatibility inventory pinned by the repository-intelligence contract.
const SURFACE: &str = include_str!("../../../fixtures/contracts/projectatlas-v0.3.26-surface.json");
/// Host-bound executable replay evidence for the generated compatibility inventory.
const COMPATIBILITY_EVIDENCE: &str =
    include_str!("../../../fixtures/contracts/projectatlas-v0.3.26-compatibility-evidence.json");
/// Executable command and tool behavior cases bound to the frozen compatibility surface.
const BEHAVIOR_CASES: &str =
    include_str!("../../../fixtures/contracts/projectatlas-v0.3.26-behavior-cases.json");
/// Risk-based verification plan for both v0.4 `OpenSpec` changes.
const VERIFICATION_PLAN: &str = include_str!("../../../openspec/task-verification-plan.json");
/// Repository-intelligence implementation tasks.
const INTELLIGENCE_TASKS: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/tasks.md");
/// Repository-intelligence architecture and pre-mortem decisions.
const INTELLIGENCE_DESIGN: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/design.md");
/// Post-stabilization repository quality tasks.
const QUALITY_TASKS: &str =
    include_str!("../../../openspec/changes/enforce-rust-test-quality-gates/tasks.md");
/// Post-stabilization repository quality proposal.
const QUALITY_PROPOSAL: &str =
    include_str!("../../../openspec/changes/enforce-rust-test-quality-gates/proposal.md");
/// Post-stabilization repository quality architecture decisions.
const QUALITY_DESIGN: &str =
    include_str!("../../../openspec/changes/enforce-rust-test-quality-gates/design.md");
/// Post-stabilization repository quality requirements.
const QUALITY_SPEC: &str = include_str!(
    "../../../openspec/changes/enforce-rust-test-quality-gates/specs/rust-test-quality-gates/spec.md"
);
/// `OpenSpec`-to-GitHub issue ownership map.
const ISSUE_MAP: &str = include_str!("../../../openspec/issue-map.json");
/// Candidate delivery inventory for language and repository-intelligence capabilities.
const CAPABILITY_REGISTRY: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
/// Pinned corpus, environment, benchmark, and statistical pre-registration.
const EVALUATION_MANIFEST: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
/// Golden full/incremental graph records used to prove canonical normalization.
const CANONICAL_GRAPH_FIXTURE: &str =
    include_str!("../../../fixtures/contracts/projectatlas-v0.4-canonical-graph.json");
/// Host, plugin lifecycle, dependency, unsafe, FFI, and containment audit.
const HOST_SAFETY: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-host-safety.json");
/// Independent Phase 0 review findings and their local dispositions.
const PHASE_REVIEW_RECORD: &str =
    include_str!("../../../docs/benchmarks/results/phase-0-truth-and-baselines/reviews.json");
/// Current candidate lockfile used to reject stale host-safety rebinding.
const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");
/// Workspace lint and dependency policy bound to the host-safety evidence.
const WORKSPACE_MANIFEST: &[u8] = include_bytes!("../../../Cargo.toml");
/// Codex plugin manifest bound to the host-lifecycle evidence.
const CODEX_PLUGIN_MANIFEST: &[u8] =
    include_bytes!("../../../plugins/projectatlas/.codex-plugin/plugin.json");
/// Claude Code plugin manifest bound to the host-lifecycle evidence.
const CLAUDE_PLUGIN_MANIFEST: &[u8] =
    include_bytes!("../../../plugins/projectatlas/.claude-plugin/plugin.json");
/// `OpenCode` template bound to the host-lifecycle evidence.
const OPENCODE_TEMPLATE: &[u8] =
    include_bytes!("../../../plugins/projectatlas/opencode/opencode.json");
/// Windows installer bound to the host-lifecycle evidence.
const WINDOWS_INSTALLER: &[u8] =
    include_bytes!("../../../plugins/projectatlas/scripts/install-runtime.ps1");
/// POSIX installer bound to the host-lifecycle evidence.
const POSIX_INSTALLER: &[u8] =
    include_bytes!("../../../plugins/projectatlas/scripts/install-runtime.sh");
/// Cross-platform source-artifact digest contract.
const SOURCE_DIGEST_MODE_UTF8_LF: &str = "utf8-lf";
/// GitHub issue that owns the repository-intelligence delivery program.
const REPOSITORY_INTELLIGENCE_ISSUE: u64 = 308;
/// GitHub issue that owns the repository-wide Rust quality program.
const RUST_TEST_QUALITY_ISSUE: u64 = 309;
/// GitHub issue that owns the first repository-intelligence implementation phase.
const REPOSITORY_INTELLIGENCE_PHASE_ISSUE: u64 = 311;

/// Minimal MCP client used to inspect the compiled tool schema.
#[derive(Clone, Default)]
struct ContractClient;

impl ClientHandler for ContractClient {}

/// Live workspace and dependency evidence used by the safety contract.
struct CurrentSafetyEvidence {
    owned_crates: BTreeSet<String>,
    unsafe_constructs: usize,
    extern_blocks: usize,
    link_attributes: usize,
    lockfile_packages: usize,
    workspace_packages: usize,
    external_packages: usize,
    custom_build_packages: usize,
}

/// Closed reviewer-role catalog for the Phase 0 exit record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum PhaseReviewerRole {
    Architecture,
    Rust,
    Performance,
    Security,
    Storage,
    Platform,
    AgentWorkflow,
}

/// Closed review completion state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PhaseReviewerState {
    Resolved,
}

/// Closed authorization boundary for the Phase 0 record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PhaseReviewAuthorization {
    ImplementationReadinessOnly,
}

/// Closed claim state for readiness evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PhaseReviewClaimState {
    NoClaim,
}

/// Closed aggregate result state for the review record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PhaseReviewResultState {
    Pass,
}

/// Closed finding dispositions accepted at the Phase 0 boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PhaseFindingDisposition {
    Fixed,
    NarrowedAndDeferred,
    Deferred,
}

/// One role-specific Phase 0 review result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseRoleReview {
    role: PhaseReviewerRole,
    state: PhaseReviewerState,
    finding_ids: Vec<String>,
    evidence_paths: Vec<String>,
}

/// One independently reviewed Phase 0 finding.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseReviewFinding {
    finding_id: String,
    subject: String,
    blocking: bool,
    disposition: PhaseFindingDisposition,
    evidence_paths: Vec<String>,
    deferred_to_tasks: Vec<String>,
}

/// Typed, fail-closed Phase 0 implementation-readiness review record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseReviewRecord {
    schema_version: u64,
    artifact_kind: String,
    phase_id: String,
    task_id: String,
    authorization: PhaseReviewAuthorization,
    claim_state: PhaseReviewClaimState,
    state: PhaseReviewResultState,
    unresolved_blocking_finding_ids: Vec<String>,
    reviewers: Vec<PhaseRoleReview>,
    findings: Vec<PhaseReviewFinding>,
}

const REQUIRED_PHASE_REVIEW_ROLES: &[PhaseReviewerRole] = &[
    PhaseReviewerRole::Architecture,
    PhaseReviewerRole::Rust,
    PhaseReviewerRole::Performance,
    PhaseReviewerRole::Security,
    PhaseReviewerRole::Storage,
    PhaseReviewerRole::Platform,
    PhaseReviewerRole::AgentWorkflow,
];

/// Expected disposition and ownership for one required Phase 0 finding.
struct RequiredPhaseFinding {
    id: &'static str,
    blocking: bool,
    disposition: PhaseFindingDisposition,
    deferred_to_tasks: &'static [&'static str],
    reviewer_roles: &'static [PhaseReviewerRole],
    evidence_paths: &'static [&'static str],
}

const REQUIRED_PHASE_FINDINGS: &[RequiredPhaseFinding] = &[
    RequiredPhaseFinding {
        id: "readiness-schema-gaps",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[PhaseReviewerRole::Architecture, PhaseReviewerRole::Rust],
        evidence_paths: &[
            "docs/benchmarks/projectatlas-v0.4-optional-pack-candidate-readiness.json",
            "crates/projectatlas-cli/src/optional_pack_candidate_readiness_tests.rs",
        ],
    },
    RequiredPhaseFinding {
        id: "phase-release-scoreboard-coupling",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[
            PhaseReviewerRole::Architecture,
            PhaseReviewerRole::AgentWorkflow,
        ],
        evidence_paths: &[
            "docs/benchmarks/projectatlas-v0.4-phase-scoreboard.json",
            "crates/projectatlas-cli/src/repository_intelligence_scoreboard_tests.rs",
            "openspec/changes/advance-rust-repository-intelligence/design.md",
            "openspec/changes/advance-rust-repository-intelligence/tasks.md",
        ],
    },
    RequiredPhaseFinding {
        id: "sqlite-feasibility-overclaim",
        blocking: true,
        disposition: PhaseFindingDisposition::NarrowedAndDeferred,
        deferred_to_tasks: &["ARRI-11.5"],
        reviewer_roles: &[PhaseReviewerRole::Performance, PhaseReviewerRole::Storage],
        evidence_paths: &[
            "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json",
            "openspec/task-verification-plan.json",
        ],
    },
    RequiredPhaseFinding {
        id: "compatibility-surface-overclaim",
        blocking: true,
        disposition: PhaseFindingDisposition::NarrowedAndDeferred,
        deferred_to_tasks: &["ARRI-8.18", "ARRI-11.12"],
        reviewer_roles: &[PhaseReviewerRole::AgentWorkflow],
        evidence_paths: &[
            "fixtures/contracts/projectatlas-v0.3.26-surface.json",
            "openspec/task-verification-plan.json",
            "openspec/changes/advance-rust-repository-intelligence/tasks.md",
        ],
    },
    RequiredPhaseFinding {
        id: "safety-inventory-overclaim",
        blocking: true,
        disposition: PhaseFindingDisposition::NarrowedAndDeferred,
        deferred_to_tasks: &["ARRI-11.18", "ARRI-11.23", "ARRI-11.28"],
        reviewer_roles: &[PhaseReviewerRole::Platform],
        evidence_paths: &[
            "docs/benchmarks/projectatlas-v0.4-host-safety.json",
            "docs/benchmarks/projectatlas-v0.4-phase-scoreboard.json",
            "openspec/changes/advance-rust-repository-intelligence/tasks.md",
        ],
    },
    RequiredPhaseFinding {
        id: "sqlite-string-selection-policy",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[PhaseReviewerRole::Rust, PhaseReviewerRole::Storage],
        evidence_paths: &[
            "crates/projectatlas-cli/src/sqlite_architecture_evaluation.rs",
            "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json",
        ],
    },
    RequiredPhaseFinding {
        id: "evaluator-boundary-gaps",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[PhaseReviewerRole::Rust, PhaseReviewerRole::Security],
        evidence_paths: &[
            "crates/projectatlas-cli/src/repository_evaluation_runner.rs",
            "crates/projectatlas-cli/src/git_process_policy.rs",
            "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json",
        ],
    },
    RequiredPhaseFinding {
        id: "callable-variable-stable-identity",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[PhaseReviewerRole::Rust],
        evidence_paths: &["crates/projectatlas-symbols/src/lib.rs"],
    },
    RequiredPhaseFinding {
        id: "checked-task-evidence-coverage-gap",
        blocking: true,
        disposition: PhaseFindingDisposition::Fixed,
        deferred_to_tasks: &[],
        reviewer_roles: &[
            PhaseReviewerRole::Security,
            PhaseReviewerRole::AgentWorkflow,
        ],
        evidence_paths: &[
            ".github/scripts/issue-checklists.py",
            "openspec/task-verification-plan.json",
        ],
    },
    RequiredPhaseFinding {
        id: "hostile-concurrent-filesystem-containment",
        blocking: false,
        disposition: PhaseFindingDisposition::Deferred,
        deferred_to_tasks: &["ARRI-11.2"],
        reviewer_roles: &[PhaseReviewerRole::Security, PhaseReviewerRole::Platform],
        evidence_paths: &[
            "crates/projectatlas-cli/src/repository_evaluation_runner.rs",
            "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json",
        ],
    },
];

/// Parse the stable contract artifact.
fn contract() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(CONTRACT).map_err(Into::into)
}

/// Parse the generated public-surface inventory.
fn surface() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(SURFACE).map_err(Into::into)
}

/// Parse the host-bound executable compatibility evidence.
fn compatibility_evidence() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(COMPATIBILITY_EVIDENCE).map_err(Into::into)
}

/// Parse the executable compatibility behavior cases.
fn behavior_cases() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(BEHAVIOR_CASES).map_err(Into::into)
}

/// Parse the risk-based task verification plan.
fn verification_plan() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(VERIFICATION_PLAN).map_err(Into::into)
}

/// Parse the candidate capability registry.
fn capability_registry() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(CAPABILITY_REGISTRY).map_err(Into::into)
}

/// Parse the benchmark evaluation manifest.
fn evaluation_manifest() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(EVALUATION_MANIFEST).map_err(Into::into)
}

/// Parse the host lifecycle and safety evidence artifact.
fn host_safety() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(HOST_SAFETY).map_err(Into::into)
}

/// Validate that review evidence resolves to one checked-in repository file.
fn validate_phase_review_evidence_paths(
    workspace_root: &Path,
    paths: &[String],
) -> Result<(), Box<dyn Error>> {
    require(!paths.is_empty(), "review evidence paths are empty")?;
    let mut unique = BTreeSet::new();
    for raw in paths {
        require(
            !raw.trim().is_empty() && !raw.contains('\\'),
            format!("review evidence path is empty or non-normalized: {raw}"),
        )?;
        require(
            raw.split_once('/').is_some_and(|(root, _)| {
                [".github", "crates", "docs", "fixtures", "openspec"].contains(&root)
            }),
            format!("review evidence is outside accepted public roots: {raw}"),
        )?;
        let path = Path::new(raw);
        require(
            !path.is_absolute()
                && path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))),
            format!("review evidence path escapes the repository: {raw}"),
        )?;
        require(
            unique.insert(raw.as_str()),
            format!("duplicate review evidence path: {raw}"),
        )?;
        require(
            workspace_root.join(path).is_file(),
            format!("review evidence file does not exist: {raw}"),
        )?;
    }
    Ok(())
}

/// Validate the complete Phase 0 review record against repository-owned contracts.
fn validate_phase_review_record(source: &str, workspace_root: &Path) -> Result<(), Box<dyn Error>> {
    let record: PhaseReviewRecord = serde_json::from_str(source)?;
    require(
        record.schema_version == 1
            && record.artifact_kind == "projectatlas.phase-review-record"
            && record.phase_id == "phase-0-truth-and-baselines"
            && record.task_id == "ARRI-2.29"
            && record.authorization == PhaseReviewAuthorization::ImplementationReadinessOnly
            && record.claim_state == PhaseReviewClaimState::NoClaim
            && record.state == PhaseReviewResultState::Pass,
        "Phase 0 review identity, authorization, or no-claim state drifted",
    )?;
    require(
        record.unresolved_blocking_finding_ids.is_empty(),
        "Phase 0 review retains unresolved blocking findings",
    )?;

    let expected_roles = REQUIRED_PHASE_REVIEW_ROLES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut actual_roles = BTreeSet::new();
    let mut referenced_findings = BTreeSet::new();
    let mut finding_reviewers = BTreeMap::<&str, BTreeSet<PhaseReviewerRole>>::new();
    for review in &record.reviewers {
        require(
            actual_roles.insert(review.role),
            format!("duplicate Phase 0 reviewer role: {:?}", review.role),
        )?;
        require(
            review.state == PhaseReviewerState::Resolved && !review.finding_ids.is_empty(),
            format!("Phase 0 reviewer {:?} is incomplete", review.role),
        )?;
        validate_phase_review_evidence_paths(workspace_root, &review.evidence_paths)?;
        let mut role_findings = BTreeSet::new();
        for finding_id in &review.finding_ids {
            require(
                role_findings.insert(finding_id.as_str()),
                format!("reviewer {:?} repeats finding {finding_id}", review.role),
            )?;
            referenced_findings.insert(finding_id.as_str());
            finding_reviewers
                .entry(finding_id.as_str())
                .or_default()
                .insert(review.role);
        }
    }
    require(
        actual_roles == expected_roles,
        format!("Phase 0 reviewer-role catalog drifted: {actual_roles:?}"),
    )?;

    let authoritative_tasks = task_and_test_ids(INTELLIGENCE_TASKS)?
        .into_iter()
        .map(|(task_id, _)| format!("ARRI-{task_id}"))
        .collect::<BTreeSet<_>>();
    let mut actual_findings = BTreeSet::new();
    for finding in &record.findings {
        require(
            !finding.finding_id.trim().is_empty()
                && actual_findings.insert(finding.finding_id.as_str()),
            format!(
                "duplicate or empty Phase 0 finding ID: {}",
                finding.finding_id
            ),
        )?;
        require(
            !finding.subject.trim().is_empty(),
            format!("Phase 0 finding {} has no subject", finding.finding_id),
        )?;
        validate_phase_review_evidence_paths(workspace_root, &finding.evidence_paths)?;
        let expected = REQUIRED_PHASE_FINDINGS
            .iter()
            .find(|expected| expected.id == finding.finding_id)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "unknown Phase 0 finding ID: {}",
                    finding.finding_id
                ))
            })?;
        let expected_tasks = expected
            .deferred_to_tasks
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let actual_tasks = finding
            .deferred_to_tasks
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected_reviewers = expected
            .reviewer_roles
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let actual_reviewers = finding_reviewers
            .get(finding.finding_id.as_str())
            .cloned()
            .unwrap_or_default();
        let expected_evidence = expected
            .evidence_paths
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let actual_evidence = finding
            .evidence_paths
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        require(
            finding.blocking == expected.blocking
                && finding.disposition == expected.disposition
                && actual_tasks == expected_tasks
                && actual_reviewers == expected_reviewers
                && actual_evidence == expected_evidence,
            format!(
                "Phase 0 finding {} severity, disposition, reviewer ownership, evidence, or exact task ownership drifted",
                finding.finding_id
            ),
        )?;
        let deferred = matches!(
            finding.disposition,
            PhaseFindingDisposition::NarrowedAndDeferred | PhaseFindingDisposition::Deferred
        );
        require(
            deferred != finding.deferred_to_tasks.is_empty(),
            format!(
                "Phase 0 finding {} has inconsistent deferral ownership",
                finding.finding_id
            ),
        )?;
        require(
            !(finding.blocking && finding.disposition == PhaseFindingDisposition::Deferred),
            format!(
                "blocking Phase 0 finding {} was deferred without a current narrowing or fix",
                finding.finding_id
            ),
        )?;
        let mut owned_tasks = BTreeSet::new();
        for task_id in &finding.deferred_to_tasks {
            require(
                owned_tasks.insert(task_id.as_str()) && authoritative_tasks.contains(task_id),
                format!(
                    "Phase 0 finding {} has duplicate or unknown task owner {task_id}",
                    finding.finding_id
                ),
            )?;
        }
    }
    let expected_findings = REQUIRED_PHASE_FINDINGS
        .iter()
        .map(|finding| finding.id)
        .collect::<BTreeSet<_>>();
    require(
        actual_findings == expected_findings && referenced_findings == expected_findings,
        "Phase 0 finding inventory or reviewer coverage drifted",
    )
}

/// Normalize one graph value according to the repository-intelligence comparison contract.
fn normalize_graph_value(value: &Value, field: Option<&str>, excluded: &BTreeSet<String>) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !excluded.contains(*key))
                .map(|(key, value)| {
                    (
                        key.clone(),
                        normalize_graph_value(value, Some(key), excluded),
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| normalize_graph_value(value, None, excluded))
                .collect(),
        ),
        Value::String(value) => {
            let value = value.replace("\r\n", "\n").replace('\r', "\n");
            if field.is_some_and(|field| field.ends_with("path")) {
                Value::String(value.replace('\\', "/"))
            } else {
                Value::String(value)
            }
        }
        _ => value.clone(),
    }
}

/// Return canonical graph records sorted by the registered total-order tuple.
fn canonical_graph_records(policy: &Value, rows: &Value) -> Result<Vec<Value>, Box<dyn Error>> {
    let graph = value_at(policy, "/graph_snapshot")?;
    let excluded = string_set(&graph["exclude_fields"])?;
    let order = string_set(&graph["records"])?;
    let sort_fields = graph["record_order"]
        .as_array()
        .ok_or_else(|| io::Error::other("graph record_order is not an array"))?
        .iter()
        .map(|field| {
            field
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("graph sort field is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut canonical = rows
        .as_array()
        .ok_or_else(|| io::Error::other("graph fixture rows are not an array"))?
        .iter()
        .map(|row| {
            let record_kind = row["record_kind"]
                .as_str()
                .ok_or_else(|| io::Error::other("graph row lacks record_kind"))?;
            require(
                order.contains(record_kind),
                format!("unknown graph record kind {record_kind}"),
            )?;
            Ok(normalize_graph_value(row, None, &excluded))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    canonical.sort_by_cached_key(|row| {
        let tuple = sort_fields
            .iter()
            .map(|field| row.get(field).unwrap_or(&Value::Null).to_string())
            .collect::<Vec<_>>();
        (tuple, row.to_string())
    });
    Ok(canonical)
}

/// Return task and declared unit-test identities from one `OpenSpec` task file.
fn task_and_test_ids(source: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("- [ ] ")
                .or_else(|| line.strip_prefix("- [x] "))
        })
        .map(|task| {
            let task_id = task
                .split_whitespace()
                .next()
                .ok_or_else(|| io::Error::other("task has no identifier"))?;
            let test_start = ["UT:ARRI-", "TQG-UT-"]
                .into_iter()
                .find_map(|marker| task.find(marker))
                .ok_or_else(|| {
                    io::Error::other(format!("task {task_id} has no test identifier"))
                })?;
            let test_id = task[test_start..]
                .split(|character: char| {
                    !character.is_ascii_alphanumeric()
                        && character != ':'
                        && character != '-'
                        && character != '.'
                })
                .next()
                .ok_or_else(|| io::Error::other(format!("task {task_id} has malformed test ID")))?;
            Ok((task_id.to_string(), test_id.to_string()))
        })
        .collect()
}

/// Validate that each pre-mortem action is owned by an authoritative task.
fn validate_pre_mortem_mitigations(
    design: &str,
    authoritative_tasks: &BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    let risk_id = Regex::new(r"^PM-\d{2}$")?;
    let task_id = Regex::new(r"ARRI-(\d+\.\d+)")?;
    let mut risk_ids = BTreeSet::new();
    let mut row_count = 0;

    for row in design.lines().filter(|line| line.starts_with("| PM-")) {
        let cells = row
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>();
        require(cells.len() == 5, format!("malformed pre-mortem row: {row}"))?;
        require(
            risk_id.is_match(cells[0]) && risk_ids.insert(cells[0].to_string()),
            format!("duplicate or malformed pre-mortem risk ID: {}", cells[0]),
        )?;
        require(
            !cells[3].is_empty(),
            format!("{} has no mitigation action", cells[0]),
        )?;
        let owners = task_id
            .captures_iter(cells[4])
            .map(|capture| format!("ARRI-{}", &capture[1]))
            .collect::<BTreeSet<_>>();
        require(
            !owners.is_empty(),
            format!("{} has no mitigation task owner", cells[0]),
        )?;
        require(
            owners.is_subset(authoritative_tasks),
            format!(
                "{} references unknown mitigation tasks: {:?}",
                cells[0],
                owners.difference(authoritative_tasks).collect::<Vec<_>>()
            ),
        )?;
        row_count += 1;
    }

    require(row_count == 29, "pre-mortem mitigation row count drifted")
}

/// Parse a dotted task ID into its two numeric components.
fn task_id_parts(task_id: &str) -> Result<(u16, u16), Box<dyn Error>> {
    let (section, task) = task_id
        .split_once('.')
        .ok_or_else(|| io::Error::other(format!("invalid task ID {task_id}")))?;
    Ok((section.parse()?, task.parse()?))
}

/// Return whether a task ID is inside one inclusive dotted-ID range.
fn task_in_range(task_id: &str, first: &str, last: &str) -> Result<bool, Box<dyn Error>> {
    let task_id = task_id_parts(task_id)?;
    Ok(task_id >= task_id_parts(first)? && task_id <= task_id_parts(last)?)
}

/// Resolve one required JSON pointer.
fn value_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, Box<dyn Error>> {
    value.pointer(pointer).ok_or_else(|| {
        io::Error::other(format!("contract is missing JSON pointer {pointer}")).into()
    })
}

/// Return string values from one required JSON array.
fn string_set(value: &Value) -> Result<BTreeSet<String>, Box<dyn Error>> {
    value
        .as_array()
        .ok_or_else(|| io::Error::other("expected JSON array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("expected JSON string").into())
        })
        .collect()
}

/// Return keys from one required JSON object.
fn object_keys(value: &Value) -> Result<BTreeSet<String>, Box<dyn Error>> {
    Ok(value
        .as_object()
        .ok_or_else(|| io::Error::other("expected JSON object"))?
        .keys()
        .cloned()
        .collect())
}

/// Index JSON object rows by one required string field.
fn rows_by_key(rows: &[Value], key: &str) -> Result<BTreeMap<String, Value>, Box<dyn Error>> {
    rows.iter()
        .map(|row| {
            let value = row[key]
                .as_str()
                .ok_or_else(|| io::Error::other(format!("inventory row has no string {key}")))?;
            Ok((value.to_string(), row.clone()))
        })
        .collect()
}

/// Return unique string keys from rows and reject duplicate or malformed entries.
fn unique_row_keys(
    rows: &[Value],
    key: &str,
    context: &str,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let keys = rows
        .iter()
        .map(|row| {
            row[key]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other(format!("{context} row has no string {key}")))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        keys.len() == rows.len(),
        format!("{context} contains a duplicate {key}"),
    )?;
    Ok(keys)
}

/// Require every frozen row unchanged while permitting only allowlisted additions.
fn require_frozen_rows(
    expected: &[Value],
    actual: &[Value],
    key: &str,
    allowed_additions: &BTreeSet<String>,
    additive_parent: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let expected = rows_by_key(expected, key)?;
    let actual = rows_by_key(actual, key)?;
    let additions = actual
        .keys()
        .filter(|name| !expected.contains_key(*name))
        .cloned()
        .collect::<BTreeSet<_>>();
    require(
        additions.is_subset(allowed_additions),
        format!("compiled surface has unapproved additions: {additions:?}"),
    )?;

    for (name, expected_row) in expected {
        let actual_row = actual
            .get(&name)
            .ok_or_else(|| io::Error::other(format!("frozen surface row {name:?} is missing")))?;
        if additive_parent == Some(name.as_str()) && !additions.is_empty() {
            let mut expected_row = expected_row;
            let mut actual_row = actual_row.clone();
            expected_row
                .as_object_mut()
                .ok_or_else(|| io::Error::other("frozen inventory row is not an object"))?
                .remove("long_help_sha256");
            actual_row
                .as_object_mut()
                .ok_or_else(|| io::Error::other("compiled inventory row is not an object"))?
                .remove("long_help_sha256");
            require(
                expected_row == actual_row,
                format!("frozen surface row {name:?} drifted outside additive help"),
            )?;
        } else {
            require(
                &expected_row == actual_row,
                format!("frozen surface row {name:?} drifted"),
            )?;
        }
    }
    Ok(())
}

/// Require this artifact to remain explicitly non-completing until evidence closes its gaps.
fn require_pending_evidence_state(policy: &Value, task: &str) -> Result<(), Box<dyn Error>> {
    require(
        value_at(policy, "/delivery_status/tasks_checked")? == &json!(false),
        "the evidence contract cannot mark implementation tasks checked",
    )?;
    let status = value_at(policy, &format!("/delivery_status/{task}/status"))?
        .as_str()
        .ok_or_else(|| io::Error::other(format!("{task} delivery status is not a string")))?;
    require(
        status != "complete",
        format!("{task} cannot be complete without retained evidence"),
    )
}

/// Fail a focused contract test with an actionable message.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Require truthful initial unit-test state without inventing future commands.
fn validate_initial_unit_test_definition(row: &Value) -> Result<(), Box<dyn Error>> {
    let state = row["unit_test"]["state"]
        .as_str()
        .ok_or_else(|| io::Error::other("unit-test state is absent"))?;
    match state {
        "implemented_uncommitted" => require(
            row["unit_test"]["function"]
                .as_str()
                .is_some_and(|function| !function.trim().is_empty())
                && row["unit_test"]["command"]["executable"]
                    .as_str()
                    .is_some_and(|executable| !executable.trim().is_empty())
                && row["unit_test"]["command"]["arguments"].is_array(),
            "implemented unit test lacks an executable command",
        ),
        "definition_pending_stable_implementation" | "planned_not_implemented" => require(
            row["unit_test"]["function"].is_null() && row["unit_test"]["command"].is_null(),
            "pending unit test fabricates a function or command",
        ),
        other => {
            Err(io::Error::other(format!("unsupported initial unit-test state {other}")).into())
        }
    }
}

/// Require one evidence row to use declared risk and evidence-layer vocabularies.
fn validate_evidence_classification(
    row: &Value,
    risk_levels: &BTreeSet<String>,
    evidence_layers: &serde_json::Map<String, Value>,
) -> Result<(), Box<dyn Error>> {
    let required_layers = row["required_evidence_layers"]
        .as_array()
        .ok_or_else(|| io::Error::other("required evidence layers are absent"))?;
    require(
        risk_levels.contains(row["risk"].as_str().unwrap_or_default())
            && !required_layers.is_empty()
            && required_layers.iter().all(|layer| {
                layer
                    .as_str()
                    .is_some_and(|name| evidence_layers.contains_key(name))
            }),
        "verification row uses an undeclared risk or evidence layer",
    )
}

/// Require every named field on a JSON object.
fn require_fields(value: &Value, fields: &Value) -> Result<(), Box<dyn Error>> {
    let object = value
        .as_object()
        .ok_or_else(|| io::Error::other("required-field target is not an object"))?;
    for field in string_set(fields)? {
        require(
            object.contains_key(&field),
            format!("required field {field} is absent"),
        )?;
    }
    Ok(())
}

/// Require every nested evidence and gate state to use the artifact's closed vocabularies.
fn validate_host_safety_states(policy: &Value) -> Result<(), Box<dyn Error>> {
    fn visit(
        value: &Value,
        evidence_states: &BTreeSet<String>,
        gate_states: &BTreeSet<String>,
    ) -> Result<(), Box<dyn Error>> {
        match value {
            Value::Object(object) => {
                if let Some(state) = object.get("evidence_state") {
                    let state = state
                        .as_str()
                        .ok_or_else(|| io::Error::other("evidence_state is not a string"))?;
                    require(
                        evidence_states.contains(state),
                        format!("undeclared evidence_state {state}"),
                    )?;
                }
                if let Some(state) = object.get("gate_state") {
                    let state = state
                        .as_str()
                        .ok_or_else(|| io::Error::other("gate_state is not a string"))?;
                    require(
                        gate_states.contains(state),
                        format!("undeclared gate_state {state}"),
                    )?;
                }
                for child in object.values() {
                    visit(child, evidence_states, gate_states)?;
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, evidence_states, gate_states)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    require(
        policy["format"] == "projectatlas.repository-intelligence-host-safety"
            && policy["artifact_id"] == "projectatlas-v0.4-repository-intelligence-host-safety",
        "repository-intelligence host-safety identity drifted",
    )?;
    require(
        policy["source_snapshot"]["working_tree_state"]
            .as_str()
            .is_some_and(|state| !state.contains("phase0")),
        "host-safety source state uses a development-stage identity",
    )?;
    let evidence_states = object_keys(&policy["evidence_state_values"])?;
    let gate_states = object_keys(&policy["gate_state_values"])?;
    require(
        evidence_states
            == BTreeSet::from([
                "hosted_required".to_string(),
                "not_available".to_string(),
                "proven_local".to_string(),
            ]),
        format!("host-safety evidence-state vocabulary drifted: {evidence_states:?}"),
    )?;
    require(
        gate_states
            == BTreeSet::from([
                "pass".to_string(),
                "pending".to_string(),
                "release_blocker".to_string(),
            ]),
        format!("host-safety gate-state vocabulary drifted: {gate_states:?}"),
    )?;

    visit(policy, &evidence_states, &gate_states)
}

/// Validate that every local host observation is reproducible from one bound envelope.
fn validate_host_command_evidence(policy: &Value) -> Result<(), Box<dyn Error>> {
    const HISTORICAL_BINDING_ID: &str = "windows-dev-c672442-8c66fec8";
    const HISTORICAL_HEAD_COMMIT: &str = "c672442438404411389ef86e2efd767f3a4b2be0";
    const HISTORICAL_HEAD_TREE: &str = "cc9bc004837c8843b84151cb269fba41e8944116";
    const HISTORICAL_LOCKFILE_SHA256: &str =
        "8c66fec898d4535a0cdd4f88ff986f206bb53d7d8f6d548cc9f7d5cd2bcc841d";
    const HISTORICAL_MANIFEST_SHA256: &str =
        "709867f2d9bb4790f5c0e8356633efa2c34aa8466a6aaba524c3ad2cfe4d2bb7";
    const CURRENT_BINDING_ID: &str = "windows-dev-0d619b8-2e2e0073-20260717T085518Z";
    const CURRENT_MANIFEST_SHA256: &str =
        "e41c50d5292b6b6e0ed87175b7ab04640309f642cf04863c5eeb9c65eafe191e";
    let evidence = &policy["command_evidence"];
    let binding_id = evidence["binding_id"]
        .as_str()
        .ok_or_else(|| io::Error::other("host command binding id is absent"))?;
    let is_sha256 = |value: &Value| {
        value.as_str().is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    require(
        binding_id == HISTORICAL_BINDING_ID
            && evidence.get("lock_identity_rebind").is_none()
            && evidence["historical_capture"] == true
            && evidence["current_candidate_eligible"] == false
            && evidence["current_candidate_exclusion"]
                .as_str()
                .is_some_and(|reason| reason.contains("cannot prove the current candidate"))
            && evidence["host_id"] == policy["audit_host"]["id"]
            && evidence["head_commit"] == HISTORICAL_HEAD_COMMIT
            && evidence["head_tree"] == HISTORICAL_HEAD_TREE
            && evidence["lockfile_sha256"] == HISTORICAL_LOCKFILE_SHA256
            && evidence["lockfile_sha256"]
                != policy["source_snapshot"]["dirty_candidate_lockfile"]["sha256"]
            && evidence["lockfile_sha256"] != sha256(CARGO_LOCK.as_bytes())
            && evidence["raw_outputs_checked_in"] == false
            && evidence["manifest_persistence"]
                .as_str()
                .is_some_and(|state| state.contains("ignored local development evidence"))
            && evidence["manifest_ref"].as_str().is_some_and(|path| {
                path.starts_with(".projectatlas/research/v04-results/host-safety/")
                    && path.ends_with("/command-observations.json")
            })
            && evidence["manifest_sha256"] == HISTORICAL_MANIFEST_SHA256,
        "historical host command envelope was rebound or presented as current evidence",
    )?;

    let resolutions = evidence["resolution"]
        .as_array()
        .ok_or_else(|| io::Error::other("host command resolutions are not an array"))?;
    let resolutions = rows_by_key(resolutions, "tool")?;
    require(
        resolutions.keys().cloned().collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "claude".to_string(),
                "codex".to_string(),
                "opencode".to_string(),
            ])
            && resolutions.values().all(|row| {
                row["discovered_kind"] == "ExternalScript"
                    && row["executed_via"] == "cmd.exe"
                    && row["discovered_target"].as_str().is_some_and(|path| {
                        path.starts_with("%APPDATA%/npm/")
                            && std::path::Path::new(path)
                                .extension()
                                .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
                    })
                    && row["executed_target"].as_str().is_some_and(|path| {
                        path.starts_with("%APPDATA%/npm/")
                            && std::path::Path::new(path)
                                .extension()
                                .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd"))
                    })
                    && is_sha256(&row["executed_sha256"])
            }),
        "Windows host command shims are unresolved, incomplete, or personally pathed",
    )?;

    let rows = policy["commands"]
        .as_array()
        .ok_or_else(|| io::Error::other("host command observations are not an array"))?;
    let commands = rows_by_key(rows, "id")?;
    require(
        commands.keys().cloned().collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "candidate-advisory-audit".to_string(),
                "candidate-policy-audit".to_string(),
                "claude-plugin-surface".to_string(),
                "claude-version".to_string(),
                "codex-plugin-surface".to_string(),
                "codex-projectatlas-installation".to_string(),
                "codex-version".to_string(),
                "debug-pe-imports".to_string(),
                "dependency-inventory".to_string(),
                "geiger-availability".to_string(),
                "opencode-host-surface".to_string(),
                "opencode-version".to_string(),
                "owned-unsafe-source-scan".to_string(),
                "source-identity".to_string(),
                "vet-availability".to_string(),
            ]),
        "host command observation inventory drifted",
    )?;
    for (id, row) in &commands {
        let expected_exit = match id.as_str() {
            "owned-unsafe-source-scan" => 1,
            "geiger-availability" | "vet-availability" => 101,
            _ => 0,
        };
        let expected_suffix = format!("/{id}.output.bin");
        require(
            row["binding_id"] == binding_id
                && row["exact_invocation_manifest_id"] == id.as_str()
                && row["argv"].as_array().is_some_and(|argv| !argv.is_empty())
                && row["resolved_invocation"]
                    .as_str()
                    .is_some_and(|invocation| !invocation.trim().is_empty())
                && row["observed_at_utc"]
                    .as_str()
                    .is_some_and(|timestamp| timestamp.contains('T') && timestamp.ends_with('Z'))
                && row["timeout_seconds"] == 120
                && row["exit_code"] == expected_exit
                && row["timed_out"] == false
                && is_sha256(&row["output_sha256"])
                && row["raw_artifact_ref"].as_str().is_some_and(|path| {
                    path.starts_with(".projectatlas/research/v04-results/host-safety/")
                        && path.ends_with(&expected_suffix)
                }),
            format!("host command observation {id} is incomplete, stale, or unbound"),
        )?;
    }

    let current_evidence = &policy["candidate_lock_command_evidence"];
    let current_binding_id = current_evidence["binding_id"]
        .as_str()
        .ok_or_else(|| io::Error::other("current lock command binding id is absent"))?;
    require(
        current_binding_id == CURRENT_BINDING_ID
            && current_binding_id != binding_id
            && current_evidence["host_id"] == policy["audit_host"]["id"]
            && current_evidence["head_commit"] == policy["source_snapshot"]["head_commit"]
            && current_evidence["head_tree"] == policy["source_snapshot"]["head_tree"]
            && current_evidence["lockfile_sha256"]
                == policy["source_snapshot"]["dirty_candidate_lockfile"]["sha256"]
            && current_evidence["lockfile_sha256"] == sha256(CARGO_LOCK.as_bytes())
            && current_evidence["worktree_claim_eligible"] == false
            && current_evidence["claim_exclusion"]
                .as_str()
                .is_some_and(|reason| reason.contains("reviewed candidate commit"))
            && current_evidence["manifest_ref"]
                .as_str()
                .is_some_and(|path| {
                    path.starts_with(".projectatlas/research/v04-results/host-safety/")
                        && path.ends_with("/command-observations.json")
                })
            && current_evidence["manifest_sha256"] == CURRENT_MANIFEST_SHA256
            && current_evidence["raw_outputs_checked_in"] == false
            && current_evidence["manifest_persistence"]
                .as_str()
                .is_some_and(|state| state.contains("ignored local development evidence"))
            && current_evidence["timeout_seconds"] == 120,
        "current lock command envelope is unbound or presented as release evidence",
    )?;

    let current_rows = current_evidence["checks"]
        .as_array()
        .ok_or_else(|| io::Error::other("current lock checks are not an array"))?;
    let current_checks = rows_by_key(current_rows, "id")?;
    require(
        current_checks.keys().cloned().collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "candidate-advisory-audit".to_string(),
                "candidate-policy-audit".to_string(),
                "dependency-inventory".to_string(),
            ]),
        "current lock check inventory drifted",
    )?;
    for (id, row) in &current_checks {
        let manifest_observation_id = format!("{id}-current-lock");
        let expected_suffix = format!("/{manifest_observation_id}.output.bin");
        require(
            row["binding_id"] == current_binding_id
                && row["lockfile_sha256"] == current_evidence["lockfile_sha256"]
                && row["manifest_observation_id"] == manifest_observation_id
                && row["argv"].as_array().is_some_and(|argv| !argv.is_empty())
                && row["observed_at_utc"]
                    .as_str()
                    .is_some_and(|timestamp| timestamp.contains('T') && timestamp.ends_with('Z'))
                && row["timeout_seconds"] == current_evidence["timeout_seconds"]
                && row["exit_code"] == 0
                && row["timed_out"] == false
                && row["duration_ms"]
                    .as_u64()
                    .is_some_and(|duration| duration > 0)
                && is_sha256(&row["output_sha256"])
                && row["raw_artifact_ref"].as_str().is_some_and(|path| {
                    path.starts_with(".projectatlas/research/v04-results/host-safety/")
                        && path.ends_with(&expected_suffix)
                })
                && row["evidence_state"] == "proven_local"
                && row["gate_state"] == "release_blocker"
                && row["release_condition"]
                    .as_str()
                    .is_some_and(|condition| condition.contains("reviewed candidate commit")),
            format!("current lock check {id} is incomplete, stale, or claim-eligible"),
        )?;
    }

    for (id, reference) in [
        (
            "dependency-inventory",
            "candidate_lock_command_evidence/dependency-inventory",
        ),
        (
            "candidate-advisory-audit",
            "candidate_lock_command_evidence/candidate-advisory-audit",
        ),
        (
            "candidate-policy-audit",
            "candidate_lock_command_evidence/candidate-policy-audit",
        ),
    ] {
        require(
            commands[id]["current_candidate_relevance"] == "superseded_by_current_lock_check"
                && commands[id]["current_candidate_ref"] == reference,
            format!("historical command {id} is presented as current lock evidence"),
        )?;
    }
    require(
        commands["dependency-inventory"]["result"]
            == "historical dependency inventory superseded for current-candidate decisions; current counts are owned by unsafe_native_ffi_inventory.dependency_graph and its bounded locked-metadata validator",
        "historical dependency result duplicates or contradicts current typed counts",
    )?;
    let stale_rows = current_evidence["unrerun_lock_sensitive_checks"]
        .as_array()
        .ok_or_else(|| io::Error::other("unrerun lock-sensitive checks are not an array"))?;
    let stale_checks = rows_by_key(stale_rows, "id")?;
    require(
        stale_checks.keys().cloned().collect::<BTreeSet<_>>()
            == BTreeSet::from(["debug-pe-imports".to_string()])
            && commands["debug-pe-imports"]["current_candidate_relevance"] == "stale"
            && commands["debug-pe-imports"]["current_candidate_gate_state"] == "release_blocker"
            && stale_checks["debug-pe-imports"]["historical_binding_id"] == binding_id
            && stale_checks["debug-pe-imports"]["current_candidate_relevance"] == "stale"
            && stale_checks["debug-pe-imports"]["evidence_state"] == "not_available"
            && stale_checks["debug-pe-imports"]["gate_state"] == "release_blocker",
        "unrerun PE import evidence is not stale and release-blocking",
    )?;
    require(
        commands["owned-unsafe-source-scan"]["expected_exit_codes"] == json!([0, 1])
            && commands["owned-unsafe-source-scan"]["output_sha256"]
                == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            && commands["geiger-availability"]["expected_exit_codes"] == json!([0, 101])
            && commands["vet-availability"]["expected_exit_codes"] == json!([0, 101]),
        "no-match or unavailable-tool exit semantics are not explicit",
    )
}

/// Validate every source file that underpins host lifecycle or safety conclusions.
fn validate_host_source_artifacts(policy: &Value) -> Result<(), Box<dyn Error>> {
    let rows = policy["source_artifacts"]
        .as_array()
        .ok_or_else(|| io::Error::other("host source artifacts are not an array"))?;
    let rows = rows_by_key(rows, "id")?;
    let expected = [
        (
            "workspace-lint-policy",
            "Cargo.toml",
            WORKSPACE_MANIFEST,
            "pass",
        ),
        (
            "codex-plugin-manifest",
            "plugins/projectatlas/.codex-plugin/plugin.json",
            CODEX_PLUGIN_MANIFEST,
            "pass",
        ),
        (
            "claude-plugin-manifest",
            "plugins/projectatlas/.claude-plugin/plugin.json",
            CLAUDE_PLUGIN_MANIFEST,
            "pass",
        ),
        (
            "opencode-template",
            "plugins/projectatlas/opencode/opencode.json",
            OPENCODE_TEMPLATE,
            "pass",
        ),
        (
            "windows-installer",
            "plugins/projectatlas/scripts/install-runtime.ps1",
            WINDOWS_INSTALLER,
            "release_blocker",
        ),
        (
            "posix-installer",
            "plugins/projectatlas/scripts/install-runtime.sh",
            POSIX_INSTALLER,
            "release_blocker",
        ),
    ];
    require(
        rows.keys().cloned().collect::<BTreeSet<_>>()
            == expected
                .iter()
                .map(|(id, _, _, _)| (*id).to_string())
                .collect(),
        "host source artifact inventory drifted",
    )?;
    for (id, path, bytes, gate_state) in expected {
        let row = &rows[id];
        require(
            row["path"] == path
                && row["digest_mode"] == SOURCE_DIGEST_MODE_UTF8_LF
                && row["sha256"] == canonical_source_digest(bytes)?
                && row["evidence_state"] == "proven_local"
                && row["gate_state"] == gate_state,
            format!("host source artifact {id} is missing, stale, or misclassified"),
        )?;
    }
    Ok(())
}

/// Validate the fail-closed plugin-store capability truth table.
fn validate_plugin_store_lifecycle(policy: &Value) -> Result<(), Box<dyn Error>> {
    validate_host_safety_states(policy)?;
    validate_host_source_artifacts(policy)?;
    let lifecycle = &policy["plugin_store_lifecycle"];
    require(
        lifecycle["task_id"] == "ARRI-2.26"
            && lifecycle["evidence_state"] == "proven_local"
            && lifecycle["gate_state"] == "release_blocker"
            && lifecycle["hidden_manual_step_allowed"] == false
            && lifecycle["manual_installer_does_not_satisfy_one_action_contract"] == true
            && lifecycle["native_install_provisioning"]
                .as_str()
                .is_some_and(|state| state.starts_with("not_available")),
        "plugin-store lifecycle hides a manual step or claims unavailable provisioning",
    )?;
    let hosts = lifecycle["hosts"]
        .as_array()
        .ok_or_else(|| io::Error::other("plugin lifecycle hosts are not an array"))?;
    require(
        hosts
            .iter()
            .map(|host| host["host"].as_str().unwrap_or_default().to_string())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "claude-code".to_string(),
                "codex".to_string(),
                "opencode".to_string(),
            ])
            && hosts.iter().all(|host| {
                host["version"]
                    .as_str()
                    .is_some_and(|version| !version.trim().is_empty())
                    && host["clean_host_store_e2e"] == "hosted_required"
                    && host["gate_state"] == "release_blocker"
            }),
        "plugin host truth table is incomplete, stale, or not fail-closed",
    )?;
    let hosted = policy["hosted_required_evidence"]
        .as_array()
        .ok_or_else(|| io::Error::other("hosted evidence is not an array"))?;
    let store_matrix = hosted
        .iter()
        .find(|row| row["id"] == "clean-store-lifecycle-matrix")
        .ok_or_else(|| io::Error::other("clean-store lifecycle matrix is absent"))?;
    require(
        string_set(&store_matrix["platforms"])?
            == BTreeSet::from([
                "linux-x86_64".to_string(),
                "macos-arm64".to_string(),
                "macos-x86_64".to_string(),
                "windows-x86_64".to_string(),
            ])
            && store_matrix["evidence_state"] == "hosted_required"
            && store_matrix["gate_state"] == "release_blocker",
        "clean-store lifecycle platform matrix is incomplete or non-blocking",
    )
}

/// Validate the selected single concrete lifecycle owner and preservation policy.
fn validate_host_lifecycle_owner(policy: &Value) -> Result<(), Box<dyn Error>> {
    validate_host_safety_states(policy)?;
    let lifecycle = &policy["host_lifecycle_ownership"];
    let owner = &lifecycle["owner"];
    let processkit_locked = CARGO_LOCK.split("[[package]]").any(|package| {
        package.lines().any(|line| line == "name = \"processkit\"")
            && package.lines().any(|line| line == "version = \"2.2.3\"")
    });
    require(
        lifecycle["task_id"] == "ARRI-2.27"
            && lifecycle["gate_state"] == "release_blocker"
            && owner["workspace_crate"] == "projectatlas-cli"
            && owner["planned_module"] == "host_lifecycle"
            && owner["status"] == "selected-not-implemented"
            && owner["new_crate"] == false
            && owner["trait_or_factory"] == false,
        "host lifecycle owner is duplicated, abstracted, stale, or prematurely complete",
    )?;
    require(
        lifecycle["rust_mechanism"]["variation"] == "closed"
            && string_set(&lifecycle["rust_mechanism"]["journal_states"])?
                == BTreeSet::from([
                    "applied".to_string(),
                    "compensated".to_string(),
                    "planned".to_string(),
                    "verified".to_string(),
                ])
            && string_set(&lifecycle["rust_mechanism"]["lifecycle_actions"])?
                == BTreeSet::from([
                    "install".to_string(),
                    "reinstall".to_string(),
                    "remove".to_string(),
                    "repair".to_string(),
                    "rollback".to_string(),
                    "update".to_string(),
                ]),
        "lifecycle closed-state contract is incomplete",
    )?;
    require(
        string_set(&lifecycle["preservation_contract"]["never_managed"])?
            == BTreeSet::from([
                "authored purposes".to_string(),
                "project settings".to_string(),
                "project telemetry".to_string(),
                "project-local database".to_string(),
            ])
            && lifecycle["preservation_contract"]["project_data_deletion_requires_separate_confirmation"]
                == true
            && lifecycle["dependency_candidates"][0]["name"] == "processkit"
            && lifecycle["dependency_candidates"][0]["version"] == "2.2.3"
            && lifecycle["dependency_candidates"][0]["features"] == json!(["process-control"])
            && lifecycle["dependency_candidates"][0]["selection_state"]
                == "not_selected_for_host_lifecycle"
            && lifecycle["dependency_candidates"][0]["development_use"]
                == "selected_for_evidence_process_supervision_and_language_registry_generation"
            && lifecycle["dependency_candidates"][0]["allowed_claim"] == "development-tooling-only"
            && lifecycle["dependency_candidates"][0]["evidence_state"] == "proven_local"
            && processkit_locked,
        "lifecycle preservation or candidate-selection policy drifted",
    )
}

/// Validate owned safety, transitive native boundaries, advisories, and containment truth.
fn validate_safety_inventory(
    policy: &Value,
    current: &CurrentSafetyEvidence,
) -> Result<(), Box<dyn Error>> {
    validate_host_safety_states(policy)?;
    validate_host_source_artifacts(policy)?;
    let inventory = &policy["unsafe_native_ffi_inventory"];
    let owned = &inventory["projectatlas_owned"];
    let recorded_crates = string_set(&owned["crates"])?;
    require(
        inventory["task_id"] == "ARRI-2.28"
            && inventory["gate_state"] == "release_blocker"
            && owned["crate_count"] == current.owned_crates.len()
            && recorded_crates == current.owned_crates
            && owned["unsafe_policy"] == "forbid"
            && owned["unsafe_policy_unconditional"] == true
            && owned["all_crates_inherit_workspace_lints"] == true
            && owned["unsafe_blocks"] == current.unsafe_constructs
            && owned["extern_blocks"] == current.extern_blocks
            && owned["link_attributes"] == current.link_attributes
            && owned["gate_state"] == "pass",
        "ProjectAtlas-owned unsafe-forbid evidence is incomplete or failing",
    )?;
    require(
        inventory["dependency_graph"]["lockfile_packages"] == current.lockfile_packages
            && inventory["dependency_graph"]["workspace_packages"] == current.workspace_packages
            && inventory["dependency_graph"]["external_packages"] == current.external_packages
            && inventory["dependency_graph"]["audited_external_custom_build_packages"]
                == current.custom_build_packages
            && inventory["dependency_graph"]["derivation_command"]
                == json!([
                    "cargo",
                    "metadata",
                    "--offline",
                    "--locked",
                    "--format-version",
                    "1"
                ])
            && inventory["dependency_graph"]["custom_build_rule"]
                == "count unique external metadata packages with at least one target kind equal to custom-build"
            && inventory["dependency_graph"]["boundary_inventory_scope"]
                == "preliminary-known-boundaries-and-build-script-risk-signal-not-complete-transitive-unsafe-or-ffi-proof"
            && string_set(
                &inventory["dependency_graph"]["complete_transitive_boundary_proof_tasks"],
            )? == BTreeSet::from([
                "ARRI-11.18".to_string(),
                "ARRI-11.23".to_string(),
                "ARRI-11.28".to_string(),
            ]),
        "dependency/native boundary counts drifted without review",
    )?;
    let native_rows = inventory["native_boundaries"]
        .as_array()
        .ok_or_else(|| io::Error::other("native boundaries are not an array"))?;
    let native_rows = rows_by_key(native_rows, "id")?;
    let native_ids = native_rows.keys().cloned().collect::<BTreeSet<_>>();
    require(
        native_ids
            == BTreeSet::from([
                "blake3-optimized-code".to_string(),
                "bundled-sqlite".to_string(),
                "processkit-development-process-supervision".to_string(),
                "tree-sitter-runtime-and-grammars".to_string(),
                "windows-runtime-imports".to_string(),
            ]),
        format!("native boundary inventory drifted: {native_ids:?}"),
    )?;
    let processkit = &native_rows["processkit-development-process-supervision"];
    require(
        processkit["path"]
            == "projectatlas-cli dev-dependency and projectatlas-lints development-tool dependency -> processkit -> windows-sys/libc"
            && processkit["safe_wrapper"] == "processkit 2.2.3 with process-control only"
            && processkit["runtime_location"]
                == "development-only evidence runners and language-registry generator"
            && processkit["production_runtime_dependency"] == false
            && processkit["windows_observation"]
                == "Job Object process-tree supervision proven by focused local tests"
            && processkit["linux_macos_observation"] == "hosted_required"
            && processkit["evidence_state"] == "proven_local"
            && processkit["gate_state"] == "pending",
        "development process-supervision boundary is hidden or overclaimed",
    )?;
    require(
        inventory["containment"]["current_projectatlas_owned_process_containment"] == false
            && inventory["containment"]["strength"] == "none-production-runtime"
            && inventory["containment"]["calibration_evidence_containment"]
                == "windows-job-object-proven-local"
            && inventory["containment"]["processkit_selection"] == "development-tooling-only"
            && inventory["containment"]["processkit_production_selection"] == "not_selected"
            && inventory["containment"]["cross_platform_evidence"] == "hosted_required"
            && inventory["containment"]["gate_state"] == "release_blocker",
        "absent process containment is hidden or marked passing",
    )?;
    let advisories = inventory["advisories"]
        .as_array()
        .ok_or_else(|| io::Error::other("advisories are not an array"))?;
    require(
        advisories.len() == 1,
        "advisory evidence is missing or duplicated",
    )?;
    let advisory = &advisories[0];
    require(
        advisory["id"] == "RUSTSEC-2026-0204"
            && advisory["candidate_version"] == "0.9.20"
            && advisory["candidate_lockfile_sha256"] == sha256(CARGO_LOCK.as_bytes())
            && advisory["candidate_audit"] == "pass-local-dirty"
            && advisory["candidate_deny"] == "pass-local-dirty"
            && advisory["evidence_state"] == "proven_local"
            && advisory["gate_state"] == "release_blocker",
        "advisory remediation evidence is stale, failed, or prematurely released",
    )?;
    let audit_tool_gaps = inventory["audit_tool_gaps"]
        .as_array()
        .ok_or_else(|| io::Error::other("audit tool gaps are not an array"))?;
    require(
        audit_tool_gaps
            .iter()
            .map(|row| row["tool"].as_str().unwrap_or_default())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["cargo-geiger", "cargo-vet"])
            && audit_tool_gaps.iter().all(|row| {
                row["availability"] == "absent"
                    && row["evidence_state"] == "not_available"
                    && row["gate_state"] == "pending"
            }),
        "unavailable complete transitive-boundary tooling was hidden",
    )?;
    let blockers = policy["release_blockers"]
        .as_array()
        .ok_or_else(|| io::Error::other("release blockers are not an array"))?;
    require(
        blockers
            .iter()
            .map(|row| row["id"].as_str().unwrap_or_default().to_string())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "dependency-advisory-commit-evidence".to_string(),
                "host-native-provisioning".to_string(),
                "native-worker-containment".to_string(),
                "typed-lifecycle-owner-implementation".to_string(),
            ])
            && blockers
                .iter()
                .all(|row| row["gate_state"] == "release_blocker"),
        "required host/safety release blocker inventory drifted",
    )
}

/// Count unsafe syntax, foreign blocks, and native-link attributes in parsed Rust source.
#[derive(Default)]
struct OwnedBoundaryVisitor {
    unsafe_constructs: usize,
    extern_blocks: usize,
    link_attributes: usize,
}

impl<'ast> syn::visit::Visit<'ast> for OwnedBoundaryVisitor {
    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        self.link_attributes += usize::from(node.path().is_ident("link"));
        syn::visit::visit_attribute(self, node);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.unsafe_constructs += 1;
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.unsafe_constructs += usize::from(node.sig.unsafety.is_some());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.unsafe_constructs += usize::from(node.sig.unsafety.is_some());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.unsafe_constructs += usize::from(node.sig.unsafety.is_some());
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.unsafe_constructs += usize::from(node.unsafety.is_some());
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.unsafe_constructs += usize::from(node.unsafety.is_some());
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        self.unsafe_constructs += usize::from(node.unsafety.is_some());
        self.extern_blocks += 1;
        syn::visit::visit_item_foreign_mod(self, node);
    }
}

/// Reconcile owned manifests and Rust sources with the current workspace.
fn current_safety_evidence() -> Result<CurrentSafetyEvidence, Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace: toml::Value = toml::from_str(std::str::from_utf8(WORKSPACE_MANIFEST)?)?;
    require(
        workspace["workspace"]["lints"]["rust"]["unsafe_code"].as_str() == Some("forbid"),
        "workspace no longer unconditionally forbids unsafe code",
    )?;
    let members = workspace["workspace"]["members"]
        .as_array()
        .ok_or_else(|| io::Error::other("workspace members are not an array"))?;
    let processkit = &workspace["workspace"]["dependencies"]["processkit"];
    let processkit_features = processkit["features"]
        .as_array()
        .ok_or_else(|| io::Error::other("workspace processkit features are not an array"))?;
    require(
        processkit["version"].as_str() == Some("2.2.3")
            && processkit["default-features"].as_bool() == Some(false)
            && processkit_features.len() == 1
            && processkit_features[0].as_str() == Some("process-control"),
        "workspace processkit version or feature ownership drifted",
    )?;
    let mut owned_crates = BTreeSet::new();
    let mut boundaries = OwnedBoundaryVisitor::default();

    for member in members {
        let relative_path = member
            .as_str()
            .ok_or_else(|| io::Error::other("workspace member is not a string"))?;
        let member_root = workspace_root.join(relative_path);
        let manifest_path = member_root.join("Cargo.toml");
        let manifest: toml::Value = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
        require(
            manifest["lints"]["workspace"].as_bool() == Some(true),
            format!(
                "{} does not inherit workspace lints",
                manifest_path.display()
            ),
        )?;
        let crate_name = manifest["package"]["name"]
            .as_str()
            .ok_or_else(|| io::Error::other("workspace package name is absent"))?;
        require(
            owned_crates.insert(crate_name.to_string()),
            format!("workspace package name {crate_name} is duplicated"),
        )?;
        for source_path in rust_source_paths(&member_root)? {
            let source = fs::read_to_string(&source_path)?;
            boundaries.visit_file(&syn::parse_file(&source).map_err(|error| {
                io::Error::other(format!(
                    "failed to parse {}: {error}",
                    source_path.display()
                ))
            })?);
        }
    }

    let metadata = current_locked_cargo_metadata()?;
    validate_processkit_dependency_scope(&metadata)?;
    let (lockfile_packages, workspace_packages, external_packages, custom_build_packages) =
        current_dependency_counts(&metadata)?;
    require(
        workspace_packages == owned_crates.len(),
        format!(
            "cargo metadata reports {workspace_packages} workspace packages but manifests define {}",
            owned_crates.len()
        ),
    )?;
    Ok(CurrentSafetyEvidence {
        owned_crates,
        unsafe_constructs: boundaries.unsafe_constructs,
        extern_blocks: boundaries.extern_blocks,
        link_attributes: boundaries.link_attributes,
        lockfile_packages,
        workspace_packages,
        external_packages,
        custom_build_packages,
    })
}

/// Return every owned Rust source while rejecting symlink-based scan gaps.
fn rust_source_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut directories = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = directories.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_symlink() {
                return Err(io::Error::other(format!(
                    "owned source scan refuses symlink {}",
                    path.display()
                ))
                .into());
            }
            if file_type.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    Ok(sources)
}

/// Require process supervision to remain outside the packaged CLI dependency graph.
fn validate_processkit_dependency_scope(metadata: &Value) -> Result<(), Box<dyn Error>> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| io::Error::other("cargo metadata packages are not an array"))?;
    let workspace_members = string_set(&metadata["workspace_members"])?;
    let mut cli_seen = false;
    let mut lints_seen = false;

    for package in packages {
        let package_id = package["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("cargo metadata package id is not a string"))?;
        if !workspace_members.contains(package_id) {
            continue;
        }
        let package_name = package["name"]
            .as_str()
            .ok_or_else(|| io::Error::other("cargo metadata package name is not a string"))?;
        let dependencies = package["dependencies"]
            .as_array()
            .ok_or_else(|| io::Error::other("cargo metadata dependencies are not an array"))?;
        let mut processkit = dependencies
            .iter()
            .filter(|dependency| dependency["name"] == "processkit");
        let dependency = processkit.next();
        let expected_kind = match package_name {
            "projectatlas-cli" => {
                cli_seen = true;
                Some("dev")
            }
            "projectatlas-lints" => {
                lints_seen = true;
                None
            }
            _ => {
                require(
                    dependency.is_none(),
                    format!("unexpected processkit dependency in workspace package {package_name}"),
                )?;
                continue;
            }
        };
        let dependency = dependency.ok_or_else(|| {
            io::Error::other(format!(
                "workspace package {package_name} does not declare processkit"
            ))
        })?;
        require(
            processkit.next().is_none(),
            format!("workspace package {package_name} must declare processkit exactly once"),
        )?;
        let features = dependency["features"]
            .as_array()
            .ok_or_else(|| io::Error::other("processkit dependency features are not an array"))?;
        require(
            dependency["kind"].as_str() == expected_kind
                && dependency["req"].as_str() == Some("^2.2.3")
                && dependency["optional"].as_bool() == Some(false)
                && dependency["uses_default_features"].as_bool() == Some(false)
                && features.len() == 1
                && features[0].as_str() == Some("process-control"),
            format!(
                "workspace package {package_name} has an invalid processkit scope or feature set"
            ),
        )?;
    }
    require(
        cli_seen && lints_seen,
        "processkit dependency owners are missing from Cargo metadata",
    )
}

/// Derive dependency and custom-build counts from the current locked Cargo graph.
fn current_dependency_counts(
    metadata: &Value,
) -> Result<(usize, usize, usize, usize), Box<dyn Error>> {
    let lock: toml::Value = toml::from_str(CARGO_LOCK)?;
    let lockfile_packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("Cargo.lock has no package array"))?
        .len();
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| io::Error::other("cargo metadata packages are not an array"))?;
    require(
        packages.len() == lockfile_packages,
        format!(
            "Cargo.lock has {lockfile_packages} packages but cargo metadata has {}",
            packages.len()
        ),
    )?;
    let workspace_members = string_set(&metadata["workspace_members"])?;
    let mut external_packages = 0_usize;
    let mut custom_build_packages = 0_usize;
    for package in packages {
        let package_id = package["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("cargo metadata package id is not a string"))?;
        if workspace_members.contains(package_id) {
            continue;
        }
        external_packages += 1;
        let targets = package["targets"]
            .as_array()
            .ok_or_else(|| io::Error::other("cargo metadata targets are not an array"))?;
        if targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "custom-build"))
        }) {
            custom_build_packages += 1;
        }
    }
    Ok((
        lockfile_packages,
        workspace_members.len(),
        external_packages,
        custom_build_packages,
    ))
}

/// Run locked offline Cargo metadata with bounded execution and output capture.
fn current_locked_cargo_metadata() -> Result<Value, Box<dyn Error>> {
    const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new(cargo)
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .current_dir(workspace_root)
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= METADATA_TIMEOUT {
            child.kill()?;
            let _terminated_status = child.wait()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "cargo metadata exceeded its 30-second contract-test timeout",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    stdout.seek(SeekFrom::Start(0))?;
    stderr.seek(SeekFrom::Start(0))?;
    let mut stdout_text = String::new();
    let mut stderr_text = String::new();
    stdout.read_to_string(&mut stdout_text)?;
    stderr.read_to_string(&mut stderr_text)?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "cargo metadata failed with {status}: {stderr_text}"
        ))
        .into());
    }
    Ok(serde_json::from_str(&stdout_text)?)
}

/// Render bytes as a lowercase SHA-256 digest.
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Hash one UTF-8 source artifact after canonicalizing host line endings.
fn canonical_source_digest(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| io::Error::other(format!("source artifact is not UTF-8: {error}")))?;
    Ok(sha256(text.replace("\r\n", "\n").as_bytes()))
}

/// Normalize host line endings before hashing replay output.
fn normalize_replay_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Replay every frozen CLI path through Clap's real help parser and hash the observations.
fn cli_help_replay_digest(rows: &[Value]) -> Result<String, Box<dyn Error>> {
    let empty_digest = sha256(&[]);
    let mut observations = String::new();
    for row in rows {
        let path = row["path"]
            .as_str()
            .ok_or_else(|| io::Error::other("CLI inventory path is not a string"))?;
        let mut argv = path.split_whitespace().collect::<Vec<_>>();
        argv.push("--help");
        let error = Cli::command()
            .try_get_matches_from(argv)
            .err()
            .ok_or_else(|| io::Error::other("--help replay reached command execution"))?;
        require(
            format!("{:?}", error.kind()) == "DisplayHelp",
            format!("{path} --help returned {:?}", error.kind()),
        )?;
        require(
            error.exit_code() == 0,
            format!("{path} --help returned exit {}", error.exit_code()),
        )?;
        let stdout = normalize_replay_text(&error.to_string());
        writeln!(
            observations,
            "{path}\0{}\0{}\0{empty_digest}\0{}\00",
            error.exit_code(),
            sha256(stdout.as_bytes()),
            stdout.len(),
        )?;
    }
    Ok(sha256(observations.as_bytes()))
}

/// Replay the generic parser failures captured by the host-bound evidence.
fn validate_cli_error_replay(evidence: &Value) -> Result<(), Box<dyn Error>> {
    for case in value_at(evidence, "/cli/generic_error_replay/cases")?
        .as_array()
        .ok_or_else(|| io::Error::other("CLI error cases are not an array"))?
    {
        let argv = case["argv"]
            .as_array()
            .ok_or_else(|| io::Error::other("CLI error argv is not an array"))?
            .iter()
            .map(|arg| {
                arg.as_str()
                    .ok_or_else(|| io::Error::other("CLI error argv item is not a string"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let error = Cli::command()
            .try_get_matches_from(argv)
            .err()
            .ok_or_else(|| io::Error::other("frozen invalid argv parsed successfully"))?;
        let stderr = normalize_replay_text(&error.to_string());
        let stderr_digest = sha256(stderr.as_bytes());
        require(
            format!("{:?}", error.kind()) == case["error_kind"],
            format!("CLI error kind drifted for {}", case["id"]),
        )?;
        require(
            error.exit_code() == case["exit_code"],
            format!("CLI error exit drifted for {}", case["id"]),
        )?;
        require(
            stderr_digest == case["compiled_stderr_sha256"],
            format!(
                "CLI error output drifted for {}: expected {}, observed {stderr_digest}",
                case["id"], case["compiled_stderr_sha256"]
            ),
        )?;
        require(
            stderr.len() == case["compiled_stderr_bytes"],
            format!(
                "CLI error byte count drifted for {}: expected {}, observed {}",
                case["id"],
                case["compiled_stderr_bytes"],
                stderr.len()
            ),
        )?;
        require(
            case["stdout_sha256"] == sha256(&[]) && case["stdout_bytes"] == 0,
            format!("CLI error stdout contract is not empty for {}", case["id"]),
        )?;
    }
    Ok(())
}

/// Build a structured inventory from the compiled Clap command tree.
fn compiled_cli_tree() -> Result<Vec<Value>, Box<dyn Error>> {
    let mut rows = Vec::new();
    collect_cli_command(&Cli::command(), "projectatlas", &mut rows)?;
    rows.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    Ok(rows)
}

/// Append one command and every nested subcommand to the compiled inventory.
fn collect_cli_command(
    command: &ClapCommand,
    path: &str,
    rows: &mut Vec<Value>,
) -> Result<(), Box<dyn Error>> {
    let mut help = Vec::new();
    command.clone().write_long_help(&mut help)?;
    let arguments = command
        .get_arguments()
        .map(|argument| {
            let possible_values = argument
                .get_value_parser()
                .possible_values()
                .map_or_else(Vec::<String>::new, |values| {
                    values.map(|value| value.get_name().to_string()).collect()
                });
            json!({
                "id": argument.get_id().as_str(),
                "short": argument.get_short().map(|value| value.to_string()),
                "long": argument.get_long(),
                "index": argument.get_index(),
                "required": argument.is_required_set(),
                "action": format!("{:?}", argument.get_action()),
                "num_args": argument.get_num_args().map(|value| value.to_string()),
                "defaults": argument.get_default_values().iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>(),
                "possible_values": possible_values,
            })
        })
        .collect::<Vec<_>>();
    rows.push(json!({
        "path": path,
        "aliases": command.get_all_aliases().collect::<Vec<_>>(),
        "subcommand_required": command.is_subcommand_required_set(),
        "arg_required_else_help": command.is_arg_required_else_help_set(),
        "arguments": arguments,
        "long_help_sha256": sha256(&help),
    }));
    for child in command.get_subcommands() {
        let child_path = format!("{path} {}", child.get_name());
        collect_cli_command(child, &child_path, rows)?;
    }
    Ok(())
}

/// Build a structured inventory from RMCP's generated `tools/list` schema.
async fn compiled_mcp_contract() -> Result<(String, Vec<Value>), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let server = ProjectAtlasMcpServer::new(
        temp.path().join("projectatlas.db"),
        None,
        "repository-intelligence-contract".to_string(),
        false,
    );
    let (server_transport, client_transport) = tokio::io::duplex(65_536);
    let server_handle = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .map_err(|error| error.to_string())?
            .waiting()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    });
    let client = ContractClient.serve(client_transport).await?;
    let protocol_version = client
        .peer_info()
        .ok_or_else(|| io::Error::other("MCP initialization returned no server information"))?
        .protocol_version
        .as_str()
        .to_string();
    let tools = client.peer().list_tools(None).await?;
    let mut rows = tools
        .tools
        .iter()
        .map(|tool| {
            let input_schema = serde_json::to_value(&tool.input_schema)?;
            let mut structural_schema = input_schema.clone();
            remove_schema_prose(&mut structural_schema);
            Ok(json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": structural_schema,
                "input_schema_sha256": sha256(&serde_json::to_vec(&input_schema)?),
            }))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    rows.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    client.cancel().await?;
    server_handle.await?.map_err(std::io::Error::other)?;
    Ok((protocol_version, rows))
}

/// Remove descriptive prose while retaining the complete executable JSON Schema shape.
fn remove_schema_prose(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("description");
            object.remove("$schema");
            for child in object.values_mut() {
                remove_schema_prose(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                remove_schema_prose(child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Validate the closed executable behavior-case inventory against the frozen surface.
fn validate_behavior_case_inventory(
    expected: &Value,
    policy: &Value,
    evidence: &Value,
    invocable_paths: &BTreeSet<String>,
    tool_names: &BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    let cases = behavior_cases()?;
    require(
        value_at(&cases, "/schema_version")? == &json!(1),
        "compatibility behavior cases use an unsupported schema version",
    )?;
    require(
        value_at(&cases, "/baseline/runtime_version")?
            == value_at(expected, "/baseline/runtime_version")?
            && value_at(&cases, "/baseline/surface_sha256")?
                == &Value::String(sha256(SURFACE.as_bytes())),
        "compatibility behavior cases are not bound to the frozen runtime surface",
    )?;
    require(
        value_at(
            &cases,
            "/contract_sources/cli_options_defaults_aliases_and_output_modes",
        )? == "surface.cli"
            && value_at(
                &cases,
                "/contract_sources/mcp_request_schemas_and_required_properties",
            )? == "surface.mcp",
        "behavior cases duplicated the authoritative option/default/schema inventories",
    )?;

    let cli_profiles = value_at(&cases, "/cli_profiles")?
        .as_object()
        .ok_or_else(|| io::Error::other("CLI behavior profiles are not an object"))?;
    for (name, profile) in cli_profiles {
        require_fields(
            profile,
            &json!(["exit_code", "stdout", "stderr", "bounded_stream"]),
        )?;
        require(
            profile["bounded_stream"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            format!("CLI profile {name} has no bounded stream contract"),
        )?;
    }
    let mcp_profiles = value_at(&cases, "/mcp_profiles")?
        .as_object()
        .ok_or_else(|| io::Error::other("MCP behavior profiles are not an object"))?;
    for (name, profile) in mcp_profiles {
        require_fields(
            profile,
            &json!(["transport", "content_type", "encoding", "error_root"]),
        )?;
        require(
            profile["transport"] == "tools_call_result"
                && profile["content_type"] == "text"
                && profile["encoding"] == "toon",
            format!("MCP profile {name} bypasses the frozen RMCP text/TOON contract"),
        )?;
    }

    let cli_rows = value_at(&cases, "/cli_cases")?
        .as_array()
        .ok_or_else(|| io::Error::other("CLI behavior cases are not an array"))?;
    require(
        unique_row_keys(cli_rows, "invocation", "CLI behavior cases")? == *invocable_paths,
        "CLI behavior cases do not cover exactly every invocable command",
    )?;
    for row in cli_rows {
        require_fields(
            row,
            &json!([
                "invocation",
                "profile",
                "arguments",
                "required_stdout",
                "root_selection",
                "side_effect"
            ]),
        )?;
        let invocation = row["invocation"]
            .as_str()
            .ok_or_else(|| io::Error::other("CLI invocation is not a string"))?;
        let profile = row["profile"]
            .as_str()
            .ok_or_else(|| io::Error::other("CLI profile is not a string"))?;
        require(
            cli_profiles.contains_key(profile),
            format!("CLI case {invocation} references unknown profile {profile}"),
        )?;
        let arguments = row["arguments"]
            .as_array()
            .ok_or_else(|| io::Error::other("CLI arguments are not an array"))?;
        let argument_tokens = arguments
            .iter()
            .map(|argument| {
                argument
                    .as_str()
                    .ok_or_else(|| io::Error::other("CLI argument is not a string"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let path_tokens = invocation
            .strip_prefix("projectatlas ")
            .ok_or_else(|| io::Error::other("CLI invocation lacks projectatlas prefix"))?
            .split_whitespace()
            .collect::<Vec<_>>();
        require(
            argument_tokens.starts_with(&path_tokens),
            format!("CLI case {invocation} arguments do not invoke its frozen path"),
        )?;
        let required_stdout = string_set(&row["required_stdout"])?;
        require(
            profile != "toon_success" || !required_stdout.is_empty(),
            format!("CLI TOON case {invocation} has no required response root or field"),
        )?;
        if profile == "diagnostic_success" {
            require(
                row.get("required_stderr")
                    .map(string_set)
                    .transpose()?
                    .is_some_and(|fields| !fields.is_empty()),
                format!("CLI diagnostic case {invocation} has no stderr contract"),
            )?;
        }
        require(
            row["root_selection"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
                && row["side_effect"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
            format!("CLI case {invocation} lacks root-selection or side-effect ownership"),
        )?;
    }
    let output_modes = value_at(&cases, "/output_mode_cases")?
        .as_array()
        .ok_or_else(|| io::Error::other("output-mode cases are not an array"))?;
    let required_output_modes = ["cli-json-runtime-info", "cli-token-tui"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    require(
        unique_row_keys(output_modes, "id", "output-mode cases")? == required_output_modes,
        "JSON or token-TUI output-mode replay is missing",
    )?;
    for row in output_modes {
        require_fields(row, &json!(["id", "arguments", "stdout", "bounded_stream"]))?;
        require(
            row["arguments"].is_array() && row["bounded_stream"] == "process_exit",
            "output-mode case lacks arguments or bounded completion",
        )?;
    }

    let mcp_rows = value_at(&cases, "/mcp_cases")?
        .as_array()
        .ok_or_else(|| io::Error::other("MCP behavior cases are not an array"))?;
    require(
        unique_row_keys(mcp_rows, "tool", "MCP behavior cases")? == *tool_names,
        "MCP behavior cases do not cover exactly every frozen tool",
    )?;
    for row in mcp_rows {
        require_fields(
            row,
            &json!([
                "tool",
                "profile",
                "arguments",
                "required_text",
                "root_selection",
                "side_effect"
            ]),
        )?;
        let tool = row["tool"]
            .as_str()
            .ok_or_else(|| io::Error::other("MCP tool is not a string"))?;
        let profile = row["profile"]
            .as_str()
            .ok_or_else(|| io::Error::other("MCP profile is not a string"))?;
        require(
            mcp_profiles.contains_key(profile),
            format!("MCP case {tool} references unknown profile {profile}"),
        )?;
        require(
            row["arguments"].is_object() && !string_set(&row["required_text"])?.is_empty(),
            format!("MCP case {tool} lacks arguments or required response roots/fields"),
        )?;
        require(
            row["root_selection"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
                && row["side_effect"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
            format!("MCP case {tool} lacks root-selection or side-effect ownership"),
        )?;
    }
    let defaulted_cli = string_set(value_at(
        &cases,
        "/default_replay/cli_cases_using_omitted_defaults",
    )?)?;
    let defaulted_mcp = string_set(value_at(
        &cases,
        "/default_replay/mcp_tools_using_omitted_defaults",
    )?)?;
    require(
        value_at(&cases, "/default_replay/authoritative_source")? == "surface"
            && !defaulted_cli.is_empty()
            && defaulted_cli.is_subset(invocable_paths)
            && !defaulted_mcp.is_empty()
            && defaulted_mcp.is_subset(tool_names),
        "default replay is missing or detached from the authoritative surface",
    )?;

    let expected_mcp = value_at(expected, "/mcp")?
        .as_array()
        .ok_or_else(|| io::Error::other("frozen MCP inventory is not an array"))?;
    let compact_mcp = serde_json::to_vec(expected_mcp)?;
    let description_bytes = expected_mcp.iter().try_fold(0_usize, |total, row| {
        row["description"]
            .as_str()
            .map(|description| total + description.len())
            .ok_or_else(|| io::Error::other("MCP description is not a string"))
    })?;
    let schema_bytes = expected_mcp.iter().try_fold(0_usize, |total, row| {
        serde_json::to_vec(&row["input_schema"])
            .map(|schema| total + schema.len())
            .map_err(io::Error::other)
    })?;
    let tools_list = value_at(&cases, "/tools_list_baseline")?;
    require(
        tools_list["canonical_compact_utf8_bytes"] == json!(compact_mcp.len())
            && tools_list["canonical_compact_sha256"] == sha256(&compact_mcp)
            && tools_list["description_utf8_bytes"] == json!(description_bytes)
            && tools_list["input_schema_utf8_bytes"] == json!(schema_bytes)
            && tools_list["estimated_tokens_lower_bound"] == json!(compact_mcp.len().div_ceil(4))
            && tools_list["estimated_tokens_upper_bound"] == json!(compact_mcp.len().div_ceil(3)),
        format!(
            "measured MCP tools/list baseline drifted: bytes={}, sha256={}, descriptions={}, schemas={}, token_bounds={}..={}",
            compact_mcp.len(),
            sha256(&compact_mcp),
            description_bytes,
            schema_bytes,
            compact_mcp.len().div_ceil(4),
            compact_mcp.len().div_ceil(3),
        ),
    )?;
    require(
        value_at(
            evidence,
            "/mcp/inventory_and_schema_replay/canonical_compact_utf8_bytes",
        )? == &tools_list["canonical_compact_utf8_bytes"]
            && value_at(
                evidence,
                "/mcp/inventory_and_schema_replay/canonical_compact_sha256",
            )? == &tools_list["canonical_compact_sha256"]
            && value_at(
                evidence,
                "/mcp/inventory_and_schema_replay/description_utf8_bytes",
            )? == &tools_list["description_utf8_bytes"]
            && value_at(
                evidence,
                "/mcp/inventory_and_schema_replay/input_schema_utf8_bytes",
            )? == &tools_list["input_schema_utf8_bytes"]
            && value_at(
                evidence,
                "/mcp/inventory_and_schema_replay/estimated_tokens_lower_bound",
            )? == &tools_list["estimated_tokens_lower_bound"]
            && value_at(
                evidence,
                "/mcp/inventory_and_schema_replay/estimated_tokens_upper_bound",
            )? == &tools_list["estimated_tokens_upper_bound"],
        "compatibility evidence does not expose the measured MCP tools/list baseline",
    )?;

    let cli_failures = value_at(&cases, "/failure_cases/cli")?
        .as_array()
        .ok_or_else(|| io::Error::other("CLI failure cases are not an array"))?;
    let mcp_failures = value_at(&cases, "/failure_cases/mcp")?
        .as_array()
        .ok_or_else(|| io::Error::other("MCP failure cases are not an array"))?;
    require(
        cli_failures.len() >= 2
            && mcp_failures.len() >= 2
            && unique_row_keys(cli_failures, "id", "CLI failure cases")?.len()
                == cli_failures.len()
            && unique_row_keys(mcp_failures, "id", "MCP failure cases")?.len()
                == mcp_failures.len(),
        "focused command/tool failure coverage is missing",
    )?;
    for row in mcp_failures {
        let profile = row["profile"]
            .as_str()
            .ok_or_else(|| io::Error::other("MCP failure profile is not a string"))?;
        require(
            mcp_profiles
                .get(profile)
                .is_some_and(|profile| profile["error_root"] == "error"),
            "MCP failure case does not require the error root",
        )?;
    }
    let cli_unknown_option = value_at(&cases, "/failure_coverage/cli_unknown_option")?
        .as_str()
        .ok_or_else(|| io::Error::other("CLI exhaustive failure option is not a string"))?;
    let mcp_missing_project_path = value_at(&cases, "/failure_coverage/mcp_missing_project_path")?
        .as_str()
        .ok_or_else(|| io::Error::other("MCP missing-project placeholder is not a string"))?;
    let not_found_tools = string_set(value_at(&cases, "/failure_coverage/mcp_not_found_tools")?)?;
    let no_domain_failure_tools = string_set(value_at(
        &cases,
        "/failure_coverage/mcp_no_domain_failure_tools",
    )?)?;
    let runtime_independent_tools = mcp_rows
        .iter()
        .filter(|row| row["root_selection"] == "runtime_independent")
        .map(|row| {
            row["tool"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("MCP failure-coverage tool is not a string"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        cli_unknown_option.starts_with("--")
            && mcp_missing_project_path == "{{missing_project_root}}"
            && not_found_tools
                == ["atlas_task_cancel", "atlas_task_status"]
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect()
            && no_domain_failure_tools
                == ["atlas_runtime_info"]
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect()
            && runtime_independent_tools
                == not_found_tools
                    .union(&no_domain_failure_tools)
                    .cloned()
                    .collect(),
        "exhaustive CLI/MCP failure replay policy is incomplete",
    )?;

    let root_cases = value_at(&cases, "/root_selection_cases")?
        .as_array()
        .ok_or_else(|| io::Error::other("root-selection cases are not an array"))?;
    let required_root_cases = [
        "cli-explicit-db-and-config-must-agree",
        "mcp-per-call-project-requires-local-index",
        "mcp-session-project-mutation",
        "persistent-root-binding",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect::<BTreeSet<_>>();
    require(
        unique_row_keys(root_cases, "id", "root-selection cases")? == required_root_cases,
        "root-selection replay does not cover explicit, per-call, session, and persistent modes",
    )?;
    require(
        value_at(&cases, "/task_behavior/status_tool")? == "atlas_task_status"
            && value_at(&cases, "/task_behavior/cancel_tool")? == "atlas_task_cancel"
            && value_at(&cases, "/task_behavior/contract_task_id")? == "task-progress-contract"
            && value_at(&cases, "/task_behavior/contract_cancel_result")? == "already_finished"
            && value_at(&cases, "/task_behavior/unknown_task_status")? == "not_found"
            && value_at(&cases, "/task_behavior/unknown_task_cancel")? == "not_found",
        "bounded MCP task status/cancel behavior is not frozen",
    )?;

    let cli_workflow = string_set(value_at(&cases, "/normal_workflows/cli")?)?;
    let mcp_workflow = string_set(value_at(&cases, "/normal_workflows/mcp")?)?;
    require(
        cli_workflow.len() == 7
            && cli_workflow.is_subset(invocable_paths)
            && mcp_workflow.len() == 7
            && mcp_workflow.is_subset(tool_names)
            && value_at(&cases, "/normal_workflows/automatic_graph_enrichment")?
                == value_at(policy, "/compatibility/mcp/automatic_graph_enrichment")?
            && value_at(&cases, "/normal_workflows/required_extra_graph_calls")?
                == value_at(policy, "/compatibility/mcp/required_extra_graph_calls")?
            && value_at(&cases, "/normal_workflows/required_extra_graph_calls")? == &json!(0),
        "normal atlas-first workflow or automatic graph-enrichment contract drifted",
    )?;
    require(
        value_at(&cases, "/hosted_replay/platforms")?
            == value_at(evidence, "/cross_platform_replay/platforms")?
            && value_at(&cases, "/hosted_replay/state")? == "required",
        "hosted compatibility replay platform coverage drifted",
    )?;
    Ok(())
}

/// ARRI-2.4: preserve the compiled baseline and bound additions.
#[tokio::test]
async fn compatibility_contract_preserves_compiled_baseline() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    let expected = surface()?;
    let evidence = compatibility_evidence()?;
    require_pending_evidence_state(&policy, "ARRI-2.4")?;
    require(
        value_at(&evidence, "/overall_state")? == "partial"
            && value_at(&evidence, "/task_complete")? == false,
        "ARRI-2.4 evidence must remain partial while runtime replay gaps are open",
    )?;
    let surface_digest = sha256(SURFACE.as_bytes());
    require(
        value_at(&evidence, "/binding/surface_artifact/sha256")?.as_str()
            == Some(surface_digest.as_str()),
        "compatibility evidence is not bound to the frozen surface artifact",
    )?;
    let behavior_digest = sha256(BEHAVIOR_CASES.as_bytes());
    require(
        value_at(&evidence, "/binding/behavior_case_artifact/sha256")?.as_str()
            == Some(behavior_digest.as_str()),
        "compatibility evidence is not bound to the executable behavior cases",
    )?;
    require(
        value_at(&evidence, "/binding/planning_revision")?
            == value_at(&expected, "/baseline/planning_commit")?
            && value_at(&evidence, "/binding/planning_tree")?
                == value_at(&expected, "/baseline/source_tree")?
            && value_at(&evidence, "/binding/runtime_source_revision")?
                == value_at(&expected, "/baseline/runtime_source_commit")?,
        "compatibility evidence revision binding drifted",
    )?;
    let expected_cli = value_at(&expected, "/cli")?
        .as_array()
        .ok_or_else(|| io::Error::other("CLI inventory is not an array"))?;
    let compiled_cli = compiled_cli_tree()?;
    let allowed_cli = string_set(value_at(
        &policy,
        "/simplicity/allowed_new_analysis_cli_commands",
    )?)?;
    require_frozen_rows(
        expected_cli,
        &compiled_cli,
        "path",
        &allowed_cli,
        Some("projectatlas"),
    )?;
    require(
        value_at(&evidence, "/cli/inventory/rows")? == expected_cli.len()
            && value_at(&evidence, "/cli/inventory/state")? == "proven_local",
        "CLI evidence count or state drifted",
    )?;
    let help_replay_digest = cli_help_replay_digest(expected_cli)?;
    require(
        value_at(
            &evidence,
            "/cli/parser_help_replay/compiled_parser_observation_digest",
        )?
        .as_str()
            == Some(help_replay_digest.as_str()),
        format!(
            "CLI parser/help runtime replay drifted: expected {}, observed {help_replay_digest}",
            value_at(
                &evidence,
                "/cli/parser_help_replay/compiled_parser_observation_digest",
            )?
        ),
    )?;
    validate_cli_error_replay(&evidence)?;

    let expected_mcp = value_at(&expected, "/mcp")?
        .as_array()
        .ok_or_else(|| io::Error::other("MCP inventory is not an array"))?;
    let (protocol_version, compiled_mcp) = compiled_mcp_contract().await?;
    let allowed_mcp = string_set(value_at(&policy, "/simplicity/allowed_new_analysis_tools")?)?;
    require_frozen_rows(expected_mcp, &compiled_mcp, "name", &allowed_mcp, None)?;
    require(
        value_at(&policy, "/compatibility/mcp/protocol_version")?
            == &Value::String(protocol_version),
        "negotiated MCP protocol version drifted from the contract",
    )?;

    let cli_rows = &compiled_cli;
    let all_paths = cli_rows
        .iter()
        .map(|row| {
            row["path"]
                .as_str()
                .ok_or_else(|| io::Error::other("CLI inventory path is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut invocable_paths = BTreeSet::new();
    for row in cli_rows {
        let path = row["path"]
            .as_str()
            .ok_or_else(|| io::Error::other("CLI inventory path is not a string"))?;
        let prefix = format!("{path} ");
        let has_direct_child = all_paths.iter().any(|candidate| {
            candidate
                .strip_prefix(&prefix)
                .is_some_and(|remainder| !remainder.contains(' '))
        });
        let subcommand_required = row["subcommand_required"]
            .as_bool()
            .ok_or_else(|| io::Error::other("CLI subcommand_required is not a bool"))?;
        if !has_direct_child || !subcommand_required {
            invocable_paths.insert(path.to_string());
        }
    }
    require(
        invocable_paths
            == object_keys(value_at(
                &policy,
                "/compatibility/cli/response_family_by_invocation",
            )?)?,
        "CLI response-shape inventory does not cover every invocable command",
    )?;

    let tool_names = compiled_mcp
        .iter()
        .map(|row| {
            row["name"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("MCP tool name is not a string"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        tool_names
            == object_keys(value_at(
                &policy,
                "/compatibility/mcp/success_root_by_tool",
            )?)?,
        "MCP success-shape inventory does not cover every tool",
    )?;
    require(
        tool_names == object_keys(value_at(&policy, "/compatibility/mcp/defaults_by_tool")?)?,
        "MCP default inventory does not cover every tool",
    )?;
    validate_behavior_case_inventory(&expected, &policy, &evidence, &invocable_paths, &tool_names)?;
    require(
        value_at(&evidence, "/mcp/inventory_and_schema_replay/tools")? == tool_names.len()
            && value_at(&evidence, "/mcp/inventory_and_schema_replay/state")? == "proven_local"
            && value_at(
                &evidence,
                "/mcp/inventory_and_schema_replay/protocol_version",
            )? == value_at(&policy, "/compatibility/mcp/protocol_version")?,
        "MCP runtime schema evidence drifted",
    )?;
    let mut runtime = serde_json::to_value(super::build_runtime_info())?;
    let runtime_object = runtime
        .as_object_mut()
        .ok_or_else(|| io::Error::other("runtime identity is not an object"))?;
    let runtime_tools = string_set(
        runtime_object
            .get("mcp_tools")
            .ok_or_else(|| io::Error::other("runtime identity lacks mcp_tools"))?,
    )?;
    runtime_object.remove("mcp_tools");
    runtime_object.remove("executable");
    require(
        runtime_tools == tool_names,
        "runtime identity MCP tool list drifted from tools/list",
    )?;
    require(
        runtime
            == *value_at(
                &evidence,
                "/cli/runtime_identity_replay/expected_identity_without_executable_or_tool_list",
            )?,
        "runtime identity replay drifted",
    )?;
    require(
        value_at(&policy, "/compatibility/mcp/required_extra_graph_calls")? == &json!(0),
        "normal MCP workflows gained a required graph call",
    )?;
    require(
        value_at(&evidence, "/cli/command_behavior_replay/state")? == "proven_local"
            && value_at(&evidence, "/cli/command_behavior_replay/invocable_paths")?
                == &json!(invocable_paths.len())
            && value_at(
                &evidence,
                "/cli/command_behavior_replay/runtime_paths_proven",
            )? == &json!(invocable_paths.len())
            && value_at(&evidence, "/mcp/tool_behavior_replay/state")? == "proven_local"
            && value_at(&evidence, "/mcp/tool_behavior_replay/tools")? == &json!(tool_names.len())
            && value_at(&evidence, "/mcp/tool_behavior_replay/runtime_tools_proven")?
                == &json!(tool_names.len())
            && value_at(
                &evidence,
                "/mcp/tool_behavior_replay/normal_workflow_required_extra_graph_calls",
            )? == &json!(0)
            && value_at(&evidence, "/last_local_run/state")? == "passed"
            && value_at(&evidence, "/last_local_run/commit_bound")? == false
            && value_at(&evidence, "/last_local_run/behavior_e2e/cli_success_cases")?
                == &json!(invocable_paths.len())
            && value_at(&evidence, "/last_local_run/behavior_e2e/mcp_success_cases")?
                == &json!(tool_names.len())
            && value_at(&evidence, "/cross_platform_replay/state")? == "hosted_required",
        "local CLI/MCP behavior replay evidence or hosted gap state drifted",
    )?;
    Ok(())
}

/// Validate the evaluator's closed process and materialized-source boundary.
fn validate_evaluator_containment(manifest: &Value) -> Result<(), Box<dyn Error>> {
    let containment = value_at(manifest, "/reproduction/containment_eligibility")?;
    let containment_fields = json!([
        "owner",
        "leader_and_descendants_supervised",
        "deadline_tears_down_complete_tree",
        "parent_death_hardening_requested",
        "output_overflow_status",
        "timeout_status",
        "repository_custom_filter_dependency_status",
        "source_workspace_concurrency_assumption",
        "assumption_violation_status",
        "coordinated_parent_swap_residual",
        "materialized_entry_read_limit_bytes",
        "materialized_checkout_read_limit_bytes",
        "platform_mechanisms",
        "process_group_limitation"
    ]);
    require_fields(containment, &containment_fields)?;
    let platform_fields = json!(["windows", "linux", "macos_and_bsd"]);
    require_fields(&containment["platform_mechanisms"], &platform_fields)?;
    require(
        object_keys(containment)? == string_set(&containment_fields)?
            && object_keys(&containment["platform_mechanisms"])? == string_set(&platform_fields)?
            && containment["owner"] == "processkit-private-process-tree"
            && containment["leader_and_descendants_supervised"] == true
            && containment["deadline_tears_down_complete_tree"] == true
            && containment["parent_death_hardening_requested"] == true
            && containment["output_overflow_status"] == "ineligible"
            && containment["timeout_status"] == "ineligible"
            && containment["repository_custom_filter_dependency_status"] == "ineligible"
            && containment["source_workspace_concurrency_assumption"]
                == "isolated-quiescent-no-concurrent-same-user-filesystem-namespace-mutation"
            && containment["assumption_violation_status"] == "ineligible"
            && containment["coordinated_parent_swap_residual"]
                == "outside-eligible-run-threat-model"
            && containment["materialized_entry_read_limit_bytes"] == 67_108_864
            && containment["materialized_checkout_read_limit_bytes"] == 268_435_456
            && containment["platform_mechanisms"]["windows"] == "job-object"
            && containment["platform_mechanisms"]["linux"] == "cgroup-v2-or-process-group"
            && containment["platform_mechanisms"]["macos_and_bsd"] == "process-group"
            && containment["process_group_limitation"]
                == "trusted-descendants-can-escape-with-setsid",
        "evaluator containment contract is incomplete or overstates its source/process boundary",
    )
}

/// ARRI-2.5: validate the single authoritative reproduction manifest.
#[test]
fn evaluation_manifest_defines_reproduction_fields() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    let manifest = evaluation_manifest()?;
    require_pending_evidence_state(&policy, "ARRI-2.5")?;
    let manifest_reference = value_at(&policy, "/evaluation_manifest")?;
    require_fields(
        &manifest,
        &json!([
            "schema_version",
            "format",
            "manifest_id",
            "projectatlas",
            "reproduction",
            "corpora",
            "profiles",
            "toolchains",
            "environments",
            "experiment_design",
            "calibration",
            "operations",
            "result_schema",
            "measurements",
            "claim_status"
        ]),
    )?;
    require(
        manifest_reference["authoritative_artifact"]
            == "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json"
            && manifest_reference["format"] == manifest["format"]
            && manifest_reference["schema_version"] == manifest["schema_version"]
            && manifest_reference["embedded_copy_allowed"] == false
            && manifest_reference.get("schema").is_none()
            && manifest_reference.get("initial").is_none(),
        "repository-intelligence contract duplicates or drifts from the authoritative evaluation manifest",
    )?;
    let reproduction = &manifest["reproduction"];
    let source = &reproduction["source_eligibility"];
    let runner = &reproduction["calibration_runner"];
    require(
        policy["format"] == "projectatlas.repository-intelligence-contracts"
            && policy["contract_id"] == "projectatlas-v0.4-repository-intelligence"
            && manifest["manifest_id"] == "projectatlas-v0.4-repository-intelligence-evaluation"
            && reproduction["schema_authority"] == "this-document"
            && source["clean_committed_source_required"] == true
            && source["dirty_run_status"] == "exploratory-ineligible"
            && source["baseline_runtime_cargo_lock_must_match_baseline_digest"] == true
            && source["evaluator_cargo_lock_must_match_evaluator_digest"] == true
            && source["runner_source_must_match_compiled_bytes"] == true,
        "official benchmark source eligibility is not fail-closed",
    )?;
    require(
        manifest["profiles"][0]["config_identity"] == "projectatlas-default-v0.3.26"
            && manifest["profiles"][1]["config_identity"] == "fts-candidate-v1"
            && manifest["profiles"][2]["config_identity"] == "native-parser-pack-v1"
            && manifest["profiles"][3]["config_identity"] == "wasm-parser-pack-v1"
            && manifest["profiles"][4]["config_identity"] == "semantic-pack-v1",
        "evaluation profile identities use development-stage names",
    )?;
    let declared_toolchain = &manifest["toolchains"][0];
    require(
        declared_toolchain["id"] == "windows-msvc-reference"
            && declared_toolchain["declared_rustc"] == "1.93.1 (01f6ddf75 2026-02-11)"
            && declared_toolchain["declared_cargo"] == "1.93.1 (083ac5135 2025-12-15)"
            && declared_toolchain["declared_llvm"] == "21.1.8"
            && declared_toolchain["declared_host"] == "x86_64-pc-windows-msvc"
            && declared_toolchain["observation_state"]
                == "preregistered-not-observed-by-calibration-runner"
            && declared_toolchain["claim_eligible"] == false
            && declared_toolchain.get("rustc").is_none()
            && declared_toolchain.get("cargo").is_none()
            && declared_toolchain.get("llvm").is_none()
            && declared_toolchain.get("locked").is_none(),
        "declared toolchain was presented as observed calibration evidence",
    )?;
    require(
        string_set(&source["required_run_bindings"])?
            == BTreeSet::from([
                "cargo_lock_sha256".to_string(),
                "command_sha256".to_string(),
                "environment".to_string(),
                "executable_sha256".to_string(),
                "git".to_string(),
                "head_commit".to_string(),
                "manifest_sha256".to_string(),
                "observed_environment".to_string(),
                "runner_source_sha256".to_string(),
                "worktree_state_sha256".to_string(),
            ])
            && manifest["projectatlas"]["cargo_lock_sha256"] == sha256(CARGO_LOCK.as_bytes()),
        "run identity does not bind source, manifest, executable, command, environment, and Cargo.lock",
    )?;
    require(
        runner["source_path"] == "crates/projectatlas-cli/src/calibration_evidence_runner.rs"
            && runner["example_name"] == "calibration-evidence-runner"
            && runner["build_command"]
                == json!([
                    "cargo",
                    "build",
                    "--release",
                    "--locked",
                    "-p",
                    "projectatlas-cli",
                    "--example",
                    "calibration-evidence-runner"
                ])
            && runner["eligible_build_profile"] == "release"
            && runner["debug_assertions_must_be_disabled"] == true
            && runner["build_command_role"] == "reproduction-instruction-not-execution-proof"
            && runner["command_surface"] == json!(["run", "execute", "workload"])
            && runner["public_command"] == "run"
            && runner["internal_commands"] == json!(["execute", "workload"])
            && runner["process_supervision"] == "processkit-private-process-tree"
            && runner["workload_supervision"] == "processkit-private-process-tree"
            && runner["workload_timeout_source"]
                == "calibration.eligible_workloads[].timeout_seconds"
            && runner["tree_timeout_seconds"] == 5_400
            && runner["per_stream_capture_limit_bytes"] == 8_388_608
            && runner["warmups"] == 3
            && runner["repetitions"] == 15
            && runner["environment_evidence"] == "name-presence-sha256-only"
            && runner["raw_environment_transport"] == "process-environment-only"
            && runner["workload_result_transport"]
                == "single-use-no-clobber-file-consumed-and-removed"
            && runner["no_nonces_or_handoffs"] == true
            && runner["forced_environment"] == json!({"RUST_BACKTRACE": "0"})
            && reproduction.get("external_launcher").is_none()
            && reproduction.get("controller_handoff").is_none()
            && reproduction.get("calibration_controller_command").is_none()
            && reproduction.get("calibration_inner_command").is_none()
            && reproduction.get("cargo_targets").is_none(),
        "single calibration runner contract drifted or retained obsolete launch layers",
    )?;
    require(
        string_set(&runner["allowed_environment_names"])?
            == BTreeSet::from([
                "CARGO_HOME".to_string(),
                "COMSPEC".to_string(),
                "DEVELOPER_DIR".to_string(),
                "HOME".to_string(),
                "INCLUDE".to_string(),
                "LIB".to_string(),
                "LIBPATH".to_string(),
                "MACOSX_DEPLOYMENT_TARGET".to_string(),
                "PATH".to_string(),
                "PATHEXT".to_string(),
                "PROCESSOR_ARCHITECTURE".to_string(),
                "Platform".to_string(),
                "RUSTUP_HOME".to_string(),
                "RUSTUP_TOOLCHAIN".to_string(),
                "SDKROOT".to_string(),
                "SYSTEMROOT".to_string(),
                "TEMP".to_string(),
                "TMP".to_string(),
                "UCRTVersion".to_string(),
                "UniversalCRTSdkDir".to_string(),
                "VCINSTALLDIR".to_string(),
                "VCToolsInstallDir".to_string(),
                "WINDIR".to_string(),
                "WindowsSDKVersion".to_string(),
                "WindowsSdkBinPath".to_string(),
                "WindowsSdkDir".to_string(),
            ]),
        "controlled environment allowlist drifted from the dedicated runner",
    )?;
    validate_evaluator_containment(&manifest)?;
    let evidence = &reproduction["evidence_schema"];
    let artifact_kinds = string_set(&evidence["artifact_kinds"])?;
    require(
        evidence["schema_version"] == 1
            && evidence["artifact_kind_field"] == "artifact_kind"
            && evidence["claim_status"] == "not-evaluated"
            && artifact_kinds
                == BTreeSet::from([
                    "projectatlas.calibration.completion".to_string(),
                    "projectatlas.calibration.failure".to_string(),
                    "projectatlas.calibration.pilot".to_string(),
                    "projectatlas.calibration.process".to_string(),
                    "projectatlas.calibration.start".to_string(),
                ])
            && object_keys(&evidence["required_fields_by_artifact_kind"])? == artifact_kinds
            && string_set(&evidence["lifecycle_stages"])?
                == BTreeSet::from([
                    "aggregate".to_string(),
                    "completion".to_string(),
                    "execution".to_string(),
                    "provenance".to_string(),
                    "reservation".to_string(),
                    "verification".to_string(),
                ])
            && evidence["plaintext_environment_values_forbidden"] == true
            && evidence["retained_stream_bytes_out_of_line"] == true
            && string_set(&evidence["nested_binding_schemas"]["source"])?
                == BTreeSet::from([
                    "cargo_lock_sha256".to_string(),
                    "dirty".to_string(),
                    "git".to_string(),
                    "head_commit".to_string(),
                    "runner_source_sha256".to_string(),
                    "worktree_state_sha256".to_string(),
                ])
            && string_set(&evidence["nested_binding_schemas"]["git"])?
                == BTreeSet::from([
                    "path".to_string(),
                    "sha256".to_string(),
                    "version".to_string(),
                ])
            && string_set(&evidence["nested_binding_schemas"]["environment_entry"])?
                == BTreeSet::from([
                    "name".to_string(),
                    "present".to_string(),
                    "value_sha256".to_string(),
                ])
            && string_set(&evidence["nested_binding_schemas"]["invocation"])?
                == BTreeSet::from([
                    "arguments".to_string(),
                    "build_profile".to_string(),
                    "command_sha256".to_string(),
                    "environment".to_string(),
                    "executable".to_string(),
                    "executable_role".to_string(),
                    "executable_sha256".to_string(),
                ])
            && string_set(&evidence["nested_binding_schemas"]["observed_environment"])?
                == BTreeSet::from([
                    "controlled_environment_sha256".to_string(),
                    "executable_sha256".to_string(),
                    "observed_architecture".to_string(),
                    "observed_os_family".to_string(),
                    "reference_environment_id".to_string(),
                ])
            && string_set(&evidence["nested_binding_schemas"]["process_stream"])?
                == BTreeSet::from([
                    "bytes".to_string(),
                    "file".to_string(),
                    "sha256".to_string(),
                ]),
        "closed typed evidence schema drifted",
    )?;
    for (kind, fields) in evidence["required_fields_by_artifact_kind"]
        .as_object()
        .ok_or_else(|| io::Error::other("evidence record schemas are missing"))?
    {
        let fields = string_set(fields)?;
        require(
            fields.contains("schema_version")
                && fields.contains("artifact_kind")
                && fields.contains("claim_status"),
            format!("evidence record `{kind}` lacks its common schema bindings"),
        )?;
    }
    require(
        string_set(
            &evidence["required_fields_by_artifact_kind"]["projectatlas.calibration.process"],
        )?
        .is_superset(&BTreeSet::from([
            "duration_ns".to_string(),
            "output_truncated".to_string(),
            "stderr".to_string(),
            "stdout".to_string(),
            "timed_out".to_string(),
        ])) && string_set(
            &evidence["required_fields_by_artifact_kind"]["projectatlas.calibration.completion"],
        )?
        .is_superset(&BTreeSet::from([
            "artifact_sha256".to_string(),
            "process_sha256".to_string(),
            "raw_attempts_sha256".to_string(),
            "sample_count".to_string(),
        ])) && evidence["required_fields_by_artifact_kind"]["projectatlas.calibration.process"]
            .as_array()
            .is_some_and(|fields| fields.iter().all(|field| field != "tree_terminated"))
            && string_set(
                &evidence["required_fields_by_artifact_kind"]["projectatlas.calibration.start"],
            )?
            .contains("observed_environment")
            && string_set(
                &evidence["required_fields_by_artifact_kind"]["projectatlas.calibration.pilot"],
            )?
            .contains("observed_environment"),
        "process, completion, or observed-environment bindings drifted",
    )?;
    let artifacts = &reproduction["artifact_paths"];
    require(
        artifacts["root"] == ".projectatlas/research/v04-results"
            && artifacts["calibration_positions"]["before"]["pilot"]
                == ".projectatlas/research/v04-results/calibration-before.json"
            && artifacts["calibration_positions"]["before"]["raw_attempts"]
                == ".projectatlas/research/v04-results/calibration-before-samples.jsonl"
            && artifacts["calibration_positions"]["after"]["pilot"]
                == ".projectatlas/research/v04-results/calibration-after.json"
            && artifacts["calibration_positions"]["after"]["raw_attempts"]
                == ".projectatlas/research/v04-results/calibration-after-samples.jsonl"
            && artifacts["journal_directory_rule"] == "aggregate-path-with-run-extension"
            && artifacts["local_ignored_evidence"] == true
            && artifacts["no_clobber_required"] == true
            && artifacts["one_journal_per_position"] == true
            && artifacts["one_completion_marker_per_position"] == true
            && string_set(&artifacts["success_required_absent"])?
                == BTreeSet::from(["failure.json".to_string()])
            && string_set(&artifacts["success_required_present"])?
                == BTreeSet::from([
                    "aggregate".to_string(),
                    "completion.json".to_string(),
                    "process.json".to_string(),
                    "process.stderr".to_string(),
                    "process.stdout".to_string(),
                    "raw-attempts".to_string(),
                    "start.json".to_string(),
                ]),
        "single-journal before/after artifact contract drifted",
    )?;
    let host = &reproduction["reference_host_eligibility"];
    let reference_environment = manifest["environments"]
        .as_array()
        .and_then(|rows| rows.first())
        .ok_or_else(|| io::Error::other("reference environment is missing"))?;
    require(
        host["environment_id"] == reference_environment["id"]
            && host["environment_evidence_must_match"] == true
            && host["calibration_positions"] == json!(["before", "after"])
            && host["both_calibrations_required"] == true
            && host["executable_digest_must_match"] == true
            && reference_environment["claim_eligible"] == false
            && reference_environment["os_family"] == "windows"
            && reference_environment["architecture"] == "x86_64"
            && string_set(&reference_environment["identity_dimensions"])?
                == BTreeSet::from([
                    "controlled-environment-sha256".to_string(),
                    "executable-sha256".to_string(),
                    "observed-architecture".to_string(),
                    "observed-os-family".to_string(),
                    "reference-environment-id".to_string(),
                ])
            && string_set(&host["observed_identity_fields"])?
                == BTreeSet::from([
                    "controlled_environment_sha256".to_string(),
                    "executable_sha256".to_string(),
                    "observed_architecture".to_string(),
                    "observed_os_family".to_string(),
                    "reference_environment_id".to_string(),
                ])
            && string_set(&host["unmeasured_host_dimensions_not_claimed"])?
                == BTreeSet::from([
                    "cpu-model".to_string(),
                    "exact-os-build".to_string(),
                    "exact-toolchain-version".to_string(),
                    "firmware-identity".to_string(),
                    "logical-core-count".to_string(),
                    "memory-size".to_string(),
                    "physical-core-count".to_string(),
                    "power-source-or-scheme".to_string(),
                    "storage-volume-identity".to_string(),
                ]),
        "reference-host before/after executable or environment policy is incomplete",
    )?;
    Ok(())
}

/// Keep evaluator source eligibility bounded and honest.
#[test]
fn evaluator_source_boundary_is_explicit() -> Result<(), Box<dyn Error>> {
    let manifest = evaluation_manifest()?;
    validate_evaluator_containment(&manifest)?;

    let mut filter_eligible = manifest.clone();
    filter_eligible["reproduction"]["containment_eligibility"]["repository_custom_filter_dependency_status"] =
        json!("eligible");
    let mut concurrent_workspace = manifest.clone();
    concurrent_workspace["reproduction"]["containment_eligibility"]["source_workspace_concurrency_assumption"] =
        json!("concurrent-writers-allowed");
    let mut hidden_residual = manifest.clone();
    hidden_residual["reproduction"]["containment_eligibility"]["coordinated_parent_swap_residual"] =
        json!("fully-contained");
    let mut unbounded_entry = manifest.clone();
    unbounded_entry["reproduction"]["containment_eligibility"]["materialized_entry_read_limit_bytes"] =
        json!(0);
    let mut unknown_field = manifest;
    unknown_field["reproduction"]["containment_eligibility"]["implicit_trust"] = json!(true);

    require(
        validate_evaluator_containment(&filter_eligible).is_err()
            && validate_evaluator_containment(&concurrent_workspace).is_err()
            && validate_evaluator_containment(&hidden_residual).is_err()
            && validate_evaluator_containment(&unbounded_entry).is_err()
            && validate_evaluator_containment(&unknown_field).is_err(),
        "evaluator containment accepted a hidden filter, concurrency, residual, limit, or schema drift",
    )
}

/// ARRI-2.29: every required review is resolved without widening Phase 0 claims.
#[test]
fn arri_2_29_phase_review_and_evaluator_boundaries_are_closed() -> Result<(), Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    validate_phase_review_record(PHASE_REVIEW_RECORD, &workspace_root)?;
    validate_evaluator_containment(&evaluation_manifest()?)?;

    let source: Value = serde_json::from_str(PHASE_REVIEW_RECORD)?;
    let validate = |value: &Value| -> Result<(), Box<dyn Error>> {
        validate_phase_review_record(&serde_json::to_string(value)?, &workspace_root)
    };

    let mut duplicate_role = source.clone();
    duplicate_role["reviewers"][1]["role"] = duplicate_role["reviewers"][0]["role"].clone();
    require(
        validate(&duplicate_role).is_err(),
        "duplicate Phase 0 reviewer role was accepted",
    )?;

    let mut duplicate_finding = source.clone();
    duplicate_finding["findings"][1]["finding_id"] =
        duplicate_finding["findings"][0]["finding_id"].clone();
    require(
        validate(&duplicate_finding).is_err(),
        "duplicate Phase 0 finding ID was accepted",
    )?;

    let mut missing_evidence_owner = source.clone();
    let security_review = missing_evidence_owner["reviewers"]
        .as_array_mut()
        .and_then(|reviews| {
            reviews
                .iter_mut()
                .find(|review| review["role"] == "security")
        })
        .ok_or_else(|| io::Error::other("security review is missing"))?;
    security_review["finding_ids"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("security finding IDs are not an array"))?
        .retain(|finding| finding != "checked-task-evidence-coverage-gap");
    require(
        validate(&missing_evidence_owner).is_err(),
        "checked-task evidence finding lost security ownership",
    )?;

    let mut substituted_evidence = source.clone();
    let evidence_finding = substituted_evidence["findings"]
        .as_array_mut()
        .and_then(|findings| {
            findings
                .iter_mut()
                .find(|finding| finding["finding_id"] == "checked-task-evidence-coverage-gap")
        })
        .ok_or_else(|| io::Error::other("checked-task evidence finding is missing"))?;
    evidence_finding["evidence_paths"][0] = json!(".github/scripts/release-notes.py");
    require(
        validate(&substituted_evidence).is_err(),
        "checked-task evidence finding accepted unrelated evidence",
    )?;

    let mut open_disposition = source.clone();
    open_disposition["findings"][0]["disposition"] = json!("open");
    require(
        validate(&open_disposition).is_err(),
        "open Phase 0 finding disposition was accepted",
    )?;

    let mut missing_evidence = source.clone();
    missing_evidence["reviewers"][0]["evidence_paths"][0] =
        json!("docs/benchmarks/missing-phase-review-evidence.json");
    require(
        validate(&missing_evidence).is_err(),
        "missing Phase 0 review evidence was accepted",
    )?;

    let mut escaping_evidence = source.clone();
    escaping_evidence["findings"][0]["evidence_paths"][0] = json!("../Cargo.toml");
    require(
        validate(&escaping_evidence).is_err(),
        "repository-escaping Phase 0 evidence was accepted",
    )?;

    let mut private_root_evidence = source.clone();
    private_root_evidence["reviewers"][0]["evidence_paths"][0] = json!("Cargo.toml");
    require(
        validate(&private_root_evidence).is_err(),
        "review evidence outside accepted public roots was accepted",
    )?;

    let mut unknown_task = source.clone();
    unknown_task["findings"][2]["deferred_to_tasks"] = json!(["ARRI-99.99"]);
    require(
        validate(&unknown_task).is_err(),
        "unknown Phase 0 deferral owner was accepted",
    )?;

    let mut unrelated_task = source.clone();
    unrelated_task["findings"][2]["deferred_to_tasks"] = json!(["ARRI-11.4"]);
    require(
        validate(&unrelated_task).is_err(),
        "unrelated existing Phase 0 deferral owner was accepted",
    )?;

    let mut weakened_severity = source.clone();
    weakened_severity["findings"][0]["blocking"] = json!(false);
    require(
        validate(&weakened_severity).is_err(),
        "weakened Phase 0 finding severity was accepted",
    )?;

    let mut changed_disposition = source.clone();
    changed_disposition["findings"][0]["disposition"] = json!("narrowed-and-deferred");
    changed_disposition["findings"][0]["deferred_to_tasks"] = json!(["ARRI-11.4"]);
    require(
        validate(&changed_disposition).is_err(),
        "changed Phase 0 finding disposition was accepted",
    )?;

    let mut unresolved = source.clone();
    unresolved["unresolved_blocking_finding_ids"] = json!(["evaluator-boundary-gaps"]);
    require(
        validate(&unresolved).is_err(),
        "unresolved Phase 0 blocker was accepted",
    )?;

    let mut release_claim = source.clone();
    release_claim["claim_state"] = json!("release-ready");
    require(
        validate(&release_claim).is_err(),
        "Phase 0 release claim was accepted",
    )?;

    let mut unknown_field = source;
    unknown_field["product_or_release_claim"] = json!("superior");
    require(
        validate(&unknown_field).is_err(),
        "unknown product or release claim was accepted",
    )
}

/// ARRI-2.6: reject fabricated measurements before baseline capture.
#[test]
fn baseline_contract_rejects_unmeasured_claims() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    let manifest = evaluation_manifest()?;
    require_pending_evidence_state(&policy, "ARRI-2.6")?;
    require(
        manifest["measurements"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "authoritative manifest unexpectedly claims benchmark measurements",
    )?;
    require(
        manifest["claim_status"] == "not-measured"
            && manifest["projectatlas"]["future_candidate_commit"].is_null()
            && manifest["calibration"]["eligible_workloads"]
                .as_array()
                .is_some_and(|rows| rows.iter().all(|row| row["baseline_median_ns"].is_null())),
        "unmeasured baseline state contains a candidate, calibration result, or claim",
    )?;
    Ok(())
}

/// ARRI-2.7: pin deterministic graph-comparison rules.
#[test]
fn graph_snapshot_contract_lists_determinism_rules() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.7")?;
    let graph = value_at(&policy, "/graph_snapshot")?;
    let exclusions = string_set(value_at(graph, "/exclude_fields")?)?;
    require(
        exclusions
            == BTreeSet::from([
                "active_epoch".to_string(),
                "active_slot".to_string(),
                "created_at".to_string(),
                "duration_ns".to_string(),
                "finished_at".to_string(),
                "last_changed_epoch".to_string(),
                "row_id".to_string(),
                "session_id".to_string(),
                "slot_id".to_string(),
                "started_at".to_string(),
                "task_id".to_string(),
                "telemetry".to_string(),
                "updated_at".to_string(),
            ]),
        "graph exclusion inventory drifted",
    )?;
    require(
        !exclusions.contains("stable_key"),
        "stable graph identity was excluded",
    )?;
    require(
        string_set(value_at(graph, "/comparisons")?)?
            == BTreeSet::from([
                "full_vs_incremental".to_string(),
                "repeated_run".to_string(),
                "supported_platform".to_string(),
                "worker_1_vs_n".to_string(),
            ]),
        "canonical graph comparison matrix drifted",
    )?;
    require(
        value_at(graph, "/first_difference")? == &json!(true),
        "first diff is disabled",
    )?;
    require(
        value_at(graph, "/stable_identity_required")? == &json!(true),
        "stable identity is not required",
    )?;
    Ok(())
}

/// ARRI-2.7: execute canonical full/incremental comparison.
#[test]
fn graph_snapshot_contract_canonicalizes_golden_records() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.7")?;
    let fixture: Value = serde_json::from_str(CANONICAL_GRAPH_FIXTURE)?;
    let full = canonical_graph_records(&policy, &fixture["full"])?;
    let incremental = canonical_graph_records(&policy, &fixture["incremental"])?;
    require(
        full == incremental,
        "excluded storage metadata, ordering, paths, or line endings change canonical output",
    )?;
    let changed = canonical_graph_records(&policy, &fixture["semantic_change"])?;
    require(
        full != changed,
        "semantic relation change vanished during canonicalization",
    )?;
    let first = full
        .iter()
        .zip(&changed)
        .find(|(left, right)| left != right)
        .ok_or_else(|| io::Error::other("semantic fixture has no first difference"))?;
    require(
        first.0["record_kind"] == "relation"
            && first.0["stable_key"] == "edge-01"
            && first.0["relation_kind"] == "calls"
            && first.1["relation_kind"] == "references",
        "canonical first semantic difference is not actionable",
    )
}

/// ARRI-2.7: own the complete canonical snapshot definition with one stable test ID.
#[test]
fn graph_snapshot_contract_is_mechanical() -> Result<(), Box<dyn Error>> {
    graph_snapshot_contract_lists_determinism_rules()?;
    graph_snapshot_contract_canonicalizes_golden_records()
}

/// ARRI-2.8: reconcile the typed default-core hard-budget contract.
#[test]
fn default_core_budget_contract_matches_typed_limits() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.8")?;
    validate_default_core_budget_contract(&policy)?;

    let configured_worker = DefaultCoreBudgets::default()
        .with_budget(DefaultCoreBudgetKind::WorkerCount, 8)?
        .get(DefaultCoreBudgetKind::WorkerCount);
    require(
        configured_worker.enforcement() == BudgetEnforcement::Advisory,
        "configured advisory budget claimed runtime enforcement",
    )?;

    let mut wrong_value = policy.clone();
    runtime_budget_row_mut(&mut wrong_value, "source_file_bytes")?["value"] = json!(1);
    require(
        validate_default_core_budget_contract(&wrong_value).is_err(),
        "default-core budget value drift was accepted",
    )?;

    let mut false_enforcement = policy.clone();
    runtime_budget_row_mut(&mut false_enforcement, "resolution_candidates")?["enforcement"] =
        json!(BudgetEnforcement::Advisory.contract_id());
    require(
        validate_default_core_budget_contract(&false_enforcement).is_err(),
        "runtime-enforced budget was accepted as advisory",
    )?;

    let mut duplicate = policy;
    let runtime_rows = duplicate
        .pointer_mut("/budgets/default_core/runtime_limits")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("runtime limits are not an array"))?;
    let duplicate_row = runtime_rows
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("runtime limit inventory is empty"))?;
    runtime_rows.push(duplicate_row);
    require(
        validate_default_core_budget_contract(&duplicate).is_err(),
        "duplicate default-core budget was accepted",
    )?;
    Ok(())
}

/// Validate the machine contract against the typed default-core source of truth.
fn validate_default_core_budget_contract(policy: &Value) -> Result<(), Box<dyn Error>> {
    let budgets = value_at(policy, "/budgets")?;
    let known_enforcement = string_set(value_at(budgets, "/enforcement_values")?)?;
    let runtime_rows = value_at(budgets, "/default_core/runtime_limits")?
        .as_array()
        .ok_or_else(|| io::Error::other("runtime limits are not an array"))?;
    let mut rows_by_id = BTreeMap::new();
    for row in runtime_rows {
        let id = row["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("budget id is not a string"))?;
        require(
            rows_by_id.insert(id, row).is_none(),
            format!("default-core budget {id} is duplicated"),
        )?;
    }

    let typed = DefaultCoreBudgets::default();
    require(
        rows_by_id.len() == typed.as_slice().len(),
        "default-core budget inventory does not match the typed owner",
    )?;
    let mut observed_enforcement = BTreeSet::new();
    for budget in typed.as_slice() {
        let id = budget.kind().contract_id();
        let row = rows_by_id
            .get(id)
            .ok_or_else(|| io::Error::other(format!("typed budget {id} is missing")))?;
        require(
            row["value"].as_u64() == Some(budget.value()),
            format!("{id} value drifted from the typed budget"),
        )?;
        require(
            row["unit"].as_str() == Some(budget.unit().contract_id()),
            format!("{id} unit drifted from the typed budget"),
        )?;
        require(
            row["policy"] == "hard_ceiling",
            format!("{id} is not a hard ceiling"),
        )?;
        let enforcement = row["enforcement"]
            .as_str()
            .ok_or_else(|| io::Error::other("budget enforcement is not a string"))?;
        require(
            known_enforcement.contains(enforcement),
            format!("{id} has unknown enforcement"),
        )?;
        require(
            enforcement == budget.enforcement().contract_id(),
            format!("{id} enforcement drifted from the typed budget"),
        )?;
        observed_enforcement.insert(enforcement);
    }
    require(
        observed_enforcement.contains(BudgetEnforcement::RuntimeEnforced.contract_id())
            && observed_enforcement.contains(BudgetEnforcement::Advisory.contract_id()),
        "default-core budgets do not expose enforced and advisory status",
    )?;

    let required_dimensions = [
        DefaultCoreBudgetKind::SourceFileBytes,
        DefaultCoreBudgetKind::AstDepth,
        DefaultCoreBudgetKind::SymbolsPerFile,
        DefaultCoreBudgetKind::RelationsPerFile,
        DefaultCoreBudgetKind::WorkerCount,
        DefaultCoreBudgetKind::StageTime,
        DefaultCoreBudgetKind::WorkingMemory,
        DefaultCoreBudgetKind::QueryDepth,
        DefaultCoreBudgetKind::VisitedNodes,
        DefaultCoreBudgetKind::ExpandedEdges,
        DefaultCoreBudgetKind::ReturnedRows,
        DefaultCoreBudgetKind::ResponseBytes,
        DefaultCoreBudgetKind::CancellationPoll,
        DefaultCoreBudgetKind::CancellationGrace,
    ];
    require(
        required_dimensions
            .iter()
            .all(|kind| rows_by_id.contains_key(kind.contract_id())),
        "a required default-core budget dimension is missing",
    )?;
    Ok(())
}

/// Return one mutable default-core runtime budget row by stable identity.
fn runtime_budget_row_mut<'a>(
    policy: &'a mut Value,
    id: &str,
) -> Result<&'a mut Value, Box<dyn Error>> {
    policy
        .pointer_mut("/budgets/default_core/runtime_limits")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("runtime limits are not an array"))?
        .iter_mut()
        .find(|row| row["id"].as_str() == Some(id))
        .ok_or_else(|| io::Error::other(format!("runtime budget {id} is missing")).into())
}

/// ARRI-2.9: optional packs have independent hard ceilings.
#[test]
fn optional_pack_budget_contract_is_separate_and_unmeasured() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.9")?;
    let pack_contract = value_at(&policy, "/budgets/optional_pack_contract")?;
    require(
        pack_contract["disabled_by_default"] == true
            && pack_contract["may_spend_default_core_allowance"] == false,
        "optional packs can weaken or spend default-core allowances",
    )?;
    let required_fields = string_set(&pack_contract["required_limit_fields"])?;
    let rows = pack_contract["accepted_pack_budgets"]
        .as_array()
        .ok_or_else(|| io::Error::other("pack budgets are not an array"))?;
    require(
        rows.len() == 2,
        "parser and semantic pack budgets are not separate",
    )?;
    let mut pack_ids = BTreeSet::new();
    for row in rows {
        let pack_id = row["pack_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("pack budget lacks pack_id"))?;
        require(
            pack_ids.insert(pack_id.to_string()),
            format!("duplicate pack budget {pack_id}"),
        )?;
        require(
            row["status"] == "preregistered_not_measured"
                && row["enforcement"] == "pack_manifest_required"
                && row["may_borrow_default_core_allowance"] == false
                && row["disabled_by_default"] == true
                && row["removable"] == true,
            format!("pack {pack_id} is accepted, coupled, or enabled before evidence"),
        )?;
        let limits = row["limits"]
            .as_object()
            .ok_or_else(|| io::Error::other("pack limits are not an object"))?;
        require(
            limits.keys().cloned().collect::<BTreeSet<_>>() == required_fields,
            format!("pack {pack_id} hard-limit fields drifted"),
        )?;
        for (id, value) in limits {
            require(
                value.as_u64().is_some_and(|value| value > 0),
                format!("pack {pack_id} limit {id} is not positive"),
            )?;
        }
    }
    require(
        pack_ids
            == BTreeSet::from([
                "broad-language-pack".to_string(),
                "semantic-pack".to_string(),
            ]),
        "optional pack budget identities drifted",
    )
}

/// ARRI-2.16: track accepted and benchmark-pending decisions.
#[test]
fn architecture_decision_contract_tracks_pending_decisions() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.16")?;
    let decisions = value_at(&policy, "/architecture_decisions")?
        .as_array()
        .ok_or_else(|| io::Error::other("architecture decisions are not an array"))?;
    let mut topics = BTreeSet::new();
    let mut provisional_topics = BTreeSet::new();
    for decision in decisions {
        let status = decision["status"]
            .as_str()
            .ok_or_else(|| io::Error::other("architecture decision status is not a string"))?;
        require(
            matches!(status, "accepted" | "provisional"),
            "architecture decision has an unknown status",
        )?;
        for field in [
            "id",
            "topic",
            "decision",
            "alternative",
            "risk",
            "verification",
        ] {
            require(
                decision[field]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                format!("architecture decision field {field} is empty"),
            )?;
        }
        let topic = decision["topic"]
            .as_str()
            .ok_or_else(|| io::Error::other("architecture topic is not a string"))?
            .to_string();
        topics.insert(topic.clone());
        if status == "provisional" {
            require(
                decision["blocking_evidence"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty()),
                format!("provisional decision {topic} has no blocking evidence"),
            )?;
            provisional_topics.insert(topic);
        }
    }
    require(
        topics
            == BTreeSet::from([
                "benchmark_corpus_and_hardware".to_string(),
                "fts_acceleration".to_string(),
                "optional_semantic_pack".to_string(),
                "parser_host".to_string(),
                "publication".to_string(),
                "rollback_retention".to_string(),
                "semantic_provider_boundary".to_string(),
                "typed_graph_layout".to_string(),
            ]),
        "architecture decision topics drifted",
    )?;
    require(
        provisional_topics
            == BTreeSet::from([
                "benchmark_corpus_and_hardware".to_string(),
                "fts_acceleration".to_string(),
                "parser_host".to_string(),
            ]),
        "benchmark-dependent architecture decisions are not provisional",
    )?;
    Ok(())
}

/// ARRI-2.19: bound normal workflow and public-surface growth.
#[test]
fn simplicity_contract_bounds_surface_growth() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.19")?;
    let simplicity = value_at(&policy, "/simplicity")?;
    require(
        simplicity["preserve_existing_public_names"] == true,
        "public names are not frozen",
    )?;
    require(
        simplicity["max_new_analysis_tools"] == 3,
        "analysis tool budget is not three",
    )?;
    require(
        string_set(value_at(simplicity, "/allowed_new_analysis_tools")?)?
            == BTreeSet::from([
                "atlas_architecture".to_string(),
                "atlas_impact".to_string(),
                "atlas_trace".to_string(),
            ]),
        "allowed analysis tool set drifted",
    )?;
    require(
        string_set(value_at(simplicity, "/allowed_new_analysis_cli_commands")?)?
            == BTreeSet::from([
                "projectatlas architecture".to_string(),
                "projectatlas impact".to_string(),
                "projectatlas trace".to_string(),
            ]),
        "allowed analysis CLI command set drifted",
    )?;
    require(
        simplicity["custom_query_language"] == false,
        "custom query language was enabled",
    )?;
    require(
        simplicity["normal_workflow_new_required_calls"] == 0,
        "normal workflow gained calls",
    )?;
    require(
        simplicity["optional_packs_disabled_by_default"] == true,
        "packs default on",
    )?;
    require(
        simplicity["optional_packs_removable"] == true,
        "packs are not removable",
    )?;
    require(
        simplicity["program_new_crates_without_independent_consumer"] == 0,
        "a speculative crate entered the program",
    )?;
    require(
        simplicity["default_graph_enrichment_max_bytes"] == 512
            && simplicity["default_graph_enrichment_max_tokens"] == 128
            && simplicity["frozen_workflow_growth_max_bytes"] == 1536
            && simplicity["frozen_workflow_growth_max_tokens"] == 384,
        "agent-facing response growth budgets drifted",
    )?;
    require(
        string_set(value_at(simplicity, "/phase_exit_review")?)?
            == BTreeSet::from([
                "consolidate".to_string(),
                "defer".to_string(),
                "delete".to_string(),
                "keep_optional".to_string(),
            ]),
        "phase-exit simplicity dispositions drifted",
    )?;
    require(
        value_at(simplicity, "/new_crate_rule")?
            .as_str()
            .is_some_and(|rule| !rule.trim().is_empty()),
        "new-crate ownership rule is absent",
    )?;
    Ok(())
}

/// Validate the closed KISS/DRY/ownership review schema used at each phase exit.
fn validate_phase_exit_ownership_review(policy: &Value) -> Result<(), Box<dyn Error>> {
    let review = value_at(policy, "/simplicity/phase_exit_ownership_review")?;
    require(
        review["required_at_every_phase_exit"] == true,
        "phase-exit ownership review is not mandatory",
    )?;
    require(
        string_set(value_at(review, "/required_scope_ids")?)?
            == BTreeSet::from([
                "abstractions".to_string(),
                "code".to_string(),
                "configuration".to_string(),
                "dependencies".to_string(),
                "dynamic_dispatch".to_string(),
                "outputs".to_string(),
                "traits".to_string(),
            ]),
        "phase-exit ownership review scope drifted",
    )?;
    let dispositions = string_set(value_at(policy, "/simplicity/phase_exit_review")?)?;
    require(
        review["dispositions_pointer"] == "/simplicity/phase_exit_review"
            && dispositions
                == BTreeSet::from([
                    "consolidate".to_string(),
                    "defer".to_string(),
                    "delete".to_string(),
                    "keep_optional".to_string(),
                ]),
        "phase-exit ownership dispositions drifted",
    )?;
    require(
        string_set(value_at(review, "/phase_record_required_fields")?)?
            == BTreeSet::from([
                "blocking_finding_count".to_string(),
                "findings".to_string(),
                "phase_id".to_string(),
                "reviewed_commit".to_string(),
                "reviewer".to_string(),
                "scope_results".to_string(),
                "state".to_string(),
            ])
            && string_set(value_at(review, "/scope_record_required_fields")?)?
                == BTreeSet::from([
                    "finding_ids".to_string(),
                    "reviewed".to_string(),
                    "scope_id".to_string(),
                ])
            && string_set(value_at(review, "/finding_required_fields")?)?
                == BTreeSet::from([
                    "blocking".to_string(),
                    "completion_evidence".to_string(),
                    "disposition".to_string(),
                    "finding_id".to_string(),
                    "owner".to_string(),
                    "rationale".to_string(),
                    "scope_id".to_string(),
                    "subject".to_string(),
                ]),
        "phase-exit review record schema drifted",
    )?;

    let completion = value_at(review, "/disposition_completion_requirements")?;
    require(
        object_keys(completion)? == dispositions
            && string_set(value_at(completion, "/delete")?)?
                == BTreeSet::from(["absence_evidence".to_string()])
            && string_set(value_at(completion, "/consolidate")?)?
                == BTreeSet::from([
                    "canonical_owner".to_string(),
                    "replacement_evidence".to_string(),
                ])
            && string_set(value_at(completion, "/defer")?)?
                == BTreeSet::from([
                    "owner".to_string(),
                    "tracking_reference".to_string(),
                    "trigger".to_string(),
                ])
            && string_set(value_at(completion, "/keep_optional")?)?
                == BTreeSet::from([
                    "disabled_by_default_evidence".to_string(),
                    "removal_evidence".to_string(),
                ]),
        "phase-exit disposition completion requirements drifted",
    )?;

    let blocking = value_at(review, "/blocking_finding_behavior")?;
    require(
        blocking["missing_review_blocks_exit"] == true
            && blocking["missing_scope_blocks_exit"] == true
            && blocking["undispositioned_finding_blocks_exit"] == true
            && blocking["unsatisfied_completion_requirements_block_exit"] == true
            && blocking["pass_requires_zero_blocking_findings"] == true
            && blocking["blocked_state"] == "blocked"
            && blocking["pass_state"] == "pass",
        "phase-exit blocking-finding behavior is not fail-closed",
    )
}

/// ARRI-2.20: require one closed KISS/DRY/ownership review at every phase exit.
#[test]
fn arri_2_20_phase_exit_ownership_review_is_binding() -> Result<(), Box<dyn Error>> {
    let policy = contract()?;
    require_pending_evidence_state(&policy, "ARRI-2.20")?;
    validate_phase_exit_ownership_review(&policy)?;

    let mut missing_scope = policy.clone();
    missing_scope["simplicity"]["phase_exit_ownership_review"]["required_scope_ids"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("phase-exit scopes are not an array"))?
        .pop();

    let mut open_disposition = policy.clone();
    open_disposition["simplicity"]["phase_exit_review"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("phase-exit dispositions are not an array"))?
        .push(json!("retain"));

    let mut incomplete_defer = policy.clone();
    incomplete_defer["simplicity"]["phase_exit_ownership_review"]
        ["disposition_completion_requirements"]["defer"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("defer requirements are not an array"))?
        .pop();

    let mut permissive_blocking = policy;
    permissive_blocking["simplicity"]["phase_exit_ownership_review"]["blocking_finding_behavior"]
        ["pass_requires_zero_blocking_findings"] = json!(false);

    require(
        validate_phase_exit_ownership_review(&missing_scope).is_err()
            && validate_phase_exit_ownership_review(&open_disposition).is_err()
            && validate_phase_exit_ownership_review(&incomplete_defer).is_err()
            && validate_phase_exit_ownership_review(&permissive_blocking).is_err(),
        "phase-exit ownership review accepted an incomplete or permissive contract",
    )
}

/// ARRI-2.21 contract test: every v0.4 task has one real initial owner.
#[test]
fn arri_2_21_verification_plan_covers_every_v04_task() -> Result<(), Box<dyn Error>> {
    let plan = verification_plan()?;
    let intelligence = task_and_test_ids(INTELLIGENCE_TASKS)?;
    let quality = task_and_test_ids(QUALITY_TASKS)?;
    let authoritative_tests = [
        (
            "advance-rust-repository-intelligence",
            intelligence.as_slice(),
        ),
        ("enforce-rust-test-quality-gates", quality.as_slice()),
    ]
    .into_iter()
    .flat_map(|(change, tasks)| {
        tasks
            .iter()
            .map(move |(task_id, test_id)| (format!("{change}:{task_id}"), test_id.as_str()))
    })
    .collect::<BTreeMap<_, _>>();
    let task_sources = plan["task_sources"]
        .as_array()
        .ok_or_else(|| io::Error::other("task_sources is not an array"))?;
    let expected_sources = BTreeMap::from([
        (
            "advance-rust-repository-intelligence",
            (
                "openspec/changes/advance-rust-repository-intelligence/tasks.md",
                "UT:ARRI-",
            ),
        ),
        (
            "enforce-rust-test-quality-gates",
            (
                "openspec/changes/enforce-rust-test-quality-gates/tasks.md",
                "TQG-UT-",
            ),
        ),
    ]);
    require(
        task_sources.len() == expected_sources.len()
            && task_sources.iter().all(|source| {
                let Some(change) = source["change"].as_str() else {
                    return false;
                };
                expected_sources.get(change).is_some_and(|(path, prefix)| {
                    source["path"] == *path && source["test_id_prefix"] == *prefix
                })
            }),
        "verification-plan task sources drifted from the authoritative OpenSpec files",
    )?;

    let owner_ranges = plan["owner_ranges"]
        .as_array()
        .ok_or_else(|| io::Error::other("owner_ranges is not an array"))?;
    let mut task_ids = BTreeSet::new();
    let mut test_ids = BTreeSet::new();
    for (change, tasks) in [
        ("advance-rust-repository-intelligence", &intelligence),
        ("enforce-rust-test-quality-gates", &quality),
    ] {
        for (task_id, test_id) in tasks {
            require(
                task_ids.insert(format!("{change}:{task_id}")),
                format!("task ID {change}:{task_id} is reused"),
            )?;
            require(
                test_ids.insert(test_id.clone()),
                format!("unit-test ID {test_id} is reused"),
            )?;
            let matching = owner_ranges
                .iter()
                .filter(|row| row["change"] == change)
                .map(|row| {
                    let first = row["first_task"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("owner range lacks first_task"))?;
                    let last = row["last_task"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("owner range lacks last_task"))?;
                    task_in_range(task_id, first, last)
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?
                .into_iter()
                .filter(|matches| *matches)
                .count();
            require(
                matching == 1,
                format!("{change} task {task_id} maps to {matching} owner ranges"),
            )?;
        }
    }

    let risk_levels = string_set(&plan["risk_levels"])?;
    let evidence_layers = plan["evidence_layers"]
        .as_object()
        .ok_or_else(|| io::Error::other("evidence_layers is not an object"))?;
    let stable_rows = plan["stable_row_overrides"]
        .as_array()
        .ok_or_else(|| io::Error::other("stable_row_overrides is not an array"))?;
    let stable_task_ids = stable_rows
        .iter()
        .map(|row| {
            format!(
                "{}:{}",
                row["change"].as_str().unwrap_or_default(),
                row["task_id"].as_str().unwrap_or_default()
            )
        })
        .collect::<BTreeSet<_>>();
    require(
        stable_rows.len() == stable_task_ids.len(),
        "stable repository-intelligence rows contain duplicate task identities",
    )?;
    for row in stable_rows {
        let change = row["change"]
            .as_str()
            .ok_or_else(|| io::Error::other("stable row lacks a change identity"))?;
        let task_id = row["task_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("stable row lacks a task identity"))?;
        let identity = format!("{change}:{task_id}");
        let declared_test_id = row["unit_test"]["test_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("stable row lacks a unit-test identity"))?;
        require(
            authoritative_tests.get(&identity).copied() == Some(declared_test_id),
            format!("stable row {identity} is not owned by an authoritative task and test ID"),
        )?;
        validate_initial_unit_test_definition(row)?;
        validate_evidence_classification(row, &risk_levels, evidence_layers)?;
        require(
            row["unit_test"]["state"] == "implemented_uncommitted",
            "stable repository-intelligence row is not implemented",
        )?;
        require(
            row["result"]["state"] == "pending_commit_bound_run"
                && row["result"]["run_identity"].is_null(),
            "uncommitted repository-intelligence row fabricates successful evidence",
        )?;
    }
    let planned_rows = plan["planned_row_definitions"]
        .as_array()
        .ok_or_else(|| io::Error::other("planned_row_definitions is not an array"))?;
    require(
        planned_rows
            .iter()
            .map(|row| row["task_id"].as_str().unwrap_or_default().to_string())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["11.37".to_string(), "11.38".to_string()]),
        "late release task verification definitions drifted",
    )?;
    require(
        planned_rows.len() == 2,
        "late release task verification definitions contain duplicate rows",
    )?;
    for row in planned_rows {
        let task_id = row["task_id"].as_str().unwrap_or_default();
        validate_initial_unit_test_definition(row)?;
        validate_evidence_classification(row, &risk_levels, evidence_layers)?;
        require(
            row["change"] == "advance-rust-repository-intelligence"
                && row["unit_test"]["test_id"] == format!("UT:ARRI-{task_id}")
                && row["unit_test"]["state"] == "planned_not_implemented",
            format!("ARRI-{task_id} lacks a truthful planned unit-test definition"),
        )?;
        require(
            intelligence.iter().any(|(authoritative_task, test_id)| {
                authoritative_task == task_id
                    && row["unit_test"]["test_id"].as_str() == Some(test_id.as_str())
            }),
            format!("ARRI-{task_id} is not bound to its authoritative task/test pair"),
        )?;
        require(
            !row["requirement"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .is_empty()
                && !row["scenario"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                && !row["owner"].as_str().unwrap_or_default().trim().is_empty()
                && row["changed_artifacts"]
                    .as_array()
                    .is_some_and(|artifacts| !artifacts.is_empty())
                && row["timeout_seconds"]
                    .as_u64()
                    .is_some_and(|timeout| timeout > 0),
            format!("ARRI-{task_id} planned verification metadata is incomplete"),
        )?;
        require(
            !row["unit_test"]["assertion"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .is_empty()
                && row["unit_test"]["covered_inputs"]
                    .as_array()
                    .is_some_and(|inputs| {
                        !inputs.is_empty()
                            && inputs.iter().all(|input| {
                                input.as_str().is_some_and(|value| !value.trim().is_empty())
                            })
                    }),
            format!("ARRI-{task_id} lacks an assertion or covered inputs"),
        )?;
        require(
            row["result"]["state"] == "not_started"
                && row["result"]["implementation_commit"].is_null()
                && row["result"]["covered_input_digest"].is_null()
                && row["result"]["run_identity"].is_null()
                && row["result"]["artifact_digest"].is_null(),
            format!("ARRI-{task_id} fabricates implementation or run evidence"),
        )?;
    }
    let mut fabricated_pending = planned_rows[0].clone();
    fabricated_pending["unit_test"]["function"] = json!("invented_future_test");
    require(
        validate_initial_unit_test_definition(&fabricated_pending).is_err(),
        "planned unit test accepted a fabricated function",
    )?;
    let mut commandless_implemented = stable_rows[0].clone();
    commandless_implemented["unit_test"]["command"] = Value::Null;
    require(
        validate_initial_unit_test_definition(&commandless_implemented).is_err(),
        "implemented unit test accepted a missing command",
    )?;
    let mut invalid_risk = stable_rows[0].clone();
    invalid_risk["risk"] = json!("L9-unknown");
    require(
        validate_evidence_classification(&invalid_risk, &risk_levels, evidence_layers).is_err(),
        "stable row accepted an undeclared risk",
    )?;
    let mut invalid_layer = stable_rows[0].clone();
    invalid_layer["required_evidence_layers"] = json!(["unknown-layer"]);
    require(
        validate_evidence_classification(&invalid_layer, &risk_levels, evidence_layers).is_err(),
        "stable row accepted an undeclared evidence layer",
    )?;
    Ok(())
}

/// Validate that repository-wide quality closes the release after feature stabilization.
fn validate_quality_release_prerequisite(
    issue_map_source: &str,
    verification_plan_source: &str,
    intelligence_tasks: &str,
    quality_tasks: &str,
    quality_proposal: &str,
    quality_design: &str,
    quality_spec: &str,
) -> Result<(), Box<dyn Error>> {
    let issue_map: Value = serde_json::from_str(issue_map_source)?;
    let intelligence = &issue_map["changes"]["advance-rust-repository-intelligence"];
    let quality = &issue_map["changes"]["enforce-rust-test-quality-gates"];
    require(
        issue_map["schema_version"] == 2
            && intelligence["contract"] == "evidence-v2"
            && intelligence["primary_issue"] == REPOSITORY_INTELLIGENCE_ISSUE
            && issue_mapping_covers_tasks(
                intelligence,
                intelligence_tasks,
                &BTreeSet::from([
                    REPOSITORY_INTELLIGENCE_ISSUE,
                    REPOSITORY_INTELLIGENCE_PHASE_ISSUE,
                ]),
            )?
            && quality["contract"] == "evidence-v2"
            && quality["primary_issue"] == RUST_TEST_QUALITY_ISSUE
            && issue_mapping_covers_tasks(
                quality,
                quality_tasks,
                &BTreeSet::from([RUST_TEST_QUALITY_ISSUE]),
            )?,
        "v0.4 feature and quality changes are not mapped to issues 308 and 309",
    )?;

    let plan: Value = serde_json::from_str(verification_plan_source)?;
    let order = &plan["execution_order"];
    require(
        order["stable_behavior_requires_focused_unit_test"] == true
            && order["risk_required_layers_run_with_behavior"] == true
            && order["repository_wide_saturation_change"] == "enforce-rust-test-quality-gates"
            && order["repository_wide_saturation_position"]
                == "last_implementation_code_phase_before_public_docs_and_closeout"
            && order["full_coverage_before_stabilization"] == false
            && order["full_mutation_before_stabilization"] == false,
        "verification plan permits quality saturation before stable feature behavior",
    )?;
    require(
        order["release_tail"]
            == json!([
                "ARRI-11.37",
                "TQG-1.1-through-10.9",
                "ARRI-11.38",
                "TQG-10.10",
                "ARRI-11.39",
                "TQG-10.11",
                "TQG-10.12",
                "TQG-10.13"
            ]),
        "release tail does not preserve documentation, hosted evidence, quality proof, and post-merge sealing order",
    )?;

    let required_sources: [(&str, &str, &[&str]); 5] = [
        (
            "repository-intelligence tasks",
            intelligence_tasks,
            &[
                "The quality change blocks v0.4 release, not feature implementation.",
                "final v0.4 release prerequisite for `cargo nextest`, `cargo llvm-cov`, and `cargo mutants`",
                "not as a prerequisite to implement repository-intelligence architecture or features",
                "distinguish source mutation testing from repository mutation/fault fixtures",
                "`cargo test --doc`",
                "rather than treating repository mutations as source mutants",
            ],
        ),
        (
            "quality tasks",
            quality_tasks,
            &[
                "Begin implementation of this change only after the repository-intelligence architecture, migrations, public contracts, features, and their focused risk-based tests stabilize.",
                "This does not defer focused unit, integration, CLI/MCP E2E, or affected-platform tests for new behavior",
                "pinned all-feature nextest and stable workspace doctests",
                "run pinned LLVM coverage",
                "complete pinned 16-shard full mutation gate",
            ],
        ),
        (
            "quality proposal",
            quality_proposal,
            &[
                "separate blocking CI jobs for `cargo nextest`, stable `cargo test --doc`, `cargo llvm-cov`, and `cargo mutants`",
                "do not rely on unstable llvm-cov doctest instrumentation or claim source mutation duplicates repository mutation/fault fixtures",
                "final v0.4 release prerequisite for repository-intelligence delivery, not as a prerequisite to implement repository-intelligence functionality",
            ],
        ),
        (
            "quality design",
            quality_design,
            &[
                "Its repository-wide implementation and evidence campaign follows stabilization of the repository-intelligence architecture and features",
                "focused tests for each new stable behavior remain mandatory when that behavior lands",
                "Replacing existing repository mutation/fault fixtures with source mutation testing, or counting those fixtures as `cargo-mutants` evidence.",
                "This change remains a hard v0.4 release prerequisite; it is not a prerequisite to start repository-intelligence implementation.",
            ],
        ),
        (
            "quality specification",
            quality_spec,
            &[
                "Post-stabilization repository-wide quality closure",
                "This ordering SHALL NOT defer tests for new stable behavior",
                "The completed quality change SHALL remain a hard v0.4 release prerequisite, not a prerequisite to start feature implementation.",
                "separate blocking conclusions for non-doctest nextest execution, stable doctests, LLVM source coverage, and changed-source mutation",
            ],
        ),
    ];
    for (label, source, fragments) in required_sources {
        for fragment in fragments {
            require(
                source.contains(fragment),
                format!("{label} is missing release-order contract: {fragment}"),
            )?;
        }
    }

    let intelligence_task_order = task_and_test_ids(intelligence_tasks)?
        .into_iter()
        .map(|(task, _)| task)
        .collect::<Vec<_>>();
    let quality_task_order = task_and_test_ids(quality_tasks)?
        .into_iter()
        .map(|(task, _)| task)
        .collect::<Vec<_>>();
    let task_position = |tasks: &[String], task: &str, owner: &str| {
        tasks
            .iter()
            .position(|candidate| candidate == task)
            .ok_or_else(|| {
                Box::<dyn Error>::from(io::Error::other(format!(
                    "{owner} tasks are missing {task}"
                )))
            })
    };
    let saturation = task_position(&quality_task_order, "10.1", "quality")?;
    let nextest_and_doctests = task_position(&quality_task_order, "10.5", "quality")?;
    let coverage = task_position(&quality_task_order, "10.6", "quality")?;
    let mutation = task_position(&quality_task_order, "10.7", "quality")?;
    let architecture_review = task_position(&quality_task_order, "10.9", "quality")?;
    let readiness = task_position(&quality_task_order, "10.10", "quality")?;
    let quality_hosted_evidence = task_position(&quality_task_order, "10.11", "quality")?;
    let final_quality_proof = task_position(&quality_task_order, "10.12", "quality")?;
    let issue_sealing = task_position(&quality_task_order, "10.13", "quality")?;
    let format_audit = task_position(&intelligence_task_order, "11.37", "intelligence")?;
    let public_docs = task_position(&intelligence_task_order, "11.38", "intelligence")?;
    let feature_hosted_evidence = task_position(&intelligence_task_order, "11.39", "intelligence")?;
    require(
        saturation < nextest_and_doctests
            && nextest_and_doctests < coverage
            && coverage < mutation
            && mutation < architecture_review
            && architecture_review < readiness
            && readiness < quality_hosted_evidence
            && quality_hosted_evidence < final_quality_proof
            && final_quality_proof < issue_sealing
            && format_audit < public_docs
            && public_docs < feature_hosted_evidence,
        "v0.4 tasks do not preserve saturation, documentation, evidence, proof, and sealing order",
    )
}

/// ARRI-2.30: final quality gates block release, never feature implementation.
#[test]
fn arri_2_30_quality_release_prerequisite_is_ordered() -> Result<(), Box<dyn Error>> {
    validate_quality_release_prerequisite(
        ISSUE_MAP,
        VERIFICATION_PLAN,
        INTELLIGENCE_TASKS,
        QUALITY_TASKS,
        QUALITY_PROPOSAL,
        QUALITY_DESIGN,
        QUALITY_SPEC,
    )?;

    let partially_checked_quality_tasks = QUALITY_TASKS.replacen("- [ ] 10.1 ", "- [x] 10.1 ", 1);
    validate_quality_release_prerequisite(
        ISSUE_MAP,
        VERIFICATION_PLAN,
        INTELLIGENCE_TASKS,
        &partially_checked_quality_tasks,
        QUALITY_PROPOSAL,
        QUALITY_DESIGN,
        QUALITY_SPEC,
    )?;

    let mut wrong_issue: Value = serde_json::from_str(ISSUE_MAP)?;
    wrong_issue["changes"]["enforce-rust-test-quality-gates"]["primary_issue"] = json!(310);
    let wrong_issue = serde_json::to_string(&wrong_issue)?;
    require(
        validate_quality_release_prerequisite(
            &wrong_issue,
            VERIFICATION_PLAN,
            INTELLIGENCE_TASKS,
            QUALITY_TASKS,
            QUALITY_PROPOSAL,
            QUALITY_DESIGN,
            QUALITY_SPEC,
        )
        .is_err(),
        "quality prerequisite accepted the wrong mapped issue",
    )?;

    let premature_quality = QUALITY_TASKS.replacen(
        "Begin implementation of this change only after",
        "Begin implementation of this change before",
        1,
    );
    require(
        validate_quality_release_prerequisite(
            ISSUE_MAP,
            VERIFICATION_PLAN,
            INTELLIGENCE_TASKS,
            &premature_quality,
            QUALITY_PROPOSAL,
            QUALITY_DESIGN,
            QUALITY_SPEC,
        )
        .is_err(),
        "quality prerequisite accepted pre-stabilization saturation",
    )?;

    let missing_doctests = QUALITY_PROPOSAL.replacen(
        "stable `cargo test --doc`",
        "unspecified documentation tests",
        1,
    );
    require(
        validate_quality_release_prerequisite(
            ISSUE_MAP,
            VERIFICATION_PLAN,
            INTELLIGENCE_TASKS,
            QUALITY_TASKS,
            &missing_doctests,
            QUALITY_DESIGN,
            QUALITY_SPEC,
        )
        .is_err(),
        "quality prerequisite accepted missing stable doctest evidence",
    )?;

    let conflated_mutation = INTELLIGENCE_TASKS.replacen(
        "rather than treating repository mutations as source mutants",
        "by treating repository mutations as source mutants",
        1,
    );
    require(
        validate_quality_release_prerequisite(
            ISSUE_MAP,
            VERIFICATION_PLAN,
            &conflated_mutation,
            QUALITY_TASKS,
            QUALITY_PROPOSAL,
            QUALITY_DESIGN,
            QUALITY_SPEC,
        )
        .is_err(),
        "quality prerequisite conflated repository faults with source mutants",
    )?;

    let mut misplaced_issueops: Value = serde_json::from_str(VERIFICATION_PLAN)?;
    misplaced_issueops["execution_order"]["release_tail"] = json!([
        "ARRI-11.37",
        "TQG-1.1-through-10.9",
        "TQG-10.10",
        "ARRI-11.38",
        "ARRI-11.39",
        "TQG-10.11",
        "TQG-10.12",
        "TQG-10.13"
    ]);
    let misplaced_issueops = serde_json::to_string(&misplaced_issueops)?;
    require(
        validate_quality_release_prerequisite(
            ISSUE_MAP,
            &misplaced_issueops,
            INTELLIGENCE_TASKS,
            QUALITY_TASKS,
            QUALITY_PROPOSAL,
            QUALITY_DESIGN,
            QUALITY_SPEC,
        )
        .is_err(),
        "quality prerequisite accepted IssueOps before public documentation",
    )
}

/// ARRI-2.31: every pre-mortem mitigation is an owned `OpenSpec` action.
#[test]
fn arri_2_31_pre_mortem_mitigations_are_task_owned() -> Result<(), Box<dyn Error>> {
    let authoritative_tasks = task_and_test_ids(INTELLIGENCE_TASKS)?
        .into_iter()
        .map(|(task, _)| format!("ARRI-{task}"))
        .collect::<BTreeSet<_>>();
    validate_pre_mortem_mitigations(INTELLIGENCE_DESIGN, &authoritative_tasks)?;

    let duplicate = INTELLIGENCE_DESIGN.replacen("| PM-02 |", "| PM-01 |", 1);
    require(
        validate_pre_mortem_mitigations(&duplicate, &authoritative_tasks).is_err(),
        "duplicate pre-mortem risk ID was accepted",
    )?;
    let orphan =
        INTELLIGENCE_DESIGN.replacen("`ARRI-8.7`, `ARRI-8.8`", "No owning task is declared.", 1);
    require(
        validate_pre_mortem_mitigations(&orphan, &authoritative_tasks).is_err(),
        "orphan pre-mortem mitigation was accepted",
    )?;
    let unknown = INTELLIGENCE_DESIGN.replacen("`ARRI-5.1`", "`ARRI-99.99`", 1);
    require(
        validate_pre_mortem_mitigations(&unknown, &authoritative_tasks).is_err(),
        "unknown pre-mortem mitigation task was accepted",
    )
}

/// ARRI-2.22 contract test: final evidence validation is fail-closed and deferred.
#[test]
fn arri_2_22_validator_contract_rejects_invalid_evidence() -> Result<(), Box<dyn Error>> {
    let plan = verification_plan()?;
    let validator = &plan["validator_contract"];
    require(
        validator["implementation_change"] == "enforce-rust-test-quality-gates"
            && validator["implementation_issue"] == RUST_TEST_QUALITY_ISSUE,
        "final validator is not owned by the mapped quality change",
    )?;
    require(
        validator["activation"] == "after_repository_intelligence_stabilization",
        "final evidence validator activates before feature stabilization",
    )?;
    let required = BTreeSet::from([
        "covered_input_drift".to_string(),
        "exit_only_assertion".to_string(),
        "failed".to_string(),
        "flaky".to_string(),
        "invalid_not_applicable".to_string(),
        "abbreviated_sha_openspec_permalink".to_string(),
        "branch_only_openspec_permalink".to_string(),
        "content_mismatched_openspec_permalink".to_string(),
        "foreign_repository_openspec_permalink".to_string(),
        "local_remote_drift".to_string(),
        "missing".to_string(),
        "missing_openspec_permalink".to_string(),
        "missing_required_layer".to_string(),
        "nonexistent_openspec_permalink".to_string(),
        "orphaned_evidence".to_string(),
        "retry_only_success".to_string(),
        "skipped".to_string(),
        "stale_for_commit".to_string(),
        "stale_openspec_permalink".to_string(),
        "timed_out".to_string(),
        "unchecked_dependency".to_string(),
        "under_classified_risk".to_string(),
        "wrong_test_identity".to_string(),
        "wrong_change_openspec_permalink".to_string(),
        "zero_tests_selected".to_string(),
    ]);
    let rejected = string_set(&validator["reject"])?;
    let missing = required
        .difference(&rejected)
        .cloned()
        .collect::<BTreeSet<_>>();
    require(
        missing.is_empty(),
        format!("validator rejection contract is incomplete: {missing:?}"),
    )
}

/// ARRI-2.23 contract test: focused behavior tests precede final saturation.
#[test]
fn arri_2_23_behavior_tests_precede_saturation() -> Result<(), Box<dyn Error>> {
    let plan = verification_plan()?;
    let order = &plan["execution_order"];
    require(
        order["stable_behavior_requires_focused_unit_test"] == true
            && order["risk_required_layers_run_with_behavior"] == true,
        "behavior-local tests are not required in the implementation slice",
    )?;
    require(
        order["repository_wide_saturation_change"] == "enforce-rust-test-quality-gates"
            && order["repository_wide_saturation_position"]
                == "last_implementation_code_phase_before_public_docs_and_closeout"
            && order["release_tail"]
                == serde_json::json!([
                    "ARRI-11.37",
                    "TQG-1.1-through-10.9",
                    "ARRI-11.38",
                    "TQG-10.10",
                    "ARRI-11.39",
                    "TQG-10.11",
                    "TQG-10.12",
                    "TQG-10.13"
                ]),
        "repository-wide saturation and closeout order drifted",
    )?;
    require(
        order["full_coverage_before_stabilization"] == false
            && order["full_mutation_before_stabilization"] == false,
        "coverage or full mutation was scheduled before stabilization",
    )
}

/// ARRI-2.24 contract test: the future validator's fixture catalog is complete.
#[test]
fn arri_2_24_self_test_catalog_is_complete() -> Result<(), Box<dyn Error>> {
    let plan = verification_plan()?;
    let fixtures = string_set(&plan["self_test_fixtures"])?;
    let required = BTreeSet::from([
        "exit_only_assertion".to_string(),
        "abbreviated_sha_openspec_permalink".to_string(),
        "branch_only_openspec_permalink".to_string(),
        "content_mismatched_openspec_permalink".to_string(),
        "foreign_repository_openspec_permalink".to_string(),
        "invalid_not_applicable_unit_test".to_string(),
        "local_remote_drift".to_string(),
        "missing_required_layer".to_string(),
        "missing_openspec_permalink".to_string(),
        "missing_successful_run".to_string(),
        "missing_task_row".to_string(),
        "missing_unit_test_id".to_string(),
        "nonexistent_openspec_permalink".to_string(),
        "retry_only_success".to_string(),
        "stale_successful_run".to_string(),
        "stale_openspec_permalink".to_string(),
        "timeout_failure".to_string(),
        "unchecked_dependency".to_string(),
        "under_classified_risk".to_string(),
        "valid_complete_evidence".to_string(),
        "valid_full_sha_openspec_permalinks".to_string(),
        "wrong_change_openspec_permalink".to_string(),
        "zero_tests_selected".to_string(),
    ]);
    require(
        fixtures == required,
        format!("verification self-test catalog drifted: {fixtures:?}"),
    )
}

/// Return whether one issue mapping covers every authoritative task exactly once.
fn issue_mapping_covers_tasks(
    mapping: &Value,
    task_source: &str,
    expected_issues: &BTreeSet<u64>,
) -> Result<bool, Box<dyn Error>> {
    let owners = mapping["owners"]
        .as_array()
        .ok_or_else(|| io::Error::other("issue owners are not an array"))?;
    let issues = owners
        .iter()
        .filter_map(|owner| owner["issue"].as_u64())
        .collect::<BTreeSet<_>>();
    if &issues != expected_issues {
        return Ok(false);
    }
    for (task_id, _) in task_and_test_ids(task_source)? {
        let task = task_id_parts(&task_id)?;
        let matches = owners
            .iter()
            .filter(|owner| {
                let Some(first) = owner["first_task"].as_str() else {
                    return false;
                };
                let Some(last) = owner["last_task"].as_str() else {
                    return false;
                };
                match (task_id_parts(first), task_id_parts(last)) {
                    (Ok(first), Ok(last)) => task >= first && task <= last,
                    _ => false,
                }
            })
            .count();
        if matches != 1 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// ARRI-2.25 contract test: `IssueOps` ownership and activation are explicit.
#[test]
fn arri_2_25_issueops_ownership_is_explicit() -> Result<(), Box<dyn Error>> {
    let plan = verification_plan()?;
    let issue_map: Value = serde_json::from_str(ISSUE_MAP)?;
    let issueops = &plan["issueops"];
    let intelligence = &issue_map["changes"]["advance-rust-repository-intelligence"];
    let quality = &issue_map["changes"]["enforce-rust-test-quality-gates"];
    require(
        issue_map["schema_version"] == 2
            && intelligence["contract"] == "evidence-v2"
            && intelligence["primary_issue"] == REPOSITORY_INTELLIGENCE_ISSUE
            && issue_mapping_covers_tasks(
                intelligence,
                INTELLIGENCE_TASKS,
                &BTreeSet::from([
                    REPOSITORY_INTELLIGENCE_ISSUE,
                    REPOSITORY_INTELLIGENCE_PHASE_ISSUE,
                ]),
            )?
            && quality["contract"] == "evidence-v2"
            && quality["primary_issue"] == RUST_TEST_QUALITY_ISSUE
            && issue_mapping_covers_tasks(
                quality,
                QUALITY_TASKS,
                &BTreeSet::from([RUST_TEST_QUALITY_ISSUE]),
            )?,
        "local issue map does not own both v0.4 changes",
    )?;
    require(
        issueops["authoritative_issues"]["advance-rust-repository-intelligence"]
            == REPOSITORY_INTELLIGENCE_ISSUE
            && issueops["authoritative_issues"]["enforce-rust-test-quality-gates"]
                == RUST_TEST_QUALITY_ISSUE,
        "verification plan and issue map disagree",
    )?;
    require(
        issueops["local_and_remote_state_must_match"] == true
            && issueops["openspec_permalink_policy"]["repository"] == "styler-ai/ProjectAtlas"
            && issueops["openspec_permalink_policy"]["commit_identity"] == "full_40_hex_sha"
            && issueops["openspec_permalink_policy"]["resolution"] == "github_api"
            && issueops["openspec_permalink_policy"]["required_spec_collection"]["link_kind"]
                == "full_sha_tree"
            && issueops["openspec_permalink_policy"]["required_spec_collection"]["every_matching_file_must_resolve"]
                == true
            && issueops["openspec_permalink_policy"]["linked_content_must_match_authoritative_state"]
                == true
            && issueops["openspec_permalink_policy"]["branch_links_allowed"] == false
            && issueops["openspec_permalink_policy"]["abbreviated_shas_allowed"] == false
            && issueops["evidence_rendering_owner"] == "enforce-rust-test-quality-gates"
            && issueops["final_gate_activation"] == "after_repository_intelligence_stabilization",
        "IssueOps evidence ownership or activation order drifted",
    )
}

/// ARRI-2.1: the candidate delivery inventory is exhaustive and owned.
#[test]
fn capability_inventory_is_binding_and_owned() -> Result<(), Box<dyn Error>> {
    let registry = capability_registry()?;
    validate_capability_inventory(&registry)?;

    let mut empty = registry.clone();
    empty["capabilities"] = json!([]);
    empty["counts"]["capabilities"] = json!(0);
    require(
        validate_capability_inventory(&empty).is_err(),
        "empty capability inventory was accepted",
    )?;

    let mut duplicate = registry.clone();
    let capabilities = duplicate["capabilities"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("capabilities is not an array"))?;
    let duplicate_row = capabilities
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("capability fixture is empty"))?;
    capabilities.push(duplicate_row);
    duplicate["counts"]["capabilities"] = json!(capabilities.len());
    require(
        validate_capability_inventory(&duplicate).is_err(),
        "duplicate capability ID was accepted",
    )?;

    for (field, invalid, message) in [
        ("owner", json!(""), "blank capability owner was accepted"),
        (
            "pack_id",
            json!("missing-pack"),
            "unknown capability pack was accepted",
        ),
        (
            "public_surfaces",
            json!([]),
            "capability without a public surface was accepted",
        ),
        (
            "fixture_ids",
            json!([]),
            "capability without a fixture was accepted",
        ),
        (
            "acceptance_rule",
            json!(""),
            "blank capability acceptance rule was accepted",
        ),
    ] {
        let mut invalid_registry = registry.clone();
        invalid_registry["capabilities"][0][field] = invalid;
        require(
            validate_capability_inventory(&invalid_registry).is_err(),
            message,
        )?;
    }

    let mut missing_platform = registry;
    missing_platform["required_platforms"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("required_platforms is not an array"))?
        .pop();
    require(
        validate_capability_inventory(&missing_platform).is_err(),
        "reduced capability platform matrix was accepted",
    )
}

/// Validate one binding capability inventory without trusting declared totals.
fn validate_capability_inventory(registry: &Value) -> Result<(), Box<dyn Error>> {
    require(
        registry["format"] == "projectatlas.capability-registry"
            && registry["binding_role"] == "delivery-inventory",
        "capability registry is not the binding delivery inventory",
    )?;
    require(
        string_set(&registry["required_platforms"])?
            == BTreeSet::from([
                "linux-x86_64".to_string(),
                "macos-aarch64".to_string(),
                "macos-x86_64".to_string(),
                "windows-x86_64".to_string(),
            ]),
        "capability platform matrix drifted",
    )?;
    let pack_ids = registry["packs"]
        .as_array()
        .ok_or_else(|| io::Error::other("packs is not an array"))?
        .iter()
        .map(|pack| {
            pack["pack_id"]
                .as_str()
                .filter(|id| !id.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("pack lacks a nonempty ID"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(!pack_ids.is_empty(), "capability pack inventory is empty")?;
    let required_capability_fields = BTreeSet::from([
        "acceptance_rule".to_string(),
        "advertised".to_string(),
        "capability_id".to_string(),
        "evidence_state".to_string(),
        "family".to_string(),
        "fixture_ids".to_string(),
        "owner".to_string(),
        "pack_id".to_string(),
        "public_surfaces".to_string(),
    ]);
    let capabilities = registry["capabilities"]
        .as_array()
        .ok_or_else(|| io::Error::other("capabilities is not an array"))?;
    require(!capabilities.is_empty(), "capability inventory is empty")?;
    let mut ids = BTreeSet::new();
    for capability in capabilities {
        let object = capability
            .as_object()
            .ok_or_else(|| io::Error::other("capability row is not an object"))?;
        require(
            required_capability_fields.is_subset(&object.keys().cloned().collect()),
            "capability row lacks ownership or acceptance evidence fields",
        )?;
        let id = capability["capability_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("capability row lacks an ID"))?;
        require(ids.insert(id), format!("duplicate capability ID {id}"))?;
        require(
            capability["advertised"] == false && capability["evidence_state"] == "pending",
            format!("candidate capability {id} is advertised without evidence"),
        )?;
        let owner = capability["owner"].as_str().unwrap_or_default();
        let pack_id = capability["pack_id"].as_str().unwrap_or_default();
        require(
            !owner.trim().is_empty()
                && pack_ids.contains(pack_id)
                && capability["public_surfaces"]
                    .as_array()
                    .is_some_and(|rows| {
                        !rows.is_empty()
                            && rows.iter().all(|row| {
                                row.as_str().is_some_and(|value| !value.trim().is_empty())
                            })
                    })
                && capability["fixture_ids"].as_array().is_some_and(|rows| {
                    !rows.is_empty()
                        && rows
                            .iter()
                            .all(|row| row.as_str().is_some_and(|value| !value.trim().is_empty()))
                })
                && capability["acceptance_rule"]
                    .as_str()
                    .is_some_and(|rule| !rule.trim().is_empty()),
            format!(
                "capability {id} lacks an owner, known pack, surface, fixture, or measurable rule"
            ),
        )?;
    }
    require(
        registry["counts"]["capabilities"] == capabilities.len(),
        "capability count does not reconcile",
    )
}

/// ARRI-2.2: freeze the neutral 212-mode/207-parser delivery target.
#[test]
fn language_target_has_212_modes_and_207_parsers() -> Result<(), Box<dyn Error>> {
    let registry = capability_registry()?;
    let modes = registry["modes"]
        .as_array()
        .ok_or_else(|| io::Error::other("modes is not an array"))?;
    let parsers = registry["parsers"]
        .as_array()
        .ok_or_else(|| io::Error::other("parsers is not an array"))?;
    require(modes.len() == 212, "candidate mode target is not 212")?;
    require(parsers.len() == 207, "normalized parser target is not 207")?;
    require(
        registry["counts"]["accepted_language_crosswalk_entries"] == 160
            && registry["counts"]["current_public_modes"] == 63,
        "accepted-language crosswalk or retained public-mode count drifted",
    )?;
    let mode_ids = modes
        .iter()
        .map(|mode| {
            mode["public_mode"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("mode lacks public_mode"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        mode_ids.len() == modes.len(),
        "candidate mode IDs are not unique",
    )?;
    let parser_ids = modes
        .iter()
        .map(|mode| {
            mode["parser_id"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("mode lacks parser_id"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        parser_ids.len() == parsers.len(),
        "mode rows inflate normalized parser count",
    )?;
    let frozen_modes = projectatlas_core::language::LANGUAGE_SPECS
        .iter()
        .map(|spec| spec.language.to_string())
        .collect::<BTreeSet<_>>();
    let retained_modes = modes
        .iter()
        .filter(|mode| mode["origin"] == "v0.3.26-public-mode")
        .map(|mode| {
            mode["public_mode"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("retained mode lacks public_mode"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        retained_modes == frozen_modes,
        "candidate registry does not preserve every current public mode",
    )?;
    require(
        registry["status"] == "candidate-pending-evidence"
            && registry["achieved_manifest"].is_null()
            && registry["accepted_set_policy"]["advertisement_requires_achieved_manifest"] == true
            && registry["accepted_set_policy"]["target_runnable_modes"] == 212
            && registry["accepted_set_policy"]["target_normalized_parser_capabilities"] == 207,
        "candidate counts were converted into unsupported achieved claims",
    )
}

/// ARRI-2.3: every advertised family has an acceptance row.
#[test]
fn capability_family_contract_has_measurable_completion() -> Result<(), Box<dyn Error>> {
    let registry = capability_registry()?;
    validate_capability_family_contract(&registry)?;

    let mut missing_family = registry.clone();
    let capabilities = missing_family["capabilities"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("capabilities is not an array"))?;
    capabilities.retain(|capability| capability["family"] != "snapshot");
    let retained = capabilities.len();
    missing_family["counts"]["capabilities"] = json!(retained);
    require(
        validate_capability_family_contract(&missing_family).is_err(),
        "capability inventory without the snapshot family was accepted",
    )?;

    for (pack_id, installed_by_default, message) in [
        (
            "semantic-pack",
            true,
            "optional semantic pack was accepted as default core",
        ),
        (
            "default-core",
            false,
            "required default core was accepted as optional",
        ),
    ] {
        let mut invalid = registry.clone();
        let pack = invalid["packs"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("packs is not an array"))?
            .iter_mut()
            .find(|pack| pack["pack_id"] == pack_id)
            .ok_or_else(|| io::Error::other(format!("missing pack {pack_id}")))?;
        pack["installed_by_default"] = json!(installed_by_default);
        require(
            validate_capability_family_contract(&invalid).is_err(),
            message,
        )?;
    }
    Ok(())
}

/// Validate capability-family coverage and default-versus-optional pack ownership.
fn validate_capability_family_contract(registry: &Value) -> Result<(), Box<dyn Error>> {
    validate_capability_inventory(registry)?;
    let families = registry["capabilities"]
        .as_array()
        .ok_or_else(|| io::Error::other("capabilities is not an array"))?
        .iter()
        .map(|capability| {
            capability["family"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("capability lacks family"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        families
            == BTreeSet::from([
                "agent-workflow".to_string(),
                "analysis".to_string(),
                "enrichment".to_string(),
                "entity".to_string(),
                "federation".to_string(),
                "incremental".to_string(),
                "pack".to_string(),
                "relation".to_string(),
                "search".to_string(),
                "snapshot".to_string(),
            ]),
        format!("accepted capability-family inventory drifted: {families:?}"),
    )?;
    let pack_installation = registry["packs"]
        .as_array()
        .ok_or_else(|| io::Error::other("packs is not an array"))?
        .iter()
        .map(|pack| {
            let pack_id = pack["pack_id"]
                .as_str()
                .ok_or_else(|| io::Error::other("pack ID is absent"))?;
            let installed_by_default = pack["installed_by_default"]
                .as_bool()
                .ok_or_else(|| io::Error::other(format!("{pack_id} install policy is absent")))?;
            Ok((pack_id.to_string(), installed_by_default))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    require(
        pack_installation
            == BTreeMap::from([
                ("broad-language-pack".to_string(), false),
                ("default-core".to_string(), true),
                ("semantic-pack".to_string(), false),
            ]),
        "default-core and optional pack installation policy drifted",
    )
}

/// ARRI-2.5: pin three corpus strata and reproduction schema.
#[test]
fn evaluation_manifest_pins_three_strata() -> Result<(), Box<dyn Error>> {
    let manifest = evaluation_manifest()?;
    require(
        manifest["format"] == "projectatlas.evaluation-manifest"
            && manifest["claim_status"] == "not-measured",
        "evaluation manifest format or no-claim state drifted",
    )?;
    let corpora = manifest["corpora"]
        .as_array()
        .ok_or_else(|| io::Error::other("corpora is not an array"))?;
    require(
        corpora.len() == 3,
        "evaluation corpus does not have three strata",
    )?;
    require(
        corpora
            .iter()
            .map(|corpus| corpus["stratum"].as_str().unwrap_or_default().to_string())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "large".to_string(),
                "medium".to_string(),
                "small".to_string(),
            ]),
        "small, medium, and large strata are not each pinned",
    )?;
    let expected_corpora = BTreeMap::from([
        (
            "projectatlas-self",
            (
                "medium",
                202,
                5_093_135,
                "73e67c0efd51e0f8cadf7b6bcf0c37713c4dd2b25df9ff672f20b46037250eb9",
                "18045076eb68e2cfaf6bb30e60ec95b6b3dafad55ef04c8ee69708eba846ba51",
                "c08a09eaaa87300b66ccb02677fb1e11e1bc03bda2524ee963527e1f2a44542c",
            ),
        ),
        (
            "rust-analyzer",
            (
                "large",
                2_335,
                21_793_329,
                "1daa550845561ef26f85a2db8fb7c7397b3f6c9f0f768f7450ecb7c87a028b6c",
                "2135d3869dfed4ff2be6c0ed08422b903dd00ab88fbdb5b5e13ff8469da858fa",
                "4985f193fec7d291509daa851687af3d30eb85e1315615663684e31a09312b43",
            ),
        ),
        (
            "serde-json",
            (
                "small",
                92,
                733_812,
                "9bc51962b06faeb5f481ec5d4b66c130b685d740d547021cf2a3737aee29690e",
                "c15014085fd50e61323d0a53db9a04ec26a4440d503c3659b20b7e446421c7da",
                "12e089498f3acfe4a5185974e2ffc10cb2a1cebb3d3d2e6771ee0c5028fc5b2a",
            ),
        ),
    ]);
    let mut seen_corpora = BTreeSet::new();
    for corpus in corpora {
        let id = corpus["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("corpus lacks ID"))?;
        let (expected_stratum, expected_files, expected_bytes, tree_digest, blob_digest, digest) =
            expected_corpora
                .get(id)
                .ok_or_else(|| io::Error::other(format!("unexpected corpus {id}")))?;
        require(seen_corpora.insert(id), format!("duplicate corpus {id}"))?;
        for field in ["repository", "commit", "tree", "license"] {
            require(
                corpus[field]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                format!("corpus lacks pinned {field}"),
            )?;
        }
        require(
            corpus["clean_required"] == true
                && corpus["submodules_allowed"] == false
                && corpus["lfs_allowed"] == false,
            "corpus materialization policy is not fail-closed",
        )?;
        let stratum = corpus["stratum"]
            .as_str()
            .ok_or_else(|| io::Error::other(format!("corpus {id} lacks stratum")))?;
        let tracked_files = corpus["tracked_files"]
            .as_u64()
            .ok_or_else(|| io::Error::other(format!("corpus {id} lacks tracked_files")))?;
        let tracked_bytes = corpus["tracked_logical_bytes"]
            .as_u64()
            .ok_or_else(|| io::Error::other(format!("corpus {id} lacks tracked bytes")))?;
        require(
            stratum == *expected_stratum
                && tracked_files == *expected_files
                && tracked_bytes == *expected_bytes,
            format!("corpus {id} count, bytes, or stratum drifted"),
        )?;
        let stratum_policy = &manifest["strata"][stratum];
        let minimum = stratum_policy["tracked_logical_bytes_min"]
            .as_u64()
            .unwrap_or_default();
        let maximum = stratum_policy["tracked_logical_bytes_max"]
            .as_u64()
            .unwrap_or(u64::MAX);
        require(
            tracked_bytes >= minimum && tracked_bytes <= maximum,
            format!("corpus {id} falls outside its {stratum} byte boundaries"),
        )?;
        require(
            corpus["tree_manifest_sha256"] == *tree_digest
                && corpus["blob_size_manifest_sha256"] == *blob_digest
                && corpus["canonical_manifest_sha256"] == *digest
                && corpus["materialization_state"] == "verified"
                && corpus["submodules"] == 0
                && corpus["lfs_pointers"] == 0
                && corpus["case_fold_collisions"] == 0,
            format!("corpus {id} materialization evidence is incomplete or drifted"),
        )?;
    }
    require(
        seen_corpora == expected_corpora.keys().copied().collect(),
        "evaluation corpus set does not match the verified materializations",
    )?;
    let required_operations = BTreeSet::from([
        "cold-full-scan".to_string(),
        "fts-differential".to_string(),
        "graph-lookup".to_string(),
        "lexical-search".to_string(),
        "mcp-call-flow".to_string(),
        "no-change-scan".to_string(),
        "one-file-refresh".to_string(),
        "parser-host-native".to_string(),
        "parser-host-wasm".to_string(),
        "semantic-candidate".to_string(),
        "sqlite-strategy".to_string(),
        "warm-full-scan".to_string(),
    ]);
    let operations = manifest["operations"]
        .as_array()
        .ok_or_else(|| io::Error::other("operations is not an array"))?
        .iter()
        .map(|operation| {
            operation["id"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("operation lacks ID"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        operations == required_operations,
        format!("evaluation operation inventory drifted: {operations:?}"),
    )
}

/// Validate the complete statistical pre-registration without using any observed result.
fn validate_statistics_contract(manifest: &Value) -> Result<(), Box<dyn Error>> {
    let design = &manifest["experiment_design"];
    require(
        design["sample_unit"] == "one operation on one corpus/profile/environment/runtime tuple"
            && design["strata"] == json!(["small", "medium", "large"])
            && design["warmups"] == 3
            && design["paired_repetitions"] == 15
            && design["minimum_valid_pairs"] == 10,
        "experimental unit, strata, warmups, or paired sample policy drifted",
    )?;
    require(
        design["block_order"]
            == "AB or BA from the final-byte low bit of SHA-256(projectatlas.evaluation-order.v2 followed by u64-le length-prefixed decoded-seed, UTF-8 cell-id, u64-le repetition, and UTF-8 pair-id fields); 0 selects AB and 1 selects BA"
            && design["rng"]["algorithm"] == "sha256-domain-separated-ordering"
            && design["rng"]["version"] == "2"
            && design["rng"]["seed_hex"]
                == "042b3b999b906526d603fbc1deaf9682ad0c440623ee35bbe143cfd09c77edd3",
        "versioned deterministic ordering identity, encoding, or seed drifted",
    )?;
    require(
        design["timeout_treatment"]
            == "retain as failure and worst-direction infinite ratio; never exclude"
            && design["failure_treatment"]
                == "any correctness, integrity, containment, compatibility, or required-platform failure blocks the dimension and aggregate exit"
            && design["outlier_policy"]
                == "retain all preregistered observations; report raw values, median, p50, p95, MAD, and sensitivity without automatic deletion"
            && design["independence_policy"]
                == "repeated model runs or requests from one fixture or task are one clustered experimental unit, never independent units"
            && design["degenerate_sample_policy"]
                == "zero denominators, one-class fixtures, bootstrap-degenerate samples, and cells below minimum positive or negative counts are ineligible and never reported as 100 percent"
            && design["reruns"]
                == "only declared infrastructure failure permits a full cell rerun; retain all attempts and never select the best attempt",
        "timeout, failure, outlier, independence, denominator, or rerun policy drifted",
    )?;
    require(
        design["index_worker_counts"] == json!([1, 8])
            && design["query_concurrency"] == json!([1, 4, 16])
            && design["mixed_workload"]["publication_tasks"] == 1
            && design["mixed_workload"]["concurrent_readers"] == 8,
        "worker or concurrency cells are not frozen",
    )?;
    let intervals = &design["confidence_intervals"];
    let paired = &intervals["paired_time_rss_and_geometric_means"];
    let latency = &intervals["latency_percentiles"];
    let accuracy = &intervals["accuracy_and_agent_metrics"];
    require(
        paired["method"] == "deterministic bias-corrected bootstrap of paired log ratios"
            && paired["cluster_unit"] == "repository/run"
            && paired["resamples"] == 10_000
            && paired["confidence"] == 0.95
            && paired["decision_bound"] == "one-sided adverse",
        "time/RSS bootstrap is not bias-corrected and repository/run clustered",
    )?;
    require(
        latency["method"] == "deterministic hierarchical bootstrap"
            && latency["cluster_levels"] == json!(["run", "request"])
            && latency["warmup_requests_per_cell"] == 100
            && latency["measured_requests_per_cell"] == 1_000
            && latency["resamples"] == 10_000
            && latency["confidence"] == 0.95
            && latency["decision_bound"] == "one-sided adverse",
        "latency bootstrap is not hierarchical over runs and requests",
    )?;
    let fts = value_at(manifest, "/architecture_evaluations/fts_differential")?;
    let timed_query_iterations = fts["timed_query_iterations"]
        .as_u64()
        .ok_or_else(|| io::Error::other("FTS timed request count is missing"))?;
    let fts_warmup_requests = fts["warmups"]
        .as_u64()
        .and_then(|warmups| warmups.checked_mul(timed_query_iterations))
        .ok_or_else(|| io::Error::other("FTS warmup request count overflowed"))?;
    let fts_measured_requests = fts["repetitions"]
        .as_u64()
        .and_then(|repetitions| repetitions.checked_mul(timed_query_iterations))
        .ok_or_else(|| io::Error::other("FTS measured request count overflowed"))?;
    let minimum_warmup_requests = latency["warmup_requests_per_cell"]
        .as_u64()
        .ok_or_else(|| io::Error::other("latency warmup minimum is missing"))?;
    let minimum_measured_requests = latency["measured_requests_per_cell"]
        .as_u64()
        .ok_or_else(|| io::Error::other("latency measurement minimum is missing"))?;
    require(
        fts_warmup_requests >= minimum_warmup_requests
            && fts_measured_requests >= minimum_measured_requests,
        "FTS raw observations do not satisfy the registered latency sample minimums",
    )?;
    require(
        accuracy["method"] == "deterministic paired bootstrap"
            && accuracy["cluster_unit"] == "unique fixture/task"
            && accuracy["repository_strata"] == json!(["small", "medium", "large"])
            && accuracy["stratum_weights"] == json!({"small": 1, "medium": 1, "large": 1})
            && accuracy["weight_normalization"]
                == "normalize the frozen integer weights to sum to one"
            && accuracy["resamples"] == 10_000
            && accuracy["confidence"] == 0.95
            && accuracy["decision_bound"] == "one-sided adverse"
            && intervals["seed_derivation"] == "SHA-256(global seed || metric family || cell ID)",
        "accuracy/agent bootstrap lacks frozen strata or fixture/task clustering",
    )?;
    require(
        design["paired_comparison"]["lower_is_better_ratio"] == "candidate / baseline"
            && design["paired_comparison"]["higher_is_better_difference"] == "candidate - baseline",
        "paired comparison direction drifted",
    )?;
    let multiplicity = &design["multiplicity"];
    require(
        multiplicity["gate_policy"]
            == "each required family passes independently; no aggregate compensation"
            && multiplicity["claim_family_correction"]
                == "Holm step-down family-wise error correction"
            && multiplicity["applies_to"]
                == json!([
                    "required corpora",
                    "required languages",
                    "required relation families",
                    "primary metrics"
                ])
            && multiplicity["family_alpha"] == 0.05
            && multiplicity["uncorrected_exploratory_results_cannot_pass_claims"] == true,
        "claim-family correction or experimental-unit safeguards drifted",
    )?;
    let cold_repetitions = manifest["operations"]
        .as_array()
        .and_then(|operations| {
            operations
                .iter()
                .find(|operation| operation["id"] == "cold-full-scan")
        })
        .and_then(|operation| operation["repetitions"].as_u64())
        .ok_or_else(|| io::Error::other("cold-full-scan sample count is missing"))?;
    require(
        cold_repetitions == 30,
        "cold full-scan launch count drifted",
    )?;
    let decisions = &manifest["decision_functions"];
    let correctness = &decisions["correctness"];
    require(
        correctness["method"] == "Wilson score interval"
            && correctness["confidence"] == 0.95
            && correctness["minimum_positive_examples_per_family"] == 20
            && correctness["minimum_negative_examples_per_family"] == 20
            && correctness["precision_floor"] == 0.95
            && correctness["recall_floor"] == 0.90
            && correctness["semantic_precision_floor"] == 0.90
            && correctness["semantic_recall_floor"] == 0.80
            && correctness["decision"]
                == "every advertised family lower confidence bound meets its floor",
        "correctness interval, denominator, floor, or family gate drifted",
    )?;
    let non_inferiority = &decisions["non_inferiority"];
    require(
        non_inferiority["performance_ratio_upper"] == 1.05
            && non_inferiority["rss_ratio_upper"] == 1.05
            && non_inferiority["bytes_ratio_upper"] == 1.05
            && non_inferiority["agent_quality_difference_lower"] == 0.0
            && non_inferiority["compatibility_required"] == true,
        "non-inferiority or compatibility gate drifted",
    )?;
    let superiority = &decisions["superiority"];
    require(
        superiority["cold_index_per_corpus_ratio_upper"] == 1.10
            && superiority["cold_index_geometric_mean_ratio_upper"] == 0.80
            && superiority["peak_rss_geometric_mean_ratio_upper"] == 0.80
            && superiority["structural_retrieval_p95_ratio_upper_exclusive"] == 1.0
            && superiority["agent_quality_point_estimate_difference_lower"] == 0.05
            && superiority["agent_quality_corrected_bound_lower_exclusive"] == 0.0
            && superiority["minimum_improved_pairs"] == 12,
        "dimension-specific performance, RSS, retrieval, or agent-quality gate drifted",
    )?;
    require(
        decisions["absolute_budget"]["decision"]
            == "observed p95 or exact byte count is at or below the preregistered limit"
            && decisions["phase_exit"]["decision"]
                == "all required cells are present, eligible, compatible, deterministic, contained, and pass their independent decision; missing or ineligible is failure",
        "absolute-budget or fail-closed phase-exit decision drifted",
    )
}

/// ARRI-2.10: pre-register statistical decisions before results.
#[test]
fn statistics_contract_preregisters_decisions() -> Result<(), Box<dyn Error>> {
    let manifest = evaluation_manifest()?;
    validate_statistics_contract(&manifest)?;

    let mutations = [
        ("/experiment_design/rng/version", json!("1")),
        ("/experiment_design/block_order", json!("host-width order")),
        ("/experiment_design/minimum_valid_pairs", json!(9)),
        ("/experiment_design/reruns", json!("keep the fastest rerun")),
        (
            "/decision_functions/superiority/cold_index_geometric_mean_ratio_upper",
            json!(0.95),
        ),
        (
            "/decision_functions/superiority/agent_quality_point_estimate_difference_lower",
            json!(0.0),
        ),
        (
            "/decision_functions/correctness/minimum_negative_examples_per_family",
            json!(0),
        ),
    ];
    for (pointer, replacement) in mutations {
        let mut changed = manifest.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or_else(|| io::Error::other(format!("mutation pointer is missing: {pointer}")))? =
            replacement;
        require(
            validate_statistics_contract(&changed).is_err(),
            format!("statistical contract accepted mutation {pointer}"),
        )?;
    }
    let mut changed = manifest;
    let cold = changed["operations"]
        .as_array_mut()
        .and_then(|operations| {
            operations
                .iter_mut()
                .find(|operation| operation["id"] == "cold-full-scan")
        })
        .ok_or_else(|| io::Error::other("cold-full-scan mutation target is missing"))?;
    cold["repetitions"] = json!(29);
    require(
        validate_statistics_contract(&changed).is_err(),
        "statistical contract accepted 29 cold launches",
    )
}

/// ARRI-2.11: freeze boundary-specific raw latency goals.
#[test]
fn latency_contract_has_absolute_boundary_goals() -> Result<(), Box<dyn Error>> {
    let manifest = evaluation_manifest()?;
    let goals = manifest["latency_goals"]
        .as_array()
        .ok_or_else(|| io::Error::other("latency_goals is not an array"))?;
    let actual = goals
        .iter()
        .map(|goal| {
            let id = goal["id"]
                .as_str()
                .ok_or_else(|| io::Error::other("latency goal lacks ID"))?;
            let value = goal["p95_goal_ms"]
                .as_u64()
                .ok_or_else(|| io::Error::other("latency goal lacks p95_goal_ms"))?;
            require(
                goal["tolerance_factor"]
                    .as_f64()
                    .is_some_and(|factor| factor <= 1.25),
                format!("latency goal {id} exceeds tolerance 1.25"),
            )?;
            Ok((id.to_string(), value))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    require(
        actual
            == BTreeMap::from([
                ("mcp-simple-warm".to_string(), 50),
                ("mcp-three-hop-warm".to_string(), 150),
                ("service-three-hop-warm".to_string(), 50),
                ("sqlite-simple-warm".to_string(), 1),
            ]),
        format!("latency goals drifted: {actual:?}"),
    )?;
    require(
        manifest["calibration"]["divide_latency_by_calibration"] == false
            && manifest["calibration"]["tolerance_factor"] == 1.25
            && manifest["calibration"]["state"] == "preregistered-pilot-pending",
        "calibration policy normalizes latency or claims an unrun pilot",
    )?;
    let calibration_workloads = manifest["calibration"]["eligible_workloads"]
        .as_array()
        .ok_or_else(|| io::Error::other("calibration workloads are not an array"))?;
    require(
        calibration_workloads.len() == 2
            && calibration_workloads.iter().all(|workload| {
                workload["repetitions"] == 15
                    && workload["timeout_seconds"] == 120
                    && workload["baseline_median_ns"].is_null()
                    && workload["eligible_ratio_min"] == 0.8
                    && workload["eligible_ratio_max"] == 1.25
            }),
        "calibration workload repetitions, timeout, envelope, or pending result drifted",
    )?;
    let eligibility = &manifest["calibration"]["benchmark_eligibility"];
    require(
        eligibility["required_positions"] == json!(["before", "after"])
            && eligibility["same_environment_identity_required"] == true
            && eligibility["same_test_executable_sha256_required"] == true
            && eligibility["all_attempts_retained"] == true
            && eligibility["all_attempts_must_succeed"] == true
            && eligibility["every_workload_median_must_remain_inside_frozen_envelope"] == true,
        "before/after calibration eligibility does not fail closed",
    )?;
    require(
        string_set(&manifest["timing_decomposition"])?
            == BTreeSet::from([
                "deserialization".to_string(),
                "process-runtime-state".to_string(),
                "response-write".to_string(),
                "serialization".to_string(),
                "service".to_string(),
                "sqlite".to_string(),
                "transport".to_string(),
            ]),
        "MCP timing decomposition is incomplete",
    )
}

#[test]
fn repository_intelligence_host_safety_identity_and_states_are_durable()
-> Result<(), Box<dyn Error>> {
    validate_host_safety_states(&host_safety()?)
}

/// ARRI-2.26: unavailable native provisioning remains visible and release-blocking.
#[test]
fn arri_2_26_host_capability_truth_table_rejects_hidden_manual_steps() -> Result<(), Box<dyn Error>>
{
    let policy = host_safety()?;
    validate_host_command_evidence(&policy)?;
    validate_plugin_store_lifecycle(&policy)?;

    let mut stale_command = policy.clone();
    stale_command["commands"][0]["output_sha256"] = json!("stale");
    require(
        validate_host_command_evidence(&stale_command).is_err(),
        "stale host command digest was accepted",
    )?;
    let mut stale_source = policy.clone();
    stale_source["source_artifacts"][0]["sha256"] = json!("stale");
    require(
        validate_plugin_store_lifecycle(&stale_source).is_err(),
        "stale host source digest was accepted",
    )?;
    require(
        canonical_source_digest(b"line-one\r\nline-two\r\n")?
            == canonical_source_digest(b"line-one\nline-two\n")?,
        "source digest changed across host line endings",
    )?;
    require(
        canonical_source_digest(b"line-one\rline-two")?
            != canonical_source_digest(b"line-one\nline-two")?,
        "source digest normalized a bare carriage return",
    )?;
    require(
        canonical_source_digest(&[0xff]).is_err(),
        "non-UTF-8 source artifact was accepted",
    )?;
    let mut raw_source_digest = policy.clone();
    raw_source_digest["source_artifacts"][0]["digest_mode"] = json!("raw-bytes");
    require(
        validate_plugin_store_lifecycle(&raw_source_digest).is_err(),
        "host-dependent raw source digest was accepted",
    )?;
    let mut malformed = policy.clone();
    malformed["plugin_store_lifecycle"]["hosts"][0]["evidence_state"] = json!("assumed");
    require(
        validate_plugin_store_lifecycle(&malformed).is_err(),
        "undeclared plugin evidence state was accepted",
    )?;
    let mut missing = policy.clone();
    missing["plugin_store_lifecycle"]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("plugin lifecycle is not an object"))?
        .remove("native_install_provisioning");
    require(
        validate_plugin_store_lifecycle(&missing).is_err(),
        "missing native provisioning state was accepted",
    )?;
    let mut stale = policy.clone();
    stale["plugin_store_lifecycle"]["hosts"][0]["version"] = json!("");
    require(
        validate_plugin_store_lifecycle(&stale).is_err(),
        "stale host version was accepted",
    )?;
    let mut hidden_blocker = policy;
    hidden_blocker["plugin_store_lifecycle"]["gate_state"] = json!("pass");
    require(
        validate_plugin_store_lifecycle(&hidden_blocker).is_err(),
        "unavailable one-action install was marked passing",
    )
}

/// ARRI-2.27: one concrete owner preserves project data and fails closed.
#[test]
fn arri_2_27_lifecycle_owner_is_single_typed_and_preserving() -> Result<(), Box<dyn Error>> {
    let policy = host_safety()?;
    validate_host_command_evidence(&policy)?;
    validate_host_lifecycle_owner(&policy)?;

    let mut malformed = policy.clone();
    malformed["host_lifecycle_ownership"]["owner"]["new_crate"] = json!("false");
    require(
        validate_host_lifecycle_owner(&malformed).is_err(),
        "stringly typed lifecycle ownership was accepted",
    )?;
    let mut missing = policy.clone();
    missing["host_lifecycle_ownership"]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("host lifecycle is not an object"))?
        .remove("owner");
    require(
        validate_host_lifecycle_owner(&missing).is_err(),
        "missing lifecycle owner was accepted",
    )?;
    let mut stale = policy.clone();
    stale["host_lifecycle_ownership"]["owner"]["status"] = json!("implemented");
    require(
        validate_host_lifecycle_owner(&stale).is_err(),
        "unverified implemented lifecycle status was accepted",
    )?;
    let mut hidden_blocker = policy;
    hidden_blocker["host_lifecycle_ownership"]["gate_state"] = json!("pass");
    require(
        validate_host_lifecycle_owner(&hidden_blocker).is_err(),
        "unimplemented lifecycle recovery was marked passing",
    )
}

/// ARRI-2.28: owned safety and transitive boundary evidence stays fail-closed.
#[test]
fn arri_2_28_safety_inventory_fails_closed_on_advisory_or_missing_evidence()
-> Result<(), Box<dyn Error>> {
    let policy = host_safety()?;
    let current = current_safety_evidence()?;
    validate_host_command_evidence(&policy)?;
    validate_safety_inventory(&policy, &current)?;

    let mut malformed = policy.clone();
    malformed["unsafe_native_ffi_inventory"]["containment"]["evidence_state"] =
        json!("not-checked");
    require(
        validate_safety_inventory(&malformed, &current).is_err(),
        "undeclared safety evidence state was accepted",
    )?;
    let mut missing = policy.clone();
    missing["unsafe_native_ffi_inventory"]["advisories"] = json!([]);
    require(
        validate_safety_inventory(&missing, &current).is_err(),
        "missing advisory remediation evidence was accepted",
    )?;
    let mut stale = policy.clone();
    stale["unsafe_native_ffi_inventory"]["advisories"][0]["candidate_lockfile_sha256"] =
        json!("stale");
    require(
        validate_safety_inventory(&stale, &current).is_err(),
        "stale advisory lockfile evidence was accepted",
    )?;
    let mut missing_boundary = policy.clone();
    missing_boundary["unsafe_native_ffi_inventory"]["native_boundaries"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("native boundaries are not an array"))?
        .retain(|row| row["id"] != "processkit-development-process-supervision");
    require(
        validate_safety_inventory(&missing_boundary, &current).is_err(),
        "missing development process-supervision boundary was accepted",
    )?;
    let mut hidden_scope = policy.clone();
    hidden_scope["unsafe_native_ffi_inventory"]["dependency_graph"]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("dependency graph is not an object"))?
        .remove("boundary_inventory_scope");
    require(
        validate_safety_inventory(&hidden_scope, &current).is_err(),
        "preliminary boundary inventory was presented as complete transitive proof",
    )?;
    let mut hidden_blocker = policy;
    hidden_blocker["unsafe_native_ffi_inventory"]["containment"]["gate_state"] = json!("pass");
    require(
        validate_safety_inventory(&hidden_blocker, &current).is_err(),
        "absent process containment was marked passing",
    )
}
