//! Task-specific unit coverage for Rust quality policy and evidence validation.

use super::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::error::Error as _;
use std::path::Path;

/// Assert a fallible test setup step and stop the current test after reporting failure.
macro_rules! assert_ok {
    ($expression:expr) => {{
        let result = $expression;
        assert!(
            result.is_ok(),
            "operation failed: {:?}",
            result.as_ref().err()
        );
        let Ok(value) = result else { return };
        value
    }};
}

/// Collect test-function names from one parsed Rust source tree.
fn collect_test_functions(items: &[syn::Item], names: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Fn(function)
                if function
                    .attrs
                    .iter()
                    .any(|attribute| attribute.path().is_ident("test")) =>
            {
                names.insert(function.sig.ident.to_string());
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_test_functions(nested, names);
                }
            }
            _ => {}
        }
    }
}

/// Parse all test functions reachable from the declared Cargo test target.
fn target_test_functions(
    root: &RepositoryRoot,
    arguments: &[String],
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let sources = if arguments
        .windows(2)
        .any(|pair| pair == ["--bin", "cargo-projectatlas-lints"])
    {
        vec![
            "crates/projectatlas-lints/src/main.rs",
            "crates/projectatlas-lints/src/test_quality_tests.rs",
        ]
    } else if arguments.windows(2).any(|pair| pair == ["--test", "e2e"]) {
        vec!["crates/projectatlas-cli/tests/e2e.rs"]
    } else {
        return Err(std::io::Error::other(format!(
            "unsupported Cargo task-test target: {arguments:?}"
        ))
        .into());
    };

    let mut names = BTreeSet::new();
    for relative in sources {
        let path = root.input(relative)?;
        let source = read_text(&path)?;
        let parsed = syn::parse_file(&source)?;
        collect_test_functions(&parsed.items, &mut names);
    }
    Ok(names)
}

fn workspace_root() -> Result<RepositoryRoot, Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root
        .to_str()
        .ok_or_else(|| std::io::Error::other("workspace path must be UTF-8"))?;
    Ok(RepositoryRoot::open(root, Path::new("."))?)
}

fn policy(root: &RepositoryRoot) -> Result<QualityPolicy, Box<dyn std::error::Error>> {
    let path = root.input("test-quality.toml")?;
    Ok(read_toml(&path)?)
}

fn policy_failure(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
) -> Result<String, Box<dyn std::error::Error>> {
    match validate_policy(root, policy) {
        Err(error) => Ok(error.to_string()),
        Ok(()) => Err(std::io::Error::other("mutated policy must fail").into()),
    }
}

fn coverage_exception(
    root: &RepositoryRoot,
    id: &str,
    start_line: u64,
    end_line: u64,
) -> Result<QualityException, Box<dyn std::error::Error>> {
    let source = root.input("Cargo.toml")?;
    Ok(QualityException::Coverage {
        id: id.to_string(),
        path: "Cargo.toml".to_string(),
        start_line,
        end_line,
        category: ExceptionCategory::ToolLimitation,
        rationale: "Focused fixture for exact exception validation.".to_string(),
        owner: "maintainers".to_string(),
        tracking_issue: "#309".to_string(),
        approved_by: "release-owner".to_string(),
        approved_on: "2026-07-11".to_string(),
        source_sha256: digest_file(&source)?,
        expires_on: Some("2999-01-01".to_string()),
        expires_release: None,
    })
}

#[test]
fn task_tqg_ut_1_1() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    assert_ok!(validate_policy(&root, &policy));

    policy.tools.cargo_nextest = "0.9.139".to_string();
    policy.scope.exclude_globs.push("crates/**".to_string());
    let failure = assert_ok!(policy_failure(&root, &policy));
    assert!(failure.contains("cargo_nextest must be 0.9.140"));
    assert!(failure.contains("scope.exclude_globs must remain empty"));
}

#[test]
fn task_tqg_ut_1_2() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    let observed = &policy.observed.coverage;
    assert_eq!(
        (
            observed.lines_covered,
            observed.lines_total,
            observed.regions_covered,
            observed.regions_total,
            observed.functions_covered,
            observed.functions_total,
            observed.missed_lines,
        ),
        (24_130, 27_495, 34_041, 40_094, 2_045, 2_369, 3_365)
    );
    assert_eq!(
        (
            policy.historical.nextest.test_count,
            policy.historical.nextest.suite_count,
            policy.historical.nextest.ignored_count,
        ),
        (286, 9, 0)
    );
    assert!(!policy.historical.nextest.eligible_floor_evidence);
    assert!(!policy.observed.coverage.eligible_floor_evidence);

    policy.observed.coverage.lines_total += 1;
    policy.historical.nextest.eligible_floor_evidence = true;
    let failure = assert_ok!(policy_failure(&root, &policy));
    assert!(failure.contains("historical nextest snapshot"));
    assert!(failure.contains("observed coverage derived counts"));
}

#[test]
fn task_tqg_ut_1_3() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    assert_eq!(policy.historical.mutation_inventory.total, 4_911);
    assert_eq!(policy.observed.mutation_inventory.total, 4_931);
    assert_eq!(
        policy.historical.mutation_inventory.packages.total(),
        Some(4_911)
    );
    assert_eq!(
        policy.observed.mutation_inventory.packages.total(),
        Some(4_931)
    );
    assert_eq!(policy.observed.mutation_inventory.historical_drift, 20);
    assert!(!policy.observed.mutation_inventory.skip_calls_defaults);

    policy.observed.mutation_inventory.historical_drift = 19;
    assert!(assert_ok!(policy_failure(&root, &policy)).contains("mutation drift arithmetic"));
}

#[test]
fn task_tqg_ut_1_4() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    for target in [
        &policy.targets.coverage.lines,
        &policy.targets.coverage.regions,
        &policy.targets.coverage.functions,
    ] {
        assert!(target.raw_basis_points > 0);
        assert!(target.adjusted_basis_points > 0);
        assert!(target.tracking_issue > 0);
    }
    assert!(policy.targets.mutation.tracking_issue > 0);
    assert!(!policy.target_policy.target_gap_waivers_allowed);

    policy.targets.coverage.lines.tracking_issue = 0;
    policy.target_policy.target_gap_waivers_allowed = true;
    let failure = assert_ok!(policy_failure(&root, &policy));
    assert!(failure.contains("coverage.lines.tracking_issue must be nonzero"));
    assert!(failure.contains("target_policy must prohibit gap waivers"));
}

#[test]
fn task_tqg_ut_1_5() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    assert!(policy.timeouts.values().iter().all(|(_, value)| *value > 0));
    assert!(policy.retention.artifact_days >= policy.retention.release_decision_window_days);

    policy.timeouts.coverage_command_seconds = policy.timeouts.coverage_job_seconds + 1;
    policy.retention.artifact_days = policy.retention.release_decision_window_days - 1;
    let failure = assert_ok!(policy_failure(&root, &policy));
    assert!(failure.contains("coverage command timeout exceeds job timeout"));
    assert!(failure.contains("artifact retention must cover"));
}

#[test]
fn task_tqg_ut_1_6() {
    let root = assert_ok!(workspace_root());
    let valid = assert_ok!(coverage_exception(&root, "coverage-cargo-1", 1, 2));
    let mut errors = Vec::new();
    assert_ok!(validate_exceptions(
        &root,
        "v0.4.0-00",
        std::slice::from_ref(&valid),
        &mut errors,
    ));
    assert!(errors.is_empty(), "valid exception failed: {errors:?}");

    let mut expired = valid.clone();
    assert!(matches!(expired, QualityException::Coverage { .. }));
    let QualityException::Coverage { expires_on, .. } = &mut expired else {
        return;
    };
    *expires_on = Some("2000-01-01".to_string());
    let overlapping = assert_ok!(coverage_exception(&root, "coverage-cargo-2", 2, 3));
    let mut errors = Vec::new();
    assert_ok!(validate_exceptions(
        &root,
        "v0.4.0-00",
        &[expired, valid, overlapping],
        &mut errors,
    ));
    assert!(errors.iter().any(|error| error.contains("is expired")));
    assert!(
        errors
            .iter()
            .any(|error| error.contains("coverage exceptions overlap"))
    );
}

#[test]
fn task_tqg_ut_1_7() {
    let root = assert_ok!(workspace_root());
    let cargo_path = assert_ok!(root.input("Cargo.toml"));
    let cargo = assert_ok!(read_text(&cargo_path));
    let lint_manifest_path = assert_ok!(root.input("crates/projectatlas-lints/Cargo.toml"));
    let lint_manifest = assert_ok!(read_text(&lint_manifest_path));
    let validator_path = assert_ok!(root.input("crates/projectatlas-lints/src/test_quality.rs"));
    let validator = assert_ok!(read_text(&validator_path));
    let ci_path = assert_ok!(root.input(".github/workflows/ci.yml"));
    let ci = assert_ok!(read_text(&ci_path));

    assert!(cargo.contains("unsafe_code = \"forbid\""));
    assert!(!cargo.contains("projectatlas-test-quality"));
    assert!(!lint_manifest.contains("projectatlas-test-quality"));
    assert!(!validator.contains("unsafe {"));
    assert!(!ci.to_ascii_lowercase().contains("tarpaulin"));
}

#[test]
fn task_tqg_ut_3_1() {
    let parsed = assert_ok!(FixedArgs::parse(&[
        "--root".to_string(),
        ".".to_string(),
        "--policy".to_string(),
        "test-quality.toml".to_string(),
        "--json".to_string(),
    ]));
    assert_eq!(assert_ok!(parsed.required_one("--root")), ".");
    assert!(parsed.json);

    let duplicate = assert_ok!(FixedArgs::parse(&[
        "--root".to_string(),
        ".".to_string(),
        "--root".to_string(),
        ".".to_string(),
    ]));
    assert!(matches!(
        duplicate.required_one("--root"),
        Err(QualityError::Usage(_))
    ));
}

#[test]
fn task_tqg_ut_3_2() {
    let statuses = [
        QualityStatus::Passed,
        QualityStatus::MissingTool,
        QualityStatus::NoTests,
        QualityStatus::NoMutants,
        QualityStatus::TestFailure,
        QualityStatus::BaselineFailure,
        QualityStatus::MissedMutant,
        QualityStatus::MutantTimeout,
        QualityStatus::CommandTimeout,
        QualityStatus::JobTimeout,
        QualityStatus::Cancelled,
        QualityStatus::CorruptEvidence,
        QualityStatus::IncompleteEvidence,
        QualityStatus::StaleEvidence,
        QualityStatus::PolicyFailure,
        QualityStatus::InfrastructureFailure,
    ];
    let codes = statuses
        .iter()
        .map(|status| status.exit_code())
        .collect::<BTreeSet<_>>();
    assert_eq!(codes.len(), statuses.len());
    for status in statuses {
        let encoded = assert_ok!(serde_json::to_string(&status));
        let decoded: QualityStatus = assert_ok!(serde_json::from_str(&encoded));
        assert_eq!(decoded, status);
    }
    assert!(serde_json::from_str::<QualityStatus>("\"future-status\"").is_err());
}

#[test]
fn task_tqg_ut_3_3() {
    let root = assert_ok!(workspace_root());
    assert!(matches!(
        root.input("../Cargo.toml"),
        Err(QualityError::PathEscape(_))
    ));
    assert!(matches!(
        root.input("crates"),
        Err(QualityError::PathEscape(_))
    ));

    let temporary = assert_ok!(tempfile::tempdir());
    let temporary_path = assert_ok!(
        temporary
            .path()
            .to_str()
            .ok_or_else(|| std::io::Error::other("temporary path must be UTF-8"))
    );
    let wrong = RepositoryRoot::open(temporary_path, Path::new("."));
    assert!(matches!(wrong, Err(QualityError::WrongRoot(_))));
}

#[test]
fn task_tqg_ut_3_4() {
    let nextest: NativeNextestInventory = assert_ok!(serde_json::from_value(json!({
        "test-count": 1,
        "rust-suites": {
            "suite": {
                "status": "listed",
                "testcases": {"case": {"kind": "test", "ignored": false}}
            }
        },
        "future-field": true
    })));
    assert_eq!(nextest.test_count, 1);
    assert_eq!(nextest.rust_suites.len(), 1);

    let llvm: LlvmCoverageExport = assert_ok!(serde_json::from_value(json!({
        "data": [],
        "type": "llvm.coverage.json.export",
        "version": "2.0.1",
        "cargo_llvm_cov": {"version": "0.8.7", "manifest_path": "Cargo.toml"},
        "future-field": true
    })));
    assert!(llvm.data.is_empty());

    let mutant: NativeMutant = assert_ok!(serde_json::from_value(json!({
        "name": "replace value",
        "package": "projectatlas-core",
        "file": "crates/projectatlas-core/src/lib.rs",
        "function": null,
        "span": {
            "start": {"line": 1, "column": 1},
            "end": {"line": 1, "column": 2}
        },
        "replacement": "0",
        "genre": "FnValue",
        "future-field": true
    })));
    assert_eq!(mutant.package, "projectatlas-core");
    assert!(serde_json::from_value::<NativeNextestInventory>(json!({})).is_err());
    assert!(serde_json::from_value::<NativeMutant>(json!({"name": "truncated"})).is_err());
}

#[test]
fn task_tqg_ut_3_5() {
    let directory = assert_ok!(tempfile::tempdir());
    let first = directory.path().join("first.txt");
    let second = directory.path().join("second.txt");
    assert_ok!(std::fs::write(&first, b"evidence-a"));
    assert_ok!(std::fs::write(&second, b"evidence-b"));
    let first_digest = assert_ok!(digest_file(&first));
    let second_digest = assert_ok!(digest_file(&second));
    assert_ne!(first_digest, second_digest);
    assert_ok!(validate_digest(&first_digest, "first fixture"));
    assert_ok!(validate_commit("c672442438404411389ef86e2efd767f3a4b2be0"));
    assert!(validate_commit("C672442438404411389EF86E2EFD767F3A4B2BE0").is_err());
}

#[test]
fn task_tqg_ut_3_6() {
    let raw = MetricCounts::new(9, 10);
    let adjusted = MetricCounts::new(9, 9);
    assert_eq!(assert_ok!(raw.basis_points()), 9_000);
    assert_eq!(assert_ok!(adjusted.basis_points()), 10_000);
    assert!(!raw.meets(9_500));
    assert!(adjusted.meets(9_500));
    assert!(MetricCounts::new(11, 10).validate().is_err());
    assert!(MetricCounts::new(0, 0).validate().is_err());
}

#[test]
fn task_tqg_ut_3_7() {
    let root = assert_ok!(workspace_root());
    let base = assert_ok!(policy(&root));
    let mut current = assert_ok!(policy(&root));
    assert_ok!(validate_policy_ratchet(&base, &current));

    current.targets.coverage.lines.raw_basis_points -= 1;
    current.targets.mutation.adjusted_viable_kill_basis_points -= 1;
    current.scope.include_globs.clear();
    let failure = validate_policy_ratchet(&base, &current);
    assert!(failure.is_err(), "lowered policy must fail");
    let Err(failure) = failure else { return };
    let failure = failure.to_string();
    assert!(failure.contains("coverage target lines was lowered"));
    assert!(failure.contains("mutation target was lowered"));
    assert!(failure.contains("owned source scope lost include glob"));
}

#[test]
fn task_tqg_ut_3_8() {
    let invalid_json = serde_json::from_str::<serde_json::Value>("{");
    assert!(invalid_json.is_err(), "fixture JSON must be invalid");
    let Err(invalid_json) = invalid_json else {
        return;
    };
    let failures = [
        QualityError::Usage("bad command".to_string()),
        QualityError::WrongRoot("wrong root".to_string()),
        QualityError::Json {
            path: PathBuf::from("fixture.json"),
            source: Box::new(invalid_json),
        },
        QualityError::Policy(vec!["policy".to_string()]),
        QualityError::Evidence("evidence".to_string()),
        QualityError::Status {
            status: QualityStatus::NoTests,
            message: "zero runnable tests".to_string(),
        },
    ];
    let statuses = failures
        .iter()
        .map(QualityError::status)
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![
            QualityStatus::InfrastructureFailure,
            QualityStatus::InfrastructureFailure,
            QualityStatus::CorruptEvidence,
            QualityStatus::PolicyFailure,
            QualityStatus::IncompleteEvidence,
            QualityStatus::NoTests,
        ]
    );
    assert!(failures[2].source().is_some());
    assert_eq!(failures[0].exit_code(), EXIT_USAGE);
    assert_eq!(failures[5].exit_code(), QualityStatus::NoTests.exit_code());
}

/// Ensure every completed Rust task command runs one real test rather than succeeding with zero tests.
#[test]
fn verification_plan_cargo_filters_resolve_once() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root()?;
    let tasks = parse_openspec_tasks(&read_text(
        &root.input("openspec/changes/enforce-rust-test-quality-gates/tasks.md")?,
    )?)?;
    let completed = tasks
        .iter()
        .filter(|task| task.checked)
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    let path = root.input("openspec/task-verification.json")?;
    let plan: VerificationPlan = read_json(&path)?;
    let change = plan
        .changes
        .get("enforce-rust-test-quality-gates")
        .ok_or_else(|| std::io::Error::other("quality-gate verification change is missing"))?;
    let mut failures = Vec::new();
    let mut catalogs = BTreeMap::<Vec<String>, BTreeSet<String>>::new();

    for task in &change.tasks {
        if task.command.executable != "cargo" || !completed.contains(task.task_id.as_str()) {
            continue;
        }
        let filter = format!("task_tqg_ut_{}", task.task_id.replace('.', "_"));
        let catalog = if let Some(catalog) = catalogs.get(&task.command.arguments) {
            catalog
        } else {
            let catalog = target_test_functions(&root, &task.command.arguments)?;
            catalogs.insert(task.command.arguments.clone(), catalog);
            catalogs
                .get(&task.command.arguments)
                .ok_or_else(|| std::io::Error::other("test catalog insertion failed"))?
        };
        let matches = usize::from(catalog.contains(&filter));
        if matches != 1 {
            failures.push(format!(
                "task {} filter {filter} resolves to {matches} tests in {:?}",
                task.task_id, task.command.arguments
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(std::io::Error::other(failures.join("\n")).into())
    }
}

/// Keep the Rust validator aligned with declared behavioral test anchors.
#[test]
fn verification_plan_accepts_declared_task_test_anchors() -> Result<(), Box<dyn std::error::Error>>
{
    let root = workspace_root()?;
    let tasks = parse_openspec_tasks(&read_text(
        &root.input("openspec/changes/advance-rust-repository-intelligence/tasks.md")?,
    )?)?;
    let plan: VerificationPlan = read_json(&root.input("openspec/task-verification.json")?)?;
    let change = plan
        .changes
        .get("advance-rust-repository-intelligence")
        .ok_or_else(|| std::io::Error::other("feature verification change is missing"))?;
    for task_id in ["1.1", "2.1"] {
        let parsed = tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| std::io::Error::other(format!("task {task_id} is missing")))?;
        let planned = change
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| {
                std::io::Error::other(format!("verification task {task_id} is missing"))
            })?;
        validate_verification_task(&root, parsed, planned)?;
    }
    Ok(())
}

/// Keep metadata-only task closure narrow enough to preserve commit binding.
#[test]
fn task_evidence_metadata_paths_are_narrow() {
    assert!(task_evidence_metadata_path("openspec/task-evidence.json"));
    assert!(task_evidence_metadata_path(
        "openspec/changes/advance-rust-repository-intelligence/tasks.md"
    ));
    assert!(task_evidence_metadata_path(
        "docs/benchmarks/results/phase-0-truth-and-baselines/task-verification-a95a9de.json"
    ));
    assert!(!task_evidence_metadata_path("openspec/tasks.md"));
    assert!(!task_evidence_metadata_path(
        "openspec/changes/advance-rust-repository-intelligence/design.md"
    ));
    assert!(!task_evidence_metadata_path(
        "openspec/changes/advance-rust-repository-intelligence/nested/tasks.md"
    ));
    assert!(!task_evidence_metadata_path(
        "openspec/changes/-invalid/tasks.md"
    ));
    assert!(!task_evidence_metadata_path(
        "openspec/task-verification.json"
    ));
    assert!(!task_evidence_metadata_path(
        "docs/benchmarks/results/phase-0-truth-and-baselines/reviews.json"
    ));
    assert!(!task_evidence_metadata_path(
        "crates/projectatlas-core/src/lib.rs"
    ));
}
