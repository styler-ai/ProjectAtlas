//! Materialize one truthful initial verification row for every v0.4 `OpenSpec` task.

use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

const PLAN: &str = include_str!("../../../openspec/task-verification-plan.json");
const EXECUTABLE_PLAN: &str = include_str!("../../../openspec/task-verification.json");
const ISSUE_MAP: &str = include_str!("../../../openspec/issue-map.json");
const PROPOSAL: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/proposal.md");
const DESIGN: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/design.md");
const INTELLIGENCE_TASKS: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/tasks.md");
const QUALITY_TASKS: &str =
    include_str!("../../../openspec/changes/enforce-rust-test-quality-gates/tasks.md");
const PLANNING_COMMIT: &str = "c672442438404411389ef86e2efd767f3a4b2be0";
const INTELLIGENCE_CHANGE: &str = "advance-rust-repository-intelligence";
const INTELLIGENCE_SPECIFICATIONS: [(&str, &str); 7] = [
    (
        "cross-repository-intelligence",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/cross-repository-intelligence/spec.md"
        ),
    ),
    (
        "graph-retrieval-and-analysis",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/graph-retrieval-and-analysis/spec.md"
        ),
    ),
    (
        "incremental-index-integrity",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/incremental-index-integrity/spec.md"
        ),
    ),
    (
        "language-intelligence-registry",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/language-intelligence-registry/spec.md"
        ),
    ),
    (
        "local-semantic-retrieval",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/local-semantic-retrieval/spec.md"
        ),
    ),
    (
        "repository-intelligence-benchmarks",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/repository-intelligence-benchmarks/spec.md"
        ),
    ),
    (
        "repository-knowledge-graph",
        include_str!(
            "../../../openspec/changes/advance-rust-repository-intelligence/specs/repository-knowledge-graph/spec.md"
        ),
    ),
];

/// Parsed identity and acceptance text for one authoritative `OpenSpec` task.
#[derive(Debug)]
struct TaskDefinition {
    change: &'static str,
    task_id: String,
    description: String,
    test_id: String,
}

/// Generate and validate all initial task-evidence rows from compact plan defaults.
#[test]
fn every_v04_task_materializes_one_complete_initial_evidence_row() -> Result<(), Box<dyn Error>> {
    let plan: Value = serde_json::from_str(PLAN)?;
    let tasks = authoritative_tasks()?;
    let rows = materialize_rows(&plan, &tasks)?;
    require(rows.len() == tasks.len(), "materialized row count drifted")?;

    let mut identities = BTreeSet::new();
    let authoritative = tasks
        .iter()
        .map(|task| (format!("{}:{}", task.change, task.task_id), task))
        .collect::<BTreeMap<_, _>>();
    let required_fields = string_array(&plan["row_schema"]["required_fields"])?;
    let unit_fields = string_array(&plan["row_schema"]["unit_test_required_fields"])?;
    let result_fields = string_array(&plan["row_schema"]["result_required_fields"])?;
    for row in &rows {
        let identity = format!(
            "{}:{}",
            required_string(row, "change")?,
            required_string(row, "task_id")?
        );
        require(
            identities.insert(identity.clone()),
            format!("duplicate materialized row {identity}"),
        )?;
        let task = authoritative
            .get(&identity)
            .ok_or_else(|| io::Error::other(format!("unknown materialized row {identity}")))?;
        require(
            row["unit_test"]["test_id"] == task.test_id,
            format!("{identity} test ID drifted"),
        )?;
        require_fields(row, &required_fields, &identity)?;
        require_fields(
            &row["unit_test"],
            &unit_fields,
            &format!("{identity}.unit_test"),
        )?;
        require_fields(
            &row["result"],
            &result_fields,
            &format!("{identity}.result"),
        )?;
        require(
            !required_string(row, "requirement")?.trim().is_empty()
                && !required_string(row, "scenario")?.trim().is_empty(),
            format!("{identity} lacks requirement or scenario text"),
        )?;
        require(
            row["changed_artifacts"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
                && row["required_evidence_layers"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                && row["timeout_seconds"]
                    .as_u64()
                    .is_some_and(|value| value > 0),
            format!("{identity} lacks artifacts, evidence layers, or a timeout"),
        )?;
        let unit_state = required_string(&row["unit_test"], "state")?;
        match unit_state {
            "implemented_uncommitted" => {
                require(
                    row["unit_test"]["function"]
                        .as_str()
                        .is_some_and(|function| !function.trim().is_empty())
                        && row["unit_test"]["command"]["executable"]
                            .as_str()
                            .is_some_and(|executable| !executable.trim().is_empty())
                        && row["unit_test"]["command"]["arguments"].is_array(),
                    format!("{identity} implemented unit test lacks an executable command"),
                )?;
            }
            "definition_pending_stable_implementation" | "planned_not_implemented" => {
                require(
                    row["unit_test"]["function"].is_null() && row["unit_test"]["command"].is_null(),
                    format!("{identity} pending unit test fabricates a function or command"),
                )?;
            }
            other => {
                return Err(io::Error::other(format!(
                    "{identity} has unsupported initial unit-test state {other}"
                ))
                .into());
            }
        }
        let result_state = required_string(&row["result"], "state")?;
        require(
            matches!(result_state, "not_started" | "pending_commit_bound_run")
                && [
                    "implementation_commit",
                    "covered_input_digest",
                    "run_identity",
                    "artifact_digest",
                ]
                .iter()
                .all(|field| row["result"][field].is_null()),
            format!("{identity} fabricates successful or commit-bound run evidence"),
        )?;
    }
    require(
        identities.len() == tasks.len(),
        "not every v0.4 task has one materialized row",
    )?;
    implemented_rows_match_executable_plan(&rows)
}

/// Prove the complete planning package remains present and internally linked.
#[test]
fn task_arri_ut_arri_1_1() -> Result<(), Box<dyn Error>> {
    for heading in [
        "## Context",
        "## Goals / Non-Goals",
        "## Decisions",
        "## Phased Delivery",
        "## Risks / Trade-offs",
        "### Pre-Mortem",
        "## Acceptance Gate Summary",
    ] {
        require(DESIGN.contains(heading), format!("design lacks {heading}"))?;
    }
    let mut capabilities = BTreeSet::new();
    for (capability, specification) in INTELLIGENCE_SPECIFICATIONS {
        require(
            capabilities.insert(capability),
            format!("duplicate capability specification {capability}"),
        )?;
        require(
            PROPOSAL.contains(&format!("- `{capability}`:")),
            format!("proposal does not declare {capability}"),
        )?;
        require(
            specification.starts_with("## ADDED Requirements")
                && specification.contains("### Scenario:"),
            format!("{capability} lacks requirements or scenarios"),
        )?;
    }
    require(
        INTELLIGENCE_TASKS.contains("[UT:ARRI-1.1]"),
        "planning checklist lacks ARRI-1.1",
    )
}

/// Bind the original `ProjectAtlas` issue setup to its immutable planning commit.
#[test]
fn task_arri_ut_arri_1_2() -> Result<(), Box<dyn Error>> {
    let task_source = git_output(&[
        "show",
        &format!("{PLANNING_COMMIT}:openspec/changes/{INTELLIGENCE_CHANGE}/tasks.md"),
    ])?;
    let issue_map = git_output(&[
        "show",
        &format!("{PLANNING_COMMIT}:openspec/issue-map.json"),
    ])?;
    require(
        task_source.contains("Create one ProjectAtlas GitHub feature issue")
            && task_source.contains("implementation has not started")
            && !task_source.contains("github.com/yoanbernabeu"),
        "initial issue contract is missing or names a foreign source",
    )?;
    let mapping: Value = serde_json::from_str(&issue_map)?;
    require(
        mapping["changes"][INTELLIGENCE_CHANGE] == 308,
        "initial planning commit did not bind issue 308",
    )
}

/// Keep the current change mapped to its evidence-v2 issue authority.
#[test]
fn task_arri_ut_arri_1_3() -> Result<(), Box<dyn Error>> {
    let mapping: Value = serde_json::from_str(ISSUE_MAP)?;
    let change = &mapping["changes"][INTELLIGENCE_CHANGE];
    require(
        change["contract"] == "evidence-v2" && change["primary_issue"] == 308,
        "repository-intelligence issue mapping drifted",
    )?;
    let owners = required_array(change, "owners")?;
    require(
        owners.len() == 2
            && owners[0]["issue"] == 308
            && owners[1]["issue"] == 311
            && owners[0]["last_task"] == "8.25"
            && owners[1]["first_task"] == "9.1",
        "repository-intelligence issue ownership is not deterministic and disjoint",
    )
}

/// Prove every authoritative task maps to exactly one ordered issue owner.
#[test]
fn task_arri_ut_arri_1_4() -> Result<(), Box<dyn Error>> {
    let mapping: Value = serde_json::from_str(ISSUE_MAP)?;
    let owners = required_array(&mapping["changes"][INTELLIGENCE_CHANGE], "owners")?;
    let tasks = authoritative_tasks()?
        .into_iter()
        .filter(|task| task.change == INTELLIGENCE_CHANGE)
        .collect::<Vec<_>>();
    let mut previous = None;
    for task in tasks {
        let ordinal = task_ordinal(&task.task_id)?;
        require(
            previous.is_none_or(|value| value < ordinal),
            format!("task {} is out of order", task.task_id),
        )?;
        previous = Some(ordinal);
        let matches = owners
            .iter()
            .filter(|owner| {
                task_in_range(
                    &task.task_id,
                    owner["first_task"].as_str().unwrap_or_default(),
                    owner["last_task"].as_str().unwrap_or_default(),
                )
                .unwrap_or(false)
            })
            .count();
        require(
            matches == 1,
            format!("task {} maps to {matches} issue owners", task.task_id),
        )?;
    }
    Ok(())
}

/// Keep strict `OpenSpec` and `IssueOps` validation commands executable and exact.
#[test]
fn task_arri_ut_arri_1_5() -> Result<(), Box<dyn Error>> {
    let plan: Value = serde_json::from_str(PLAN)?;
    let commands = required_array(&plan["planning_validation"], "commands")?;
    let expected = json!([
        {
            "executable": "openspec",
            "arguments": ["validate", INTELLIGENCE_CHANGE, "--strict", "--no-interactive"]
        },
        {
            "executable": "python",
            "arguments": [".github/scripts/issue-checklists.py", "--self-test", "--root", "."]
        }
    ]);
    require(
        Value::Array(commands.to_vec()) == expected,
        "planning validation command contract drifted",
    )
}

/// Verify the immutable planning commit contains only publishable planning artifacts.
#[test]
fn task_arri_ut_arri_1_6() -> Result<(), Box<dyn Error>> {
    let output = git_output(&[
        "diff-tree",
        "--no-commit-id",
        "--name-only",
        "-r",
        PLANNING_COMMIT,
    ])?;
    let paths = output
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    require(!paths.is_empty(), "planning commit has no public artifacts")?;
    for path in &paths {
        require(
            *path == "openspec/issue-map.json"
                || path.starts_with("openspec/changes/advance-rust-repository-intelligence/")
                || path.starts_with("openspec/changes/enforce-rust-test-quality-gates/"),
            format!("planning commit contains non-planning artifact {path}"),
        )?;
        require(
            !Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
                && !path.starts_with("Cargo")
                && !path.starts_with("crates/")
                && !path.starts_with("fixtures/")
                && !path.starts_with(".github/"),
            format!("planning commit contains implementation artifact {path}"),
        )?;
        let contents = git_output(&["show", &format!("{PLANNING_COMMIT}:{path}")])?;
        for token in contents
            .split_whitespace()
            .filter(|token| token.contains("github.com/"))
        {
            require(
                token.contains("github.com/styler-ai/ProjectAtlas"),
                format!("planning artifact {path} links a foreign GitHub source"),
            )?;
        }
    }
    require(
        paths.iter().any(|path| path.ends_with("/proposal.md"))
            && paths.iter().any(|path| path.ends_with("/design.md"))
            && paths.iter().any(|path| path.ends_with("/tasks.md"))
            && paths.iter().any(|path| path.contains("/specs/")),
        "planning commit lacks a proposal, design, tasks, or specifications",
    )
}

/// Ensure compact materialization rejects missing ownership and duplicate explicit rows.
#[test]
fn task_evidence_materialization_rejects_contract_gaps() -> Result<(), Box<dyn Error>> {
    let plan: Value = serde_json::from_str(PLAN)?;
    let tasks = authoritative_tasks()?;

    let mut missing_owner = plan.clone();
    missing_owner["owner_ranges"] = Value::Array(Vec::new());
    require(
        materialize_rows(&missing_owner, &tasks).is_err(),
        "missing owner ranges produced rows",
    )?;

    let mut duplicate_override = plan;
    let duplicate = duplicate_override["stable_row_overrides"]
        .as_array()
        .and_then(|rows| rows.first())
        .cloned()
        .ok_or_else(|| io::Error::other("stable override fixture is missing"))?;
    duplicate_override["planned_row_definitions"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("planned rows are not an array"))?
        .push(duplicate);
    require(
        materialize_rows(&duplicate_override, &tasks).is_err(),
        "duplicate explicit rows were accepted",
    )
}

/// Parse both authoritative task files without copying their task text into the plan.
fn authoritative_tasks() -> Result<Vec<TaskDefinition>, Box<dyn Error>> {
    let task_pattern = Regex::new(r"(?m)^- \[[ x]\] (?P<task>\d+\.\d+) (?P<description>.+)$")?;
    let test_id_pattern = Regex::new(r"(?:UT:ARRI-|TQG-UT-)\d+\.\d+")?;
    let mut tasks = Vec::new();
    for (change, source) in [
        ("advance-rust-repository-intelligence", INTELLIGENCE_TASKS),
        ("enforce-rust-test-quality-gates", QUALITY_TASKS),
    ] {
        for captures in task_pattern.captures_iter(source) {
            let description = captures["description"].trim();
            let test_ids = test_id_pattern
                .find_iter(description)
                .map(|matched| matched.as_str().to_string())
                .collect::<Vec<_>>();
            require(
                test_ids.len() == 1,
                format!(
                    "{change}:{} declares {} task-specific test identifiers",
                    &captures["task"],
                    test_ids.len()
                ),
            )?;
            tasks.push(TaskDefinition {
                change,
                task_id: captures["task"].to_string(),
                description: description.to_string(),
                test_id: test_ids.into_iter().next().ok_or_else(|| {
                    io::Error::other("validated task-specific test identifier is missing")
                })?,
            });
        }
    }
    Ok(tasks)
}

/// Project authoritative tasks and compact owner defaults into complete initial rows.
fn materialize_rows(plan: &Value, tasks: &[TaskDefinition]) -> Result<Vec<Value>, Box<dyn Error>> {
    let owner_ranges = required_array(plan, "owner_ranges")?;
    let implemented_ranges = required_array(plan, "implemented_range_definitions")?;
    let materialization = &plan["row_materialization"];
    require(
        materialization["strategy"] == "authoritative-task-text-plus-owner-range-defaults",
        "task row materialization strategy drifted",
    )?;
    let scenario_template = materialization["scenario_template"]
        .as_str()
        .ok_or_else(|| io::Error::other("scenario template is missing"))?;
    let initial_unit_state = materialization["initial_unit_test_state"]
        .as_str()
        .ok_or_else(|| io::Error::other("initial unit-test state is missing"))?;
    let initial_result_state = materialization["initial_result_state"]
        .as_str()
        .ok_or_else(|| io::Error::other("initial result state is missing"))?;
    let timeouts = materialization["timeout_seconds_by_risk"]
        .as_object()
        .ok_or_else(|| io::Error::other("risk timeout map is missing"))?;
    let overrides = explicit_rows(plan)?;

    let mut rows = Vec::with_capacity(tasks.len());
    for task in tasks {
        let identity = format!("{}:{}", task.change, task.task_id);
        if let Some(row) = overrides.get(&identity) {
            require(
                row["unit_test"]["test_id"] == task.test_id,
                format!("{identity} override test ID drifted"),
            )?;
            rows.push((*row).clone());
            continue;
        }

        let matching_implemented_ranges = implemented_ranges
            .iter()
            .filter(|range| {
                range["change"] == task.change
                    && task_in_range(
                        &task.task_id,
                        range["first_task"].as_str().unwrap_or_default(),
                        range["last_task"].as_str().unwrap_or_default(),
                    )
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        require(
            matching_implemented_ranges.len() <= 1,
            format!(
                "{identity} maps to {} implemented ranges",
                matching_implemented_ranges.len()
            ),
        )?;
        if let [range] = matching_implemented_ranges.as_slice() {
            let test_suffix = task
                .test_id
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let filter = format!(
                "{}{}",
                required_string(&range["unit_test"], "filter_prefix")?,
                test_suffix
            );
            let function = format!(
                "{}{}",
                required_string(&range["unit_test"], "function_prefix")?,
                test_suffix
            );
            let description = task
                .description
                .strip_suffix(&format!(" [{}]", task.test_id))
                .unwrap_or(&task.description);
            let mut characters = description.chars();
            let assertion = match characters.next() {
                Some(first) => format!(
                    "Verify {}{}",
                    first.to_ascii_lowercase(),
                    characters.as_str()
                ),
                None => {
                    return Err(io::Error::other(format!("{identity} description is empty")).into());
                }
            };
            let mut arguments = string_array(&range["unit_test"]["command_arguments_prefix"])?;
            arguments.push(filter);
            rows.push(json!({
                "change": task.change,
                "task_id": task.task_id,
                "requirement": task.description,
                "scenario": format!("Complete {}:{} and verify its authoritative acceptance statement.", task.change, task.task_id),
                "owner": range["owner"],
                "changed_artifacts": range["changed_artifacts"],
                "risk": range["risk"],
                "required_evidence_layers": range["required_evidence_layers"],
                "timeout_seconds": range["timeout_seconds"],
                "unit_test": {
                    "test_id": task.test_id,
                    "function": function,
                    "command": {
                        "executable": range["unit_test"]["command_executable"],
                        "arguments": arguments
                    },
                    "assertion": assertion,
                    "covered_inputs": range["changed_artifacts"],
                    "state": range["unit_test"]["state"]
                },
                "result": range["result"]
            }));
            continue;
        }

        let matching_ranges = owner_ranges
            .iter()
            .filter(|range| {
                range["change"] == task.change
                    && task_in_range(
                        &task.task_id,
                        range["first_task"].as_str().unwrap_or_default(),
                        range["last_task"].as_str().unwrap_or_default(),
                    )
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        require(
            matching_ranges.len() == 1,
            format!("{identity} maps to {} owner ranges", matching_ranges.len()),
        )?;
        let owner = matching_ranges[0];
        let risk = owner["risk"]
            .as_str()
            .ok_or_else(|| io::Error::other(format!("{identity} risk is missing")))?;
        let timeout = timeouts
            .get(risk)
            .and_then(Value::as_u64)
            .ok_or_else(|| io::Error::other(format!("{identity} timeout is missing")))?;
        let scenario = scenario_template
            .replace("{change}", task.change)
            .replace("{task_id}", &task.task_id);
        rows.push(json!({
            "change": task.change,
            "task_id": task.task_id,
            "requirement": task.description,
            "scenario": scenario,
            "owner": owner["owner"],
            "changed_artifacts": owner["planned_artifacts"],
            "risk": risk,
            "required_evidence_layers": owner["default_layers"],
            "timeout_seconds": timeout,
            "unit_test": {
                "test_id": task.test_id,
                "function": Value::Null,
                "command": Value::Null,
                "assertion": task.description,
                "covered_inputs": owner["planned_artifacts"],
                "state": initial_unit_state
            },
            "result": {
                "state": initial_result_state,
                "implementation_commit": Value::Null,
                "covered_input_digest": Value::Null,
                "run_identity": Value::Null,
                "artifact_digest": Value::Null
            }
        }));
    }
    Ok(rows)
}

/// Reconcile every implemented authoring row with the executable `IssueOps` ledger.
fn implemented_rows_match_executable_plan(rows: &[Value]) -> Result<(), Box<dyn Error>> {
    let executable: Value = serde_json::from_str(EXECUTABLE_PLAN)?;
    for row in rows {
        if row["unit_test"]["state"] != "implemented_uncommitted" {
            continue;
        }
        let change = required_string(row, "change")?;
        let task_id = required_string(row, "task_id")?;
        let task = executable["changes"][change]["tasks"]
            .as_array()
            .and_then(|tasks| tasks.iter().find(|task| task["task_id"] == task_id))
            .ok_or_else(|| {
                io::Error::other(format!("{change}:{task_id} lacks an executable row"))
            })?;
        let test_id = required_string(&row["unit_test"], "test_id")?;
        require(
            task["test_ids"] == json!([test_id])
                && task["assertion"] == row["unit_test"]["assertion"]
                && task["command"] == row["unit_test"]["command"]
                && task["timeout_seconds"] == row["timeout_seconds"],
            format!("{change}:{task_id} authoring and executable fields drifted"),
        )?;

        let covered = required_array(task, "covered_inputs")?
            .iter()
            .filter_map(|input| input["path"].as_str())
            .collect::<BTreeSet<_>>();
        for artifact in required_array(row, "changed_artifacts")?
            .iter()
            .filter_map(Value::as_str)
            .filter(|path| {
                !path.starts_with("GitHub issue ") && !path.starts_with(".projectatlas/")
            })
        {
            require(
                covered.contains(artifact),
                format!("{change}:{task_id} does not cover authored artifact {artifact}"),
            )?;
        }

        let sources = required_array(task, "test_sources")?;
        require(
            sources.len() == 1
                && sources[0]["test_id"] == test_id
                && row["changed_artifacts"]
                    .as_array()
                    .is_some_and(|paths| paths.contains(&sources[0]["path"])),
            format!("{change}:{task_id} test source is missing or unowned"),
        )?;
        let function = required_string(&row["unit_test"], "function")?;
        let anchor = required_string(&sources[0], "anchor")?;
        require(
            function.rsplit("::").next() == Some(anchor),
            format!("{change}:{task_id} test source anchor drifted"),
        )?;
    }
    Ok(())
}

/// Collect explicit stable and late-release row definitions by task identity.
fn explicit_rows(plan: &Value) -> Result<BTreeMap<String, &Value>, Box<dyn Error>> {
    let mut rows = BTreeMap::new();
    for key in ["stable_row_overrides", "planned_row_definitions"] {
        for row in required_array(plan, key)? {
            let identity = format!(
                "{}:{}",
                required_string(row, "change")?,
                required_string(row, "task_id")?
            );
            require(
                rows.insert(identity.clone(), row).is_none(),
                format!("duplicate explicit verification row {identity}"),
            )?;
        }
    }
    Ok(rows)
}

/// Return whether one dotted numeric task ID is inside an inclusive range.
fn task_in_range(task: &str, first: &str, last: &str) -> Result<bool, Box<dyn Error>> {
    let task = task_ordinal(task)?;
    Ok(task >= task_ordinal(first)? && task <= task_ordinal(last)?)
}

/// Convert a dotted task ID into an order-preserving pair.
fn task_ordinal(task: &str) -> Result<(u16, u16), Box<dyn Error>> {
    let (section, item) = task
        .split_once('.')
        .ok_or_else(|| io::Error::other(format!("invalid task ID {task}")))?;
    Ok((section.parse()?, item.parse()?))
}

/// Return a required array field.
fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("{key} is not an array")).into())
}

/// Return a required string field.
fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    value[key]
        .as_str()
        .ok_or_else(|| io::Error::other(format!("{key} is not a string")).into())
}

/// Convert a JSON string array into an owned vector.
fn string_array(value: &Value) -> Result<Vec<String>, Box<dyn Error>> {
    value
        .as_array()
        .ok_or_else(|| io::Error::other("value is not a string array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| io::Error::other("string array contains a non-string").into())
        })
        .collect()
}

/// Require every named field to exist, including intentionally null initial values.
fn require_fields(value: &Value, fields: &[String], owner: &str) -> Result<(), Box<dyn Error>> {
    let object = value
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{owner} is not an object")))?;
    let missing = fields
        .iter()
        .filter(|field| !object.contains_key(field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    require(
        missing.is_empty(),
        format!("{owner} lacks required fields {missing:?}"),
    )
}

/// Fail a contract test with a readable error instead of panicking.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Run one fixed Git query and return its UTF-8 stdout.
fn git_output(arguments: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git").args(arguments).output()?;
    require(
        output.status.success(),
        format!(
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    )?;
    Ok(String::from_utf8(output.stdout)?.replace("\r\n", "\n"))
}
