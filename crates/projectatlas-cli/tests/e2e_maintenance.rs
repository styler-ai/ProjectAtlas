//! Purpose: Validate purpose, lint, telemetry, and terminal maintenance contracts.
#![allow(unused_imports)]

mod support;
use assert_cmd::Command;
use predicates::prelude::*;
#[cfg(feature = "optional-parser-supervisor")]
use projectatlas_cli::optional_parser_lifecycle::OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH;
#[cfg(all(debug_assertions, feature = "optional-parser-supervisor"))]
use projectatlas_cli::optional_parser_lifecycle::OptionalParserPackLifecycle;
#[cfg(all(debug_assertions, feature = "optional-parser-supervisor"))]
use projectatlas_cli::parser_supervisor::{
    ParserSupervisorError, install_currentness_test_hook, install_pre_spawn_test_hook,
};
#[cfg(all(debug_assertions, feature = "optional-parser-supervisor"))]
use projectatlas_core::IndexCancellation;
use projectatlas_core::graph::{
    Completeness, ConfidenceClass, CoverageRecord, CoverageScope, CoverageState, EntitySelector,
    ExtendedRelationKind, ExternalSelector, GraphEntity, GraphIdentityText, GraphLimitKind,
    GraphRelationKind, LogicalRelation, PackageSelector, RelationResolution, RepositoryFilePath,
    RepositoryNodePath,
};
use projectatlas_core::language::{BROAD_SOURCE_EXTENSIONS, detect_language_for_path};
#[cfg(all(
    debug_assertions,
    feature = "optional-parser-supervisor",
    target_os = "linux"
))]
use projectatlas_core::optional_parser_pack::OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION;
#[cfg(feature = "optional-parser-supervisor")]
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_ID, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES, OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES,
    OPTIONAL_PARSER_PACK_MAX_FILE_BYTES, OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES, PackRelativePath,
};
#[cfg(all(debug_assertions, feature = "optional-parser-supervisor"))]
use projectatlas_core::optional_parser_protocol::{PARSER_MAX_OUTPUT_BYTES, ParserRequestLimits};
use projectatlas_core::relation_capabilities::{RELATION_FAMILY_CAPABILITIES, RelationFamilyState};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_core::telemetry::{
    READ_AVOIDANCE_CONFIDENCE_MODELED, READ_AVOIDANCE_SCOPE,
    TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE, TOKEN_BASELINE_DIRECTORY_WALK, usage_from_estimates,
};
use projectatlas_core::{NodeKind, PurposeSource, normalize_native_path_display};
use projectatlas_db::{
    AtlasStore, HealthResolution, IndexedFileText, PlannerStatisticsPolicy, PlannerStatisticsState,
    RepositoryGraphRelationQuery, TelemetryCheckpointState,
};
use projectatlas_fs::{FsError, ScanOptions, scan_repo};
use ratatui::buffer::CellWidth;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
#[cfg(any(windows, feature = "optional-parser-supervisor"))]
use std::ffi::OsStr;
#[cfg(all(target_os = "macos", feature = "optional-parser-supervisor"))]
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as StdCommand, Stdio};
use std::sync::mpsc::{self, Receiver};
#[cfg(all(debug_assertions, feature = "optional-parser-supervisor"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};
use support::{
    GIT_REPOSITORY_ENVIRONMENT_VARIABLES, McpDatabaseSnapshot, complete_mcp_test_after_shutdown,
    git_command_for_root, json_at, json_summary_command, mcp_contract_executable,
    mcp_database_snapshot, mcp_tool_text, require_json_array_len, require_json_bool,
    require_json_contains, require_json_string, require_json_usize, require_json_usize_at_least,
    require_json_usize_greater_than, run_mcp_stdio, run_mcp_stdio_with_env, sha256_hex,
    sqlite_table_digests, workspace_root,
};
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";

const SRC_DIR_NAME: &str = "src";

const TESTS_DIR_NAME: &str = "tests";

const ATLAS_DIR_NAME: &str = ".projectatlas";

const MISSING_INDEX_DIR_NAME: &str = "missing-index";

const GITHOOKS_DIR_NAME: &str = ".githooks";

const PRE_PUSH_HOOK_FILE_NAME: &str = "pre-push";

const AGENT_INTEGRATION_DOC_FILE_NAME: &str = "agent-integration.md";

const WORKFLOW_DOC_FILE_NAME: &str = "workflow.md";

const OPTIONAL_PARSER_PACK_WORKFLOW_FILE_NAME: &str = "optional-parser-pack.yml";

const DOCS_WORKFLOW_FILE_NAME: &str = "04-docs.yml";

const AUTO_RELEASE_WORKFLOW_FILE_NAME: &str = "03-auto-release.yml";

const CARGO_LOCK_FILE_NAME: &str = "Cargo.lock";

const WRONG_PROJECT_OWNER_DIR_NAME: &str = "wrong-owner";

#[test]
fn token_tui_cli_respects_selected_terminal_viewport() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), "fn main() {}\n")?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    run_scan(&repo, &database)?;
    let before = mcp_database_snapshot(&database)?;

    for (columns, rows, arguments, required) in [
        (
            40,
            4,
            vec!["token", "--view", "tui"],
            "ProjectAtlas Token Impact",
        ),
        (
            40,
            8,
            vec!["token", "--session", "界", "--view", "tui"],
            "界",
        ),
        (80, 49, vec!["token", "--view", "tui"], "Average avoided:"),
        (
            80,
            50,
            vec!["token", "--view", "tui"],
            "A V E R A G E   T O K E N S   A V O I D E D",
        ),
        (
            40,
            4,
            vec!["token", "--view", "tui", "--trend", "month"],
            "ProjectAtlas Token Trends",
        ),
        (
            80,
            29,
            vec!["token", "--view", "tui", "--trend", "month"],
            "ProjectAtlas Token Trends",
        ),
        (
            80,
            30,
            vec!["token", "--view", "tui", "--trend", "month"],
            "S A V E D   T O K E N S   T R E N D",
        ),
    ] {
        let output = Command::new(mcp_contract_executable())
            .current_dir(&repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .env("COLUMNS", columns.to_string())
            .env("LINES", rows.to_string())
            .arg("--db")
            .arg(&database)
            .args(arguments)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "bounded token TUI failed at {columns}x{rows}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        let dashboard = String::from_utf8(output.stdout)?;
        require_tui_output_within_viewport(&dashboard, columns, rows)?;
        if !strip_ansi_csi_sequences(&dashboard).contains(required) {
            return Err(io::Error::other(format!(
                "bounded token TUI omitted {required:?} at {columns}x{rows}"
            ))
            .into());
        }
    }

    let wide_short = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .env("COLUMNS", "200")
        .env("LINES", "8")
        .arg("--db")
        .arg(&database)
        .args(["token", "--view", "tui"])
        .output()?;
    if !wide_short.status.success() {
        return Err(io::Error::other("wide, short token TUI failed").into());
    }
    let wide_short = String::from_utf8(wide_short.stdout)?;
    require_tui_output_within_viewport(&wide_short, 200, 8)?;
    if strip_ansi_csi_sequences(&wide_short).contains("ATLAS MAP") {
        return Err(io::Error::other("wide, short compact TUI rendered the Atlas panel").into());
    }

    for (columns, rows) in [("0", "0"), ("invalid", "invalid")] {
        let output = Command::new(mcp_contract_executable())
            .current_dir(&repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .env("COLUMNS", columns)
            .env("LINES", rows)
            .arg("--db")
            .arg(&database)
            .args(["token", "--view", "tui"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "invalid terminal fallback failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        let dashboard = String::from_utf8(output.stdout)?;
        require_tui_output_within_viewport(&dashboard, 140, 50)?;
    }

    // Rust stdout treats Unix EBADF as absent stdio; a closed pipe reports portable BrokenPipe.
    let (rejected_output_reader, rejected_output_writer) = io::pipe()?;
    drop(rejected_output_reader);
    let rejected_output = StdCommand::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .env("COLUMNS", "40")
        .env("LINES", "4")
        .arg("--db")
        .arg(&database)
        .args(["token", "--view", "tui"])
        .stdout(Stdio::from(rejected_output_writer))
        .stderr(Stdio::piped())
        .output()?;
    if rejected_output.status.success() || rejected_output.stderr.is_empty() {
        return Err(io::Error::other(
            "token TUI did not propagate a rejected stdout write through the CLI error boundary",
        )
        .into());
    }

    if mcp_database_snapshot(&database)? != before {
        return Err(io::Error::other("token TUI viewport checks mutated SQLite state").into());
    }
    Ok(())
}

#[test]
fn repository_guidance_keeps_atlas_state_local_and_legacy_export_optional()
-> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let ci_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join("ci.yml"),
    )?;
    let release_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join("release.yml"),
    )?;
    let parser_pack_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join(OPTIONAL_PARSER_PACK_WORKFLOW_FILE_NAME),
    )?;
    let docs_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join(DOCS_WORKFLOW_FILE_NAME),
    )?;
    let auto_release_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join(AUTO_RELEASE_WORKFLOW_FILE_NAME),
    )?;
    let pre_push = fs::read_to_string(
        workspace_root
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
    )?;
    let cli_manifest = fs::read_to_string(
        workspace_root
            .join("crates")
            .join("projectatlas-cli")
            .join("Cargo.toml"),
    )?;
    if !cli_manifest
        .lines()
        .any(|line| line.trim() == "default-run = \"projectatlas\"")
    {
        return Err(io::Error::other(
            "projectatlas-cli must keep the public CLI as Cargo's default binary",
        )
        .into());
    }
    let readme = fs::read_to_string(workspace_root.join("README.md"))?;
    let agent_integration = fs::read_to_string(
        workspace_root
            .join("docs")
            .join(AGENT_INTEGRATION_DOC_FILE_NAME),
    )?;
    let public_docs_index = fs::read_to_string(workspace_root.join("docs").join("index.md"))?;
    let gitignore = fs::read_to_string(workspace_root.join(".gitignore"))?;
    for required in [
        "Rust-native, high-performance local repository intelligence",
        "persistent SQLite map",
        "native Rust CLI and MCP server",
        "purposes identify the responsible area",
        "graph relationships reveal connected code",
        "compact summaries and outlines",
        "exact source slices provide the final evidence",
        "docs/design/ani-mascot-reference.png",
        "docs/agent-integration.md#runtime-installation-and-repair",
        "docs/agent-integration.md#token-reporting-and-human-tui",
        "docs/agent-integration.md#mcp-tool-sequence",
    ] {
        if !readme.contains(required) {
            return Err(io::Error::other(format!(
                "README must retain the consolidated Rust-native agent-first positioning; missing {required:?}"
            ))
            .into());
        }
    }
    let about_claim = "Every file not opened. Every folder not explored. ProjectAtlas guides coding agents with purpose metadata and an intelligent code graph, reducing token costs by over 90%.";
    let about_qualification = "The \"over 90%\" figure is a workload-specific local estimate from the published audit, not a universal savings guarantee or provider-billing result; see [One Large-Application Audit](#one-large-application-audit).";
    if readme.matches(about_claim).count() != 1
        || !readme.contains(&format!("{about_claim}\n\n{about_qualification}"))
    {
        return Err(io::Error::other(
            "README must retain the exact requested About claim once with its adjacent audit qualification",
        )
        .into());
    }
    for required in [
        "### Runtime installation and repair",
        "### Token reporting and human TUI",
        "remain in the exact source ledger",
        "rather than adding standalone bar panels",
    ] {
        if !agent_integration.contains(required) {
            return Err(io::Error::other(format!(
                "agent integration guide must own the linked setup and TUI behavior; missing {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "separate proportional bars for observed and modeled file reads avoided",
        "retained in the exact source ledger and navigation composition",
        "at wide terminal sizes, a bounded Atlas map",
    ] {
        if !public_docs_index.contains(required) {
            return Err(io::Error::other(format!(
                "public docs index must match the live token TUI; missing {required:?}"
            ))
            .into());
        }
    }
    for historical in ["frozen v0.3.26", "plain control", "3.9 times the median"] {
        if readme.contains(historical) {
            return Err(io::Error::other(format!(
                "README must not restore the historical navigation comparison {historical:?}"
            ))
            .into());
        }
    }
    for required in [
        "projectatlas token --view tui",
        "docs/assets/token-impact-tui.png",
    ] {
        if !readme.contains(required) {
            return Err(io::Error::other(format!(
                "README must show the human TUI product example; missing {required:?}"
            ))
            .into());
        }
    }
    let readme_words = readme.split_whitespace().count();
    if readme_words > 1_400 {
        return Err(io::Error::other(format!(
            "README landing page regressed into an operator-manual wall of text: {readme_words} words"
        ))
        .into());
    }
    for (workflow_name, workflow, job, first_product_build) in [
        ("ci", &ci_workflow, "rust", "cargo check "),
        (
            "release",
            &release_workflow,
            "verify",
            "cargo check --workspace",
        ),
    ] {
        let proof_job = workflow_job_block(workflow, job)?;
        if proof_job.contains("projectatlas.toon") || proof_job.contains("map --force") {
            return Err(io::Error::other(format!(
                "{workflow_name} {job} job must not require the legacy committed TOON map artifact"
            ))
            .into());
        }
        if proof_job.contains("--strict-folders") {
            return Err(io::Error::other(format!(
                "{workflow_name} {job} job must not require legacy folder .purpose linting"
            ))
            .into());
        }
        if !proof_job.contains("projectatlas-lints") || !proof_job.contains("strict-strings") {
            return Err(io::Error::other(format!(
                "{workflow_name} {job} job must run repository source policy lints"
            ))
            .into());
        }
        let source_policy_position = proof_job.find("strict-strings").ok_or_else(|| {
            io::Error::other(format!(
                "{workflow_name} {job} job has no repository source policy position"
            ))
        })?;
        let workspace_build_position = proof_job.find(first_product_build).ok_or_else(|| {
            io::Error::other(format!(
                "{workflow_name} {job} job has no expected product build {first_product_build:?}"
            ))
        })?;
        if source_policy_position > workspace_build_position {
            return Err(io::Error::other(format!(
                "{workflow_name} must reject private source before compiling the workspace"
            ))
            .into());
        }
        for forbidden in ["private-path-range", "select-private-history-range.py"] {
            if workflow.contains(forbidden) {
                return Err(io::Error::other(format!(
                    "{workflow_name} must not restore obsolete source-history policy {forbidden:?}"
                ))
                .into());
            }
        }
        for run in workflow_job_runs(workflow, job)? {
            if command_runs_projectatlas_maintenance(&run) {
                return Err(io::Error::other(format!(
                    "{workflow_name} {job} job must keep ProjectAtlas init, scan, purpose, parity, and lint maintenance local"
                ))
                .into());
            }
        }
    }
    for (workflow_name, workflow, job, first_build) in [
        (
            "optional parser pack",
            &parser_pack_workflow,
            "construct",
            "cargo metadata",
        ),
        ("documentation", &docs_workflow, "deploy", "cargo doc"),
    ] {
        let build_job = workflow_job_block(workflow, job)?;
        if !build_job.contains("projectatlas-lints") || !build_job.contains("strict-strings") {
            return Err(io::Error::other(format!(
                "{workflow_name} build must run repository source policy lints"
            ))
            .into());
        }
        let source_policy_position = build_job.find("strict-strings").ok_or_else(|| {
            io::Error::other(format!(
                "{workflow_name} build has no repository source policy position"
            ))
        })?;
        let first_build_position = build_job.find(first_build).ok_or_else(|| {
            io::Error::other(format!(
                "{workflow_name} build has no expected build command {first_build:?}"
            ))
        })?;
        if source_policy_position > first_build_position {
            return Err(io::Error::other(format!(
                "{workflow_name} must reject private source before product build commands"
            ))
            .into());
        }
        for forbidden in ["private-path-range", "select-private-history-range.py"] {
            if build_job.contains(forbidden) {
                return Err(io::Error::other(format!(
                    "{workflow_name} must not restore obsolete source-history policy {forbidden:?}"
                ))
                .into());
            }
        }
    }
    if !pre_push.contains("projectatlas-lints") || !pre_push.contains("strict-strings") {
        return Err(io::Error::other(
            "pre-push must run the same repository source policy lint as hosted builds",
        )
        .into());
    }
    let pre_push_source_policy = pre_push
        .find("strict-strings")
        .ok_or_else(|| io::Error::other("pre-push has no repository source policy position"))?;
    let pre_push_build = pre_push
        .find("cargo check ")
        .ok_or_else(|| io::Error::other("pre-push has no selected Rust check position"))?;
    if pre_push_source_policy > pre_push_build {
        return Err(io::Error::other(
            "pre-push must reject private source before selected Rust compilation",
        )
        .into());
    }
    for forbidden in ["private-path-updates", "private-path-range"] {
        if pre_push.contains(forbidden) {
            return Err(io::Error::other(format!(
                "pre-push must not restore obsolete source-history policy {forbidden:?}"
            ))
            .into());
        }
    }
    if !auto_release_workflow.contains("promotion_sha=\"$(git rev-parse 'HEAD^{commit}')\"")
        || !auto_release_workflow.contains("[[ \"$promotion_sha\" != \"$GITHUB_SHA\" ]]")
        || !auto_release_workflow.contains("--ref main")
        || auto_release_workflow.contains("HEAD^2")
    {
        return Err(io::Error::other("auto-release must preserve promotion identity").into());
    }
    for maintenance in ["init", "scan", "purpose", "parity", "lint"] {
        for command in [
            format!("projectatlas {maintenance}"),
            format!("cargo run --locked -p projectatlas-cli -- {maintenance}"),
        ] {
            if !command_runs_projectatlas_maintenance(&command) {
                return Err(io::Error::other(format!(
                    "workflow policy failed to recognize local-only ProjectAtlas maintenance command {command:?}"
                ))
                .into());
            }
        }
    }
    for command in [
        "cargo test --locked -p projectatlas-cli --test e2e_lifecycle",
        "cargo run --locked -p projectatlas-cli -- runtime-info",
        "./projectatlas --format json runtime-info",
    ] {
        if command_runs_projectatlas_maintenance(command) {
            return Err(io::Error::other(format!(
                "workflow policy rejected allowed ProjectAtlas verification command {command:?}"
            ))
            .into());
        }
    }
    if ci_workflow.contains("\n  install-smoke:") {
        return Err(io::Error::other(
            "CI must not launch an installed ProjectAtlas runtime; installer behavior belongs to isolated Rust E2E tests",
        )
        .into());
    }

    let guidance_paths = [
        "templates/AGENTS.md",
        "plugins/projectatlas/skills/projectatlas/SKILL.md",
        "docs/workflow.md",
        "docs/adoption.md",
        "docs/agent-integration.md",
        "docs/projectatlas-3-architecture.md",
        "docs/projectatlas-3-v0.3.2-hardening-spec.md",
    ];
    let mandatory_export_phrases = [
        "scan` or `projectatlas map --force",
        "Run `projectatlas map --force`.",
        "cargo run -p projectatlas-cli -- map --force",
        "lint validates that the map is current",
        "Map is stale",
        "Generate the map",
        "Regenerate `.projectatlas/projectatlas.toon`",
        "lint --strict-folders",
        "PROJECTATLAS_SKIP_UPDATE",
    ];
    for path in guidance_paths {
        let text = fs::read_to_string(workspace_root.join(path))?;
        for phrase in mandatory_export_phrases {
            if text.contains(phrase) {
                return Err(io::Error::other(format!(
                    "{path} must not make the legacy TOON map export part of normal setup, CI, or lint behavior; found {phrase:?}"
                ))
                .into());
            }
        }
    }
    for path in ["docs/workflow.md", "docs/adoption.md"] {
        let text = fs::read_to_string(workspace_root.join(path))?;
        if !text.contains("Optional compatibility map export") {
            return Err(io::Error::other(format!(
                "{path} must describe the static TOON map as an optional compatibility export"
            ))
            .into());
        }
    }
    if !gitignore
        .lines()
        .any(|line| line == ".projectatlas/projectatlas.toon")
    {
        return Err(
            io::Error::other("legacy ProjectAtlas TOON map artifact must stay ignored").into(),
        );
    }
    if !gitignore
        .lines()
        .any(|line| line == ".projectatlas/projectatlas-purpose-review.json")
    {
        return Err(io::Error::other(
            "local ProjectAtlas purpose-review batches must stay ignored",
        )
        .into());
    }
    if !gitignore.lines().any(|line| line == "/.worktrees/") {
        return Err(io::Error::other(
            "project-local linked worktrees must stay ignored as defense in depth",
        )
        .into());
    }
    let ignored_worktree_probe = git_command_for_root(&workspace_root)
        .args([
            "check-ignore",
            "--quiet",
            "--no-index",
            ".worktrees/projectatlas-boundary-probe/src/lib.rs",
        ])
        .status()?;
    if !ignored_worktree_probe.success() {
        return Err(io::Error::other(
            "Git did not apply the root /.worktrees/ defense-in-depth policy",
        )
        .into());
    }
    for required in [
        ".projectatlas/*.lock",
        ".projectatlas/graph-stage-*/",
        ".projectatlas/optional-parser-pack.json",
    ] {
        if !gitignore.lines().any(|line| line == required) {
            return Err(io::Error::other(format!(
                "ProjectAtlas disposable graph state must stay ignored; missing {required:?}"
            ))
            .into());
        }
    }
    let tracked_review_batch = git_command_for_root(&workspace_root)
        .args([
            "ls-files",
            "--error-unmatch",
            "--",
            ".projectatlas/projectatlas-purpose-review.json",
        ])
        .output()?;
    if tracked_review_batch.status.success()
        && workspace_root
            .join(ATLAS_DIR_NAME)
            .join("projectatlas-purpose-review.json")
            .exists()
    {
        return Err(io::Error::other(
            "the local ProjectAtlas purpose-review batch must not be tracked",
        )
        .into());
    }
    let workflow_guide = fs::read_to_string(workspace_root.join("docs").join("workflow.md"))?;
    for required in [
        "ProjectAtlas scan, purpose, parity, and lint maintenance run locally",
        "lint --report-untracked --purpose-level low",
    ] {
        if !workflow_guide.contains(required) {
            return Err(io::Error::other(format!(
                "workflow guidance must preserve the local ProjectAtlas maintenance boundary; missing {required:?}"
            ))
            .into());
        }
    }
    if pre_push.contains("map --force") || pre_push.contains("--strict-folders") {
        return Err(io::Error::other(
            "pre-push hook must use the SQLite-first scan/lint flow, not legacy map or strict-folder lint",
        )
        .into());
    }
    for required in [
        "--format json scan .",
        "lint --report-untracked --purpose-level low",
    ] {
        if !pre_push.contains(required) {
            return Err(io::Error::other(format!(
                "pre-push hook is missing SQLite-first ProjectAtlas command {required:?}"
            ))
            .into());
        }
    }
    let normalized_pre_push = pre_push.replace("\r\n", "\n");
    let cleanup_block = "for git_variable in $(git rev-parse --local-env-vars); do\n  unset \"$git_variable\"\ndone";
    let cleanup_position = normalized_pre_push.find(cleanup_block).ok_or_else(|| {
        io::Error::other("pre-push hook must clear Git repository-local environment")
    })?;
    let first_gate_position = normalized_pre_push
        .find("cargo fmt")
        .ok_or_else(|| io::Error::other("pre-push hook is missing its first Rust gate"))?;
    if cleanup_position >= first_gate_position {
        return Err(io::Error::other(
            "pre-push hook must clear every Git repository-local variable before running checks",
        )
        .into());
    }
    if !normalized_pre_push
        .contains("python3 docs/benchmarks/harness/mcp_composition.py --self-test")
    {
        return Err(
            io::Error::other("pre-push hook must run the Git fixture-isolation self-test").into(),
        );
    }
    let reported_git_environment = git_command_for_root(&workspace_root)
        .args(["rev-parse", "--local-env-vars"])
        .output()?;
    if !reported_git_environment.status.success() {
        return Err(io::Error::other(format!(
            "Git repository-local environment inventory failed: {}",
            String::from_utf8_lossy(&reported_git_environment.stderr)
        ))
        .into());
    }
    for variable in String::from_utf8(reported_git_environment.stdout)?.lines() {
        if !GIT_REPOSITORY_ENVIRONMENT_VARIABLES.contains(&variable) {
            return Err(io::Error::other(format!(
                "Git reported an unhandled repository-local variable: {variable}"
            ))
            .into());
        }
    }
    Ok(())
}

fn dependabot_update<'a>(updates: &'a [Yaml], ecosystem: &str) -> io::Result<&'a Yaml> {
    let mut matching = updates
        .iter()
        .filter(|update| update["package-ecosystem"].as_str() == Some(ecosystem));
    let update = matching
        .next()
        .ok_or_else(|| io::Error::other(format!("Dependabot {ecosystem} update is missing")))?;
    if matching.next().is_some() {
        return Err(io::Error::other(format!(
            "Dependabot {ecosystem} update must be unique"
        )));
    }
    Ok(update)
}

#[test]
fn repository_delivery_and_dependency_policy_is_enforced() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let workflow_dir = workspace_root.join(".github").join("workflows");
    let release_workflow = fs::read_to_string(workflow_dir.join("release.yml"))?;
    let auto_release_workflow =
        fs::read_to_string(workflow_dir.join(AUTO_RELEASE_WORKFLOW_FILE_NAME))?;
    let release_version_policy = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("scripts")
            .join("release_version.py"),
    )?;
    let seed_asset_policy = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("scripts")
            .join("verify-main-atlas-seed-release-assets.py"),
    )?;
    let optional_parser_handoff_resolver = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("scripts")
            .join("resolve-optional-parser-handoff.py"),
    )?;
    let optional_parser_workflow =
        fs::read_to_string(workflow_dir.join(OPTIONAL_PARSER_PACK_WORKFLOW_FILE_NAME))?;
    let ci_workflow = fs::read_to_string(workflow_dir.join("ci.yml"))?;
    let dependabot = fs::read_to_string(workspace_root.join(".github").join("dependabot.yml"))?;
    let deny = fs::read_to_string(workspace_root.join("deny.toml"))?;
    let hook = fs::read_to_string(
        workspace_root
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
    )?;
    let workflow_docs =
        fs::read_to_string(workspace_root.join("docs").join(WORKFLOW_DOC_FILE_NAME))?;
    let root_manifest_text = fs::read_to_string(workspace_root.join("Cargo.toml"))?;
    let root_manifest: toml::Value = toml::from_str(&root_manifest_text)?;
    let workspace = root_manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("root Cargo.toml is missing [workspace]"))?;
    let workspace_dependencies = workspace
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("root Cargo.toml is missing [workspace.dependencies]"))?;
    let workspace_members = workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("root Cargo.toml is missing workspace members"))?;

    for (dependency, declaration) in workspace_dependencies {
        let owns_version = declaration.as_str().is_some()
            || declaration
                .as_table()
                .and_then(|table| table.get("version"))
                .and_then(toml::Value::as_str)
                .is_some();
        if !owns_version {
            return Err(io::Error::other(format!(
                "workspace dependency {dependency:?} does not own a version"
            ))
            .into());
        }
    }

    for member in workspace_members {
        let member = member
            .as_str()
            .ok_or_else(|| io::Error::other("workspace member path must be a string"))?;
        let manifest_path = workspace_root.join(member).join("Cargo.toml");
        let manifest_text = fs::read_to_string(&manifest_path)?;
        let manifest: toml::Value = toml::from_str(&manifest_text)?;
        let mut dependency_tables = Vec::new();
        for table_name in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(value) = manifest.get(table_name) {
                let table = value.as_table().ok_or_else(|| {
                    io::Error::other(format!(
                        "{} [{table_name}] must be a table",
                        manifest_path.display()
                    ))
                })?;
                dependency_tables.push((table_name.to_string(), table));
            }
        }
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            for (selector, target) in targets {
                let target = target.as_table().ok_or_else(|| {
                    io::Error::other(format!(
                        "{} target {selector:?} must be a table",
                        manifest_path.display()
                    ))
                })?;
                for table_name in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(value) = target.get(table_name) {
                        let table = value.as_table().ok_or_else(|| {
                            io::Error::other(format!(
                                "{} [target.{selector}.{table_name}] must be a table",
                                manifest_path.display()
                            ))
                        })?;
                        dependency_tables.push((format!("target.{selector}.{table_name}"), table));
                    }
                }
            }
        }
        for (scope, dependencies) in dependency_tables {
            for (dependency, declaration) in dependencies {
                let declaration = declaration.as_table().ok_or_else(|| {
                    io::Error::other(format!(
                        "{} dependency {dependency:?} in [{scope}] owns a local version",
                        manifest_path.display()
                    ))
                })?;
                if declaration.get("version").is_some()
                    || declaration.get("workspace").and_then(toml::Value::as_bool) != Some(true)
                {
                    return Err(io::Error::other(format!(
                        "{} dependency {dependency:?} in [{scope}] must inherit its workspace version",
                        manifest_path.display()
                    ))
                    .into());
                }
                if !workspace_dependencies.contains_key(dependency) {
                    return Err(io::Error::other(format!(
                        "{} dependency {dependency:?} in [{scope}] is missing from root [workspace.dependencies]",
                        manifest_path.display()
                    ))
                    .into());
                }
            }
        }
    }

    let mut workflow_paths = fs::read_dir(&workflow_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    workflow_paths.sort_unstable();
    for path in workflow_paths {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if extension != "yml" && extension != "yaml" {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("workflow filename must be UTF-8"))?;
        let workflow = fs::read_to_string(&path)?;
        assert_actions_are_sha_pinned(name, &workflow)?;
    }

    let dependabot_documents = YamlLoader::load_from_str(&dependabot)?;
    let dependabot_config = dependabot_documents
        .first()
        .ok_or_else(|| io::Error::other("Dependabot configuration is empty"))?;
    let dependabot_updates = dependabot_config["updates"]
        .as_vec()
        .ok_or_else(|| io::Error::other("Dependabot updates must be a sequence"))?;
    let cargo_update = dependabot_update(dependabot_updates, "cargo")?;
    let actions_update = dependabot_update(dependabot_updates, "github-actions")?;
    for (ecosystem, update) in [("cargo", cargo_update), ("github-actions", actions_update)] {
        for (field, actual, expected) in [
            ("directory", update["directory"].as_str(), "/"),
            ("target-branch", update["target-branch"].as_str(), "main"),
            (
                "schedule.interval",
                update["schedule"]["interval"].as_str(),
                "weekly",
            ),
        ] {
            if actual != Some(expected) {
                return Err(io::Error::other(format!(
                    "Dependabot {ecosystem} update field {field:?} must be {expected:?}, found {actual:?}"
                ))
                .into());
            }
        }
    }
    let cargo_groups = cargo_update["groups"]
        .as_hash()
        .filter(|groups| !groups.is_empty())
        .ok_or_else(|| io::Error::other("Dependabot Cargo update must define a group"))?;
    let mut grouped_update_types = BTreeSet::new();
    for (group_name, group) in cargo_groups {
        let group_name = group_name
            .as_str()
            .ok_or_else(|| io::Error::other("Dependabot Cargo group name must be a string"))?;
        let update_types = group["update-types"].as_vec().ok_or_else(|| {
            io::Error::other(format!(
                "Dependabot Cargo group {group_name:?} must define update-types"
            ))
        })?;
        for update_type in update_types {
            let update_type = update_type.as_str().ok_or_else(|| {
                io::Error::other(format!(
                    "Dependabot Cargo group {group_name:?} update type must be a string"
                ))
            })?;
            grouped_update_types.insert(update_type.to_string());
        }
    }
    let expected_grouped_update_types = BTreeSet::from(["minor".to_string(), "patch".to_string()]);
    if grouped_update_types != expected_grouped_update_types {
        return Err(io::Error::other(format!(
            "Dependabot Cargo groups must include only minor and patch updates, found {grouped_update_types:?}"
        ))
        .into());
    }

    for entry in fs::read_dir(&workflow_dir)? {
        let path = entry?.path();
        let extension = path.extension().and_then(std::ffi::OsStr::to_str);
        if extension != Some("yml") && extension != Some("yaml") {
            continue;
        }
        let workflow = fs::read_to_string(&path)?;
        for rejected in [
            "gh pr merge",
            "enablePullRequestAutoMerge",
            "enable-pull-request-automerge",
            "automerge-action",
        ] {
            if workflow.contains(rejected) {
                return Err(io::Error::other(format!(
                    "{} contains repository-owned auto-merge behavior {rejected:?}",
                    path.display()
                ))
                .into());
            }
        }
    }

    let cargo_deny_version = ci_workflow
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("tool: cargo-deny@"))
        .ok_or_else(|| io::Error::other("CI cargo-deny install must pin an exact version"))?;
    let version_parts = cargo_deny_version.split('.').collect::<Vec<_>>();
    if version_parts.len() != 3
        || version_parts
            .iter()
            .any(|part| part.parse::<u64>().is_err())
    {
        return Err(io::Error::other(format!(
            "cargo-deny version {cargo_deny_version:?} is not an exact numeric release"
        ))
        .into());
    }
    let cargo_deny_install = format!("tool: cargo-deny@{cargo_deny_version}");
    if !release_workflow.contains(&cargo_deny_install) {
        return Err(io::Error::other(
            "CI and release must install the same exact cargo-deny version",
        )
        .into());
    }
    let cargo_deny_command = "cargo deny --locked --all-features check -D warnings";
    for (owner, content) in [
        ("CI", ci_workflow.as_str()),
        ("release", release_workflow.as_str()),
        ("pre-push hook", hook.as_str()),
        ("workflow docs", workflow_docs.as_str()),
    ] {
        if !content.contains(cargo_deny_command) {
            return Err(io::Error::other(format!(
                "{owner} is missing the locked all-feature cargo-deny command"
            ))
            .into());
        }
    }

    let deny_config: toml::Value = toml::from_str(&deny)?;
    let bans = deny_config
        .get("bans")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("deny.toml is missing [bans]"))?;
    for required in [
        "multiple-versions = \"deny\"",
        "multiple-versions-include-dev = true",
        "wildcards = \"deny\"",
        "[bans.workspace-dependencies]",
        "duplicates = \"deny\"",
        "include-path-dependencies = true",
        "unused = \"deny\"",
        "yanked = \"deny\"",
        "[licenses]",
        "unknown-registry = \"deny\"",
        "unknown-git = \"deny\"",
        "allow-registry = [\"https://github.com/rust-lang/crates.io-index\"]",
    ] {
        if !deny.contains(required) {
            return Err(io::Error::other(format!(
                "cargo-deny fail-closed policy is missing {required:?}"
            ))
            .into());
        }
    }
    if bans.contains_key("skip-tree") {
        return Err(io::Error::other("cargo-deny duplicate policy must not use skip-tree").into());
    }

    if !workspace_root.join(CARGO_LOCK_FILE_NAME).is_file() {
        return Err(io::Error::other("the workspace Cargo.lock must remain committed").into());
    }
    let metadata_output = StdCommand::new("cargo")
        .current_dir(&workspace_root)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .output()?;
    if !metadata_output.status.success() {
        return Err(io::Error::other(format!(
            "locked offline Cargo metadata failed: {}",
            String::from_utf8_lossy(&metadata_output.stderr)
        ))
        .into());
    }
    let skip_entries = bans
        .get("skip")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("duplicate policy must declare exact skips"))?;
    let mut skipped_packages = BTreeSet::new();
    for entry in skip_entries {
        let entry = entry
            .as_table()
            .ok_or_else(|| io::Error::other("cargo-deny skip entry must be a table"))?;
        let package = entry
            .get("crate")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| io::Error::other("cargo-deny skip entry must name a crate"))?;
        let reason = entry
            .get("reason")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| io::Error::other("cargo-deny skip entry must explain its removal"))?;
        let (name, version) = package.split_once('@').ok_or_else(|| {
            io::Error::other(format!(
                "cargo-deny skip {package:?} must use an exact crate@version package spec"
            ))
        })?;
        if name.is_empty()
            || version.is_empty()
            || version.contains(['<', '>', '=', '*', '^', '~', ','])
            || !reason.to_ascii_lowercase().contains("remove when")
        {
            return Err(io::Error::other(format!(
                "cargo-deny skip {package:?} is not exact or lacks an upstream removal condition"
            ))
            .into());
        }
        if !skipped_packages.insert(package) {
            return Err(
                io::Error::other(format!("cargo-deny skip {package:?} is duplicated")).into(),
            );
        }
    }

    if !deny.contains(r#"triple = "x86_64-apple-darwin""#) {
        return Err(io::Error::other(
            "cargo-deny target graph must include Intel macOS release target",
        )
        .into());
    }
    if release_workflow.contains("git push origin") {
        return Err(io::Error::other(
            "release workflow must not push tags before creating the release",
        )
        .into());
    }
    if release_workflow.contains("git merge-base --is-ancestor") {
        return Err(io::Error::other(
            "release workflow must route RC ancestry through release_version.py",
        )
        .into());
    }
    for required in [
        "gh release create \"$RELEASE_VERSION\"",
        "gh release upload \"$RELEASE_VERSION\" \"${upload_assets[@]}\" --clobber",
        "--target \"$GITHUB_SHA\"",
        "PROJECTATLAS_RELEASE_EXISTS",
        "SHA256SUMS",
        "No release archives matched projectatlas-${RELEASE_VERSION}-*",
        "already points to",
        "exists without a GitHub release; continuing recovery publish",
        "continuing asset repair publish",
        "--prerelease --latest=false",
        "EXPECTED_RELEASE_PRERELEASE",
        "EXPECTED_STABLE_TAG: ${{ needs.verify.outputs.stable_tag }}",
        "PROJECTATLAS_EXPECTED_LATEST",
        "--resolve-expected-latest",
        "Verify hosted release state",
        "verify-main-atlas-seed-release-assets.py",
        "Enforce RC-first promotion",
        "--require-prior-rc-from",
        "--require-prior-rc-ancestor-of",
        "Cannot publish an RC after stable tag",
        "projectatlas-release-${{ inputs.version }}",
        "cancel-in-progress: false",
        "Require exact main head for publication",
        "Revalidate exact main head before release mutation",
        "git fetch --force origin main:refs/remotes/origin/main",
        "refs/remotes/origin/main^{commit}",
        "Release publication requires the exact origin/main head",
        "projectatlas-main-atlas-seed",
        "projectatlas-hosted-main-atlas-seed-assets.txt",
        "projectatlas-main-atlas-seed-*",
        "--staged-source release-assets",
        "Existing release contains duplicate or non-regular main Atlas seed assets",
        "--repair-upload-source release-assets",
        "projectatlas-release-repair-assets.txt",
    ] {
        if !release_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "release workflow is missing recoverable publish/checksum guard {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "ReleaseVersion",
        "VERSION_PATTERN",
        "DEVELOPMENT_PATTERN",
        "-dev.",
        "stable_tag",
        "milestone",
        "is_prerelease",
        "rc_number",
        "latest_published_rc",
        "expected_latest_tag",
    ] {
        if !release_version_policy.contains(required) {
            return Err(io::Error::other(format!(
                "release version policy is missing closed classifier field {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "projectatlas-main-atlas-seed-",
        "\".tar.zst\"",
        "\".manifest.json\"",
        "exact release tag",
        "validate_hosted_seed_assets",
        "repair_upload_assets",
    ] {
        if !seed_asset_policy.contains(required) {
            return Err(io::Error::other(format!(
                "main Atlas seed release hook is missing exact-pair guard {required:?}"
            ))
            .into());
        }
    }
    if !release_workflow.contains("permissions:\n  contents: read") {
        return Err(io::Error::other("release workflow must default to contents: read").into());
    }
    for job in ["package-unix", "package-windows"] {
        let package_job = workflow_job_block(&release_workflow, job)?;
        if !package_job.contains("timeout-minutes: 30") {
            return Err(io::Error::other(format!(
                "release package job {job} must have a bounded timeout"
            ))
            .into());
        }
    }
    let publish = workflow_job_block(&release_workflow, "publish")?;
    for required in ["contents: write", "issues: read", "pull-requests: read"] {
        if !publish.contains(required) {
            return Err(io::Error::other(format!(
                "release publish job is missing scoped permission {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "Revalidate exact main head before release mutation",
        "Release mutation requires the exact current origin/main head",
        "git fetch --force origin main:refs/remotes/origin/main",
        "git ls-remote --exit-code --tags origin \"refs/tags/$EXPECTED_STABLE_TAG\"",
        "Could not verify that stable tag $EXPECTED_STABLE_TAG is absent",
    ] {
        if !publish.contains(required) {
            return Err(io::Error::other(format!(
                "release publish job omitted exact-main mutation guard {required:?}"
            ))
            .into());
        }
    }
    let notes = publish
        .find("python3 .github/scripts/release-notes.py > release-notes.md")
        .ok_or_else(|| io::Error::other("release notes generation is missing"))?;
    let revalidation = publish
        .find("git fetch --force origin main:refs/remotes/origin/main")
        .ok_or_else(|| io::Error::other("exact-main revalidation is missing"))?;
    for mutation in ["gh release upload", "gh release create"] {
        let mutation = publish
            .find(mutation)
            .ok_or_else(|| io::Error::other(format!("release mutation {mutation:?} is missing")))?;
        if notes >= revalidation || revalidation >= mutation {
            return Err(io::Error::other(
                "release notes must precede exact-main revalidation and every release mutation",
            )
            .into());
        }
    }
    for guarded_mutation in [
        "require_stable_tag_absent_for_rc\n            gh release upload",
        "require_stable_tag_absent_for_rc\n            gh release create",
    ] {
        if !publish.contains(guarded_mutation) {
            return Err(io::Error::other(
                "each release mutation must immediately recheck the remote stable tag",
            )
            .into());
        }
    }
    for required in [
        "parser_pack_run_id:",
        "parser-pack-assets:",
        "optional-parser-pack-release-assets",
        "optional-parser-proof-inputs.py",
        "github-token: ${{ github.token }}",
        "run-id: ${{ inputs.parser_pack_run_id }}",
        "verify-optional-parser-release-assets.py",
        "projectatlas-parser-packs",
        "MCP composition integrity",
    ] {
        if !release_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "release workflow is missing optional-parser handoff guard {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "github.event_name == 'workflow_dispatch' && inputs.clean_construction && inputs.target == 'all'",
        "pull_request:\n    branches: [main]",
        "optional-parser-pack-release-assets",
        "cargo-layer-$target.json",
        "projectatlas-broad-parser-$target.tar.zst",
    ] {
        if !optional_parser_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "optional-parser workflow is missing clean release handoff guard {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "git rev-parse 'HEAD^{commit}'",
        "Auto-release checkout differs from the exact main push",
        "resolve-optional-parser-handoff.py",
        "--field parser_pack_run_id=",
    ] {
        if !auto_release_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "auto-release workflow is missing input-bound optional-parser handoff guard {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "--paginate",
        "--slurp",
        "optional-parser-pack-release-assets",
        "optional-parser-proof-inputs.py",
    ] {
        if !optional_parser_handoff_resolver.contains(required) {
            return Err(io::Error::other(format!(
                "optional-parser handoff resolver is missing pagination or eligibility guard {required:?}"
            ))
            .into());
        }
    }
    for rejected in [
        "head_sha=$promotion_sha",
        "Optional-parser handoff tree differs from the release tree",
    ] {
        if auto_release_workflow.contains(rejected) || release_workflow.contains(rejected) {
            return Err(io::Error::other(format!(
                "release workflows retain SHA-only optional-parser proof {rejected:?}"
            ))
            .into());
        }
    }
    if !auto_release_workflow.contains("permissions:\n  contents: read\n  actions: write") {
        return Err(io::Error::other(
            "auto-release workflow must narrow permissions to contents read and actions write",
        )
        .into());
    }
    Ok(())
}

#[test]
fn token_cli_and_mcp_preserve_average_maximum_edge_accounting() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let db = atlas_dir.join("projectatlas.db");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), "fn main() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let store = AtlasStore::open(&db)?;
    for (path, without, with) in [
        (SRC_DIR_NAME, 5, 2),
        (SRC_DIR_NAME, 5, 2),
        (TESTS_DIR_NAME, 7, 3),
    ] {
        let mut event = usage_from_estimates(
            "public-token-edge",
            "folders",
            Some(path.to_string()),
            None,
            without,
            with,
        );
        event.denominator_kind = TOKEN_BASELINE_DIRECTORY_WALK.to_string();
        store.record_usage(&event)?;
    }
    drop(store);

    let token = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "--db"])
        .arg(&db)
        .arg("token")
        .output()?;
    if !token.status.success() {
        return Err(io::Error::other("edge-accounting token command failed").into());
    }
    let token_json: Value = serde_json::from_slice(&token.stdout)?;
    require_json_i64(&token_json, &["average_modeled_tokens_avoided"], -1)?;
    require_json_i64(&token_json, &["average_tokens_avoided"], -1)?;
    require_json_i64(&token_json, &["maximum_tokens_avoided"], 5)?;
    require_json_i64(&token_json, &["tokens_avoided"], -1)?;
    require_json_usize(&token_json, &["repeated_baselines_deduped"], 1)?;

    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let mut mcp = McpContractSession::spawn(&executable, &repo, &db)?;
    let mcp_result = (|| -> Result<(), Box<dyn Error>> {
        let report = mcp.call_tool("atlas_token_report", &json!({}))?;
        for required in [
            "average_modeled_tokens_avoided: -1",
            "average_tokens_avoided: -1",
            "maximum_tokens_avoided: 5",
            "tokens_avoided: -1",
            "repeated_baselines_deduped: 1",
        ] {
            if !report.contains(required) {
                return Err(io::Error::other(format!(
                    "MCP edge-accounting report omitted {required:?}: {report}"
                ))
                .into());
            }
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(mcp_result, || mcp.shutdown())?;

    let store = AtlasStore::open(&db)?;
    for index in 0..140 {
        let mut event = usage_from_estimates(
            "public-token-edge",
            "search",
            None,
            Some(format!("overflow-{index}")),
            10,
            1,
        );
        event.provider = format!("overflow-provider-{index}");
        store.record_usage(&event)?;
    }
    drop(store);

    let overflow = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "--db"])
        .arg(&db)
        .arg("token")
        .output()?;
    if !overflow.status.success() {
        return Err(io::Error::other("overflow token command failed").into());
    }
    let overflow_json: Value = serde_json::from_slice(&overflow.stdout)?;
    require_json_string(
        &overflow_json,
        &["average_policy", "evidence"],
        TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE,
    )?;
    if overflow_json["tokens_avoided"] != overflow_json["average_tokens_avoided"] {
        return Err(io::Error::other("overflow token alias did not match the average").into());
    }

    let mut mcp = McpContractSession::spawn(&executable, &repo, &db)?;
    let mcp_result = (|| -> Result<(), Box<dyn Error>> {
        let report = mcp.call_tool("atlas_token_report", &json!({}))?;
        if !report.contains(TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE) {
            return Err(io::Error::other(format!(
                "MCP overflow report omitted fallback evidence: {report}"
            ))
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(mcp_result, || mcp.shutdown())?;
    Ok(())
}

/// Require ANSI terminal output to remain inside a selected character-cell viewport.
fn require_tui_output_within_viewport(
    output: &str,
    columns: usize,
    rows: usize,
) -> Result<(), Box<dyn Error>> {
    let visible = strip_ansi_csi_sequences(output);
    let lines = visible.lines().collect::<Vec<_>>();
    if lines.len() > rows {
        return Err(io::Error::other(format!(
            "TUI emitted {} rows for a {columns}x{rows} viewport",
            lines.len()
        ))
        .into());
    }
    if let Some(line) = lines
        .iter()
        .find(|line| usize::from((**line).cell_width()) > columns)
    {
        return Err(io::Error::other(format!(
            "TUI emitted {} visible cells for a {columns}x{rows} viewport",
            (*line).cell_width()
        ))
        .into());
    }
    Ok(())
}

/// Remove the CSI control sequences emitted by the token dashboard serializer.
fn strip_ansi_csi_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            characters.next();
            for code in characters.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn run_scan(repo: &Path, db: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(mcp_contract_executable())
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["scan", "."])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "scan failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

fn run_watch_once_report(repo: &Path, db: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new(mcp_contract_executable())
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["watch", ".", "--once"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "watch --once failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn agent_purpose_and_health_resolution_gate_flow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("a.rs"), "pub fn alpha() {}\n")?;
    fs::write(repo.join(SRC_DIR_NAME).join("b.rs"), "pub fn beta() {}\n")?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_purposes:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-purpose"))
        .stdout(predicate::str::contains("suggested-purpose-review"));

    for (path, purpose) in [
        (".", "Repository root for agent purpose gate tests."),
        (SRC_DIR_NAME, "Rust source folder for purpose gate tests."),
        (
            "src/a.rs",
            "Alpha test module for duplicate purpose handling.",
        ),
        (
            "src/b.rs",
            "Alpha test module for duplicate purpose handling.",
        ),
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .arg("--db")
            .arg(&db)
            .args(["purpose", "set", path, purpose])
            .assert()
            .success();
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_purposes: 0"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "duplicate-purpose:src/b.rs:src/a.rs",
        ));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "health",
            "resolve",
            "duplicate-purpose:src/b.rs:src/a.rs",
            "duplicate-purpose",
            "src/b.rs",
            "--related-path",
            "src/a.rs",
            "--rationale",
            "Both tiny fixtures intentionally share a role in this test.",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("health_resolution:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("health_findings[0]"));

    fs::write(
        repo.join(SRC_DIR_NAME).join("a.rs"),
        "pub fn alpha() {}\npub fn changed_alpha() {}\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("text_index:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("stale_purposes: 0"))
        .stdout(predicate::str::contains("approved_purposes: 4"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("stale-purpose:src/a.rs:").not());

    Ok(())
}

#[test]
fn generated_file_purpose_suggestions_require_agent_approval() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("service.rs"),
        "//! Service module docs.\n/// Service API for tests.\npub struct Service;\n\nimpl Service {\n    /// Run the service.\n    pub fn run(&self) {}\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_suggestions: 1"))
        .stdout(predicate::str::contains("suggested_purposes: 1"))
        .stdout(predicate::str::contains("missing_purposes: 2"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "Service", "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/service.rs"))
        .stdout(predicate::str::contains(
            "rust source defining type and function Service, run",
        ));

    let raw_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["files", "Service", "--limit", "5"])
        .output()?;
    if !raw_files.status.success() {
        return Err(io::Error::other("json files command failed").into());
    }
    let files_json: Value = serde_json::from_slice(&raw_files.stdout)?;
    let file_entry = files_json
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["path"] == "src/service.rs")
        })
        .ok_or_else(|| io::Error::other("service file entry was missing"))?;
    require_json_string(
        file_entry,
        &["content_summary"],
        "rust source defining type and function Service, run.",
    )?;
    require_json_string(file_entry, &["status"], "suggested")?;
    require_json_string(
        file_entry,
        &["file_purpose"],
        "Implement the service source around Service and run.",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("file_summary:"))
        .stdout(predicate::str::contains("file_purpose_status: suggested"))
        .stdout(predicate::str::contains("content_summary:"))
        .stdout(predicate::str::contains(
            "rust source defining type and function Service, run.",
        ))
        .stdout(predicate::str::contains("Service"))
        .stdout(predicate::str::contains("run"));

    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs"])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other("json summary command failed").into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    require_json_string(&summary_json, &["file_path"], "src/service.rs")?;
    require_json_string(&summary_json, &["language"], "rust")?;
    require_json_usize(&summary_json, &["line_count"], 8)?;
    require_json_usize(&summary_json, &["symbol_count"], 2)?;
    require_json_string(&summary_json, &["file_purpose_status"], "suggested")?;
    require_json_string(&summary_json, &["file_purpose_source"], "generated")?;
    require_json_bool(&summary_json, &["file_purpose_agent_reviewed"], false)?;
    require_json_string(&summary_json, &["docstring"], "Service module docs.")?;
    require_json_usize(&summary_json, &["total_exports"], 2)?;
    require_json_string(&summary_json, &["exports", "0"], "Service")?;
    require_json_string(&summary_json, &["exports", "1"], "run")?;
    require_json_string(
        &summary_json,
        &["content_summary"],
        "rust source defining type and function Service, run.",
    )?;
    require_json_string(&summary_json, &["methods", "0", "name"], "run")?;
    require_json_string(&summary_json, &["methods", "0", "kind"], "method")?;
    require_json_usize(&summary_json, &["methods", "0", "line"], 7)?;
    require_json_bool(&summary_json, &["methods", "0", "exported"], true)?;
    require_json_string(
        &summary_json,
        &["methods", "0", "documentation"],
        "Run the service.",
    )?;
    require_json_string(&summary_json, &["types", "0", "name"], "Service")?;
    require_json_string(&summary_json, &["types", "0", "kind"], "struct")?;
    require_json_usize(&summary_json, &["types", "0", "line"], 3)?;
    require_json_bool(&summary_json, &["types", "0", "exported"], true)?;
    require_json_string(
        &summary_json,
        &["types", "0", "documentation"],
        "Service API for tests.",
    )?;
    require_json_array_len(&summary_json, &["functions"], 0)?;
    require_json_array_len(&summary_json, &["calls"], 0)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-purpose:."))
        .stdout(predicate::str::contains("missing-purpose:src"))
        .stdout(predicate::str::contains(
            "suggested-purpose-review:src/service.rs:",
        ));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--task",
            "e2e-purpose-queue",
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_curation:"))
        .stdout(predicate::str::contains("task: \"e2e-purpose-queue\""))
        .stdout(predicate::str::contains("active_generation:"))
        .stdout(predicate::str::contains("actionable: true"))
        .stdout(predicate::str::contains("curation_scope: low"))
        .stdout(predicate::str::contains("source_only: true"))
        .stdout(predicate::str::contains("work_key,state_token"))
        .stdout(predicate::str::contains(
            "purpose_agent_reviewed,review_priority,review_reason",
        ))
        .stdout(predicate::str::contains("false,high,folder_navigation"))
        .stdout(predicate::str::contains("missing-purpose:."))
        .stdout(predicate::str::contains("missing-purpose:src:"))
        .stdout(predicate::str::contains("suggested-purpose-review").not())
        .stdout(
            predicate::str::contains("Implement the service source around Service and run.").not(),
        );

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--limit",
            "5",
            "--include-low-priority-files",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "suggested-purpose-review:src/service.rs:",
        ))
        .stdout(predicate::str::contains(
            "false,low,generated_file_suggestion",
        ))
        .stdout(predicate::str::contains(
            "Implement the service source around Service and run.",
        ));

    for (path, purpose) in [
        (".", "Repository root for file purpose suggestion tests."),
        (
            SRC_DIR_NAME,
            "Rust source folder for file purpose suggestion tests.",
        ),
        (
            "src/service.rs",
            "Service module defining the test service type and run method.",
        ),
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .arg("--db")
            .arg(&db)
            .args(["purpose", "set", path, purpose])
            .assert()
            .success();
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_purposes: 0"))
        .stdout(predicate::str::contains("suggested_purposes: 0"));

    let raw_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["files", "Service", "--limit", "5"])
        .output()?;
    if !raw_files.status.success() {
        return Err(io::Error::other("json files command after approval failed").into());
    }
    let files_json: Value = serde_json::from_slice(&raw_files.stdout)?;
    let file_entry = files_json
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["path"] == "src/service.rs")
        })
        .ok_or_else(|| io::Error::other("approved service file entry was missing"))?;
    require_json_string(file_entry, &["status"], "approved")?;
    require_json_string(
        file_entry,
        &["file_purpose"],
        "Service module defining the test service type and run method.",
    )?;

    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs"])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other("json summary command after approval failed").into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    require_json_string(&summary_json, &["file_purpose_status"], "approved")?;
    require_json_string(&summary_json, &["file_purpose_source"], "agent")?;
    require_json_bool(&summary_json, &["file_purpose_agent_reviewed"], true)?;
    require_json_string(
        &summary_json,
        &["file_purpose"],
        "Service module defining the test service type and run method.",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("health_findings[0]"));

    Ok(())
}

#[test]
fn purpose_review_batch_applies_agent_review_without_raw_sql() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("detail.rs"),
        "pub fn trusted_detail() {}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("service.rs"),
        "//! Service module docs.\n/// Service API for tests.\npub struct Service;\n\nimpl Service {\n    /// Run the service.\n    pub fn run(&self) {}\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_suggestions: 2"));

    {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            "src/detail.rs",
            "Trusted detail implementation purpose.",
            PurposeSource::Imported,
        )?;
    }

    let bad_review = temp.path().join("bad-review.json");
    fs::write(
        &bad_review,
        serde_json::to_string_pretty(&serde_json::json!({
            "items": [
                { "path": "src/service.rs", "confirm_existing": true }
            ]
        }))?,
    )?;
    let bad_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&bad_review)
        .output()?;
    if bad_output.status.success() {
        return Err(io::Error::other("generated purpose confirm unexpectedly passed").into());
    }
    let bad_report: Value = serde_json::from_slice(&bad_output.stdout)?;
    require_json_usize(&bad_report, &["failed"], 1)?;
    require_json_string(
        &bad_report,
        &["items", "0", "error"],
        "generated suggestions require an explicit reviewed purpose",
    )?;

    let review = temp.path().join("review.json");
    fs::write(
        &review,
        serde_json::to_string_pretty(&serde_json::json!({
            "items": [
                { "path": "src/detail.rs", "confirm_existing": true },
                {
                    "path": "src/service.rs",
                    "purpose": "Service module defining the test service type and run method."
                }
            ]
        }))?,
    )?;

    let dry_run_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .output()?;
    if !dry_run_output.status.success() {
        return Err(io::Error::other(format!(
            "purpose review dry-run failed: {}",
            String::from_utf8_lossy(&dry_run_output.stderr)
        ))
        .into());
    }
    let dry_run_report: Value = serde_json::from_slice(&dry_run_output.stdout)?;
    require_json_bool(&dry_run_report, &["applied"], false)?;
    require_json_usize(&dry_run_report, &["changed"], 2)?;
    require_json_usize(&dry_run_report, &["failed"], 0)?;

    let service_dry_summary = json_summary_command(&repo, &db, "src/service.rs")?;
    require_json_string(&service_dry_summary, &["file_purpose_source"], "generated")?;
    require_json_bool(
        &service_dry_summary,
        &["file_purpose_agent_reviewed"],
        false,
    )?;

    let apply_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .arg("--apply")
        .output()?;
    if !apply_output.status.success() {
        return Err(io::Error::other(format!(
            "purpose review apply failed: {}",
            String::from_utf8_lossy(&apply_output.stderr)
        ))
        .into());
    }
    let apply_report: Value = serde_json::from_slice(&apply_output.stdout)?;
    require_json_bool(&apply_report, &["applied"], true)?;
    require_json_usize(&apply_report, &["changed"], 2)?;
    require_json_usize(&apply_report, &["failed"], 0)?;

    let detail_summary = json_summary_command(&repo, &db, "src/detail.rs")?;
    require_json_string(&detail_summary, &["file_purpose_source"], "agent")?;
    require_json_bool(&detail_summary, &["file_purpose_agent_reviewed"], true)?;
    require_json_string(
        &detail_summary,
        &["file_purpose"],
        "Trusted detail implementation purpose.",
    )?;
    let service_summary = json_summary_command(&repo, &db, "src/service.rs")?;
    require_json_string(&service_summary, &["file_purpose_source"], "agent")?;
    require_json_bool(&service_summary, &["file_purpose_agent_reviewed"], true)?;
    require_json_string(
        &service_summary,
        &["file_purpose"],
        "Service module defining the test service type and run method.",
    )?;

    let repeat_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .arg("--apply")
        .output()?;
    if !repeat_output.status.success() {
        return Err(io::Error::other("idempotent purpose review apply failed").into());
    }
    let repeat_report: Value = serde_json::from_slice(&repeat_output.stdout)?;
    require_json_usize(&repeat_report, &["changed"], 0)?;
    require_json_usize(&repeat_report, &["skipped"], 2)?;

    Ok(())
}

#[test]
fn purpose_review_adapters_enforce_shared_input_budgets() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "pub fn main_entry() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let valid_review = temp.path().join("valid-review.json");
    fs::write(
        &valid_review,
        serde_json::to_vec_pretty(&serde_json::json!({
            "items": [{
                "path": "src/main.rs",
                "purpose": "Reviewed café λ entry point."
            }]
        }))?,
    )?;
    let json_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&valid_review)
        .output()?;
    if !json_output.status.success() {
        return Err(io::Error::other(format!(
            "bounded JSON purpose review failed: {}",
            String::from_utf8_lossy(&json_output.stderr)
        ))
        .into());
    }
    let json_report: Value = serde_json::from_slice(&json_output.stdout)?;
    require_json_bool(&json_report, &["applied"], false)?;
    require_json_string(
        &json_report,
        &["items", "0", "purpose"],
        "Reviewed café λ entry point.",
    )?;
    if json_report.get("max_output_bytes").is_some() {
        return Err(io::Error::other("purpose review changed its legacy JSON schema").into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&valid_review)
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_review:"))
        .stdout(predicate::str::contains("Reviewed café λ entry point."));

    let oversized_item = temp.path().join("oversized-item-review.json");
    fs::write(
        &oversized_item,
        serde_json::to_vec(&serde_json::json!({
            "items": [{
                "path": "src/main.rs",
                "purpose": "x".repeat(64 * 1_024 + 1)
            }]
        }))?,
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&oversized_item)
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicate::str::contains("field purpose"))
        .stderr(predicate::str::contains("maximum is 65536"));
    let unchanged = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_bool(&unchanged, &["file_purpose_agent_reviewed"], false)?;

    let oversized_file = temp.path().join("oversized-review.json");
    fs::write(&oversized_file, vec![b' '; 2 * 1_024 * 1_024 + 1])?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&oversized_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "purpose review input file contains",
        ))
        .stderr(predicate::str::contains("maximum is 2097152"));

    let aggregate_items = (0..9)
        .map(|index| {
            serde_json::json!({
                "path": format!("src/{index}.rs"),
                "purpose": "x".repeat(64 * 1_024)
            })
        })
        .collect::<Vec<_>>();
    let aggregate_file = temp.path().join("aggregate-review.json");
    fs::write(
        &aggregate_file,
        serde_json::to_vec(&serde_json::json!({ "items": aggregate_items }))?,
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&aggregate_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("aggregate string bytes"));

    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "atlas_purpose_review",
                "arguments": {
                    "items": [{
                        "path": "src/main.rs",
                        "purpose": "Reviewed café λ entry point."
                    }]
                }
            }
        })
        .to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "atlas_purpose_review",
                "arguments": {
                    "apply": true,
                    "items": [{
                        "path": "src/main.rs",
                        "purpose": "x".repeat(64 * 1_024 + 1)
                    }]
                }
            }
        })
        .to_string(),
    ];
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let stdout = run_mcp_stdio(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &messages,
    )?;
    let success = mcp_tool_text(&stdout, 2)?;
    let oversized = mcp_tool_text(&stdout, 3)?;
    let repeated_item = serde_json::json!({
        "path": "src/main.rs",
        "purpose": "Bounded MCP review."
    });
    let too_many_messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "atlas_purpose_review",
                "arguments": {
                    "items": vec![repeated_item; 201]
                }
            }
        })
        .to_string(),
    ];
    let too_many_stdout = run_mcp_stdio(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &too_many_messages,
    )?;
    let too_many = mcp_tool_text(&too_many_stdout, 2)?;
    if !success.contains("purpose_review:")
        || !success.contains("Reviewed café λ entry point.")
        || !oversized.contains("field purpose")
        || !too_many.contains("maximum is 200")
    {
        return Err(io::Error::other(format!(
            "MCP purpose-review admission responses were incomplete: {stdout}; {too_many_stdout}"
        ))
        .into());
    }

    Ok(())
}

#[test]
fn conditional_purpose_review_cli_reconciles_source_before_apply() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "fn main() { run(); }\nfn run() {}\n",
    )?;
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir(&atlas_dir)?;
    fs::write(
        atlas_dir.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    let db = atlas_dir.join("projectatlas.db");

    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let queue_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--task",
            "conditional-cli-e2e",
            "--limit",
            "20",
        ])
        .output()?;
    if !queue_output.status.success() {
        return Err(io::Error::other(format!(
            "conditional purpose queue failed: {}",
            String::from_utf8_lossy(&queue_output.stderr)
        ))
        .into());
    }
    let queue: Value = serde_json::from_slice(&queue_output.stdout)?;
    require_json_string(&queue, &["task"], "conditional-cli-e2e")?;
    require_json_string(&queue, &["curation_scope"], "low")?;
    require_json_bool(&queue, &["actionable"], true)?;
    let selected = queue
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| item["path"] == "src/main.rs"))
        .ok_or_else(|| io::Error::other("conditional queue item missing"))?;
    let work_key = selected
        .get("work_key")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("conditional work key missing"))?;
    let state_token = selected
        .get("state_token")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("conditional state token missing"))?;
    if work_key.len() != 64 || state_token.len() != 64 {
        return Err(
            io::Error::other("conditional queue identities were not opaque digests").into(),
        );
    }

    let review = temp.path().join("conditional-review.json");
    fs::write(
        &review,
        serde_json::to_string_pretty(&serde_json::json!({
            "items": [{
                "path": "src/main.rs",
                "purpose": "Application entry point coordinating run.",
                "task": "conditional-cli-e2e",
                "work_key": work_key,
                "state_token": state_token
            }]
        }))?,
    )?;

    let before_stale = mcp_database_snapshot(&db)?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "fn main() { updated(); }\nfn updated() {}\n",
    )?;

    let stale_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .arg("--apply")
        .output()?;
    if !stale_output.status.success() {
        return Err(io::Error::other(format!(
            "stale conditional purpose review failed: {}",
            String::from_utf8_lossy(&stale_output.stderr)
        ))
        .into());
    }
    let stale: Value = serde_json::from_slice(&stale_output.stdout)?;
    require_json_usize(&stale, &["changed"], 0)?;
    require_json_usize(&stale, &["conflicts"], 1)?;
    require_json_string(&stale, &["items", "0", "action"], "stale")?;
    let after_stale = mcp_database_snapshot(&db)?;
    if after_stale.purpose_revision != before_stale.purpose_revision
        || after_stale.authored_purposes != before_stale.authored_purposes
    {
        return Err(io::Error::other("stale CLI work changed authored purpose state").into());
    }

    let current_queue_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--task",
            "conditional-cli-e2e",
            "--limit",
            "20",
        ])
        .output()?;
    if !current_queue_output.status.success() {
        return Err(io::Error::other(format!(
            "current conditional purpose queue failed: {}",
            String::from_utf8_lossy(&current_queue_output.stderr)
        ))
        .into());
    }
    let current_queue: Value = serde_json::from_slice(&current_queue_output.stdout)?;
    let current = current_queue
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| item["path"] == "src/main.rs"))
        .ok_or_else(|| io::Error::other("current conditional queue item missing"))?;
    let current_work_key = current
        .get("work_key")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("current conditional work key missing"))?;
    let current_state_token = current
        .get("state_token")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("current conditional state token missing"))?;
    if current_work_key == work_key || current_state_token == state_token {
        return Err(io::Error::other("source refresh did not invalidate queue identities").into());
    }
    fs::write(
        &review,
        serde_json::to_string_pretty(&serde_json::json!({
            "items": [{
                "path": "src/main.rs",
                "purpose": "Application entry point coordinating the updated run.",
                "task": "conditional-cli-e2e",
                "work_key": current_work_key,
                "state_token": current_state_token
            }]
        }))?,
    )?;

    let apply_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .arg("--apply")
        .output()?;
    if !apply_output.status.success() {
        return Err(io::Error::other(format!(
            "current conditional purpose review failed: {}",
            String::from_utf8_lossy(&apply_output.stderr)
        ))
        .into());
    }
    let applied: Value = serde_json::from_slice(&apply_output.stdout)?;
    require_json_usize(&applied, &["changed"], 1)?;
    require_json_usize(&applied, &["conflicts"], 0)?;
    require_json_string(&applied, &["items", "0", "action"], "review")?;

    let repeat_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&review)
        .arg("--apply")
        .output()?;
    if !repeat_output.status.success() {
        return Err(io::Error::other("replayed conditional purpose review failed").into());
    }
    let repeated: Value = serde_json::from_slice(&repeat_output.stdout)?;
    require_json_usize(&repeated, &["changed"], 0)?;
    require_json_usize(&repeated, &["skipped"], 1)?;
    require_json_usize(&repeated, &["conflicts"], 1)?;
    require_json_string(&repeated, &["items", "0", "action"], "accepted")?;

    let summary = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_string(&summary, &["file_purpose_source"], "agent")?;
    require_json_string(
        &summary,
        &["file_purpose"],
        "Application entry point coordinating the updated run.",
    )?;
    let source_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            "src/main.rs",
            "--start-line",
            "1",
            "--end-line",
            "2",
        ])
        .output()?;
    if !source_output.status.success() {
        return Err(io::Error::other(format!(
            "current conditional source readback failed: {}",
            String::from_utf8_lossy(&source_output.stderr)
        ))
        .into());
    }
    let current_source: Value = serde_json::from_slice(&source_output.stdout)?;
    require_json_contains(&current_source, &["content"], "updated")?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "fn main() { final_revision(); }\nfn final_revision() {}\n",
    )?;
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "set",
            "src/main.rs",
            "Application entry point coordinating the final saved run.",
        ])
        .assert()
        .success();
    let corrected = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_string(
        &corrected,
        &["file_purpose"],
        "Application entry point coordinating the final saved run.",
    )?;
    require_json_contains(&corrected, &["content_summary"], "final_revision")?;
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    let rescanned = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_string(&rescanned, &["file_purpose_source"], "agent")?;
    require_json_string(
        &rescanned,
        &["file_purpose"],
        "Application entry point coordinating the final saved run.",
    )?;
    for (path, purpose) in [
        (".", "Repository root for the conditional CLI fixture."),
        (SRC_DIR_NAME, "Contain the conditional CLI fixture source."),
    ] {
        Command::new(mcp_contract_executable())
            .current_dir(&repo)
            .arg("--db")
            .arg(&db)
            .args(["purpose", "set", path, purpose])
            .assert()
            .success();
    }
    let watch: Value = toon_format::decode_default(&run_watch_once_report(&repo, &db)?)?;
    require_json_usize(&watch, &["watch", "text_index", "candidates"], 0)?;
    require_json_usize(&watch, &["watch", "structural_summaries", "candidates"], 0)?;
    require_json_usize(&watch, &["watch", "last_symbols", "candidates"], 0)?;
    let converged_queue_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--task",
            "conditional-cli-e2e",
            "--limit",
            "20",
        ])
        .output()?;
    if !converged_queue_output.status.success() {
        return Err(io::Error::other(format!(
            "converged conditional purpose queue failed: {}",
            String::from_utf8_lossy(&converged_queue_output.stderr)
        ))
        .into());
    }
    let converged_queue: Value = serde_json::from_slice(&converged_queue_output.stdout)?;
    require_json_bool(&converged_queue, &["actionable"], false)?;
    require_json_usize(&converged_queue, &["returned"], 0)?;
    let queue_is_empty = converged_queue
        .get("items")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    if !queue_is_empty {
        return Err(io::Error::other("current CLI purpose queue did not converge to empty").into());
    }
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "low"])
        .assert()
        .success();
    let config = atlas_dir.join("config.toml");
    let project_root = normalize_native_path_display(fs::canonicalize(&repo)?);
    fs::write(&config, format!("[project]\nroot = \"{project_root}\"\n"))?;
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(&config)
        .args(["scan", "."])
        .assert()
        .success();
    let relative_db = Path::new(TEST_REPO_DIR)
        .join(ATLAS_DIR_NAME)
        .join("projectatlas.db");
    let relative_config = Path::new(TEST_REPO_DIR)
        .join(ATLAS_DIR_NAME)
        .join("config.toml");
    Command::new(mcp_contract_executable())
        .current_dir(temp.path())
        .arg("--db")
        .arg(relative_db)
        .arg("--config")
        .arg(relative_config)
        .args([
            "purpose",
            "set",
            "src/main.rs",
            "Application entry point addressed outside the repository.",
        ])
        .assert()
        .success();
    let addressed = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_string(
        &addressed,
        &["file_purpose"],
        "Application entry point addressed outside the repository.",
    )?;
    Ok(())
}

#[test]
fn persistent_mcp_purpose_review_reconciles_source_before_apply() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    let source = repo.join(SRC_DIR_NAME).join("main.rs");
    fs::write(&source, "fn main() { first(); }\nfn first() {}\n")?;
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir(&atlas_dir)?;
    fs::write(
        atlas_dir.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    let db = atlas_dir.join("projectatlas.db");
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let executable = mcp_contract_executable();
    let mut mcp = McpContractSession::spawn(&executable, &repo, &db)?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        let queue_text = mcp.call_tool(
            "atlas_purpose_queue",
            &serde_json::json!({
                "task": "conditional-mcp-e2e",
                "limit": 20,
                "include_low_priority_files": true
            }),
        )?;
        let queue: Value = toon_format::decode_default(&queue_text)?;
        let selected = queue
            .get("purpose_curation_items")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().find(|item| item["path"] == "src/main.rs"))
            .ok_or_else(|| io::Error::other("MCP conditional queue item missing"))?;
        let work_key = selected
            .get("work_key")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("MCP conditional work key missing"))?
            .to_string();
        let state_token = selected
            .get("state_token")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("MCP conditional state token missing"))?
            .to_string();

        mcp.call_tool("atlas_watch_once", &serde_json::json!({"path": "."}))?;
        let after_noop_watch: Value = toon_format::decode_default(&mcp.call_tool(
            "atlas_purpose_queue",
            &serde_json::json!({
                "task": "conditional-mcp-e2e",
                "limit": 20,
                "include_low_priority_files": true
            }),
        )?)?;
        let unchanged = after_noop_watch
            .get("purpose_curation_items")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().find(|item| item["path"] == "src/main.rs"))
            .ok_or_else(|| io::Error::other("MCP queue did not converge after no-op watch"))?;
        if unchanged.get("work_key").and_then(Value::as_str) != Some(work_key.as_str())
            || unchanged.get("state_token").and_then(Value::as_str) != Some(state_token.as_str())
        {
            return Err(io::Error::other(
                "no-op MCP watch changed the unchanged purpose work identity",
            )
            .into());
        }

        let before_stale = mcp_database_snapshot(&db)?;
        fs::write(&source, "fn main() { second(); }\nfn second() {}\n")?;
        let stale_text = mcp.call_tool(
            "atlas_purpose_review",
            &serde_json::json!({
                "apply": true,
                "items": [{
                    "path": "src/main.rs",
                    "purpose": "Run the first application responsibility.",
                    "task": "conditional-mcp-e2e",
                    "work_key": work_key,
                    "state_token": state_token
                }]
            }),
        )?;
        let stale: Value = toon_format::decode_default(&stale_text)?;
        require_json_usize(&stale, &["purpose_review", "changed"], 0)?;
        require_json_usize(&stale, &["purpose_review", "conflicts"], 1)?;
        require_json_string(&stale, &["purpose_review_items", "0", "action"], "stale")?;
        let after_stale = mcp_database_snapshot(&db)?;
        if after_stale.purpose_revision != before_stale.purpose_revision
            || after_stale.authored_purposes != before_stale.authored_purposes
        {
            return Err(io::Error::other("stale MCP work changed authored purpose state").into());
        }

        let current_queue_text = mcp.call_tool(
            "atlas_purpose_queue",
            &serde_json::json!({
                "task": "conditional-mcp-e2e",
                "limit": 20,
                "include_low_priority_files": true
            }),
        )?;
        let current_queue: Value = toon_format::decode_default(&current_queue_text)?;
        let current = current_queue
            .get("purpose_curation_items")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().find(|item| item["path"] == "src/main.rs"))
            .ok_or_else(|| io::Error::other("current MCP conditional queue item missing"))?;
        let current_work_key = current
            .get("work_key")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("current MCP conditional work key missing"))?;
        let current_state_token = current
            .get("state_token")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("current MCP conditional state token missing"))?;
        let applied_text = mcp.call_tool(
            "atlas_purpose_review",
            &serde_json::json!({
                "apply": true,
                "items": [{
                    "path": "src/main.rs",
                    "purpose": "Run the current application responsibility.",
                    "task": "conditional-mcp-e2e",
                    "work_key": current_work_key,
                    "state_token": current_state_token
                }]
            }),
        )?;
        let applied: Value = toon_format::decode_default(&applied_text)?;
        require_json_usize(&applied, &["purpose_review", "changed"], 1)?;
        require_json_usize(&applied, &["purpose_review", "conflicts"], 0)?;

        let summary_text = mcp.call_tool(
            "atlas_file_summary",
            &serde_json::json!({"file": "src/main.rs"}),
        )?;
        if !summary_text.contains("Run the current application responsibility.")
            || !summary_text.contains("second")
        {
            return Err(io::Error::other(format!(
                "MCP current source and purpose did not converge: {summary_text}"
            ))
            .into());
        }
        mcp.call_tool("atlas_scan", &serde_json::json!({"path": "."}))?;
        let rescanned_summary = mcp.call_tool(
            "atlas_file_summary",
            &serde_json::json!({"file": "src/main.rs"}),
        )?;
        if !rescanned_summary.contains("Run the current application responsibility.")
            || !rescanned_summary.contains("second")
        {
            return Err(io::Error::other(format!(
                "MCP rescan did not retain current source and purpose: {rescanned_summary}"
            ))
            .into());
        }
        for (path, purpose) in [
            (".", "Repository root for the conditional MCP fixture."),
            (SRC_DIR_NAME, "Contain the conditional MCP fixture source."),
        ] {
            let set: Value = toon_format::decode_default(&mcp.call_tool(
                "atlas_purpose_set",
                &serde_json::json!({"path": path, "purpose": purpose}),
            )?)?;
            require_json_string(&set, &["purpose_set", "status"], "approved")?;
        }
        let final_watch: Value = toon_format::decode_default(
            &mcp.call_tool("atlas_watch_once", &serde_json::json!({"path": "."}))?,
        )?;
        require_json_usize(&final_watch, &["watch", "text_index", "candidates"], 0)?;
        require_json_usize(
            &final_watch,
            &["watch", "structural_summaries", "candidates"],
            0,
        )?;
        require_json_usize(&final_watch, &["watch", "last_symbols", "candidates"], 0)?;
        let converged_text = mcp.call_tool(
            "atlas_purpose_queue",
            &serde_json::json!({
                "task": "conditional-mcp-e2e",
                "limit": 20,
                "include_low_priority_files": true
            }),
        )?;
        let converged: Value = toon_format::decode_default(&converged_text)?;
        require_json_bool(&converged, &["purpose_curation", "actionable"], false)?;
        require_json_usize(&converged, &["purpose_curation", "returned"], 0)?;
        let queue_is_empty = converged
            .get("purpose_curation_items")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty);
        if !queue_is_empty {
            return Err(
                io::Error::other("current MCP purpose queue did not converge to empty").into(),
            );
        }
        let lint: Value = toon_format::decode_default(
            &mcp.call_tool("atlas_lint", &serde_json::json!({"purpose_level": "low"}))?,
        )?;
        require_json_bool(&lint, &["lint", "ok"], true)?;
        require_json_usize(&lint, &["lint", "exit_code"], 0)?;
        Ok(())
    })();
    complete_mcp_test_after_shutdown(result, || mcp.shutdown())
}

#[test]
fn generated_purpose_queue_avoids_generic_and_asset_noise() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("customers"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("settings"))?;
    fs::create_dir(repo.join("assets"))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("customers").join("service.rs"),
        "pub struct CustomerService;\n\nimpl CustomerService {\n    pub fn reconcile(&self) {}\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("settings").join("service.rs"),
        "pub struct SettingsService;\n\nimpl SettingsService {\n    pub fn load(&self) {}\n}\n",
    )?;
    fs::write(
        repo.join("build.gradle.kts"),
        "tasks.register<BootRun>(\"bootRunE2E\") {\n    group = \"verification\"\n}\n\ntasks {\n    register<Copy>(\"copyE2EReports\") {\n        group = \"verification\"\n    }\n}\n\nval verifyAtlas by tasks.registering {\n    group = \"verification\"\n}\n",
    )?;
    fs::write(
        repo.join("assets").join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_suggestions: 3"));

    let default_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "queue", "--limit", "20"])
        .output()?;
    if !default_output.status.success() {
        return Err(io::Error::other(format!(
            "purpose queue failed: {}",
            String::from_utf8_lossy(&default_output.stderr)
        ))
        .into());
    }
    let default_queue = String::from_utf8(default_output.stdout)?;
    if !default_queue
        .contains("Define Gradle build tasks around bootRunE2E, copyE2EReports, and verifyAtlas.")
        || !default_queue.contains("false,high,high_impact_file")
        || !default_queue.contains("folder_scope: all")
        || !default_queue.contains("file_scope: high_impact")
        || !default_queue.contains("missing-purpose:assets:")
        || default_queue.contains("assets/logo.svg")
    {
        return Err(io::Error::other(format!(
            "default purpose queue missed high-impact Gradle file or asset-root folder filtering:\n{default_queue}"
        ))
        .into());
    }
    for low_priority in [
        "Implement the customers service source around CustomerService and reconcile.",
        "Implement the settings service source around SettingsService and load.",
    ] {
        if default_queue.contains(low_priority) {
            return Err(io::Error::other(format!(
                "default purpose queue included low-priority file suggestion `{low_priority}`:\n{default_queue}"
            ))
            .into());
        }
    }

    let asset_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["purpose", "queue", "--limit", "20", "--include-assets"])
        .output()?;
    if !asset_output.status.success() {
        return Err(io::Error::other(format!(
            "asset purpose queue failed: {}",
            String::from_utf8_lossy(&asset_output.stderr)
        ))
        .into());
    }
    let asset_queue = String::from_utf8(asset_output.stdout)?;
    if !asset_queue.contains("assets/logo.svg")
        || !asset_queue.contains("file_scope: high_impact_and_assets")
    {
        return Err(io::Error::other(format!(
            "include-assets queue did not include asset file:\n{asset_queue}"
        ))
        .into());
    }
    for low_priority in [
        "Implement the customers service source around CustomerService and reconcile.",
        "Implement the settings service source around SettingsService and load.",
    ] {
        if asset_queue.contains(low_priority) {
            return Err(io::Error::other(format!(
                "include-assets queue included low-priority source suggestion `{low_priority}`:\n{asset_queue}"
            ))
            .into());
        }
    }

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--limit",
            "20",
            "--include-low-priority-files",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "broad purpose queue failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let queue = String::from_utf8(output.stdout)?;
    if !queue.contains("folder_scope: source_relevant") || !queue.contains("file_scope: all_source")
    {
        return Err(io::Error::other(format!(
            "broad purpose queue did not expose source file scope:\n{queue}"
        ))
        .into());
    }
    for expected in [
        "Implement the customers service source around CustomerService and reconcile.",
        "Implement the settings service source around SettingsService and load.",
        "Define Gradle build tasks around bootRunE2E, copyE2EReports, and verifyAtlas.",
    ] {
        if !queue.contains(expected) {
            return Err(io::Error::other(format!(
                "purpose queue missed useful suggestion `{expected}`:\n{queue}"
            ))
            .into());
        }
    }
    for noisy in [
        "Implement build.",
        "Implement service.",
        "Implement the service source",
        "assets/logo.svg",
    ] {
        if queue.contains(noisy) {
            return Err(io::Error::other(format!(
                "purpose queue included noisy suggestion/path `{noisy}`:\n{queue}"
            ))
            .into());
        }
    }

    let all_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "purpose",
            "queue",
            "--limit",
            "20",
            "--include-assets",
            "--include-low-priority-files",
        ])
        .output()?;
    if !all_output.status.success() {
        return Err(io::Error::other(format!(
            "all-file purpose queue failed: {}",
            String::from_utf8_lossy(&all_output.stderr)
        ))
        .into());
    }
    let all_queue = String::from_utf8(all_output.stdout)?;
    if !all_queue.contains("folder_scope: all") || !all_queue.contains("file_scope: all") {
        return Err(io::Error::other(format!(
            "combined purpose queue did not expose all-file scope:\n{all_queue}"
        ))
        .into());
    }
    for expected in [
        "Implement the customers service source around CustomerService and reconcile.",
        "Implement the settings service source around SettingsService and load.",
        "Define Gradle build tasks around bootRunE2E, copyE2EReports, and verifyAtlas.",
        "assets/logo.svg",
    ] {
        if !all_queue.contains(expected) {
            return Err(io::Error::other(format!(
                "combined purpose queue missed `{expected}`:\n{all_queue}"
            ))
            .into());
        }
    }

    Ok(())
}

#[test]
fn lint_formats_share_typed_cli_and_mcp_report() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas = repo.join(ATLAS_DIR_NAME);
    let source = repo.join(SRC_DIR_NAME);
    let database = atlas.join("projectatlas.db");
    let executable = mcp_contract_executable();
    fs::create_dir_all(&atlas)?;
    fs::create_dir(&source)?;
    fs::write(
        atlas.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        atlas.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\ntarget/\n")?;
    fs::write(source.join("lib.rs"), "pub fn lint_contract() {}\n")?;
    fs::write(repo.join("stray.bin"), b"untracked")?;

    Command::new(&executable)
        .current_dir(&repo)
        .arg("--db")
        .arg(&database)
        .args(["scan", "."])
        .assert()
        .success();
    let store = AtlasStore::open(&database)?;
    for node in store.load_nodes()? {
        if node.node.path == "stray.bin" {
            continue;
        }
        store.set_purpose(
            &node.node.path,
            &format!(
                "Agent-reviewed lint contract purpose for {}",
                node.node.path
            ),
            PurposeSource::Agent,
        )?;
    }
    let before = mcp_database_snapshot(&database)?;

    let clean_json = Command::new(&executable)
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if !clean_json.status.success() || !clean_json.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "clean JSON lint violated status or stream ownership: {}",
            String::from_utf8_lossy(&clean_json.stderr)
        ))
        .into());
    }
    let clean_json_value: Value = serde_json::from_slice(&clean_json.stdout)?;
    require_json_bool(&clean_json_value, &["lint", "ok"], true)?;
    require_json_usize(&clean_json_value, &["lint", "exit_code"], 0)?;

    let clean_toon = Command::new(&executable)
        .current_dir(&repo)
        .arg("--format")
        .arg("toon")
        .arg("--db")
        .arg(&database)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if !clean_toon.status.success()
        || !clean_toon.stderr.is_empty()
        || clean_toon.stdout == clean_json.stdout
    {
        return Err(io::Error::other("clean TOON lint violated format or stream ownership").into());
    }
    let clean_toon_value: Value =
        toon_format::decode_default(&String::from_utf8(clean_toon.stdout)?)?;
    if clean_toon_value != clean_json_value {
        return Err(io::Error::other("clean JSON and TOON lint facts diverged").into());
    }

    let (rejected_reader, rejected_writer) = io::pipe()?;
    drop(rejected_reader);
    let rejected = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args(["lint", "--purpose-level", "low"])
        .stdout(Stdio::from(rejected_writer))
        .stderr(Stdio::piped())
        .output()?;
    if rejected.status.success() || rejected.stderr.is_empty() {
        return Err(io::Error::other("lint did not propagate a rejected stdout write").into());
    }

    let lint_arguments = [
        "lint",
        "--report-untracked",
        "--strict-untracked",
        "--purpose-level",
        "low",
    ];
    let failing_json = Command::new(&executable)
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args(lint_arguments)
        .output()?;
    if failing_json.status.code() != Some(1) || !failing_json.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "failing JSON lint violated status or stream ownership: {}",
            String::from_utf8_lossy(&failing_json.stderr)
        ))
        .into());
    }
    let failing_json_value: Value = serde_json::from_slice(&failing_json.stdout)?;
    require_json_bool(&failing_json_value, &["lint", "ok"], false)?;
    require_json_usize(&failing_json_value, &["lint", "exit_code"], 1)?;
    let disallowed = json_at(
        &failing_json_value,
        &["lint", "map", "untracked", "disallowed"],
    )?
    .as_array()
    .ok_or_else(|| io::Error::other("typed disallowed lint paths were not an array"))?;
    if !disallowed.iter().any(|path| path == "stray.bin") {
        return Err(io::Error::other("typed lint report omitted stray.bin").into());
    }

    let failing_toon = Command::new(&executable)
        .current_dir(&repo)
        .arg("--format")
        .arg("toon")
        .arg("--db")
        .arg(&database)
        .args(lint_arguments)
        .output()?;
    if failing_toon.status.code() != Some(1)
        || !failing_toon.stderr.is_empty()
        || failing_toon.stdout == failing_json.stdout
    {
        return Err(
            io::Error::other("failing TOON lint violated format or stream ownership").into(),
        );
    }
    let failing_toon_value: Value =
        toon_format::decode_default(&String::from_utf8(failing_toon.stdout)?)?;
    if failing_toon_value != failing_json_value {
        return Err(io::Error::other("failing JSON and TOON lint facts diverged").into());
    }
    let cli_after = mcp_database_snapshot(&database)?;
    if cli_after != before {
        return Err(io::Error::other(format!(
            "CLI lint mutated SQLite state: authoritative={:?} usage={:?} generation={}=>{} purpose_revision={}=>{} publication={}=>{}",
            changed_snapshot_keys(&before.authoritative, &cli_after.authoritative),
            changed_snapshot_keys(&before.usage, &cli_after.usage),
            before.generation,
            cli_after.generation,
            before.purpose_revision,
            cli_after.purpose_revision,
            before.publication_state,
            cli_after.publication_state
        ))
        .into());
    }

    let mut mcp = McpContractSession::spawn(&executable, &repo, &database)?;
    let mcp_before = mcp_database_snapshot(&database)?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mcp_lint: Value = toon_format::decode_default(&mcp.call_tool(
            "atlas_lint",
            &serde_json::json!({
                "project_path": repo.to_string_lossy(),
                "report_untracked": true,
                "strict_untracked": true,
                "purpose_level": "low"
            }),
        )?)?;
        if json_at(&mcp_lint, &["lint"])? != json_at(&failing_json_value, &["lint"])? {
            return Err(io::Error::other("CLI and MCP typed lint reports diverged").into());
        }
        if mcp_database_snapshot(&database)? != mcp_before {
            return Err(io::Error::other("MCP lint mutated SQLite state").into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(result, || mcp.shutdown())?;

    let missing_root = temp.path().join(MISSING_INDEX_DIR_NAME);
    let missing_atlas = missing_root.join(ATLAS_DIR_NAME);
    let missing_database = missing_atlas.join("projectatlas.db");
    fs::create_dir_all(&missing_atlas)?;
    fs::write(
        missing_atlas.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        missing_atlas.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(missing_root.join(".gitignore"), ".projectatlas/\n")?;
    let missing_cli = Command::new(&executable)
        .current_dir(&missing_root)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&missing_database)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if !missing_cli.status.success() || !missing_cli.stderr.is_empty() {
        return Err(
            io::Error::other("missing-index CLI lint did not return a clean report").into(),
        );
    }
    let missing_cli: Value = serde_json::from_slice(&missing_cli.stdout)?;
    if !json_at(&missing_cli, &["lint", "index"])?.is_null() || missing_database.exists() {
        return Err(io::Error::other("missing-index CLI lint created or reported an index").into());
    }

    Connection::open(&database)?.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let wrong_root = temp.path().join(WRONG_PROJECT_OWNER_DIR_NAME);
    let wrong_atlas = wrong_root.join(ATLAS_DIR_NAME);
    let wrong_database = wrong_atlas.join("projectatlas.db");
    fs::create_dir_all(&wrong_atlas)?;
    fs::write(
        wrong_atlas.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        wrong_atlas.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(wrong_root.join(".gitignore"), ".projectatlas/\n")?;
    fs::copy(&database, &wrong_database)?;
    let wrong_before = mcp_database_snapshot(&wrong_database)?;
    let wrong_cli = Command::new(&executable)
        .current_dir(&wrong_root)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&wrong_database)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if wrong_cli.status.success()
        || !String::from_utf8_lossy(&wrong_cli.stderr).contains("project_mismatch")
        || mcp_database_snapshot(&wrong_database)? != wrong_before
    {
        return Err(
            io::Error::other("wrong-root CLI lint lost identity or no-mutation behavior").into(),
        );
    }

    let mut routing = McpContractSession::spawn(&executable, &repo, &database)?;
    let routing_result = (|| -> Result<(), Box<dyn Error>> {
        let missing_mcp: Value = toon_format::decode_default(&routing.call_tool(
            "atlas_lint",
            &serde_json::json!({
                "project_path": missing_root,
                "purpose_level": "low"
            }),
        )?)?;
        if json_at(&missing_mcp, &["lint"])? != json_at(&missing_cli, &["lint"])?
            || missing_database.exists()
        {
            return Err(io::Error::other(
                "missing-index CLI/MCP lint facts diverged or created an index",
            )
            .into());
        }
        let wrong_mcp = routing.call_tool(
            "atlas_lint",
            &serde_json::json!({
                "project_path": wrong_root,
                "purpose_level": "low"
            }),
        )?;
        if !wrong_mcp.contains("project_mismatch")
            || mcp_database_snapshot(&wrong_database)? != wrong_before
        {
            return Err(io::Error::other(
                "wrong-root MCP lint lost identity or no-mutation behavior",
            )
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(routing_result, || routing.shutdown())?;

    let stale_before = mcp_database_snapshot(&database)?;
    fs::write(source.join("lib.rs"), "pub fn lint_contract_changed() {}\n")?;
    let stale = Command::new(&executable)
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if stale.status.success()
        || !stale.stdout.is_empty()
        || !String::from_utf8_lossy(&stale.stderr).contains("refresh_required")
    {
        return Err(
            io::Error::other("stale lint did not fail closed with typed refresh guidance").into(),
        );
    }
    if mcp_database_snapshot(&database)? != stale_before {
        return Err(io::Error::other("stale lint repaired or mutated SQLite state").into());
    }
    Ok(())
}

#[test]
fn lint_purpose_levels_require_agent_review_at_configured_scope() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir(repo.join("assets"))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n\n[purpose.styles_by_extension]\n\".toml\" = \"line-comment\"\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME)
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n  assets/logo.svg,SVG brand asset for purpose lint strictness tests\n",
    )?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    fs::write(
        repo.join("Cargo.toml"),
        "# Purpose: Rust manifest for purpose lint strictness tests.\n[package]\nname = \"purpose-lint-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("detail.rs"),
        "// Purpose: Rust implementation detail for purpose lint strictness tests.\npub fn detail() {}\n",
    )?;
    fs::write(
        repo.join("assets").join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )?;
    let db = temp.path().join("projectatlas.db");
    let config = repo.join(ATLAS_DIR_NAME).join("config.toml");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "low"])
        .assert()
        .success();

    let fresh_strict = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "strict"])
        .output()?;
    if fresh_strict.status.success() {
        return Err(io::Error::other("fresh strict purpose lint unexpectedly passed").into());
    }
    let fresh_strict_stdout = String::from_utf8(fresh_strict.stdout)?;
    if !fresh_strict_stdout.contains("[missing-purpose]")
        && !fresh_strict_stdout.contains("[suggested-purpose-review]")
    {
        return Err(io::Error::other(format!(
            "fresh strict purpose lint did not report missing or suggested purposes:\n{fresh_strict_stdout}"
        ))
        .into());
    }

    let store = AtlasStore::open(&db)?;
    if !store
        .load_nodes_by_paths(&[".gitignore".to_string()])?
        .is_empty()
    {
        store.set_purpose(
            ".gitignore",
            "Agent-reviewed ignore policy for purpose lint tests",
            PurposeSource::Agent,
        )?;
    }
    for (path, purpose) in [
        (".", "Imported repository root purpose"),
        ("assets", "Imported asset folder purpose"),
        (SRC_DIR_NAME, "Imported source folder purpose"),
        ("Cargo.toml", "Imported Rust manifest purpose"),
        ("src/detail.rs", "Imported implementation detail purpose"),
        ("assets/logo.svg", "Imported SVG brand asset purpose"),
    ] {
        store.set_purpose(path, purpose, PurposeSource::Imported)?;
    }

    let low = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if !low.status.success() {
        return Err(io::Error::other(format!(
            "low purpose lint should keep first-pass curation advisory:\n{}",
            String::from_utf8_lossy(&low.stderr)
        ))
        .into());
    }
    let low_stdout = String::from_utf8(low.stdout)?;
    for unexpected in [
        "purpose-agent-review-required",
        "src/detail.rs",
        "assets/logo.svg",
    ] {
        if low_stdout.contains(unexpected) {
            return Err(io::Error::other(format!(
                "low purpose lint should not block on advisory curation work `{unexpected}`:\n{low_stdout}"
            ))
            .into());
        }
    }

    let medium = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "medium"])
        .output()?;
    if medium.status.success() {
        return Err(io::Error::other("medium purpose lint unexpectedly passed").into());
    }
    let medium_stdout = String::from_utf8(medium.stdout)?;
    if !medium_stdout.contains("[purpose-agent-review-required] src/detail.rs:") {
        return Err(io::Error::other(format!(
            "medium purpose lint missed source file:\n{medium_stdout}"
        ))
        .into());
    }
    if medium_stdout.contains("assets/logo.svg") {
        return Err(io::Error::other(format!(
            "medium purpose lint included asset file:\n{medium_stdout}"
        ))
        .into());
    }

    let strict = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "strict"])
        .output()?;
    if strict.status.success() {
        return Err(io::Error::other("strict purpose lint unexpectedly passed").into());
    }
    let strict_stdout = String::from_utf8(strict.stdout)?;
    if !strict_stdout.contains("[purpose-agent-review-required] assets/logo.svg:") {
        return Err(io::Error::other(format!(
            "strict purpose lint missed asset file:\n{strict_stdout}"
        ))
        .into());
    }

    for (path, purpose) in [
        (".", "Agent-reviewed repository root purpose"),
        ("assets", "Agent-reviewed asset folder purpose"),
        (SRC_DIR_NAME, "Agent-reviewed source folder purpose"),
        ("Cargo.toml", "Agent-reviewed Rust manifest purpose"),
        (
            "src/detail.rs",
            "Agent-reviewed implementation detail purpose",
        ),
        ("assets/logo.svg", "Agent-reviewed SVG brand asset purpose"),
    ] {
        store.set_purpose(path, purpose, PurposeSource::Agent)?;
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "strict"])
        .assert()
        .success();

    fs::write(
        repo.join("Cargo.toml"),
        "# Purpose: Rust manifest for purpose lint strictness tests.\n[package]\nname = \"purpose-lint-demo\"\nversion = \"0.1.1\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("detail.rs"),
        "// Purpose: Rust implementation detail for purpose lint strictness tests.\npub fn detail_changed() {}\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["watch", "--once"])
        .assert()
        .success();
    let changed_low = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if !changed_low.status.success() {
        return Err(io::Error::other(format!(
            "low purpose lint invalidated an accepted purpose after an ordinary source change:\n{}",
            String::from_utf8_lossy(&changed_low.stderr)
        ))
        .into());
    }
    let changed_low_stdout = String::from_utf8(changed_low.stdout)?;
    if changed_low_stdout.contains("[stale-purpose]") {
        return Err(io::Error::other(format!(
            "ordinary source changes produced stale-purpose findings:\n{changed_low_stdout}"
        ))
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "strict"])
        .assert()
        .success();

    let changed_manifest = json_summary_command(&repo, &db, "Cargo.toml")?;
    require_json_string(
        &changed_manifest,
        &["file_purpose"],
        "Agent-reviewed Rust manifest purpose",
    )?;
    require_json_string(&changed_manifest, &["file_purpose_source"], "agent")?;
    require_json_bool(&changed_manifest, &["file_purpose_agent_reviewed"], true)?;

    Ok(())
}

/// Persistent real MCP session used by E2E contract clients.
struct McpContractSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    responses: Receiver<io::Result<String>>,
    stdout_reader: Option<thread::JoinHandle<()>>,
    stderr_reader: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    next_request_id: u64,
}

#[allow(dead_code)]
impl McpContractSession {
    /// Spawn and initialize one telemetry-disabled release-candidate MCP process.
    fn spawn(executable: &Path, repo: &Path, database: &Path) -> Result<Self, Box<dyn Error>> {
        let (session, _initialized) = Self::spawn_initialized(
            executable,
            repo,
            database,
            &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        )?;
        Ok(session)
    }

    /// Spawn and initialize one release-candidate MCP process.
    fn spawn_initialized(
        executable: &Path,
        repo: &Path,
        database: &Path,
        environment: &[(&str, Option<&str>)],
    ) -> Result<(Self, Value), Box<dyn Error>> {
        let mut command = StdCommand::new(executable);
        command
            .current_dir(repo)
            .arg("--db")
            .arg(database)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in environment {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("MCP contract stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("MCP contract stdout was not piped"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("MCP contract stderr was not piped"))?;
        let (sender, responses) = mpsc::sync_channel(64);
        let stdout_reader = thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let response = match stdout.read_line(&mut line) {
                    Ok(0) => Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "MCP contract stdout closed",
                    )),
                    Ok(_) => Ok(line),
                    Err(error) => Err(error),
                };
                let terminal = response.is_err();
                if sender.send(response).is_err() || terminal {
                    break;
                }
            }
        });
        let stderr_reader = thread::spawn(move || {
            let mut output = Vec::new();
            stderr.read_to_end(&mut output)?;
            Ok(output)
        });
        let mut session = Self {
            child: Some(child),
            stdin: Some(stdin),
            responses,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            next_request_id: 1,
        };
        let operation_result = (|| -> Result<Value, Box<dyn Error>> {
            let initialized = session.request(
                "initialize",
                &serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "projectatlas-mcp-contract",
                        "version": "0.4.0"
                    }
                }),
            )?;
            if initialized.get("result").is_none() {
                return Err(io::Error::other("MCP contract initialize omitted result").into());
            }
            session.notify("notifications/initialized", &serde_json::json!({}))?;
            Ok(initialized)
        })();
        match operation_result {
            Ok(initialized) => Ok((session, initialized)),
            Err(error) => complete_mcp_test_after_shutdown(Err(error), || session.shutdown()),
        }
    }

    /// Call one real MCP tool and return its text payload.
    fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
        self.call_tool_text(name, arguments, false)
    }

    /// Call one real MCP tool and require a visible tool-level error result.
    fn call_tool_error(&mut self, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
        self.call_tool_text(name, arguments, true)
    }

    /// Call one real MCP tool and require the expected `isError` state.
    fn call_tool_text(
        &mut self,
        name: &str,
        arguments: &Value,
        expected_error: bool,
    ) -> Result<String, Box<dyn Error>> {
        let response = self.request(
            "tools/call",
            &serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        if response.get("error").is_some() {
            return Err(io::Error::other(format!(
                "MCP contract tool {name} returned a protocol error: {response}"
            ))
            .into());
        }
        let is_error = response
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "MCP contract tool {name} omitted boolean result.isError: {response}"
                ))
            })?;
        if is_error != expected_error {
            return Err(io::Error::other(format!(
                "MCP contract tool {name} returned isError={is_error}, expected {expected_error}: {response}"
            ))
            .into());
        }
        response
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                io::Error::other(format!("MCP contract tool {name} returned no text")).into()
            })
    }

    /// Send one request and wait for its matching response under a fixed deadline.
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, Box<dyn Error>> {
        let request_id = self.start_request(method, params)?;
        self.wait_for_response(request_id, method)
    }

    /// Send one request and return its id before waiting for the response.
    fn start_request(&mut self, method: &str, params: &Value) -> Result<u64, Box<dyn Error>> {
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("MCP contract request id overflowed"))?;
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        }))?;
        Ok(request_id)
    }

    /// Wait for one previously sent request under the contract deadline.
    fn wait_for_response(
        &mut self,
        request_id: u64,
        method: &str,
    ) -> Result<Value, Box<dyn Error>> {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| io::Error::other("MCP contract response deadline overflowed"))?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("MCP contract request {request_id} for {method} timed out"),
                )
                .into());
            }
            let line = self
                .responses
                .recv_timeout(remaining)
                .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))??;
            let response: Value = serde_json::from_str(line.trim())?;
            if response.get("id").and_then(Value::as_u64) == Some(request_id) {
                return Ok(response);
            }
        }
    }

    /// Wait for a follow-up response while rejecting a late response to a
    /// request that the MCP peer already cancelled.
    fn wait_for_response_rejecting(
        &mut self,
        request_id: u64,
        method: &str,
        rejected_id: u64,
        rejected_method: &str,
    ) -> Result<Value, Box<dyn Error>> {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| io::Error::other("MCP contract response deadline overflowed"))?;
        let follow_up = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("MCP contract request {request_id} for {method} timed out"),
                )
                .into());
            }
            let line = match self.responses.recv_timeout(remaining) {
                Ok(line) => line?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("MCP contract request {request_id} for {method} timed out"),
                    )
                    .into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "MCP contract response stream disconnected",
                    )
                    .into());
                }
            };
            let response: Value = serde_json::from_str(line.trim())?;
            match response.get("id").and_then(Value::as_u64) {
                Some(id) if id == rejected_id => {
                    return Err(io::Error::other(format!(
                        "MCP emitted a late response for cancelled {rejected_method}: {response}"
                    ))
                    .into());
                }
                Some(id) if id == request_id => break response,
                _ => {}
            }
        };

        let grace_deadline = Instant::now()
            .checked_add(Duration::from_millis(300))
            .ok_or_else(|| io::Error::other("MCP cancellation grace deadline overflowed"))?;
        loop {
            let remaining = grace_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(follow_up);
            }
            let line = match self.responses.recv_timeout(remaining) {
                Ok(line) => line?,
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(follow_up),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "MCP contract response stream disconnected during cancellation grace window",
                    )
                    .into());
                }
            };
            let response: Value = serde_json::from_str(line.trim())?;
            if response.get("id").and_then(Value::as_u64) == Some(rejected_id) {
                return Err(io::Error::other(format!(
                    "MCP emitted a late response for cancelled {rejected_method}: {response}"
                ))
                .into());
            }
        }
    }

    /// Send one notification without waiting for a response.
    fn notify(&mut self, method: &str, params: &Value) -> Result<(), Box<dyn Error>> {
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    /// Write and flush one newline-delimited JSON-RPC message.
    fn write_message(&mut self, message: &Value) -> Result<(), Box<dyn Error>> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("MCP contract stdin was closed"))?;
        serde_json::to_writer(&mut *stdin, message)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    /// Close stdin and require a clean bounded process exit.
    fn shutdown(mut self) -> Result<(), Box<dyn Error>> {
        self.stdin.take();
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| io::Error::other("MCP contract shutdown deadline overflowed"))?;
        let status = loop {
            let child = self
                .child
                .as_mut()
                .ok_or_else(|| io::Error::other("MCP contract child was consumed"))?;
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                let _status = child.wait()?;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "MCP contract server did not exit after stdin closed",
                )
                .into());
            }
            thread::sleep(Duration::from_millis(25));
        };
        self.child.take();
        if let Some(reader) = self.stdout_reader.take() {
            reader
                .join()
                .map_err(|_panic| io::Error::other("MCP contract stdout reader panicked"))?;
        }
        let stderr = self
            .stderr_reader
            .take()
            .ok_or_else(|| io::Error::other("MCP contract stderr reader was consumed"))?
            .join()
            .map_err(|_panic| io::Error::other("MCP contract stderr reader panicked"))??;
        if !status.success() {
            return Err(io::Error::other(format!(
                "MCP contract server failed: {}",
                String::from_utf8_lossy(&stderr)
            ))
            .into());
        }
        Ok(())
    }
}

#[allow(dead_code)]
impl Drop for McpContractSession {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(child) = self.child.as_mut() {
            drop(child.kill());
            drop(child.wait());
        }
        self.child.take();
        if let Some(reader) = self.stdout_reader.take() {
            drop(reader.join());
        }
        if let Some(reader) = self.stderr_reader.take() {
            drop(reader.join());
        }
    }
}

/// Return the keys whose values changed between two bounded snapshots.
fn changed_snapshot_keys(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|table| before.get(*table) != after.get(*table))
        .cloned()
        .collect()
}

/// Return one top-level GitHub Actions job block from a workflow document.
fn workflow_job_block(workflow: &str, job: &str) -> Result<String, Box<dyn Error>> {
    let marker = format!("  {job}:");
    let mut found = false;
    let mut block = String::new();
    for line in workflow.lines() {
        if !found {
            if line == marker {
                found = true;
                block.push_str(line);
                block.push('\n');
            }
            continue;
        }
        if line.starts_with("  ") && !line.starts_with("    ") && line.trim_end().ends_with(':') {
            break;
        }
        block.push_str(line);
        block.push('\n');
    }
    if found {
        Ok(block)
    } else {
        Err(io::Error::other(format!("workflow job {job:?} not found")).into())
    }
}

/// Return every shell command owned by one GitHub Actions job.
fn workflow_job_runs(workflow: &str, job: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let documents = YamlLoader::load_from_str(workflow)?;
    let document = documents
        .first()
        .ok_or_else(|| io::Error::other("workflow document is empty"))?;
    let steps = document["jobs"][job]["steps"]
        .as_vec()
        .ok_or_else(|| io::Error::other(format!("workflow job {job:?} has no steps")))?;
    Ok(steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .map(str::to_owned)
        .collect())
}

/// Detect `ProjectAtlas` maintenance commands that belong only in local agent workflows.
fn command_runs_projectatlas_maintenance(command: &str) -> bool {
    const MAINTENANCE_COMMANDS: [&str; 5] = ["init", "scan", "purpose", "parity", "lint"];

    let command = command.replace("\\\r\n", " ").replace("\\\n", " ");
    command
        .lines()
        .flat_map(|line| line.split([';', '|', '&']))
        .any(|segment| {
            let tokens = segment
                .split_whitespace()
                .map(|token| {
                    token.trim_matches(|character: char| {
                        matches!(character, '\'' | '"' | '(' | ')' | '{' | '}')
                    })
                })
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>();

            if let Some(executable) = tokens.iter().position(|token| {
                let name = token
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(token)
                    .to_ascii_lowercase();
                matches!(name.as_str(), "projectatlas" | "projectatlas.exe")
            }) {
                return tokens[executable + 1..]
                    .iter()
                    .any(|token| MAINTENANCE_COMMANDS.contains(token));
            }

            let Some(cargo) = tokens.iter().position(|token| {
                let name = token
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(token)
                    .to_ascii_lowercase();
                matches!(name.as_str(), "cargo" | "cargo.exe")
            }) else {
                return false;
            };
            let Some(run) = tokens[cargo + 1..]
                .iter()
                .position(|token| *token == "run")
                .map(|index| cargo + 1 + index)
            else {
                return false;
            };
            let Some(arguments) = tokens[run + 1..]
                .iter()
                .rposition(|token| *token == "--")
                .map(|index| run + 1 + index)
            else {
                return false;
            };
            let owns_projectatlas_cli = tokens[run + 1..arguments]
                .windows(2)
                .any(|pair| matches!(pair[0], "-p" | "--package") && pair[1] == "projectatlas-cli")
                || tokens[run + 1..arguments].contains(&"--package=projectatlas-cli");

            owns_projectatlas_cli
                && tokens[arguments + 1..]
                    .iter()
                    .any(|token| MAINTENANCE_COMMANDS.contains(token))
        })
}

/// Require every GitHub Actions `uses:` reference to pin an immutable 40-char SHA.
fn assert_actions_are_sha_pinned(name: &str, workflow: &str) -> Result<(), Box<dyn Error>> {
    for (index, line) in workflow.lines().enumerate() {
        let Some((_, reference)) = line.split_once("uses:") else {
            continue;
        };
        let reference = reference.split('#').next().unwrap_or("").trim();
        let Some((_, revision)) = reference.rsplit_once('@') else {
            return Err(io::Error::other(format!(
                "{name}:{} uses reference {reference:?} without an @revision",
                index + 1
            ))
            .into());
        };
        if revision.len() != 40
            || !revision
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(io::Error::other(format!(
                "{name}:{} uses reference {reference:?} is not pinned to a 40-character SHA",
                index + 1
            ))
            .into());
        }
    }
    Ok(())
}

/// Require one exact nested signed JSON integer value.
fn require_json_i64(value: &Value, path: &[&str], expected: i64) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_i64()
        .ok_or_else(|| io::Error::other(format!("expected signed integer at {path:?}")))?;
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected}, found {actual}"
        ))
        .into())
    }
}

#[test]
fn health_check_reports_duplicate_temp_folders() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("a").join("tmp"))?;
    fs::create_dir_all(repo.join("b").join("tmp"))?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .arg(&repo)
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("repeated-temporary-folder"));
    Ok(())
}

#[test]
fn purpose_file_seed_command_surface_is_removed() -> Result<(), Box<dyn Error>> {
    Command::cargo_bin("projectatlas")?
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("seed-purpose").not());
    Command::cargo_bin("projectatlas")?
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--seed-purpose").not());
    Ok(())
}

#[test]
fn init_map_and_lint_flow_uses_rust_implementation() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), "fn main() {}\n")?;
    fs::write(
        repo.join("README.md"),
        "# Demo readme for Rust map lint tests\n",
    )?;
    fs::write(repo.join("logo.png"), b"png")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("init")
        .assert()
        .success();
    let generated_config = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if generated_config.contains("purpose_filename") || generated_config.contains(".purpose") {
        return Err(io::Error::other(format!(
            "init config advertised legacy purpose files: {generated_config}"
        ))
        .into());
    }
    if repo.join(".purpose").exists() || repo.join(SRC_DIR_NAME).join(".purpose").exists() {
        return Err(io::Error::other("init created legacy .purpose files").into());
    }
    fs::write(
        repo.join(ATLAS_DIR_NAME)
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n  logo.png,Demo asset for Rust map lint tests\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", "."])
        .assert()
        .success();
    for (path, purpose) in [
        (".", "Demo repository for Rust map lint tests"),
        (SRC_DIR_NAME, "Rust source folder for CLI integration tests"),
        ("README.md", "Demo readme for Rust map lint tests"),
        (
            "src/main.rs",
            "Provide a tiny Rust entry point for ProjectAtlas tests",
        ),
        (
            ".projectatlas/projectatlas-nonsource-files.toon",
            "Declare non-source file purposes for ProjectAtlas CLI integration tests",
        ),
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["purpose", "set", path, purpose])
            .assert()
            .success();
    }
    let store = AtlasStore::open(&repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))?;
    if !store
        .load_nodes_by_paths(&[ATLAS_DIR_NAME.to_string()])?
        .is_empty()
    {
        store.set_purpose(
            ATLAS_DIR_NAME,
            "Agent-reviewed ProjectAtlas metadata folder for CLI integration tests",
            PurposeSource::Agent,
        )?;
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["map", "--force"])
        .assert()
        .success();

    let map = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"))?;
    if !map.contains("src/main.rs") {
        return Err(io::Error::other("generated atlas did not include src/main.rs").into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["lint", "--strict-folders", "--report-untracked"])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("approved_purposes: 8"));

    Ok(())
}
