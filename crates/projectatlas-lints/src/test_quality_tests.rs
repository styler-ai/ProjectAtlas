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

fn required_text_position(haystack: &str, needle: &str) -> Result<usize, std::io::Error> {
    haystack.find(needle).ok_or_else(|| {
        std::io::Error::other(format!("required workflow text is missing: {needle}"))
    })
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

fn under_target_coverage_export(
    root: &RepositoryRoot,
) -> Result<LlvmCoverageExport, Box<dyn std::error::Error>> {
    let source = root.input("crates/projectatlas-lints/src/test_quality_tests.rs")?;
    let manifest = root.input("Cargo.toml")?;
    let summary = || LlvmCoverageSummary {
        lines: LlvmMetric {
            count: 10,
            covered: 8,
            notcovered: Some(2),
        },
        regions: LlvmMetric {
            count: 10,
            covered: 8,
            notcovered: Some(2),
        },
        functions: LlvmMetric {
            count: 10,
            covered: 8,
            notcovered: Some(2),
        },
    };
    Ok(LlvmCoverageExport {
        data: vec![LlvmCoverageData {
            files: vec![LlvmCoverageFile {
                filename: source.to_string_lossy().into_owned(),
                segments: vec![(1, 1, 1, true, true, false)],
                summary: summary(),
            }],
            functions: vec![LlvmCoverageFunction {
                count: 1,
                filenames: vec![source.to_string_lossy().into_owned()],
                name: "under_target_fixture".to_string(),
                regions: vec![(1, 1, 1, 2, 1, 0, 0, 0)],
            }],
            totals: summary(),
        }],
        export_type: "llvm.coverage.json.export".to_string(),
        version: "2.0.1".to_string(),
        cargo_llvm_cov: LlvmCovTool {
            version: EXPECTED_LLVM_COV_VERSION.to_string(),
            manifest_path: manifest.to_string_lossy().into_owned(),
        },
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
    assert_ok!(std::fs::write(
        directory.path().join("Cargo.toml"),
        b"[workspace]\n"
    ));
    assert_ok!(std::fs::write(directory.path().join("Cargo.lock"), b""));
    assert_ok!(std::fs::write(&first, b"evidence-a"));
    assert_ok!(std::fs::write(&second, b"evidence-b"));
    let first_digest = assert_ok!(digest_file(&first));
    let second_digest = assert_ok!(digest_file(&second));
    assert_ne!(first_digest, second_digest);
    assert_ok!(validate_digest(&first_digest, "first fixture"));
    assert_ok!(validate_commit("c672442438404411389ef86e2efd767f3a4b2be0"));
    assert!(validate_commit("C672442438404411389EF86E2EFD767F3A4B2BE0").is_err());

    let root = assert_ok!(RepositoryRoot::open(
        directory.path().to_string_lossy().as_ref(),
        Path::new(".")
    ));
    let task = VerificationTask {
        task_id: "3.5".to_string(),
        test_ids: vec!["TQG-UT-3.5".to_string()],
        assertion: "Covered inputs have a canonical content-sensitive digest.".to_string(),
        command: VerificationCommand {
            executable: "cargo".to_string(),
            arguments: vec!["test".to_string()],
        },
        timeout_seconds: 120,
        covered_inputs: vec![CoveredInput {
            kind: CoveredInputKind::File,
            path: "first.txt".to_string(),
        }],
        test_sources: Vec::new(),
    };
    let covered_digest = assert_ok!(normalized_covered_inputs_digest(&root, &task));
    assert_eq!(covered_digest.len(), 64);
    assert!(covered_digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ok!(std::fs::write(&first, b"evidence-c"));
    assert_ne!(
        covered_digest,
        assert_ok!(normalized_covered_inputs_digest(&root, &task))
    );
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

    let mutation = MutationCounts {
        raw_total: 10,
        caught: 7,
        missed: 1,
        timed_out: 1,
        unviable: 1,
        excluded: 0,
        unresolved: 0,
    };
    let mut summary = BTreeMap::new();
    mutation.insert_summary(&mut summary);
    assert_eq!(
        summary,
        BTreeMap::from([
            ("adjusted_viable_kill_basis_points".to_string(), 7_777),
            ("caught".to_string(), 7),
            ("excluded".to_string(), 0),
            ("missed".to_string(), 1),
            ("raw_kill_basis_points".to_string(), 7_000),
            ("raw_total".to_string(), 10),
            ("timed_out".to_string(), 1),
            ("unresolved".to_string(), 0),
            ("unviable".to_string(), 1),
        ])
    );
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

#[test]
fn implementation_checkpoint_coverage_preserves_structure_without_claiming_release_targets() {
    let root = assert_ok!(workspace_root());
    let policy = assert_ok!(policy(&root));
    let export = assert_ok!(under_target_coverage_export(&root));
    let checkpoint = validate_coverage(
        &root,
        &policy,
        "linux-x86_64-gnu",
        &export,
        CoverageEnforcement::ImplementationCheckpoint,
    );
    assert!(
        checkpoint.is_ok(),
        "checkpoint coverage failed: {checkpoint:?}"
    );
    let release = validate_coverage(
        &root,
        &policy,
        "linux-x86_64-gnu",
        &export,
        CoverageEnforcement::ReleaseQuality,
    );
    assert!(matches!(
        release,
        Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            ..
        })
    ));
}

#[test]
fn release_aggregate_rejects_checkpoint_coverage() {
    let root = assert_ok!(workspace_root());
    let policy = assert_ok!(policy(&root));
    let counts = CoverageCounts {
        lines: MetricCounts::new(8, 10),
        regions: MetricCounts::new(8, 10),
        functions: MetricCounts::new(8, 10),
    };
    let checkpoint = GateResult::Coverage {
        enforcement: CoverageEnforcement::ImplementationCheckpoint,
        raw: counts,
        adjusted: counts,
        exceptions_used: 0,
    };
    assert_ok!(validate_gate_result(
        &policy,
        "linux-x86_64-gnu",
        &checkpoint,
        false
    ));
    assert!(validate_gate_result(&policy, "linux-x86_64-gnu", &checkpoint, true).is_err());

    let strict = GateResult::Coverage {
        enforcement: CoverageEnforcement::ReleaseQuality,
        raw: counts,
        adjusted: counts,
        exceptions_used: 0,
    };
    assert!(validate_gate_result(&policy, "linux-x86_64-gnu", &strict, false).is_err());
}

#[test]
fn coverage_targets_require_each_raw_and_adjusted_metric() {
    let root = assert_ok!(workspace_root());
    let policy = assert_ok!(policy(&root));
    let passing_metric = MetricCounts::new(100, 100);
    let passing = CoverageCounts {
        lines: passing_metric,
        regions: passing_metric,
        functions: passing_metric,
    };
    assert!(coverage_meets_targets(&policy, &passing, &passing));

    let failing_metric = MetricCounts::new(0, 100);
    let cases = [
        (
            "raw lines",
            CoverageCounts {
                lines: failing_metric,
                ..passing
            },
            passing,
        ),
        (
            "raw regions",
            CoverageCounts {
                regions: failing_metric,
                ..passing
            },
            passing,
        ),
        (
            "raw functions",
            CoverageCounts {
                functions: failing_metric,
                ..passing
            },
            passing,
        ),
        (
            "adjusted lines",
            passing,
            CoverageCounts {
                lines: failing_metric,
                ..passing
            },
        ),
        (
            "adjusted regions",
            passing,
            CoverageCounts {
                regions: failing_metric,
                ..passing
            },
        ),
        (
            "adjusted functions",
            passing,
            CoverageCounts {
                functions: failing_metric,
                ..passing
            },
        ),
    ];
    for (label, raw, adjusted) in cases {
        assert!(
            !coverage_meets_targets(&policy, &raw, &adjusted),
            "coverage accepted a failing {label} metric"
        );
    }

    let passing_result = GateResult::Coverage {
        enforcement: CoverageEnforcement::ReleaseQuality,
        raw: passing,
        adjusted: passing,
        exceptions_used: 0,
    };
    assert_ok!(validate_gate_result(
        &policy,
        "linux-x86_64-gnu",
        &passing_result,
        false
    ));

    let invalid_raw = GateResult::Coverage {
        enforcement: CoverageEnforcement::ReleaseQuality,
        raw: CoverageCounts {
            lines: MetricCounts::new(101, 100),
            ..passing
        },
        adjusted: passing,
        exceptions_used: 0,
    };
    assert!(validate_gate_result(&policy, "linux-x86_64-gnu", &invalid_raw, false).is_err());
    let invalid_adjusted = GateResult::Coverage {
        enforcement: CoverageEnforcement::ReleaseQuality,
        raw: passing,
        adjusted: CoverageCounts {
            functions: MetricCounts::new(101, 100),
            ..passing
        },
        exceptions_used: 0,
    };
    assert!(validate_gate_result(&policy, "linux-x86_64-gnu", &invalid_adjusted, false).is_err());
    let wrong_exception_count = GateResult::Coverage {
        enforcement: CoverageEnforcement::ReleaseQuality,
        raw: passing,
        adjusted: passing,
        exceptions_used: 1,
    };
    assert!(
        validate_gate_result(&policy, "linux-x86_64-gnu", &wrong_exception_count, false).is_err()
    );
}

#[test]
fn checkpoint_aggregate_enforces_established_platform_floor() {
    let root = assert_ok!(workspace_root());
    let mut policy = assert_ok!(policy(&root));
    let platform_id = "linux-x86_64-gnu";
    let platform = policy
        .platforms
        .iter_mut()
        .find(|platform| platform.id == platform_id);
    assert!(
        platform.is_some(),
        "quality policy lacks the Linux reference platform"
    );
    let Some(platform) = platform else { return };
    platform.coverage_floor_established = true;
    platform.lines_covered_floor = Some(9);
    platform.lines_total = Some(10);
    platform.regions_covered_floor = Some(9);
    platform.regions_total = Some(10);
    platform.functions_covered_floor = Some(9);
    platform.functions_total = Some(10);

    let below_floor = CoverageCounts {
        lines: MetricCounts::new(8, 10),
        regions: MetricCounts::new(8, 10),
        functions: MetricCounts::new(8, 10),
    };
    let below_floor_result = GateResult::Coverage {
        enforcement: CoverageEnforcement::ImplementationCheckpoint,
        raw: below_floor,
        adjusted: below_floor,
        exceptions_used: 0,
    };
    assert!(validate_gate_result(&policy, platform_id, &below_floor_result, false).is_err());

    let at_floor = CoverageCounts {
        lines: MetricCounts::new(9, 10),
        regions: MetricCounts::new(9, 10),
        functions: MetricCounts::new(9, 10),
    };
    let at_floor_result = GateResult::Coverage {
        enforcement: CoverageEnforcement::ImplementationCheckpoint,
        raw: at_floor,
        adjusted: at_floor,
        exceptions_used: 0,
    };
    assert_ok!(validate_gate_result(
        &policy,
        platform_id,
        &at_floor_result,
        false
    ));
}

#[test]
fn coverage_enforcement_manifest_identity_is_exact() {
    assert_eq!(
        CoverageEnforcement::ImplementationCheckpoint.manifest_name(),
        "implementation_checkpoint"
    );
    assert_eq!(
        CoverageEnforcement::ReleaseQuality.manifest_name(),
        "release_quality"
    );
    assert!(
        CoverageEnforcement::ImplementationCheckpoint
            .validate_manifest_name(Some("implementation_checkpoint"))
            .is_ok()
    );
    assert!(
        CoverageEnforcement::ImplementationCheckpoint
            .validate_manifest_name(Some("release_quality"))
            .is_err()
    );
    assert!(
        CoverageEnforcement::ReleaseQuality
            .validate_manifest_name(None)
            .is_err()
    );
}

#[test]
fn gate_manifest_rejects_ineligible_identity_before_artifact_reads() {
    let root = assert_ok!(workspace_root());
    let policy = assert_ok!(policy(&root));
    let commit = "0".repeat(40);
    let manifest: EvidenceManifest = assert_ok!(serde_json::from_value(json!({
        "schema_version": EVIDENCE_SCHEMA_VERSION + 1,
        "repository": policy.repository,
        "gate": "nextest",
        "status": "passed",
        "commit_sha": commit,
        "platform": {
            "id": "linux-x86_64-gnu",
            "os": "linux",
            "arch": "x86_64",
            "target": "x86_64-unknown-linux-gnu",
            "runner_image": "ubuntu-latest",
            "runner_image_version": "test"
        },
        "toolchain": {"rustc_version": "test", "llvm_version": "test"},
        "tool": {"name": "test", "version": "test"},
        "inputs": {
            "cargo_lock_sha256": "test",
            "policy_sha256": "test",
            "source_scope_sha256": "test",
            "configs": []
        },
        "command": {"executable": "cargo", "arguments": ["nextest"], "profile": "ci"},
        "timeouts": {
            "command_seconds": 1,
            "job_seconds": 1,
            "test_seconds": null,
            "build_seconds": null
        },
        "started_at_utc": "2026-01-01T00:00:00Z",
        "completed_at_utc": "2026-01-01T00:00:01Z",
        "run": {"kind": "repository_retained_local", "run_id": "test", "host": "test"},
        "artifacts": [],
        "result": {
            "kind": "nextest",
            "tests": 1,
            "suites": 1,
            "ignored": 0,
            "failed": 0,
            "errors": 0,
            "timed_out": 0
        }
    })));
    let result = validate_gate_manifest(
        &root,
        &policy,
        "unused-policy-digest",
        Path::new("unused-manifest.json"),
        &manifest,
        &commit,
        false,
    );
    assert!(matches!(
        result,
        Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            ..
        })
    ));
}

#[test]
fn gate_inputs_reject_stale_digests() {
    let root = assert_ok!(workspace_root());
    let policy = assert_ok!(policy(&root));
    let inputs = GateInputs {
        cargo_lock_sha256: "stale".to_string(),
        policy_sha256: "stale".to_string(),
        source_scope_sha256: "stale".to_string(),
        configs: Vec::new(),
    };
    assert!(
        validate_gate_inputs(
            &root,
            &policy,
            "current-policy-digest",
            GateKind::Doctest,
            &inputs,
        )
        .is_err()
    );
}

#[test]
fn gate_commands_require_bounded_cargo_arguments() {
    let command = GateCommand {
        executable: "shell".to_string(),
        arguments: Vec::new(),
        profile: "ci".to_string(),
    };
    assert!(validate_gate_command(GateKind::Nextest, &command).is_err());
}

#[test]
fn omitted_coverage_enforcement_remains_release_quality() {
    assert_eq!(
        assert_ok!(CoverageEnforcement::from_cli(None)),
        CoverageEnforcement::ReleaseQuality
    );
    assert_eq!(
        assert_ok!(CoverageEnforcement::from_cli(Some(
            "implementation-checkpoint"
        ))),
        CoverageEnforcement::ImplementationCheckpoint
    );
    assert!(CoverageEnforcement::from_cli(Some("future-mode")).is_err());

    let decoded: GateResult = assert_ok!(serde_json::from_value(json!({
        "kind": "coverage",
        "raw": {
            "lines": {"covered": 1, "total": 1},
            "regions": {"covered": 1, "total": 1},
            "functions": {"covered": 1, "total": 1}
        },
        "adjusted": {
            "lines": {"covered": 1, "total": 1},
            "regions": {"covered": 1, "total": 1},
            "functions": {"covered": 1, "total": 1}
        },
        "exceptions_used": 0
    })));
    assert!(matches!(
        decoded,
        GateResult::Coverage {
            enforcement: CoverageEnforcement::ReleaseQuality,
            ..
        }
    ));
}

#[test]
fn quality_workflows_bind_declared_runner_and_phase_contracts() {
    let root = assert_ok!(workspace_root());
    let quality_policy = assert_ok!(policy(&root));
    let ci = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/ci.yml")
    )));
    let mutation = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/05-full-mutation.yml")
    )));
    let release = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/release.yml")
    )));
    let failure_smoke = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/07-quality-failure-smoke.yml")
    )));
    let docs = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/04-docs.yml")
    )));
    let workflow_docs = assert_ok!(read_text(&assert_ok!(root.input("docs/workflow.md"))));

    let pinned_toolchain = format!(
        "RUSTUP_TOOLCHAIN: \"{}\"",
        quality_policy.reference_toolchain.rust
    );
    for (name, workflow) in [
        ("CI", ci.as_str()),
        ("full mutation", mutation.as_str()),
        ("release", release.as_str()),
        ("failure smoke", failure_smoke.as_str()),
        ("docs", docs.as_str()),
    ] {
        assert!(
            workflow.contains(&pinned_toolchain),
            "{name} does not pin the quality-policy Rust toolchain"
        );
        assert!(
            !workflow.contains("rustup toolchain install stable")
                && !workflow.contains("rustup default stable"),
            "{name} follows the moving stable toolchain"
        );
    }

    assert!(ci.contains("runs-on: ${{ matrix.runner_image }}"));
    for selector in [
        "runner_image: ubuntu-latest",
        "runner_image: windows-latest",
        "runner_image: macos-15-intel",
        "runner_image: macos-14",
    ] {
        assert!(ci.contains(selector), "missing runner selector {selector}");
    }
    assert!(!ci.contains("--arg runner_image \"$ImageOS\""));
    assert!(!mutation.contains("--arg runner_image \"$ImageOS\""));
    assert!(ci.contains("--arg runner_image \"$DECLARED_RUNNER_IMAGE\""));
    assert!(ci.contains("--arg runner_image \"$DECLARED_LINUX_RUNNER_IMAGE\""));
    assert!(mutation.contains("--arg runner_image \"$DECLARED_LINUX_RUNNER_IMAGE\""));

    assert!(ci.contains("trusted_git=/usr/bin/git"));
    assert!(ci.contains("trusted_git=\"$(command -v git.exe)\""));
    assert!(ci.contains("trusted_git_sha256"));
    assert!(ci.contains("filtered_path+=(\"$directory\")"));
    assert!(ci.contains("shasum -a 256 -- \"$1\""));
    assert!(ci.contains("trusted_git_sha256=\"$(sha256_file \"$trusted_git\")\""));
    let manifest_start = assert_ok!(required_text_position(
        &ci,
        "      - name: Build and validate coverage manifest",
    ));
    let manifest_end = manifest_start
        + assert_ok!(required_text_position(
            &ci[manifest_start..],
            "      - name: Record coverage outcome",
        ));
    let coverage_manifest = &ci[manifest_start..manifest_end];
    assert!(coverage_manifest.contains("sha256_file()"));
    assert!(coverage_manifest.contains("shasum -a 256 -- \"$1\""));
    assert!(coverage_manifest.contains("test_seconds:null,build_seconds:null"));
    assert!(!coverage_manifest.contains("test_seconds:120"));
    assert!(
        coverage_manifest
            .contains("--arg llvm_json_sha256 \"$(sha256_file \"$evidence/coverage.json\")\"")
    );

    let normalized_rustc_version = "--arg rustc_version \"$(rustc --version | awk '{print $2}')\"";
    assert!(ci.contains(normalized_rustc_version));
    assert!(mutation.contains(normalized_rustc_version));
    assert!(!ci.contains("--arg rustc_version \"$(rustc --version)\""));
    assert!(!mutation.contains("--arg rustc_version \"$(rustc --version)\""));
    assert!(ci.contains("tool:{name:\"rustc\",version:$rustc_version}"));
    assert!(release.contains("rustup toolchain install \"$env:RUSTUP_TOOLCHAIN\""));
    assert!(release.contains("rustup default \"$env:RUSTUP_TOOLCHAIN\""));

    assert!(ci.contains("required: true\n        type: string\n\npermissions:"));
    assert!(ci.contains("'implementation-checkpoint'"));
    assert!(ci.contains("--enforcement \"$COVERAGE_ENFORCEMENT\""));
    assert!(ci.contains("enforcement:$enforcement"));
    assert!(release.contains("coverage_enforcement: release-quality"));
    assert!(ci.contains("profile:\"doc\""));
    assert!(ci.contains("configs:[{role:\"nextest\""));
    assert!(workflow_docs.contains(
        "git diff --binary --no-ext-diff \"$base..HEAD\" -- > \"$mutation_root/source.diff\""
    ));
    assert!(workflow_docs.contains("--in-diff \"$mutation_root/source.diff\""));
    assert!(workflow_docs.contains("--output \"$mutation_root/native\""));
    assert!(!workflow_docs.contains("--in-diff \"$base..HEAD\""));
}

#[test]
fn strict_lint_review_replays_retained_repository_artifacts() {
    let root = assert_ok!(workspace_root());
    let fixture_directory = root
        .0
        .join("crates/projectatlas-db/tests/fixtures/schema-migrations");
    let review_text = assert_ok!(read_text(&assert_ok!(
        root.input(".projectatlas/projectatlas-purpose-review.json")
    )));
    let review: serde_json::Value = assert_ok!(serde_json::from_str(&review_text));
    let reviewed = review
        .get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .collect::<BTreeSet<_>>();
    let fixture_entries =
        assert_ok!(assert_ok!(std::fs::read_dir(fixture_directory)).collect::<Result<Vec<_>, _>>());
    let mut fixtures = fixture_entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("db"))
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .map(|name| format!("crates/projectatlas-db/tests/fixtures/schema-migrations/{name}"))
        .collect::<Vec<_>>();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "migration fixture inventory is empty");

    for fixture in fixtures {
        assert!(
            reviewed.contains(fixture.as_str()),
            "migration fixture is missing from reviewed-purpose replay: {fixture}"
        );
    }

    let result_directory = root
        .0
        .join("docs/benchmarks/results/phase-0-truth-and-baselines");
    let result_entries =
        assert_ok!(assert_ok!(std::fs::read_dir(result_directory)).collect::<Result<Vec<_>, _>>());
    let mut retained_results = result_entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("task-verification-")
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json")))
            .then_some(name)
        })
        .map(|name| format!("docs/benchmarks/results/phase-0-truth-and-baselines/{name}"))
        .collect::<Vec<_>>();
    retained_results.sort();
    assert!(
        !retained_results.is_empty(),
        "retained task-verification inventory is empty"
    );
    for result in retained_results {
        assert!(
            reviewed.contains(result.as_str()),
            "retained task-verification result is missing from reviewed-purpose replay: {result}"
        );
    }
}

#[test]
fn changed_mutation_preflights_candidates_before_empty_normalization() {
    let root = assert_ok!(workspace_root());
    let ci = assert_ok!(read_text(&assert_ok!(
        root.input(".github/workflows/ci.yml")
    )));
    let candidate_inventory = assert_ok!(required_text_position(
        &ci,
        "candidate_inventory=\"$RUNNER_TEMP/projectatlas-changed-mutation-candidates.json\""
    ));
    let inventory_failure = assert_ok!(required_text_position(
        &ci,
        "changed-source mutation candidate inventory failed"
    ));
    let inventory_shape = assert_ok!(required_text_position(
        &ci,
        "jq -e 'type == \"array\"' \"$candidate_inventory\""
    ));
    let candidate_count = assert_ok!(required_text_position(
        &ci,
        "candidate_count=\"$(jq -er 'length' \"$candidate_inventory\")\""
    ));
    let empty_inventory = assert_ok!(required_text_position(
        &ci,
        "cp \"$candidate_inventory\" \"$inventory\""
    ));
    let execution_failure = assert_ok!(required_text_position(
        &ci,
        "changed-source mutation execution failed"
    ));
    assert!(candidate_inventory < inventory_failure);
    assert!(inventory_failure < inventory_shape);
    assert!(inventory_shape < candidate_count);
    assert!(candidate_count < empty_inventory);
    assert!(empty_inventory < execution_failure);
    assert_eq!(ci.matches("\n          if ! cargo mutants").count(), 1);
    assert!(ci.contains("elif ! cargo mutants"));
    assert!(ci.contains("--list \\\n              --json > \"$candidate_inventory\""));
    assert!(ci.contains("if (( candidate_count == 0 )); then"));
    assert!(ci.contains("cargo-mutants produced an incomplete native result"));
    assert!(ci.contains("cargo_mutants_version:\"27.1.0\""));
    assert!(!ci.contains("cargo-mutants omitted native output"));
    assert!(!ci.contains("^crates/[^/]+/src/(.*/)?[^/]+\\.rs$"));
    assert!(!ci.contains("cargo mutants || true"));
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
