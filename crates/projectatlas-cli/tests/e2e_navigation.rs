//! Purpose: Validate CLI, MCP, graph, document, and language navigation contracts.
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
    McpDatabaseSnapshot, complete_mcp_test_after_shutdown, git_command_for_root, json_at,
    json_summary_command, mcp_contract_executable, mcp_database_snapshot, mcp_tool_text,
    require_json_array_len, require_json_bool, require_json_contains, require_json_string,
    require_json_usize, require_json_usize_at_least, require_json_usize_greater_than,
    run_mcp_stdio, run_mcp_stdio_with_env, sha256_hex, sqlite_table_digests, workspace_root,
};
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";

const SRC_DIR_NAME: &str = "src";

const TESTS_DIR_NAME: &str = "tests";

const GUIDE_MD_PATH: &str = "docs/guide.md";

const INSTALLER_RS_FILE_NAME: &str = "installer.rs";

const LIB_RS_FILE_NAME: &str = "lib.rs";

const GIT_DIR_NAME: &str = ".git";

const ATLAS_DIR_NAME: &str = ".projectatlas";

const TS_CONFIG_FILE_NAME: &str = "tsconfig.json";

const CARGO_LOCK_FILE_NAME: &str = "Cargo.lock";

const AGENT_EFFICIENCY_BENCHMARK_PATH: &str =
    "../../docs/benchmarks/v0.4-agent-navigation-results.json";

const AGENT_EFFICIENCY_PARTIAL_FILE: &str = "partial.json";

/// Return one `SQLite` sidecar path for exact no-mutation assertions.
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

#[test]
fn detailed_relation_cli_bounds_the_exact_json_envelope() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let source_dir = repo.join(SRC_DIR_NAME);
    fs::create_dir_all(&source_dir)?;
    fs::write(
        source_dir.join("lib.rs"),
        "pub fn first() { second(); third(); fourth(); fifth(); sixth(); seventh(); eighth(); ninth(); }\n\
         fn second() {}\nfn third() {}\nfn fourth() {}\nfn fifth() {}\n\
         fn sixth() {}\nfn seventh() {}\nfn eighth() {}\nfn ninth() {}\n",
    )?;

    let scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "scan"])
        .output()?;
    if !scan.status.success() {
        return Err(io::Error::other(format!(
            "relation CLI fixture scan failed: {}",
            String::from_utf8_lossy(&scan.stderr)
        ))
        .into());
    }
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let store = AtlasStore::open_for_project(&database, &repo)?;
    store.set_purpose(
        "src/lib.rs",
        "Own café λ relation navigation",
        PurposeSource::Agent,
    )?;
    drop(store);

    let page_output_bytes = 64 * 1024_usize;
    let first_page = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--symbol",
            "first",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "1",
            "--output-bytes",
            &page_output_bytes.to_string(),
        ])
        .output()?;
    if !first_page.status.success() {
        return Err(io::Error::other(format!(
            "first detailed relation cursor page failed: {}",
            String::from_utf8_lossy(&first_page.stderr)
        ))
        .into());
    }
    let first_payload: Value = serde_json::from_slice(&first_page.stdout)?;
    require_json_usize(&first_payload, &["symbol_relations", "returned"], 1)?;
    require_json_string(
        &first_payload,
        &["symbol_relations", "anchor", "purpose", "purpose"],
        "Own café λ relation navigation",
    )?;
    let continuation = first_payload
        .pointer("/symbol_relations/continuation")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("first detailed relation page omitted its cursor"))?;
    let second_page = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--symbol",
            "first",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "1",
            "--output-bytes",
            &page_output_bytes.to_string(),
            "--cursor",
            continuation,
        ])
        .output()?;
    if !second_page.status.success() {
        return Err(io::Error::other(format!(
            "second detailed relation cursor page failed: {}",
            String::from_utf8_lossy(&second_page.stderr)
        ))
        .into());
    }
    let second_payload: Value = serde_json::from_slice(&second_page.stdout)?;
    require_json_usize(&second_payload, &["symbol_relations", "returned"], 1)?;
    if first_payload.pointer("/symbol_relations/rows/0")
        == second_payload.pointer("/symbol_relations/rows/0")
    {
        return Err(io::Error::other("detailed relation cursor replayed the first row").into());
    }

    let maximum_output_bytes = 4 * 1024_usize;
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "50",
            "--output-bytes",
            &maximum_output_bytes.to_string(),
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "bounded detailed relation CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    if output.stdout.len() > maximum_output_bytes {
        return Err(io::Error::other(format!(
            "bounded detailed relation CLI emitted {} bytes above its {maximum_output_bytes}-byte ceiling",
            output.stdout.len()
        ))
        .into());
    }
    let payload: Value = serde_json::from_slice(&output.stdout)?;
    require_json_usize(
        &payload,
        &["symbol_relations", "work", "rendered_output_bytes"],
        output.stdout.len(),
    )?;
    require_json_string(
        &payload,
        &["symbol_relations", "anchor", "purpose", "purpose"],
        "Own café λ relation navigation",
    )?;
    require_json_usize(&payload, &["symbol_relations", "returned"], 0)?;

    let analysis = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "analysis",
            "--file",
            "src/lib.rs",
            "--symbol",
            "first",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "50",
            "--output-bytes",
            &page_output_bytes.to_string(),
            "--include-communities",
            "--include-cycles",
        ])
        .output()?;
    if !analysis.status.success() {
        return Err(io::Error::other(format!(
            "public relation analysis CLI failed: {}",
            String::from_utf8_lossy(&analysis.stderr)
        ))
        .into());
    }
    let analysis_payload: Value = serde_json::from_slice(&analysis.stdout)?;
    require_json_string(
        &analysis_payload,
        &["symbol_relations", "mode"],
        "architecture",
    )?;
    require_json_usize(
        &analysis_payload,
        &["symbol_relations", "work", "rendered_output_bytes"],
        analysis.stdout.len(),
    )?;
    if analysis_payload
        .pointer("/symbol_relations/findings")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
        || !String::from_utf8_lossy(&analysis.stdout).contains("next_call")
    {
        return Err(io::Error::other(
            "public relation analysis CLI omitted findings or reusable next-call routing",
        )
        .into());
    }

    let impact_started = Instant::now();
    let impact = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "analysis",
            "--file",
            "src/lib.rs",
            "--symbol",
            "first",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "8",
            "--edge-limit",
            "8",
            "--node-limit",
            "16",
            "--visited-limit",
            "16",
            "--occurrence-total-limit",
            "16",
            "--intermediate-bytes",
            "131072",
            "--deadline-ms",
            "1000",
            "--output-bytes",
            "65536",
            "--analysis-mode",
            "impact",
            "--vcs",
            "working-tree",
            "--include-dead-code",
        ])
        .output()?;
    let impact_elapsed = impact_started.elapsed();
    if !impact.status.success() {
        return Err(io::Error::other(format!(
            "public impact analysis CLI failed: {}",
            String::from_utf8_lossy(&impact.stderr)
        ))
        .into());
    }
    if impact_elapsed > Duration::from_secs(5) {
        return Err(io::Error::other(format!(
            "bounded impact CLI exceeded its elapsed tolerance: {impact_elapsed:?}"
        ))
        .into());
    }
    let impact_payload: Value = serde_json::from_slice(&impact.stdout)?;
    require_json_string(&impact_payload, &["symbol_relations", "mode"], "impact")?;
    require_json_usize(
        &impact_payload,
        &["symbol_relations", "work", "rendered_output_bytes"],
        impact.stdout.len(),
    )?;
    let bounded_work = [
        ("/symbol_relations/returned", 8_u64),
        ("/symbol_relations/work/relations/inspected_edges", 8),
        ("/symbol_relations/work/relations/active_nodes", 16),
        ("/symbol_relations/work/relations/visited_nodes", 16),
        ("/symbol_relations/work/analyzed_nodes", 16),
        ("/symbol_relations/work/analyzed_edges", 8),
        ("/symbol_relations/work/peak_intermediate_bytes", 131_072),
    ];
    if impact.stdout.len() > 65_536
        || bounded_work.iter().any(|(path, limit)| {
            impact_payload
                .pointer(path)
                .and_then(Value::as_u64)
                .is_none_or(|observed| observed > *limit)
        })
    {
        return Err(io::Error::other(
            "bounded impact CLI crossed a declared row/node/edge/visited/intermediate/output budget",
        )
        .into());
    }
    if !matches!(
        impact_payload.pointer("/symbol_relations/vcs/state"),
        Some(Value::String(state)) if state == "available" || state == "unavailable"
    ) {
        return Err(io::Error::other("impact CLI omitted typed VCS state").into());
    }

    let trace_target = serde_json::json!({
        "kind": "symbol",
        "file": "src/lib.rs",
        "name": "second",
        "symbol_kind": "function",
        "parent": null,
        "signature": "fn second ( )"
    })
    .to_string();
    let trace = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "analysis",
            "--file",
            "src/lib.rs",
            "--symbol",
            "first",
            "--direction",
            "outbound",
            "--depth",
            "2",
            "--limit",
            "50",
            "--analysis-mode",
            "trace",
            "--trace-target",
            &trace_target,
        ])
        .output()?;
    if !trace.status.success() {
        return Err(io::Error::other(format!(
            "public trace analysis CLI failed: {}",
            String::from_utf8_lossy(&trace.stderr)
        ))
        .into());
    }
    let trace_payload: Value = serde_json::from_slice(&trace.stdout)?;
    require_json_string(&trace_payload, &["symbol_relations", "mode"], "trace")?;
    if !trace_payload
        .pointer("/symbol_relations/findings")
        .and_then(Value::as_array)
        .is_some_and(|findings| {
            findings.iter().any(|finding| {
                finding.get("kind").and_then(Value::as_str) == Some("static_trace")
                    && finding.get("status").and_then(Value::as_str) == Some("confirmed")
                    && finding
                        .get("nodes")
                        .and_then(Value::as_array)
                        .is_some_and(|nodes| {
                            nodes.iter().any(|node| {
                                node.pointer("/node/entity/selector/symbol/name")
                                    .and_then(Value::as_str)
                                    == Some("second")
                                    && node.get("next_call").is_some()
                            })
                        })
            })
        })
    {
        return Err(io::Error::other(
            "trace CLI omitted the confirmed path or exact reusable symbol selector",
        )
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--analysis-mode",
            "impact",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "analysis controls require --view analysis",
        ));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "symbols",
            "relations",
            "--view",
            "analysis",
            "--file",
            "src/lib.rs",
            "--analysis-mode",
            "trace",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "analysis trace requires an exact file or symbol target",
        ));
    Ok(())
}

#[test]
fn impact_analysis_deadline_and_mcp_cancellation_release_resources() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("bounded-impact-adapters");
    let source_dir = repo.join(SRC_DIR_NAME);
    fs::create_dir_all(&source_dir)?;
    let mut source = String::from("pub fn entry() { leaf(); }\nfn leaf() {}\n");
    for index in 0..12_000 {
        writeln!(source, "fn unused_{index}() {{}}")?;
    }
    fs::write(source_dir.join("lib.rs"), source)?;

    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let scan = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "scan"])
        .output()?;
    if !scan.status.success() {
        return Err(io::Error::other(format!(
            "bounded impact fixture scan failed: {}",
            String::from_utf8_lossy(&scan.stderr)
        ))
        .into());
    }
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let before = mcp_database_snapshot(&database)?;
    let common_cli_args = [
        "--format",
        "json",
        "symbols",
        "relations",
        "--view",
        "analysis",
        "--file",
        "src/lib.rs",
        "--symbol",
        "entry",
        "--direction",
        "outbound",
        "--depth",
        "2",
        "--limit",
        "1000",
        "--edge-limit",
        "10000",
        "--node-limit",
        "10000",
        "--visited-limit",
        "10000",
        "--occurrence-total-limit",
        "20000",
        "--intermediate-bytes",
        "33554432",
        "--deadline-ms",
        "1",
        "--output-bytes",
        "1048576",
        "--analysis-mode",
        "impact",
        "--vcs",
        "working-tree",
        "--include-dead-code",
    ];
    let cli_started = Instant::now();
    let expired = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(common_cli_args)
        .output()?;
    if expired.status.success()
        || cli_started.elapsed() > Duration::from_secs(5)
        || !String::from_utf8_lossy(&expired.stderr)
            .to_ascii_lowercase()
            .contains("deadline")
        || mcp_database_snapshot(&database)? != before
    {
        return Err(io::Error::other(format!(
            "CLI deadline was not typed, prompt, and read-only: status={:?} elapsed={:?} stderr={}",
            expired.status.code(),
            cli_started.elapsed(),
            String::from_utf8_lossy(&expired.stderr)
        ))
        .into());
    }
    let follow_up = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "overview"])
        .output()?;
    if !follow_up.status.success() {
        return Err(io::Error::other("CLI deadline blocked the immediate overview").into());
    }
    Connection::open(&database)?.execute_batch("BEGIN IMMEDIATE; ROLLBACK;")?;

    let impact_arguments = |deadline_ms| {
        json!({
            "view": "analysis",
            "analysis_mode": "impact",
            "vcs": "working_tree",
            "file": "src/lib.rs",
            "symbol": "entry",
            "direction": "outbound",
            "depth": 2,
            "limit": 1000,
            "edge_limit": 20000,
            "node_limit": 10000,
            "visited_limit": 10000,
            "occurrence_total_limit": 20000,
            "intermediate_bytes": 33_554_432,
            "deadline_ms": deadline_ms,
            "output_bytes": 1_048_576,
            "include_dead_code": true
        })
    };
    let mut session = McpContractSession::spawn(&executable, &repo, &database)?;
    let mcp_started = Instant::now();
    let deadline_text = session.call_tool("atlas_symbol_relations", &impact_arguments(1_u64))?;
    let deadline_payload: Value = toon_format::decode_default(&deadline_text)?;
    if mcp_started.elapsed() > Duration::from_secs(5)
        || deadline_payload
            .pointer("/error/kind")
            .and_then(Value::as_str)
            != Some("error")
        || !deadline_payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.to_ascii_lowercase().contains("deadline"))
        || mcp_database_snapshot(&database)? != before
    {
        return Err(io::Error::other(format!(
            "MCP deadline was not typed, prompt, and read-only: elapsed={:?} payload={deadline_payload}",
            mcp_started.elapsed()
        ))
        .into());
    }
    session.call_tool("atlas_overview", &json!({}))?;
    Connection::open(&database)?.execute_batch("BEGIN IMMEDIATE; ROLLBACK;")?;

    let request_id = session.start_request(
        "tools/call",
        &json!({
            "name": "atlas_symbol_relations",
            "arguments": impact_arguments(5_000_u64)
        }),
    )?;
    thread::sleep(Duration::from_millis(1));
    session.notify(
        "notifications/cancelled",
        &json!({"requestId": request_id, "reason": "bounded impact contract"}),
    )?;
    // RMCP 3.x follows the MCP cancellation contract: a response to a request
    // already cancelled on the wire is intentionally suppressed.  The follow-up
    // request proves the server remains responsive and also lets the helper reject
    // an incorrectly emitted late response for the cancelled request.
    let follow_up_id = session.start_request(
        "tools/call",
        &json!({"name": "atlas_overview", "arguments": {}}),
    )?;
    let follow_up = session.wait_for_response_rejecting(
        follow_up_id,
        "tools/call",
        request_id,
        "cancelled tools/call",
    )?;
    if follow_up.get("result").is_none() || mcp_database_snapshot(&database)? != before {
        return Err(io::Error::other(format!(
            "MCP cancellation did not preserve a live, read-only follow-up: {follow_up}"
        ))
        .into());
    }
    Connection::open(&database)?.execute_batch("BEGIN IMMEDIATE; ROLLBACK;")?;
    session.shutdown()
}

#[test]
fn installed_candidate_without_git_keeps_navigation_and_typed_vcs_unavailability()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("primary-checkout-without-git");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn entry() {}\n",
    )?;
    git_success(&repo, &["init", "--quiet"])?;
    if !fs::metadata(repo.join(GIT_DIR_NAME))?.is_dir() {
        return Err(io::Error::other("no-Git fixture is not an ordinary primary checkout").into());
    }

    let executable = mcp_contract_executable();
    if !executable.is_absolute() {
        return Err(io::Error::other(format!(
            "no-Git contract requires an absolute candidate executable: {}",
            executable.display()
        ))
        .into());
    }
    let restricted_bin = temp.path().join("path-without-git");
    fs::create_dir(&restricted_bin)?;
    let restricted_path = std::env::join_paths([restricted_bin])?;
    let restricted_path_text = restricted_path
        .to_str()
        .ok_or_else(|| io::Error::other("restricted no-Git PATH is not UTF-8"))?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");

    let missing_index = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PATH", &restricted_path)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&database)
        .args(["--format", "json", "overview"])
        .output()?;
    if missing_index.status.success() {
        return Err(io::Error::other("no-Git overview reused state without a local index").into());
    }
    let missing_index_error: Value = serde_json::from_slice(&missing_index.stderr)?;
    let expected_project_root =
        projectatlas_core::normalize_native_path_display(repo.canonicalize()?);
    let expected_database = projectatlas_core::normalize_native_path_display(&database);
    require_json_string(&missing_index_error, &["error", "kind"], "init_required")?;
    require_json_string(
        &missing_index_error,
        &["error", "init_required", "project_root"],
        &expected_project_root,
    )?;
    require_json_string(
        &missing_index_error,
        &["error", "init_required", "database"],
        &expected_database,
    )?;
    require_json_string(
        &missing_index_error,
        &["error", "next", "project_path"],
        &expected_project_root,
    )?;
    if repo.join(ATLAS_DIR_NAME).exists() {
        return Err(
            io::Error::other("no-Git init_required probe created local atlas state").into(),
        );
    }

    for arguments in [
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "overview".to_string(),
        ],
    ] {
        let output = StdCommand::new(&executable)
            .current_dir(&repo)
            .env("PATH", &restricted_path)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .args(&arguments)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "installed candidate failed without Git for {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        serde_json::from_slice::<Value>(&output.stdout)?;
    }

    let project_path = repo.to_string_lossy().to_string();
    let (mut session, _initialized) = McpContractSession::spawn_initialized(
        &executable,
        &repo,
        &database,
        &[
            ("PROJECTATLAS_NO_TELEMETRY", Some("1")),
            ("PATH", Some(restricted_path_text)),
        ],
    )?;
    let operation_result = (|| -> Result<(), Box<dyn Error>> {
        for (tool, arguments, expected) in [
            (
                "atlas_session_brief",
                json!({"project_path": project_path, "query": "src/lib.rs", "compact": true}),
                "session_brief:",
            ),
            (
                "atlas_overview",
                json!({"project_path": project_path}),
                "overview:",
            ),
            (
                "atlas_file_summary",
                json!({"project_path": project_path, "file": "src/lib.rs", "compact": true}),
                "file_summary:",
            ),
        ] {
            let text = session.call_tool(tool, &arguments)?;
            if !text.contains(expected) {
                return Err(io::Error::other(format!(
                    "{tool} failed local navigation without Git: {text}"
                ))
                .into());
            }
        }

        let atlas_before_vcs = mcp_database_snapshot(&database)?;
        let impact_text = session.call_tool(
            "atlas_symbol_relations",
            &json!({
                "project_path": project_path,
                "view": "analysis",
                "analysis_mode": "impact",
                "vcs": "working_tree",
                "file": "src/lib.rs",
                "symbol": "entry",
                "direction": "outbound",
                "depth": 2,
                "limit": 50,
                "output_bytes": 65_536
            }),
        )?;
        let impact: Value = toon_format::decode_default(&impact_text)?;
        require_json_string(
            &impact,
            &["symbol_relations", "vcs", "state"],
            "unavailable",
        )?;
        let reason = json_at(&impact, &["symbol_relations", "vcs", "reason"])?
            .as_str()
            .ok_or_else(|| io::Error::other("typed VCS unavailability omitted its reason"))?;
        if !reason.contains("git could not start") {
            return Err(io::Error::other(format!(
                "missing Git did not remain typed VCS-only unavailability: {impact_text}"
            ))
            .into());
        }
        if mcp_database_snapshot(&database)? != atlas_before_vcs {
            return Err(
                io::Error::other("missing-Git VCS analysis changed local atlas state").into(),
            );
        }
        let readable = session.call_tool(
            "atlas_file_summary",
            &json!({"project_path": project_path, "file": "src/lib.rs", "compact": true}),
        )?;
        if !readable.contains("file_summary:") {
            return Err(io::Error::other(format!(
                "local atlas was unreadable after missing-Git VCS analysis: {readable}"
            ))
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(operation_result, || session.shutdown())
}

#[test]
fn cli_navigation_output_survives_telemetry_write_failure() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn owner() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("init")
        .assert()
        .success();

    let db_path = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let connection = Connection::open(db_path)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env_remove("PROJECTATLAS_NO_TELEMETRY")
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("overview:"))
        .stdout(predicate::str::contains("files:"));
    connection.execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn cli_invocations_with_one_label_use_distinct_sealed_instances() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn owner() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("init")
        .assert()
        .success();
    for _ in 0..2 {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env_remove("PROJECTATLAS_NO_TELEMETRY")
            .arg("overview")
            .assert()
            .success();
    }

    let connection = Connection::open(repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))?;
    let instances: i64 = connection.query_row(
        "SELECT COUNT(*) FROM usage_instances WHERE owner = 'cli_invocation' AND caller_label = 'default' AND state = 'sealed'",
        [],
        |row| row.get(0),
    )?;
    if instances != 2 {
        return Err(io::Error::other(format!(
            "expected two sealed CLI invocation identities, found {instances}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn bare_relative_projectatlas_config_path_drives_scan_map_and_lint() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME)
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(
        repo.join(".purpose"),
        "Repository root for bare config path regression tests\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(".purpose"),
        "Rust source folder for bare config path regression tests\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "// Purpose: Rust entry point for bare config path regression tests.\nfn main() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--db",
            ".projectatlas/projectatlas.db",
            "--config",
            ".projectatlas/config.toml",
            "scan",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("files: 3"))
        .stderr(predicate::str::contains("io error for \"\"").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--format",
            "json",
            "--db",
            ".projectatlas/projectatlas.db",
            "overview",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"files\": 3"));

    let store = AtlasStore::open(&repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))?;
    for (path, purpose) in [
        (".", "Agent-reviewed bare config regression root"),
        (
            ATLAS_DIR_NAME,
            "Agent-reviewed ProjectAtlas metadata folder for bare config tests",
        ),
        (
            SRC_DIR_NAME,
            "Agent-reviewed Rust source folder for bare config tests",
        ),
        (
            "src/main.rs",
            "Agent-reviewed Rust entry point for bare config tests",
        ),
    ] {
        if !store.load_nodes_by_paths(&[path.to_string()])?.is_empty() {
            store.set_purpose(path, purpose, PurposeSource::Agent)?;
        }
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--config",
            ".projectatlas/config.toml",
            "lint",
            "--strict-folders",
            "--report-untracked",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Atlas map missing").not());

    fs::write(
        repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"),
        "version: 1\noverview: tracked_source_files=0 tracked_nonsource_files=0 tracked_files_total=0 tracked_folders=0 source_extensions=0 exclude_dir_names=0 exclude_path_prefixes=0\nfile_hash: \"stale\"\nfolder_hash: \"stale\"\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--config",
            ".projectatlas/config.toml",
            "lint",
            "--strict-folders",
            "--report-untracked",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Atlas map").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--config", ".projectatlas/config.toml", "map", "--force"])
        .assert()
        .success()
        .stderr(predicate::str::contains("io error for \"\"").not());
    let map = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"))?;
    if !map.contains("src/main.rs") {
        return Err(io::Error::other("bare-config map omitted src/main.rs").into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--config",
            ".projectatlas/config.toml",
            "lint",
            "--strict-folders",
            "--report-untracked",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("io error for \"\"").not());
    Ok(())
}

#[test]
fn scan_overview_and_token_flow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    let mut source = "fn main() {\n    println!(\"hello\");\n}\n".to_string();
    for index in 0..120 {
        writeln!(
            &mut source,
            "fn helper_{index}() {{ println!(\"helper {index}\"); }}"
        )?;
    }
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), source)?;
    let db = temp.path().join("projectatlas.db");
    let outside_cwd = temp.path().join("outside-cwd");
    fs::create_dir(&outside_cwd)?;
    let rogue_repo = temp.path().join("rogue-repo");
    fs::create_dir(&rogue_repo)?;
    fs::create_dir(rogue_repo.join("rogue"))?;
    fs::write(rogue_repo.join("rogue").join("rogue.rs"), "fn rogue() {}\n")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("init")
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .arg(&repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("overview:"));

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("overview:"));

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["folders", SRC_DIR_NAME])
        .assert()
        .success()
        .stdout(predicate::str::contains("folders["));

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["files", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["search", "hello", "--file-pattern", "*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["outline", "src/main.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("outline:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
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
        .assert()
        .success()
        .stdout(predicate::str::contains("fn main"));

    let outside = temp.path().join("outside-project.txt");
    fs::write(&outside, "outside repo proof")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            outside.to_string_lossy().as_ref(),
            "--start-line",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "project-relative indexed file path",
        ));
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["outline", "../outside-project.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "project-relative indexed file path",
        ));
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["summary", "../outside-project.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "project-relative indexed file path",
        ));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("settings")
        .assert()
        .success()
        .stdout(predicate::str::contains("settings:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("watch-status")
        .assert()
        .success()
        .stdout(predicate::str::contains("watch_status:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("health_findings"));

    let raw_mcp_config = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("mcp-config")
        .output()?;
    if !raw_mcp_config.status.success() {
        return Err(io::Error::other("mcp-config command failed").into());
    }
    let mcp_config_json: Value = serde_json::from_slice(&raw_mcp_config.stdout)?;
    let command = mcp_config_json["mcpServers"]["projectatlas"]["command"]
        .as_str()
        .ok_or_else(|| io::Error::other("mcp command missing"))?;
    if !std::path::Path::new(command).is_absolute() {
        return Err(io::Error::other("mcp command was not absolute").into());
    }
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "0"],
        "--require-version",
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "1"],
        env!("CARGO_PKG_VERSION"),
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "2"],
        "--db",
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "4"],
        "--config",
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "6"],
        "mcp",
    )?;
    let mcp_args = mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("mcp args missing"))?;
    if mcp_args
        .iter()
        .any(|value| value.as_str() == Some("--nearest-project"))
    {
        return Err(io::Error::other(
            "default mcp-config unexpectedly enabled nearest-project routing",
        )
        .into());
    }
    let expected_root = repo.canonicalize()?;
    let config_path = mcp_args
        .get(5)
        .ok_or_else(|| io::Error::other("mcp config path missing"))?
        .as_str()
        .ok_or_else(|| io::Error::other("mcp config path missing"))?;
    if !std::path::Path::new(config_path).is_absolute() {
        return Err(io::Error::other("mcp config path was not absolute").into());
    }
    let generated_cwd = mcp_config_json["mcpServers"]["projectatlas"]["cwd"]
        .as_str()
        .ok_or_else(|| io::Error::other("mcp cwd missing"))?;
    if !std::path::Path::new(generated_cwd).is_absolute() {
        return Err(io::Error::other("mcp cwd was not absolute").into());
    }
    if cfg!(windows) && generated_cwd.starts_with(r"\\?\") {
        return Err(io::Error::other("mcp cwd used a Windows extended path prefix").into());
    }
    if std::path::Path::new(generated_cwd).canonicalize()? != expected_root {
        return Err(io::Error::other(format!(
            "mcp cwd mismatch: expected {expected_root:?}, got {generated_cwd}"
        ))
        .into());
    }

    let raw_nearest_mcp_config = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("mcp-config")
        .arg("--nearest-project")
        .output()?;
    if !raw_nearest_mcp_config.status.success() {
        return Err(io::Error::other("mcp-config --nearest-project command failed").into());
    }
    let nearest_mcp_config_json: Value = serde_json::from_slice(&raw_nearest_mcp_config.stdout)?;
    let nearest_args = nearest_mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("nearest mcp args missing"))?;
    if nearest_args.last().and_then(Value::as_str) != Some("--nearest-project")
        || !nearest_args
            .iter()
            .any(|value| value.as_str() == Some("mcp"))
    {
        return Err(io::Error::other(
            "mcp-config --nearest-project did not persist startup routing flag",
        )
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("root")
        .arg("set")
        .arg(&repo)
        .arg("--nearest-project")
        .assert()
        .success();
    let root_set_mcp_config_text =
        fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.mcp.json"))?;
    let root_set_mcp_config_json: Value = serde_json::from_str(&root_set_mcp_config_text)?;
    let root_set_args = root_set_mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("root set mcp args missing"))?;
    if root_set_args.last().and_then(Value::as_str) != Some("--nearest-project") {
        return Err(io::Error::other(
            "root set --nearest-project did not persist startup routing flag",
        )
        .into());
    }

    let claude_mcp_config = mcp_config_for_harness(&repo, &db, "claude-code")?;
    let claude_server = &claude_mcp_config["mcpServers"]["projectatlas"];
    let claude_command = claude_server["command"]
        .as_str()
        .ok_or_else(|| io::Error::other("claude mcp command missing"))?;
    if !std::path::Path::new(claude_command).is_absolute() {
        return Err(io::Error::other("claude mcp command was not absolute").into());
    }
    if claude_server.get("cwd").is_some() {
        return Err(io::Error::other("claude mcp config should not assume cwd support").into());
    }
    require_json_string(
        &claude_mcp_config,
        &["mcpServers", "projectatlas", "args", "0"],
        "--require-version",
    )?;
    require_json_string(
        &claude_mcp_config,
        &["mcpServers", "projectatlas", "args", "6"],
        "mcp",
    )?;

    let opencode_config = mcp_config_for_harness(&repo, &db, "opencode")?;
    require_json_string(
        &opencode_config,
        &["$schema"],
        "https://opencode.ai/config.json",
    )?;
    require_json_string(&opencode_config, &["mcp", "projectatlas", "type"], "local")?;
    let opencode_command = opencode_config["mcp"]["projectatlas"]["command"]
        .as_array()
        .ok_or_else(|| io::Error::other("opencode mcp command array missing"))?;
    let Some(first_command) = opencode_command.first().and_then(Value::as_str) else {
        return Err(io::Error::other("opencode command executable missing").into());
    };
    if !std::path::Path::new(first_command).is_absolute() {
        return Err(io::Error::other("opencode command executable was not absolute").into());
    }
    if !opencode_command
        .iter()
        .any(|value| value.as_str() == Some("mcp"))
    {
        return Err(io::Error::other("opencode command array does not launch mcp").into());
    }
    require_json_string(
        &opencode_config,
        &["mcp", "projectatlas", "cwd"],
        generated_cwd,
    )?;
    if opencode_config["mcp"]["projectatlas"]["enabled"] != Value::Bool(true) {
        return Err(io::Error::other("opencode mcp server is not enabled").into());
    }
    let nearest_claude_config = mcp_config_for_harness_with_nearest(&repo, &db, "claude-code")?;
    let nearest_claude_args = nearest_claude_config["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("nearest claude args missing"))?;
    if nearest_claude_args.last().and_then(Value::as_str) != Some("--nearest-project") {
        return Err(io::Error::other(
            "claude mcp-config --nearest-project did not persist startup routing flag",
        )
        .into());
    }
    let nearest_opencode_config = mcp_config_for_harness_with_nearest(&repo, &db, "opencode")?;
    let nearest_opencode_command = nearest_opencode_config["mcp"]["projectatlas"]["command"]
        .as_array()
        .ok_or_else(|| io::Error::other("nearest opencode command array missing"))?;
    if nearest_opencode_command.last().and_then(Value::as_str) != Some("--nearest-project") {
        return Err(io::Error::other(
            "opencode mcp-config --nearest-project did not persist startup routing flag",
        )
        .into());
    }
    let mut settings_args = vec!["--format".to_string(), "json".to_string()];
    for value in &mcp_args[..mcp_args.len().saturating_sub(1)] {
        settings_args.push(
            value
                .as_str()
                .ok_or_else(|| io::Error::other("mcp arg was not a string"))?
                .to_string(),
        );
    }
    settings_args.push("settings".to_string());
    let raw_settings = StdCommand::new(command)
        .current_dir(&outside_cwd)
        .args(settings_args)
        .output()?;
    if !raw_settings.status.success() {
        return Err(io::Error::other("generated mcp config did not preserve settings root").into());
    }
    let settings_json: Value = serde_json::from_slice(&raw_settings.stdout)?;
    let settings_root = settings_json["repo_root"]
        .as_str()
        .ok_or_else(|| io::Error::other("settings repo root missing"))?;
    let actual_root = std::path::Path::new(settings_root).canonicalize()?;
    if actual_root != expected_root {
        return Err(io::Error::other(format!(
            "mcp config repo root mismatch: expected {expected_root:?}, got {actual_root:?}"
        ))
        .into());
    }
    let launch_args = mcp_args
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| io::Error::other("mcp arg was not a string"))
                .map(ToString::to_string)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let outside_scan_message = format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"atlas_scan","arguments":{{"project_path":{},"path":{}}}}}}}"#,
        serde_json::to_string(&expected_root.to_string_lossy().to_string())?,
        serde_json::to_string(&rogue_repo.to_string_lossy())?
    );
    let requests = [
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_scan","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_scan","arguments":{"path":"."}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_watch_once","arguments":{"path":"."}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#.to_string(),
        outside_scan_message,
    ];
    let initialize_message = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#;
    let initialized_message =
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#;
    let mut mcp_stdout = String::new();
    // Keep this root-routing smoke sequential so it does not accidentally
    // assert a concurrent same-project writer scheduling policy.
    for request in &requests {
        mcp_stdout.push_str(&run_mcp_stdio(
            std::path::Path::new(command),
            &outside_cwd,
            &launch_args,
            &[initialize_message, initialized_message, request],
        )?);
    }
    if !mcp_stdout.contains("scan:")
        || !mcp_stdout.contains("src/main.rs")
        || !mcp_stdout.contains("watch:")
    {
        return Err(io::Error::other(format!(
            "generated mcp config did not use the project root from outside cwd: {mcp_stdout}"
        ))
        .into());
    }
    if !mcp_stdout.contains("outside the selected project root") {
        return Err(io::Error::other(format!(
            "generated mcp config allowed conflicting project_path/path roots: {mcp_stdout}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "rogue/*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rogue/rogue.rs").not());

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("token")
        .assert()
        .success()
        .stdout(predicate::str::contains("token_savings:"))
        .stdout(predicate::str::contains("detail_availability: retained"))
        .stdout(predicate::str::contains("read_avoidance:"))
        .stdout(predicate::str::contains("likely_file_reads_avoided"));
    let raw_token = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token.status.success() {
        return Err(io::Error::other("json token command failed").into());
    }
    let token_json: Value = serde_json::from_slice(&raw_token.stdout)?;
    require_json_string(&token_json, &["estimate_kind"], "heuristic")?;
    require_json_string(&token_json, &["estimator"], "chars_or_bytes_div_ceil_4")?;
    require_json_string(
        &token_json,
        &["estimate_scope"],
        "workflow_payload_estimate_not_model_billing_tokens",
    )?;
    require_json_string(&token_json, &["detail_availability"], "retained")?;
    require_json_usize_at_least(&token_json, &["calls"], 7)?;
    require_json_usize_greater_than(&token_json, &["estimated_without_projectatlas"], 0)?;
    require_json_usize_greater_than(&token_json, &["estimated_with_projectatlas"], 0)?;
    require_json_i64_greater_than(&token_json, &["estimated_saved"], 0)?;
    require_json_i64_greater_than(&token_json, &["legacy_gross_estimated_saved"], 0)?;
    require_json_i64_greater_than(&token_json, &["measured_tokens_saved"], 0)?;
    require_json_i64_greater_than(&token_json, &["gross_modeled_tokens_avoided"], 0)?;
    require_json_i64_greater_than(&token_json, &["deduped_modeled_tokens_avoided"], 0)?;
    require_json_i64_greater_than(&token_json, &["average_modeled_tokens_avoided"], 0)?;
    require_json_i64_greater_than(&token_json, &["average_tokens_avoided"], 0)?;
    require_json_i64_greater_than(&token_json, &["maximum_tokens_avoided"], 0)?;
    require_json_i64_greater_than(&token_json, &["tokens_avoided"], 0)?;
    require_json_usize(
        &token_json,
        &["average_policy", "directory_walk_baseline_percent"],
        50,
    )?;
    require_json_usize(
        &token_json,
        &["average_policy", "atlas_payload_percent"],
        100,
    )?;
    require_json_string(
        &token_json,
        &["average_policy", "evidence"],
        "fixed_policy_estimate_not_benchmark_or_provider_measurement",
    )?;
    require_json_usize_greater_than(&token_json, &["observed_file_read_replacements"], 0)?;
    require_json_usize_greater_than(&token_json, &["modeled_file_reads_avoided"], 0)?;
    require_json_usize_greater_than(&token_json, &["likely_file_reads_avoided"], 0)?;
    let estimated_without = token_json["estimated_without_projectatlas"]
        .as_i64()
        .ok_or_else(|| io::Error::other("estimated_without_projectatlas missing"))?;
    let estimated_with = token_json["estimated_with_projectatlas"]
        .as_i64()
        .ok_or_else(|| io::Error::other("estimated_with_projectatlas missing"))?;
    let estimated_saved = token_json["estimated_saved"]
        .as_i64()
        .ok_or_else(|| io::Error::other("estimated_saved missing"))?;
    if estimated_without.saturating_sub(estimated_with) != estimated_saved {
        return Err(io::Error::other(format!(
            "estimated token totals do not reconcile: {estimated_without} - {estimated_with} != {estimated_saved}"
        ))
        .into());
    }
    let measured_saved = token_json["measured_tokens_saved"]
        .as_i64()
        .ok_or_else(|| io::Error::other("measured_tokens_saved missing"))?;
    let deduped_modeled_saved = token_json["deduped_modeled_tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("deduped_modeled_tokens_avoided missing"))?;
    let average_modeled_saved = token_json["average_modeled_tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("average_modeled_tokens_avoided missing"))?;
    let average_tokens_avoided = token_json["average_tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("average_tokens_avoided missing"))?;
    let maximum_tokens_avoided = token_json["maximum_tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("maximum_tokens_avoided missing"))?;
    let tokens_avoided = token_json["tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("tokens_avoided missing"))?;
    if measured_saved.saturating_add(average_modeled_saved) != average_tokens_avoided {
        return Err(io::Error::other(format!(
            "average_tokens_avoided does not reconcile: {measured_saved} + {average_modeled_saved} != {average_tokens_avoided}"
        ))
        .into());
    }
    if measured_saved.saturating_add(deduped_modeled_saved) != maximum_tokens_avoided {
        return Err(io::Error::other(format!(
            "maximum_tokens_avoided does not reconcile: {measured_saved} + {deduped_modeled_saved} != {maximum_tokens_avoided}"
        ))
        .into());
    }
    if tokens_avoided != average_tokens_avoided {
        return Err(io::Error::other(format!(
            "tokens_avoided compatibility alias does not match average: {tokens_avoided} != {average_tokens_avoided}"
        ))
        .into());
    }
    let observed_reads = token_json["observed_file_read_replacements"]
        .as_u64()
        .ok_or_else(|| io::Error::other("observed_file_read_replacements missing"))?;
    let modeled_reads = token_json["modeled_file_reads_avoided"]
        .as_u64()
        .ok_or_else(|| io::Error::other("modeled_file_reads_avoided missing"))?;
    let likely_reads = token_json["likely_file_reads_avoided"]
        .as_u64()
        .ok_or_else(|| io::Error::other("likely_file_reads_avoided missing"))?;
    if observed_reads.saturating_add(modeled_reads) != likely_reads {
        return Err(io::Error::other(format!(
            "file-read avoidance totals do not reconcile: {observed_reads} + {modeled_reads} != {likely_reads}"
        ))
        .into());
    }
    require_json_string(&token_json, &["read_avoidance_scope"], READ_AVOIDANCE_SCOPE)?;
    require_json_string(
        &token_json,
        &["read_avoidance_confidence"],
        READ_AVOIDANCE_CONFIDENCE_MODELED,
    )?;
    let buckets = token_json["buckets"]
        .as_array()
        .ok_or_else(|| io::Error::other("token buckets missing from json report"))?;
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "full_file_compression"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "full_file"
            && bucket["confidence"] == "observed"
            && bucket["accounting_layer"] == "observed_delta"
    }) {
        return Err(io::Error::other("full-file compression token bucket missing").into());
    }
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "navigation_avoidance"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "directory_walk"
            && bucket["confidence"] == "policy_estimate"
            && bucket["accounting_layer"] == "modeled_avoidance"
    }) {
        return Err(io::Error::other("directory-walk navigation token bucket missing").into());
    }
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "navigation_avoidance"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "selected_candidates"
            && bucket["confidence"] == "inferred"
            && bucket["accounting_layer"] == "modeled_avoidance"
    }) {
        return Err(io::Error::other("selected-candidates navigation token bucket missing").into());
    }
    for (tokenizer, content, expected_tokens) in [
        ("cl100k_base", "word\n\nnext", 3),
        ("o200k_base", ".\n/", 1),
    ] {
        let calibration_repo = temp.path().join(format!("calibration-{tokenizer}"));
        fs::create_dir(&calibration_repo)?;
        fs::create_dir(calibration_repo.join(SRC_DIR_NAME))?;
        fs::write(
            calibration_repo.join(SRC_DIR_NAME).join("calibration.txt"),
            content,
        )?;
        let calibration_db = temp.path().join(format!("{tokenizer}.db"));
        drop(AtlasStore::open_for_project(
            &calibration_db,
            &calibration_repo,
        )?);
        Command::cargo_bin("projectatlas")?
            .arg("--db")
            .arg(&calibration_db)
            .arg("scan")
            .arg(&calibration_repo)
            .assert()
            .success();
        let calibration_output = Command::cargo_bin("projectatlas")?
            .arg("--format")
            .arg("json")
            .arg("--db")
            .arg(&calibration_db)
            .args(["token", "--tokenizer", tokenizer])
            .output()?;
        if !calibration_output.status.success() {
            return Err(io::Error::other("json token calibration command failed").into());
        }
        let calibration_json: Value = serde_json::from_slice(&calibration_output.stdout)?;
        require_json_string(&calibration_json, &["calibration", "tokenizer"], tokenizer)?;
        require_json_usize(&calibration_json, &["calibration", "files"], 1)?;
        require_json_usize(&calibration_json, &["calibration", "bytes"], content.len())?;
        require_json_usize(
            &calibration_json,
            &["calibration", "calibrated_tokens"],
            expected_tokens,
        )?;
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["token", "--tokenizer", "unsupported"])
        .assert()
        .failure();
    let calls_before = token_json["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing before no-telemetry check"))?;
    Command::cargo_bin("projectatlas")?
        .env("COLUMNS", "100")
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui"])
        .assert()
        .success();
    let raw_token_after_view = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token_after_view.status.success() {
        return Err(io::Error::other("json token command after tui view failed").into());
    }
    let token_after_view: Value = serde_json::from_slice(&raw_token_after_view.stdout)?;
    let calls_after_view = token_after_view["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing after tui view"))?;
    if calls_before != calls_after_view {
        return Err(io::Error::other(format!(
            "token report view mutated call count: before {calls_before}, after {calls_after_view}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/main.rs"])
        .assert()
        .success();
    let raw_token_after_summary = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token_after_summary.status.success() {
        return Err(io::Error::other("json token command after summary failed").into());
    }
    let token_after_summary: Value = serde_json::from_slice(&raw_token_after_summary.stdout)?;
    let calls_after_summary = token_after_summary["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing after summary"))?;
    if calls_after_summary <= calls_after_view {
        return Err(io::Error::other(format!(
            "summary did not increase token telemetry calls: before {calls_after_view}, after {calls_after_summary}"
        ))
        .into());
    }
    let reads_after_summary = token_after_summary["likely_file_reads_avoided"]
        .as_u64()
        .ok_or_else(|| io::Error::other("likely_file_reads_avoided missing after summary"))?;
    if reads_after_summary <= likely_reads {
        return Err(io::Error::other(format!(
            "summary did not increase likely file reads avoided: before {likely_reads}, after {reads_after_summary}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["search", "helper_99", "--file-pattern", "*.rs"])
        .assert()
        .success();
    let raw_token_after_search = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token_after_search.status.success() {
        return Err(io::Error::other("json token command after search failed").into());
    }
    let token_after_search: Value = serde_json::from_slice(&raw_token_after_search.stdout)?;
    let calls_after_search = token_after_search["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing after search"))?;
    if calls_after_search <= calls_after_summary {
        return Err(io::Error::other(format!(
            "search did not increase token telemetry calls: before {calls_after_summary}, after {calls_after_search}"
        ))
        .into());
    }
    let reads_after_search = token_after_search["likely_file_reads_avoided"]
        .as_u64()
        .ok_or_else(|| io::Error::other("likely_file_reads_avoided missing after search"))?;
    if reads_after_search <= reads_after_summary {
        return Err(io::Error::other(format!(
            "search did not increase likely file reads avoided: before {reads_after_summary}, after {reads_after_search}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            "src/main.rs",
            "--start-line",
            "3",
            "--end-line",
            "4",
        ])
        .assert()
        .success();
    let raw_token_after_slice = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token_after_slice.status.success() {
        return Err(io::Error::other("json token command after slice failed").into());
    }
    let token_after_slice: Value = serde_json::from_slice(&raw_token_after_slice.stdout)?;
    let calls_after_slice = token_after_slice["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing after slice"))?;
    if calls_after_slice <= calls_after_search {
        return Err(io::Error::other(format!(
            "slice did not increase token telemetry calls: before {calls_after_search}, after {calls_after_slice}"
        ))
        .into());
    }
    let reads_after_slice = token_after_slice["likely_file_reads_avoided"]
        .as_u64()
        .ok_or_else(|| io::Error::other("likely_file_reads_avoided missing after slice"))?;
    if reads_after_slice <= reads_after_search {
        return Err(io::Error::other(format!(
            "slice did not increase likely file reads avoided: before {reads_after_search}, after {reads_after_slice}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&db)
        .arg("overview")
        .assert()
        .success();
    let raw_token_after_no_telemetry = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token_after_no_telemetry.status.success() {
        return Err(io::Error::other("json token command after no-telemetry failed").into());
    }
    let token_after_no_telemetry: Value =
        serde_json::from_slice(&raw_token_after_no_telemetry.stdout)?;
    let calls_after = token_after_no_telemetry["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing after no-telemetry check"))?;
    if calls_after_slice != calls_after {
        return Err(io::Error::other(format!(
            "no-telemetry overview mutated call count: before {calls_after_slice}, after {calls_after}"
        ))
        .into());
    }

    let raw_trends = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["token", "--trend", "month"])
        .output()?;
    if !raw_trends.status.success() {
        return Err(io::Error::other("json token trend command failed").into());
    }
    let trends_json: Value = serde_json::from_slice(&raw_trends.stdout)?;
    require_json_string(&trends_json, &["window"], "month")?;
    require_json_string(&trends_json, &["detail_availability"], "retained")?;
    let periods = trends_json["periods"]
        .as_array()
        .ok_or_else(|| io::Error::other("trend periods missing"))?;
    if periods.is_empty() {
        return Err(io::Error::other("trend periods were empty").into());
    }
    if periods.iter().all(|period| {
        period
            .get("buckets")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    }) {
        return Err(io::Error::other("trend periods did not expose token buckets").into());
    }

    Command::cargo_bin("projectatlas")?
        .env("COLUMNS", "80")
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ProjectAtlas"))
        .stdout(predicate::str::contains("Token Impact"))
        .stdout(predicate::str::contains(
            "A V E R A G E   T O K E N S   A V O I D E D",
        ))
        .stdout(predicate::str::contains("Total Tokens Avoided"))
        .stdout(predicate::str::contains("Without ProjectAtlas"))
        .stdout(predicate::str::contains("With ProjectAtlas"))
        .stdout(predicate::str::contains("Average avoided"))
        .stdout(predicate::str::contains("Maximum avoided"))
        .stdout(predicate::str::contains(
            "N A V I G A T I O N   W O R K   A V O I D E D",
        ))
        .stdout(predicate::str::contains("File reads avoided"))
        .stdout(predicate::str::contains("Observed:"))
        .stdout(predicate::str::contains("Modeled:"))
        .stdout(predicate::str::contains("Broad folder walks skipped").not())
        .stdout(predicate::str::contains("Candidate files not opened").not())
        .stdout(predicate::str::contains("source steps account for").not())
        .stdout(predicate::str::contains("S A V I N G S"))
        .stdout(predicate::str::contains("S I G N A L"))
        .stdout(predicate::str::contains(
            "W H E R E   T H E   S A V I N G S   C A M E   F R O M",
        ))
        .stdout(predicate::str::contains("Summaries/slices"))
        .stdout(predicate::str::contains("Skipped folder"))
        .stdout(predicate::str::contains("Fewer candidates"))
        .stdout(predicate::str::contains(
            "C A L I B R A T I O N   &   N O T E S",
        ))
        .stdout(predicate::str::contains("Gross tokens: without").not())
        .stdout(predicate::str::contains("REQUESTED BENCHMARK EVIDENCE").not())
        .stdout(predicate::str::contains("latest").not())
        .stdout(predicate::str::contains("Saved-token trends").not());
    Command::cargo_bin("projectatlas")?
        .env("COLUMNS", "100")
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui", "--tokenizer", "cl100k_base"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Token Impact"))
        .stdout(predicate::str::contains("Tokenizer audit"))
        .stdout(predicate::str::contains("cl100k_base"));
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui", "--trend", "month"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ProjectAtlas Token Trends"))
        .stdout(predicate::str::contains(
            "S A V E D   T O K E N S   T R E N D",
        ))
        .stdout(predicate::str::contains("period"))
        .stdout(predicate::str::contains("saved"));
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args([
            "token", "--view", "tui", "--trend", "month", "--theme", "light",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}["))
        .stdout(predicate::str::contains("ProjectAtlas Token Trends"))
        .stdout(predicate::str::contains(
            "S A V E D   T O K E N S   T R E N D",
        ))
        .stdout(predicate::str::contains("48;2;246;242;232"))
        .stdout(predicate::str::contains("38;2;22;128;72"));
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["token", "--trend", "month", "--tokenizer", "o200k_base"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--tokenizer is only supported for token overview reports",
        ));
    Ok(())
}

/// Write a synthetic fully matched variant of the published benchmark.
fn write_fully_matched_benchmark_fixture(destination: &Path) -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(AGENT_EFFICIENCY_BENCHMARK_PATH);
    let mut artifact: Value = serde_json::from_slice(&fs::read(source)?)?;
    let runs = artifact
        .get_mut("runs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("benchmark runs are missing"))?;
    let frozen_trace = runs
        .iter()
        .find(|run| {
            run.get("arm").and_then(Value::as_str) == Some("v0.3.26")
                && run.get("execution_status").and_then(Value::as_str) == Some("completed")
        })
        .and_then(|run| run.get("trace"))
        .cloned()
        .ok_or_else(|| io::Error::other("completed frozen benchmark trace is missing"))?;
    for run in runs {
        if run.get("case").and_then(Value::as_str) == Some("huge-vscode")
            && run.get("arm").and_then(Value::as_str) == Some("v0.3.26")
        {
            let run = run
                .as_object_mut()
                .ok_or_else(|| io::Error::other("benchmark run is not an object"))?;
            run.insert(
                "execution_status".to_string(),
                Value::String("completed".to_string()),
            );
            run.insert("trace".to_string(), frozen_trace.clone());
        }
    }

    let aggregate = artifact
        .get_mut("aggregate")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("benchmark aggregate is missing"))?;
    aggregate.insert("completed".to_string(), Value::from(45));
    aggregate.insert("failed".to_string(), Value::from(0));
    let groups = aggregate
        .get_mut("groups")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("benchmark groups are missing"))?;
    let matched_group = groups
        .get("huge-vscode/plain")
        .cloned()
        .ok_or_else(|| io::Error::other("matched benchmark donor group is missing"))?;
    let frozen_group = groups
        .get_mut("huge-vscode/v0.3.26")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("frozen benchmark group is missing"))?;
    let frozen_run_ids = frozen_group
        .get("run_ids")
        .cloned()
        .ok_or_else(|| io::Error::other("frozen benchmark run ids are missing"))?;
    *frozen_group = matched_group
        .as_object()
        .cloned()
        .ok_or_else(|| io::Error::other("matched benchmark group is not an object"))?;
    frozen_group.insert("run_ids".to_string(), frozen_run_ids);

    let comparisons = aggregate
        .get_mut("comparisons")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("benchmark comparisons are missing"))?;
    let matched_comparison = comparisons
        .get("huge-vscode/v0.4-vs-plain")
        .cloned()
        .ok_or_else(|| io::Error::other("matched benchmark comparison is missing"))?;
    comparisons.insert(
        "huge-vscode/v0.4-vs-v0.3.26".to_string(),
        matched_comparison,
    );
    fs::write(destination, serde_json::to_vec(&artifact)?)?;
    Ok(())
}

/// Run one telemetry-disabled JSON token overview.
fn token_overview_json(
    repo: &Path,
    database: &Path,
    benchmark_results: Option<&str>,
) -> Result<Value, Box<dyn Error>> {
    let mut command = Command::cargo_bin("projectatlas")?;
    command
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "--db"])
        .arg(database)
        .arg("token");
    if let Some(path) = benchmark_results {
        command.args(["--benchmark-results", path]);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "token overview failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Compare adapter payloads while allowing equivalent floating-point text round-trips.
fn json_values_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| {
                (left - right).abs() <= f64::EPSILON * 8.0 * left.abs().max(right.abs()).max(1.0)
            }),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_values_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_values_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

#[test]
fn mcp_tools_list_preserves_frozen_contracts_without_index_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let executable = mcp_contract_executable();

    let inventory = run_mcp_contract_inventory(&executable, &repo, &database)?;
    assert_frozen_mcp_surfaces_compatible(&inventory)?;
    let response = mcp_response(&inventory, 2)?;
    let tools = response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("MCP tools/list response omitted tools"))?;
    assert_codex_bridge_compatible_input_schemas(tools)?;

    if database.exists() || fs::read_dir(&atlas_dir)?.next().is_some() {
        return Err(io::Error::other(
            "tools/list created project index state while advertising schemas",
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_efficiency_cli_mcp_contract_is_typed_read_only_and_isolated() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn atlas() {}\n",
    )?;
    let database = temp.path().join("projectatlas.db");
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&database)
        .args(["scan", "."])
        .assert()
        .success();

    let published_source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(AGENT_EFFICIENCY_BENCHMARK_PATH);
    let partial_path = repo.join(AGENT_EFFICIENCY_PARTIAL_FILE);
    let compatible_path = repo.join("compatible.json");
    let stale_path = repo.join("stale.json");
    let malformed_path = repo.join("malformed.json");
    let outside_path = temp.path().join("outside.json");
    fs::copy(&published_source, &partial_path)?;
    fs::copy(&published_source, &outside_path)?;
    write_fully_matched_benchmark_fixture(&compatible_path)?;
    let mut stale: Value = serde_json::from_slice(&fs::read(&published_source)?)?;
    stale
        .as_object_mut()
        .ok_or_else(|| io::Error::other("published benchmark is not an object"))?
        .insert("schema_version".to_string(), Value::from(2));
    fs::write(&stale_path, serde_json::to_vec(&stale)?)?;
    fs::write(&malformed_path, b"{")?;

    #[cfg(unix)]
    let indirect_argument = {
        std::os::unix::fs::symlink(&partial_path, repo.join("indirect.json"))?;
        "indirect.json"
    };
    #[cfg(windows)]
    let indirect_argument = {
        let junction_target = repo.join("benchmark-target");
        let junction = repo.join("indirect");
        fs::create_dir(&junction_target)?;
        fs::copy(
            &partial_path,
            junction_target.join(AGENT_EFFICIENCY_PARTIAL_FILE),
        )?;
        let output = StdCommand::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&junction_target)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "failed to create benchmark reparse-point fixture: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        "indirect/partial.json"
    };

    let mcp_config = mcp_config_for_harness(&repo, &database, "mcp-json")?;
    let (mcp_command, mcp_args) = mcp_command_and_args(&mcp_config)?;
    let connection = Connection::open(&database)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    drop(connection);
    let unavailable = token_overview_json(&repo, &database, None)?;
    require_json_string(&unavailable, &["agent_efficiency", "state"], "unavailable")?;
    require_json_string(&unavailable, &["estimate_kind"], "heuristic")?;
    let database_before = fs::read(&database)?;
    let sidecars_before = ["-wal", "-shm", "-journal"].map(|suffix| {
        let path = sqlite_sidecar_path(&database, suffix);
        let bytes = path.exists().then(|| fs::read(&path)).transpose();
        (path, bytes)
    });
    let sidecars_before = sidecars_before
        .into_iter()
        .map(|(path, bytes)| Ok((path, bytes?)))
        .collect::<Result<Vec<_>, io::Error>>()?;
    let artifact_snapshots = [
        (&partial_path, fs::read(&partial_path)?),
        (&compatible_path, fs::read(&compatible_path)?),
        (&stale_path, fs::read(&stale_path)?),
        (&malformed_path, fs::read(&malformed_path)?),
        (&outside_path, fs::read(&outside_path)?),
    ];

    let partial = token_overview_json(&repo, &database, Some(AGENT_EFFICIENCY_PARTIAL_FILE))?;
    require_json_string(&partial, &["agent_efficiency", "state"], "partial")?;
    let source_head = partial
        .pointer("/agent_efficiency/artifact/candidate_source_head")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("candidate source identity is missing"))?;
    for key in ["candidate_functional_head", "candidate_checklist_head"] {
        require_json_string(
            &partial,
            &["agent_efficiency", "artifact", key],
            source_head,
        )?;
    }
    require_json_usize(
        &partial,
        &[
            "agent_efficiency",
            "baselines",
            "0",
            "baseline_failed_trials",
        ],
        3,
    )?;
    let compatible = token_overview_json(&repo, &database, Some("compatible.json"))?;
    require_json_string(&compatible, &["agent_efficiency", "state"], "compatible")?;
    let stale = token_overview_json(&repo, &database, Some("stale.json"))?;
    require_json_string(&stale, &["agent_efficiency", "state"], "incompatible")?;
    let malformed = token_overview_json(&repo, &database, Some("malformed.json"))?;
    require_json_string(&malformed, &["agent_efficiency", "state"], "failed")?;
    let missing = token_overview_json(&repo, &database, Some("missing.json"))?;
    require_json_string(&missing, &["agent_efficiency", "state"], "failed")?;

    let toon = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&database)
        .args([
            "token",
            "--benchmark-results",
            AGENT_EFFICIENCY_PARTIAL_FILE,
        ])
        .output()?;
    if !toon.status.success() {
        return Err(io::Error::other("TOON benchmark token overview failed").into());
    }
    let toon: Value = toon_format::decode_default(&String::from_utf8(toon.stdout)?)?;
    if !json_values_equivalent(
        &toon["token_savings"]["agent_efficiency"],
        &partial["agent_efficiency"],
    ) {
        return Err(io::Error::other("CLI JSON and TOON benchmark reports diverged").into());
    }

    let absolute_argument = outside_path.to_string_lossy().to_string();
    let messages = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-agent-efficiency-e2e","version":"0.1.0"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":AGENT_EFFICIENCY_PARTIAL_FILE}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":"compatible.json"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":"stale.json"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":"malformed.json"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":"../outside.json"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":absolute_argument}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"benchmark_results":indirect_argument}}}).to_string(),
    ];
    let mcp_stdout = run_mcp_stdio_with_env(
        &mcp_command,
        &repo,
        &mcp_args,
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
    )?;
    for (id, cli_report) in [
        (2, &partial),
        (3, &compatible),
        (4, &unavailable),
        (5, &stale),
        (6, &malformed),
    ] {
        let mcp: Value = toon_format::decode_default(&mcp_tool_text(&mcp_stdout, id)?)?;
        if !json_values_equivalent(
            &mcp["token_savings"]["agent_efficiency"],
            &cli_report["agent_efficiency"],
        ) {
            return Err(io::Error::other(format!(
                "MCP and CLI benchmark reports diverged for response {id}"
            ))
            .into());
        }
    }
    for id in [7, 8, 9] {
        if !mcp_tool_text(&mcp_stdout, id)?.contains("invalid input") {
            return Err(io::Error::other(format!(
                "MCP benchmark path boundary did not reject response {id}"
            ))
            .into());
        }
    }

    for path in [
        absolute_argument.as_str(),
        "../outside.json",
        indirect_argument,
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .arg("--db")
            .arg(&database)
            .args(["token", "--benchmark-results", path])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid input"));
    }

    if fs::read(&database)? != database_before {
        return Err(io::Error::other("benchmark reports mutated the SQLite database").into());
    }
    for (path, before) in artifact_snapshots {
        if fs::read(path)? != before {
            return Err(
                io::Error::other(format!("benchmark report rewrote {}", path.display())).into(),
            );
        }
    }
    for (path, before) in sidecars_before {
        let after = path.exists().then(|| fs::read(&path)).transpose()?;
        if after != before {
            return Err(io::Error::other(format!(
                "benchmark report changed SQLite sidecar {}: before={:?}, after={:?}",
                path.display(),
                before.as_ref().map(Vec::len),
                after.as_ref().map(Vec::len)
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn large_repository_agent_funnel_stays_bounded() -> Result<(), Box<dyn Error>> {
    const MODULES: usize = 24;
    const FILES_PER_MODULE: usize = 24;
    const TOTAL_FILES: usize = MODULES * FILES_PER_MODULE;
    const TARGET_MODULE: usize = 17;
    const TARGET_FILE: usize = 13;
    const TARGET_PATH: &str = "src/module_17/file_13.rs";
    const SCAN_TIMEOUT_SECONDS: u64 = 60;

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("large-repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    for module in 0..MODULES {
        let module_dir = repo.join(SRC_DIR_NAME).join(format!("module_{module:02}"));
        fs::create_dir(&module_dir)?;
        for file in 0..FILES_PER_MODULE {
            let mut source = String::from("//! Generated large repository fixture.\n\n");
            writeln!(&mut source, "pub struct Module{module:02}File{file:02};\n")?;
            writeln!(&mut source, "impl Module{module:02}File{file:02} {{")?;
            writeln!(
                &mut source,
                "    pub fn run(&self) -> usize {{ helper_{module:02}_{file:02}() }}"
            )?;
            writeln!(&mut source, "}}\n")?;
            writeln!(
                &mut source,
                "pub fn helper_{module:02}_{file:02}() -> usize {{ {} }}",
                module + file
            )?;
            if module == TARGET_MODULE && file == TARGET_FILE {
                writeln!(
                    &mut source,
                    "pub fn target_large_repo_marker() -> usize {{ helper_{module:02}_{file:02}() }}"
                )?;
            }
            fs::write(module_dir.join(format!("file_{file:02}.rs")), source)?;
        }
    }
    let db = temp.path().join("large-projectatlas.db");

    let scan_started = Instant::now();
    let raw_scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .arg(&repo)
        .output()?;
    if !raw_scan.status.success() {
        return Err(io::Error::other(format!(
            "large repo scan failed: {}",
            String::from_utf8_lossy(&raw_scan.stderr)
        ))
        .into());
    }
    if scan_started.elapsed() > Duration::from_secs(SCAN_TIMEOUT_SECONDS) {
        return Err(io::Error::other(format!(
            "large repo scan exceeded 60s: {:?}",
            scan_started.elapsed()
        ))
        .into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize_at_least(&scan_json, &["overview", "files"], TOTAL_FILES)?;
    require_json_usize_at_least(&scan_json, &["symbols", "symbols"], TOTAL_FILES)?;
    require_json_usize_at_least(&scan_json, &["symbols", "summaries"], TOTAL_FILES)?;

    let files_started = Instant::now();
    let raw_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "files",
            "target_large_repo_marker",
            "--file-pattern",
            "*.rs",
            "--limit",
            "5",
        ])
        .output()?;
    if !raw_files.status.success() {
        return Err(io::Error::other("large repo files command failed").into());
    }
    if files_started.elapsed() > Duration::from_secs(15) {
        return Err(io::Error::other(format!(
            "large repo files query exceeded 15s: {:?}",
            files_started.elapsed()
        ))
        .into());
    }
    let files_text = String::from_utf8(raw_files.stdout)?;
    if !files_text.contains(TARGET_PATH) {
        return Err(io::Error::other(format!(
            "large repo files query did not find {TARGET_PATH}: {files_text}"
        ))
        .into());
    }

    let summary_started = Instant::now();
    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", TARGET_PATH, "--limit", "10"])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other("large repo summary command failed").into());
    }
    if summary_started.elapsed() > Duration::from_secs(15) {
        return Err(io::Error::other(format!(
            "large repo summary exceeded 15s: {:?}",
            summary_started.elapsed()
        ))
        .into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    require_json_string(&summary_json, &["file_path"], TARGET_PATH)?;
    require_json_usize_at_least(&summary_json, &["symbol_count"], 4)?;
    require_json_usize_at_least(&summary_json, &["total_methods"], 1)?;

    let raw_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "search",
            "target_large_repo_marker",
            "--file-pattern",
            "src/module_17/*.rs",
            "--limit",
            "5",
        ])
        .output()?;
    if !raw_search.status.success() {
        return Err(io::Error::other("large repo search command failed").into());
    }
    let search_json: Value = serde_json::from_slice(&raw_search.stdout)?;
    require_json_usize(&search_json, &["returned"], 1)?;
    require_json_string(&search_json, &["results", "0", "path"], TARGET_PATH)?;
    require_json_bool(&search_json, &["total_is_complete"], true)?;

    let raw_token = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("token")
        .output()?;
    if !raw_token.status.success() {
        return Err(io::Error::other("large repo token command failed").into());
    }
    let token_json: Value = serde_json::from_slice(&raw_token.stdout)?;
    require_json_usize_at_least(&token_json, &["calls"], 3)?;
    require_json_i64_greater_than(&token_json, &["estimated_saved"], 0)?;
    Ok(())
}

#[test]
fn real_scan_resolves_import_alias_called_by_across_core_languages() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("rust").join("no_alias"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("rust").join("module_alias"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("rust").join("function_alias"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("ts").join("no_alias"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("ts").join("named_alias"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("ts").join("api"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("py").join("package"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("py").join("package_entry"))?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("no_alias")
            .join("service.rs"),
        "pub fn run_no_alias() -> &'static str {\n    \"rust-no-alias\"\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("no_alias")
            .join("main.rs"),
        "use crate::rust::no_alias::service;\n\nfn start_rust_no_alias() {\n    service::run_no_alias();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("module_alias")
            .join("service.rs"),
        "pub fn run_module_alias() -> &'static str {\n    \"rust-module-alias\"\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("module_alias")
            .join("main.rs"),
        "use crate::rust::module_alias::service as rust_service;\n\nfn start_rust_module_alias() {\n    rust_service::run_module_alias();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("function_alias")
            .join("service.rs"),
        "pub fn run_function_alias() -> &'static str {\n    \"rust-function-alias\"\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("rust")
            .join("function_alias")
            .join("main.rs"),
        "use crate::rust::function_alias::service::run_function_alias as run_rust_function;\n\nfn start_rust_function_alias() {\n    run_rust_function();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("ts")
            .join("no_alias")
            .join("service.ts"),
        "export function runTsNoAlias(): string {\n  return \"typescript-no-alias\";\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("ts").join("no_alias_main.ts"),
        "import { runTsNoAlias } from \"./no_alias/service\";\n\nexport function startTsNoAlias(): string {\n  return runTsNoAlias();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("ts")
            .join("named_alias")
            .join("service.ts"),
        "export function runTsNamedAlias(): string {\n  return \"typescript-named-alias\";\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("ts")
            .join("named_alias_main.ts"),
        "import { runTsNamedAlias as runAlias } from \"./named_alias/service\";\n\nexport function startTsNamedAlias(): string {\n  return runAlias();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("ts")
            .join("api")
            .join("index.ts"),
        "export function runTsNamespace(): string {\n  return \"typescript-namespace\";\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("ts").join("namespace_main.ts"),
        "import * as api from \"./api\";\n\nexport function startTsNamespace(): string {\n  return api.runTsNamespace();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("package")
            .join("no_alias.py"),
        "def run_py_no_alias():\n    return \"python-no-alias\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("py").join("no_alias_main.py"),
        "from py.package.no_alias import run_py_no_alias\n\n\ndef start_py_no_alias():\n    return run_py_no_alias()\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("package")
            .join("named_alias.py"),
        "def run_py_named_alias():\n    return \"python-named-alias\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("named_alias_main.py"),
        "from py.package.named_alias import run_py_named_alias as run_alias\n\n\ndef start_py_named_alias():\n    return run_alias()\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("package")
            .join("module_alias.py"),
        "def run_py_module_alias():\n    return \"python-module-alias\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("module_alias_main.py"),
        "import py.package.module_alias as py_service\n\n\ndef start_py_module_alias():\n    return py_service.run_py_module_alias()\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join("py")
            .join("package_entry")
            .join("__init__.py"),
        "def run_py_entry():\n    return \"python-entry\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("py").join("entry_main.py"),
        "import py.package_entry as package_entry\n\n\ndef start_py_entry():\n    return package_entry.run_py_entry()\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    assert_summary_called_by(
        &repo,
        &db,
        "src/rust/no_alias/service.rs",
        "run_no_alias",
        "src/rust/no_alias/main.rs::start_rust_no_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/rust/module_alias/service.rs",
        "run_module_alias",
        "src/rust/module_alias/main.rs::start_rust_module_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/rust/function_alias/service.rs",
        "run_function_alias",
        "src/rust/function_alias/main.rs::start_rust_function_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/ts/no_alias/service.ts",
        "runTsNoAlias",
        "src/ts/no_alias_main.ts::startTsNoAlias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/ts/named_alias/service.ts",
        "runTsNamedAlias",
        "src/ts/named_alias_main.ts::startTsNamedAlias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/ts/api/index.ts",
        "runTsNamespace",
        "src/ts/namespace_main.ts::startTsNamespace",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/py/package/no_alias.py",
        "run_py_no_alias",
        "src/py/no_alias_main.py::start_py_no_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/py/package/named_alias.py",
        "run_py_named_alias",
        "src/py/named_alias_main.py::start_py_named_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/py/package/module_alias.py",
        "run_py_module_alias",
        "src/py/module_alias_main.py::start_py_module_alias",
    )?;
    assert_summary_called_by(
        &repo,
        &db,
        "src/py/package_entry/__init__.py",
        "run_py_entry",
        "src/py/entry_main.py::start_py_entry",
    )?;

    Ok(())
}

#[test]
fn ranked_files_and_next_include_bounded_reasons() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir(repo.join(TESTS_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(INSTALLER_RS_FILE_NAME),
        "pub fn install_runtime() { let _marker = \"hiddenNeedle\"; }\n",
    )?;
    fs::write(
        repo.join(TESTS_DIR_NAME).join(INSTALLER_RS_FILE_NAME),
        "#[test]\nfn installer_pair() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            SRC_DIR_NAME,
            "Installer runtime source folder for navigation tests.",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/installer.rs",
            "Installer runtime implementation for navigation tests.",
            PurposeSource::Agent,
        )?;
    }

    let raw_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "files",
            "installer runtime hiddenNeedle",
            "--include-content",
            "--limit",
            "2",
        ])
        .output()?;
    if !raw_files.status.success() {
        return Err(io::Error::other(format!(
            "json files command with reasons failed: {}",
            String::from_utf8_lossy(&raw_files.stderr)
        ))
        .into());
    }
    let files_json: Value = serde_json::from_slice(&raw_files.stdout)?;
    let file_entry = files_json
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["path"] == "src/installer.rs")
        })
        .ok_or_else(|| io::Error::other("reasoned installer file entry was missing"))?;
    require_json_string(
        file_entry,
        &["file_purpose"],
        "Installer runtime implementation for navigation tests.",
    )?;
    require_json_string(file_entry, &["purpose_source"], "agent")?;
    require_json_bool(file_entry, &["purpose_agent_reviewed"], true)?;
    require_json_contains(file_entry, &["reasons", "0"], "path matched")?;
    require_json_string(file_entry, &["next_call", "capability"], "summary")?;
    require_json_string(file_entry, &["next_call", "path"], "src/installer.rs")?;
    require_json_bool(file_entry, &["connections_truncated"], false)?;
    let file_reasons = json_at(file_entry, &["reasons"])?
        .as_array()
        .ok_or_else(|| io::Error::other("file reasons were not an array"))?;
    if file_reasons.len() > 6
        || !file_reasons.iter().any(|reason| {
            reason
                .as_str()
                .is_some_and(|text| text.contains("indexed text matched"))
        })
    {
        return Err(io::Error::other(format!(
            "file reasons did not stay bounded or include indexed text: {file_reasons:?}"
        ))
        .into());
    }

    let raw_next = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["next", "installer runtime hiddenNeedle", "--limit", "2"])
        .output()?;
    if !raw_next.status.success() {
        return Err(io::Error::other(format!(
            "json next command failed: {}",
            String::from_utf8_lossy(&raw_next.stderr)
        ))
        .into());
    }
    let next_json: Value = serde_json::from_slice(&raw_next.stdout)?;
    require_json_string(&next_json, &["query"], "installer runtime hiddenNeedle")?;
    require_json_string(&next_json, &["files", "0", "path"], "src/installer.rs")?;
    require_json_contains(&next_json, &["files", "0", "reasons", "0"], "path matched")?;
    require_json_string(
        &next_json,
        &["files", "0", "next_call", "capability"],
        "summary",
    )?;
    let suggestions = json_at(&next_json, &["suggestions"])?
        .as_array()
        .ok_or_else(|| io::Error::other("next suggestions were not an array"))?;
    if !suggestions.iter().any(|suggestion| {
        suggestion
            .as_str()
            .is_some_and(|text| text == "projectatlas summary src/installer.rs --limit 25")
    }) {
        return Err(io::Error::other(format!(
            "next suggestions did not include top-file summary command: {suggestions:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn cli_navigation_rows_propagate_nonempty_typed_graph_evidence() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(TESTS_DIR_NAME))?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"adapter-navigation\"\nversion = \"0.1.0\"\n",
    )?;
    for path in [
        "src/navigation_owner.rs",
        "src/navigation_local.rs",
        "src/navigation_unresolved.rs",
        "tests/navigation_owner.rs",
    ] {
        fs::write(repo.join(path), "pub fn navigation_fixture() {}\n")?;
    }
    let db = temp.path().join("projectatlas.db");
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    publish_cli_navigation_graph(&db)?;

    let json_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["files", "navigation", "--limit", "10"])
        .output()?;
    if !json_output.status.success() {
        return Err(io::Error::other(format!(
            "graph-enriched JSON files failed: {}",
            String::from_utf8_lossy(&json_output.stderr)
        ))
        .into());
    }
    let files: Value = serde_json::from_slice(&json_output.stdout)?;
    let rows = files
        .as_array()
        .ok_or_else(|| io::Error::other("graph-enriched files payload is not an array"))?;
    let owner = rows
        .iter()
        .find(|row| row["path"] == "src/navigation_owner.rs")
        .ok_or_else(|| io::Error::other("graph-enriched owner row is missing"))?;
    require_json_bool(owner, &["connections_truncated"], true)?;
    require_json_string(owner, &["next_call", "capability"], "relations")?;
    let families = rows
        .iter()
        .flat_map(|row| {
            row["connection_counts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|count| count["kind"].as_str())
        })
        .collect::<BTreeSet<_>>();
    if families
        != BTreeSet::from([
            "package",
            "import",
            "call",
            "reference",
            "test",
            "route",
            "config",
        ])
    {
        return Err(io::Error::other(format!(
            "CLI graph families were not propagated: {families:?}"
        ))
        .into());
    }
    let target_kinds = rows
        .iter()
        .flat_map(|row| {
            row["connections"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|connection| connection["target"]["kind"].as_str())
        })
        .collect::<BTreeSet<_>>();
    for expected in ["local", "package", "external", "unresolved"] {
        if !target_kinds.contains(expected) {
            return Err(io::Error::other(format!(
                "CLI graph targets omitted {expected}: {target_kinds:?}"
            ))
            .into());
        }
    }

    let toon = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "navigation", "--limit", "10"])
        .output()?;
    if !toon.status.success() {
        return Err(io::Error::other(format!(
            "graph-enriched TOON files failed: {}",
            String::from_utf8_lossy(&toon.stderr)
        ))
        .into());
    }
    let toon = String::from_utf8(toon.stdout)?;
    for expected in [
        "connection_counts",
        "connections",
        "connections_truncated: true",
        "capability: relations",
    ] {
        if !toon.contains(expected) {
            return Err(io::Error::other(format!(
                "TOON graph evidence omitted {expected:?}: {toon}"
            ))
            .into());
        }
    }

    let summary_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/navigation_owner.rs", "--limit", "10"])
        .output()?;
    if !summary_output.status.success() {
        return Err(io::Error::other(format!(
            "coverage-enriched summary failed: {}",
            String::from_utf8_lossy(&summary_output.stderr)
        ))
        .into());
    }
    let summary: Value = serde_json::from_slice(&summary_output.stdout)?;
    require_json_bool(&summary, &["coverage", "available"], true)?;
    require_json_string(&summary, &["coverage", "trust"], "partial")?;
    require_json_string(&summary, &["coverage", "next_call", "capability"], "health")?;

    let coverage_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "health-check",
            "--coverage",
            "--relation",
            "calls",
            "--coverage-state",
            "failed",
            "--reason",
            "parser failed",
            "--limit",
            "1",
        ])
        .output()?;
    if !coverage_output.status.success() {
        return Err(io::Error::other(format!(
            "filtered JSON coverage failed: {}",
            String::from_utf8_lossy(&coverage_output.stderr)
        ))
        .into());
    }
    let coverage: Value = serde_json::from_slice(&coverage_output.stdout)?;
    require_json_usize(&coverage, &["returned"], 1)?;
    require_json_string(&coverage, &["total", "state"], "exact")?;
    require_json_string(&coverage, &["rows", "0", "state"], "failed")?;
    require_json_string(
        &coverage,
        &["rows", "0", "next_call", "capability"],
        "health",
    )?;
    require_json_usize(&coverage, &["output_bytes"], coverage_output.stdout.len())?;

    let parsed_coverage = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "health-check",
            "--coverage",
            "--path-prefix",
            "src/navigation_owner.rs",
            "--parser",
            "tree-sitter",
            "--provider",
            "tree-sitter",
            "--coverage-state",
            "partial",
            "--limit",
            "1",
        ])
        .output()?;
    if !parsed_coverage.status.success() {
        return Err(io::Error::other(format!(
            "filtered TOON coverage failed: {}",
            String::from_utf8_lossy(&parsed_coverage.stderr)
        ))
        .into());
    }
    let parsed_coverage = String::from_utf8(parsed_coverage.stdout)?;
    for expected in [
        "coverage:",
        "returned: 1",
        "state: partial",
        "parser: \"tree-sitter\"",
        "provider: \"tree-sitter\"",
        "capability: summary",
        "output_bytes:",
    ] {
        if !parsed_coverage.contains(expected) {
            return Err(io::Error::other(format!(
                "TOON coverage omitted {expected:?}: {parsed_coverage}"
            ))
            .into());
        }
    }
    let toon_output_bytes = parsed_coverage
        .lines()
        .find_map(|line| line.trim().strip_prefix("output_bytes: "))
        .ok_or_else(|| io::Error::other("TOON coverage omitted numeric output_bytes"))?
        .parse::<usize>()?;
    if toon_output_bytes != parsed_coverage.len() {
        return Err(io::Error::other(format!(
            "TOON coverage output_bytes mismatch: expected {}, got {toon_output_bytes}",
            parsed_coverage.len()
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["health-check", "--coverage", "--coverage-state", "unknown"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid coverage state"));
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["health-check", "--parser", "tree-sitter"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--coverage"));
    Ok(())
}

fn publish_cli_navigation_graph(db: &Path) -> Result<(), Box<dyn Error>> {
    let mut store = AtlasStore::open(db)?;
    let project = store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("CLI navigation project identity is missing"))?;
    let current_publication = store
        .index_publication()?
        .ok_or_else(|| io::Error::other("CLI navigation publication is missing"))?;
    let fingerprint = current_publication
        .contract_fingerprint
        .clone()
        .ok_or_else(|| io::Error::other("CLI navigation fingerprint is missing"))?;
    let generation = current_publication
        .generation
        .checked_next()
        .ok_or_else(|| io::Error::other("CLI navigation generation overflow"))?;
    let file_entity = |path: &str| {
        GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(path))?,
            },
            generation,
        )
    };
    let owner = file_entity("src/navigation_owner.rs")?;
    let local = file_entity("src/navigation_local.rs")?;
    let unresolved = file_entity("src/navigation_unresolved.rs")?;
    let test = file_entity("tests/navigation_owner.rs")?;
    let package = GraphEntity::new(
        project,
        EntitySelector::Package {
            package: PackageSelector {
                manager: GraphIdentityText::new("cargo")?,
                name: GraphIdentityText::new("adapter-navigation")?,
                manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
            },
        },
        generation,
    )?;
    let external = GraphEntity::new(
        project,
        EntitySelector::External {
            external: ExternalSelector {
                system: GraphIdentityText::new("crates.io")?,
                identity: GraphIdentityText::new("serde@1")?,
            },
        },
        generation,
    )?;
    let resolved = |source: &GraphEntity, kind, target: &GraphEntity| {
        Ok::<_, Box<dyn Error>>(LogicalRelation::new(
            source,
            kind,
            RelationResolution::resolved(target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?)
    };
    let unresolved_relation = |source: &GraphEntity, kind, reference: &str| {
        Ok::<_, Box<dyn Error>>(LogicalRelation::new(
            source,
            kind,
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new(reference)?,
            },
            ConfidenceClass::High,
            Completeness::Partial,
            generation,
        )?)
    };
    let relations = vec![
        resolved(
            &owner,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            &package,
        )?,
        LogicalRelation::new(
            &owner,
            GraphRelationKind::Legacy(RelationKind::Imports),
            RelationResolution::external(&external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?,
        resolved(
            &owner,
            GraphRelationKind::Legacy(RelationKind::Calls),
            &local,
        )?,
        unresolved_relation(
            &unresolved,
            GraphRelationKind::Extended(ExtendedRelationKind::References),
            "navigation-reference",
        )?,
        resolved(
            &test,
            GraphRelationKind::Extended(ExtendedRelationKind::Tests),
            &owner,
        )?,
        resolved(
            &owner,
            GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo),
            &local,
        )?,
        unresolved_relation(
            &owner,
            GraphRelationKind::Extended(ExtendedRelationKind::Configures),
            "NAVIGATION_MODE",
        )?,
    ];
    let coverage = vec![
        CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/navigation_owner.rs"))?,
            },
            None,
            CoverageState::Partial,
            4,
            1,
            generation,
            Some(GraphIdentityText::new("one fallback fact omitted")?),
            Some(GraphLimitKind::Rows),
        )?,
        CoverageRecord::new(
            CoverageScope::Project,
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            CoverageState::Failed,
            0,
            1,
            generation,
            Some(GraphIdentityText::new("parser failed")?),
            None,
        )?,
    ];
    let nodes = store
        .load_nodes()?
        .into_iter()
        .map(|node| node.node)
        .collect::<Vec<_>>();
    {
        let mut publication = store.begin_index_publication(&fingerprint)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&nodes)?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(
            project,
            &[owner, local, unresolved, test, package, external],
            &relations,
            &[],
            &coverage,
        )?;
        publication.complete()?;
    }
    store.set_purpose(
        SRC_DIR_NAME,
        "Navigation graph folder",
        PurposeSource::Agent,
    )?;
    store.set_purpose(
        "src/navigation_owner.rs",
        "Navigation graph owner",
        PurposeSource::Agent,
    )?;
    store.set_purpose(
        "src/navigation_unresolved.rs",
        "Navigation unresolved graph owner",
        PurposeSource::Agent,
    )?;
    Ok(())
}

#[test]
fn classified_document_navigation_agrees_across_cli_and_mcp() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("classified-document-navigation");
    fs::create_dir_all(repo.join("docs"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(GUIDE_MD_PATH),
        "# Guide\n\nSee [the current source](../src/lib.rs).\n",
    )?;
    fs::write(
        repo.join("docs/empty.md"),
        "# Notes\n\nThis document deliberately contains no repository reference.\n",
    )?;
    fs::write(
        repo.join("docs/missing.md"),
        "# Missing\n\nSee [the absent source](../src/missing.rs).\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn api() {}\n",
    )?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    run_scan(&repo, &database)?;

    let symbols_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args([
            "symbols",
            "list",
            "--file",
            "docs/guide.md",
            "--content-selection",
            "documentation",
            "--limit",
            "10",
        ])
        .output()?;
    if !symbols_output.status.success() {
        return Err(io::Error::other(format!(
            "classified CLI symbols failed: {}",
            String::from_utf8_lossy(&symbols_output.stderr)
        ))
        .into());
    }
    let symbols: Value = serde_json::from_slice(&symbols_output.stdout)?;
    require_json_string(&symbols, &["0", "classification"], "documentation")?;
    require_json_string(&symbols, &["0", "kind"], "heading")?;
    require_json_string(&symbols, &["0", "name"], "Guide")?;
    require_json_usize(&symbols, &["0", "source_selector", "byte_start"], 0)?;
    require_json_usize(&symbols, &["0", "source_selector", "byte_end"], 7)?;
    let symbols_toon_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("toon")
        .arg("--db")
        .arg(&database)
        .args([
            "symbols",
            "list",
            "--file",
            "docs/guide.md",
            "--content-selection",
            "documentation",
            "--limit",
            "10",
        ])
        .output()?;
    if !symbols_toon_output.status.success() {
        return Err(io::Error::other(format!(
            "classified CLI TOON symbols failed: {}",
            String::from_utf8_lossy(&symbols_toon_output.stderr)
        ))
        .into());
    }
    let symbols_toon: Value =
        toon_format::decode_default(&String::from_utf8(symbols_toon_output.stdout)?)?;
    require_json_string(
        &symbols_toon,
        &["symbols", "0", "classification"],
        "documentation",
    )?;
    require_json_usize(
        &symbols_toon,
        &["symbols", "0", "source_selector", "byte_end"],
        7,
    )?;

    let relation_arguments = [
        "symbols",
        "relations",
        "--view",
        "detailed",
        "--file",
        "docs/guide.md",
        "--direction",
        "outbound",
        "--relation",
        "documents",
        "--content-selection",
        "documentation",
        "--limit",
        "10",
    ];
    let relations_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args(relation_arguments)
        .output()?;
    if !relations_output.status.success() {
        return Err(io::Error::other(format!(
            "classified CLI relation failed: {}",
            String::from_utf8_lossy(&relations_output.stderr)
        ))
        .into());
    }
    let cli_relation: Value = serde_json::from_slice(&relations_output.stdout)?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "content_selection"],
        "documentation",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "rows", "0", "relation", "kind", "scope"],
        "extended",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "rows", "0", "relation", "kind", "value"],
        "documents",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "rows", "0", "source", "classification"],
        "documentation",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "rows", "0", "target", "classification"],
        "source",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "rows", "0", "next_call", "capability"],
        "summary",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "anchor", "entity", "selector", "kind"],
        "file",
    )?;
    require_json_string(
        &cli_relation,
        &["symbol_relations", "anchor", "entity", "selector", "path"],
        "docs/guide.md",
    )?;
    require_json_string(
        &cli_relation,
        &[
            "symbol_relations",
            "rows",
            "0",
            "next_call",
            "content_selection",
        ],
        "source",
    )?;

    let inbound_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--direction",
            "inbound",
            "--relation",
            "documents",
            "--content-selection",
            "source",
            "--limit",
            "10",
        ])
        .output()?;
    if !inbound_output.status.success() {
        return Err(io::Error::other(format!(
            "classified CLI inbound relation failed: {}",
            String::from_utf8_lossy(&inbound_output.stderr)
        ))
        .into());
    }
    let cli_inbound: Value = serde_json::from_slice(&inbound_output.stdout)?;
    require_json_usize(&cli_inbound, &["symbol_relations", "returned"], 1)?;
    require_json_string(
        &cli_inbound,
        &["symbol_relations", "rows", "0", "inbound_view"],
        "documented_by",
    )?;
    require_json_string(
        &cli_inbound,
        &["symbol_relations", "rows", "0", "relation", "kind", "value"],
        "documents",
    )?;
    require_json_string(
        &cli_inbound,
        &[
            "symbol_relations",
            "rows",
            "0",
            "source",
            "entity",
            "selector",
            "path",
        ],
        "docs/guide.md",
    )?;
    require_json_string(
        &cli_inbound,
        &["symbol_relations", "rows", "0", "source", "classification"],
        "documentation",
    )?;
    require_json_string(
        &cli_inbound,
        &[
            "symbol_relations",
            "rows",
            "0",
            "next_call",
            "content_selection",
        ],
        "documentation",
    )?;

    let no_candidate_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "docs/empty.md",
            "--direction",
            "outbound",
            "--relation",
            "documents",
            "--content-selection",
            "documentation",
            "--limit",
            "10",
        ])
        .output()?;
    if !no_candidate_output.status.success() {
        return Err(io::Error::other(format!(
            "no-candidate CLI relation failed: {}",
            String::from_utf8_lossy(&no_candidate_output.stderr)
        ))
        .into());
    }
    let no_candidate_relation: Value = serde_json::from_slice(&no_candidate_output.stdout)?;
    require_json_usize(&no_candidate_relation, &["symbol_relations", "returned"], 0)?;
    require_json_string(
        &no_candidate_relation,
        &["symbol_relations", "total", "state"],
        "exact",
    )?;
    require_json_usize(
        &no_candidate_relation,
        &["symbol_relations", "total", "value"],
        0,
    )?;
    let cli_document_coverage = json_at(
        &no_candidate_relation,
        &["symbol_relations", "anchor", "coverage"],
    )?
    .as_array()
    .and_then(|coverage| {
        coverage
            .iter()
            .find(|row| row.pointer("/relation/value").and_then(Value::as_str) == Some("documents"))
    })
    .ok_or_else(|| io::Error::other("CLI document coverage row missing"))?;
    require_json_string(cli_document_coverage, &["state"], "no_candidates")?;

    let unresolved_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "docs/missing.md",
            "--direction",
            "outbound",
            "--relation",
            "documents",
            "--resolution",
            "unresolved",
            "--content-selection",
            "documentation",
            "--limit",
            "10",
        ])
        .output()?;
    if !unresolved_output.status.success() {
        return Err(io::Error::other(format!(
            "unresolved CLI relation failed: {}",
            String::from_utf8_lossy(&unresolved_output.stderr)
        ))
        .into());
    }
    let unresolved_relation: Value = serde_json::from_slice(&unresolved_output.stdout)?;
    require_json_usize(&unresolved_relation, &["symbol_relations", "returned"], 1)?;
    require_json_string(
        &unresolved_relation,
        &[
            "symbol_relations",
            "rows",
            "0",
            "relation",
            "resolution",
            "status",
        ],
        "unresolved",
    )?;
    require_json_string(
        &unresolved_relation,
        &[
            "symbol_relations",
            "rows",
            "0",
            "document_unresolved_reason",
        ],
        "missing",
    )?;
    let relations_toon_output = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("toon")
        .arg("--db")
        .arg(&database)
        .args(relation_arguments)
        .output()?;
    if !relations_toon_output.status.success() {
        return Err(io::Error::other(format!(
            "classified CLI TOON relation failed: {}",
            String::from_utf8_lossy(&relations_toon_output.stderr)
        ))
        .into());
    }
    let relations_toon: Value =
        toon_format::decode_default(&String::from_utf8(relations_toon_output.stdout)?)?;
    require_json_string(
        &relations_toon,
        &["symbol_relations", "rows", "0", "relation", "kind", "value"],
        "documents",
    )?;
    require_json_string(
        &relations_toon,
        &[
            "symbol_relations",
            "rows",
            "0",
            "next_call",
            "content_selection",
        ],
        "source",
    )?;

    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn api() {}\n\npub fn current_saved_source() {}\n",
    )?;
    let before_invalid_cli = mcp_database_snapshot(&database)?;
    let invalid_cli = Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&database)
        .args(["files", "--content-selection", "prose"])
        .output()?;
    if invalid_cli.status.success() || mcp_database_snapshot(&database)? != before_invalid_cli {
        return Err(io::Error::other(
            "invalid CLI content selection refreshed or changed SQLite state",
        )
        .into());
    }

    let executable = mcp_contract_executable();
    let mut session = McpContractSession::spawn(&executable, &repo, &database)?;
    let operation_result = (|| -> Result<(), Box<dyn Error>> {
        let mcp_symbols: Value = toon_format::decode_default(&session.call_tool(
            "atlas_symbols",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "file": "docs/guide.md",
                "content_selection": "documentation",
                "limit": 10
            }),
        )?)?;
        if mcp_symbols.get("symbols").is_none() {
            return Err(io::Error::other(format!(
                "classified MCP symbols omitted its rows: {mcp_symbols}"
            ))
            .into());
        }
        require_json_string(
            &mcp_symbols,
            &["symbols", "0", "classification"],
            "documentation",
        )?;
        require_json_usize(
            &mcp_symbols,
            &["symbols", "0", "source_selector", "byte_end"],
            7,
        )?;

        let mcp_outbound: Value = toon_format::decode_default(&session.call_tool(
            "atlas_symbol_relations",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "view": "detailed",
                "file": "docs/guide.md",
                "direction": "outbound",
                "relation": "documents",
                "content_selection": "documentation",
                "limit": 10
            }),
        )?)?;
        require_json_string(
            &mcp_outbound,
            &["symbol_relations", "rows", "0", "relation", "kind", "value"],
            "documents",
        )?;
        require_json_string(
            &mcp_outbound,
            &["symbol_relations", "anchor", "entity", "selector", "kind"],
            "file",
        )?;
        require_json_string(
            &mcp_outbound,
            &["symbol_relations", "rows", "0", "target", "classification"],
            "source",
        )?;

        let mcp_no_candidates: Value = toon_format::decode_default(&session.call_tool(
            "atlas_symbol_relations",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "view": "detailed",
                "file": "docs/empty.md",
                "direction": "outbound",
                "relation": "documents",
                "content_selection": "documentation",
                "limit": 10
            }),
        )?)?;
        require_json_usize(&mcp_no_candidates, &["symbol_relations", "returned"], 0)?;
        require_json_string(
            &mcp_no_candidates,
            &["symbol_relations", "total", "state"],
            "exact",
        )?;
        require_json_usize(
            &mcp_no_candidates,
            &["symbol_relations", "total", "value"],
            0,
        )?;
        let mcp_document_coverage = json_at(
            &mcp_no_candidates,
            &["symbol_relations", "anchor", "coverage"],
        )?
        .as_array()
        .and_then(|coverage| {
            coverage.iter().find(|row| {
                row.pointer("/relation/value").and_then(Value::as_str) == Some("documents")
            })
        })
        .ok_or_else(|| io::Error::other("MCP document coverage row missing"))?;
        require_json_string(mcp_document_coverage, &["state"], "no_candidates")?;

        let mcp_unresolved: Value = toon_format::decode_default(&session.call_tool(
            "atlas_symbol_relations",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "view": "detailed",
                "file": "docs/missing.md",
                "direction": "outbound",
                "relation": "documents",
                "resolution": "unresolved",
                "content_selection": "documentation",
                "limit": 10
            }),
        )?)?;
        require_json_usize(&mcp_unresolved, &["symbol_relations", "returned"], 1)?;
        require_json_string(
            &mcp_unresolved,
            &[
                "symbol_relations",
                "rows",
                "0",
                "relation",
                "resolution",
                "status",
            ],
            "unresolved",
        )?;
        require_json_string(
            &mcp_unresolved,
            &[
                "symbol_relations",
                "rows",
                "0",
                "document_unresolved_reason",
            ],
            "missing",
        )?;

        let mcp_inbound: Value = toon_format::decode_default(&session.call_tool(
            "atlas_symbol_relations",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "view": "detailed",
                "file": "src/lib.rs",
                "direction": "inbound",
                "relation": "documents",
                "content_selection": "source",
                "limit": 10
            }),
        )?)?;
        require_json_string(
            &mcp_inbound,
            &["symbol_relations", "rows", "0", "inbound_view"],
            "documented_by",
        )?;
        require_json_string(
            &mcp_inbound,
            &["symbol_relations", "anchor", "classification"],
            "source",
        )?;
        require_json_string(
            &mcp_inbound,
            &["symbol_relations", "rows", "0", "source", "classification"],
            "documentation",
        )?;

        let source_summary: Value = toon_format::decode_default(&session.call_tool(
            "atlas_file_summary",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "file": "src/lib.rs",
                "content_selection": "source",
                "limit": 10
            }),
        )?)?;
        require_json_contains(
            &source_summary,
            &["file_summary", "content_summary"],
            "current_saved_source",
        )?;
        require_json_string(
            &source_summary,
            &["file_summary", "classification"],
            "source",
        )?;

        let before_invalid_mcp = mcp_database_snapshot(&database)?;
        let invalid_mcp = session.call_tool(
            "atlas_files",
            &serde_json::json!({
                "project_path": repo.as_path(),
                "content_selection": "prose"
            }),
        )?;
        if !invalid_mcp.contains("source, documentation, or both")
            || mcp_database_snapshot(&database)? != before_invalid_mcp
        {
            return Err(io::Error::other(format!(
                "invalid MCP content selection changed state or lost allowed values: {invalid_mcp}"
            ))
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(operation_result, || session.shutdown())
}

#[test]
fn default_scan_indexes_complete_accepted_core_surface() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fixture_root = repo.join("all");
    fs::create_dir_all(&fixture_root)?;
    let db = temp.path().join("projectatlas.db");
    let mut expected = Vec::new();

    for (index, extension) in BROAD_SOURCE_EXTENSIONS.iter().enumerate() {
        let file_name = format!("file_{index:03}{extension}");
        let relative_path = format!("all/{file_name}");
        let language =
            detect_language_for_path(&relative_path, Some(extension)).ok_or_else(|| {
                io::Error::other(format!(
                    "language registry has unsupported extension {extension}"
                ))
            })?;
        fs::write(
            fixture_root.join(file_name),
            fixture_content_for_extension(extension),
        )?;
        expected.push((relative_path, language));
    }

    for (relative_path, expected_language, content) in special_language_fixtures() {
        let disk_path = repo.join(relative_path);
        if let Some(parent) = disk_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&disk_path, content)?;
        expected.push((relative_path.to_string(), expected_language.to_string()));
    }

    let raw_scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .output()?;
    if !raw_scan.status.success() {
        return Err(io::Error::other(format!(
            "language registry scan failed: {}",
            String::from_utf8_lossy(&raw_scan.stderr)
        ))
        .into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize_at_least(&scan_json, &["overview", "files"], expected.len())?;

    let limit = (expected.len() + 10).to_string();
    let raw_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "**/*", "--limit", &limit])
        .output()?;
    if !raw_files.status.success() {
        return Err(io::Error::other(format!(
            "language registry files command failed: {}",
            String::from_utf8_lossy(&raw_files.stderr)
        ))
        .into());
    }
    let files_json: Value = serde_json::from_slice(&raw_files.stdout)?;
    let file_entries = files_json
        .as_array()
        .ok_or_else(|| io::Error::other("files output was not an array"))?;
    let indexed_by_path = file_entries
        .iter()
        .filter_map(|entry| {
            let path = entry["path"].as_str()?;
            Some((path.to_string(), entry))
        })
        .collect::<BTreeMap<_, _>>();

    for (relative_path, expected_language) in &expected {
        let entry = indexed_by_path.get(relative_path.as_str()).ok_or_else(|| {
            io::Error::other(format!("missing indexed language fixture {relative_path}"))
        })?;
        require_json_string(entry, &["language"], expected_language)?;
        if entry
            .get("content_summary")
            .and_then(Value::as_str)
            .is_some_and(|summary| summary.trim().is_empty())
        {
            return Err(io::Error::other(format!(
                "empty summary for indexed language fixture {relative_path}"
            ))
            .into());
        }
        let summary = json_summary_command(&repo, &db, relative_path)?;
        require_json_string(&summary, &["language"], expected_language)?;
        let content_summary = json_at(&summary, &["content_summary"])?
            .as_str()
            .ok_or_else(|| {
                io::Error::other(format!(
                    "content summary for language fixture {relative_path} was not a string"
                ))
            })?;
        if content_summary.trim().is_empty() {
            return Err(io::Error::other(format!(
                "empty content summary for language fixture {relative_path}"
            ))
            .into());
        }
        if is_scanner_byte_summary(content_summary) {
            return Err(io::Error::other(format!(
                "byte-count scanner fallback summary for language fixture {relative_path}: {content_summary}"
            ))
            .into());
        }
        let parser_kind = json_at(&summary, &["parser_kind"])?
            .as_str()
            .ok_or_else(|| {
                io::Error::other(format!(
                    "parser kind for language fixture {relative_path} was not a string"
                ))
            })?;
        if parser_kind == "missing" {
            return Err(io::Error::other(format!(
                "missing parser kind for language fixture {relative_path}"
            ))
            .into());
        }
        let summary_status = json_at(&summary, &["summary_status"])?
            .as_str()
            .ok_or_else(|| {
                io::Error::other(format!(
                    "summary status for language fixture {relative_path} was not a string"
                ))
            })?;
        if summary_status == "missing" {
            return Err(io::Error::other(format!(
                "missing summary status for language fixture {relative_path}"
            ))
            .into());
        }
        if expected_language == "ruby" {
            require_json_string(&summary, &["parser_kind"], "fallback-symbol-graph")?;
            require_json_string(&summary, &["summary_status"], "fallback")?;
        }
    }

    Ok(())
}

#[test]
fn language_fixture_summaries_match_baselines() -> Result<(), Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| io::Error::other("workspace root not found"))?;
    let fixture_source = workspace_root.join("fixtures").join("languages");
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    copy_directory_tree(&fixture_source, &repo)?;
    fs::create_dir_all(repo.join("python"))?;
    fs::write(
        repo.join("python").join("builder.py"),
        python_baseline_fixture_source(),
    )?;
    let db = temp.path().join("projectatlas.db");

    let raw_scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .output()?;
    if !raw_scan.status.success() {
        return Err(io::Error::other(format!(
            "language fixture scan failed: {}",
            String::from_utf8_lossy(&raw_scan.stderr)
        ))
        .into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize_at_least(&scan_json, &["symbols", "parsed"], 18)?;
    require_json_usize_at_least(&scan_json, &["structural_summaries", "summarized"], 7)?;

    for baseline in language_summary_baselines()? {
        let summary = json_summary_command(&repo, &db, &baseline.path)?;
        require_json_string(&summary, &["language"], &baseline.language)?;
        require_json_string(&summary, &["parser_kind"], &baseline.parser_kind)?;
        require_json_string(&summary, &["summary_status"], &baseline.status)?;
        require_json_string(&summary, &["content_summary"], &baseline.summary)?;
        if baseline.minimum_symbol_count > 0 {
            require_json_usize_at_least(
                &summary,
                &["symbol_count"],
                baseline.minimum_symbol_count,
            )?;
        } else {
            require_json_usize(&summary, &["symbol_count"], 0)?;
        }
    }

    Ok(())
}

#[test]
fn map_and_lint_honor_configured_exclude_path_prefixes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME)
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(
        repo.join(".purpose"),
        "Repository root for prefix map tests\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(".purpose"),
        "Rust source folder for prefix map tests\n",
    )?;
    fs::write(
        repo.join("docs").join(".purpose"),
        "Documentation folder for prefix map tests\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("engine.rs"),
        "// Purpose: Active Rust source for prefix map tests.\npub fn indexed_engine() {}\n",
    )?;
    fs::write(
        repo.join("docs").join("api").join("noise.rs"),
        "pub fn excluded_from_map_and_lint() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["map", "--force"])
        .assert()
        .success();

    let map = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"))?;
    if !map.contains("src/engine.rs") {
        return Err(io::Error::other("map omitted indexed source file").into());
    }
    if map.contains("docs/api/noise.rs") || map.contains("excluded_from_map_and_lint") {
        return Err(io::Error::other("map included excluded path-prefix source").into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["lint", "--strict-folders", "--report-untracked"])
        .assert()
        .success()
        .stderr(predicate::str::contains("docs/api/noise.rs").not());
    Ok(())
}

#[test]
fn first_default_scan_skips_stale_legacy_map_purposes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"),
        "version: 1\n\
generated_at: 2026-06-28T00:00:00Z\n\
root: .\n\
folders[2]{path,summary,source}:\n\
  .,Repository root,folder\n\
  stale,Deleted legacy folder,folder\n\
files[2]{path,summary,source}:\n\
  src/main.rs,Rust entrypoint,file\n\
  stale/deleted.rs,Deleted legacy file,file\n",
    )?;
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), "fn main() {}\n")?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .assert()
        .success()
        .stdout(predicate::str::contains("scan:"))
        .stdout(predicate::str::contains("purpose_import:"))
        .stdout(predicate::str::contains("imported: 2"))
        .stdout(predicate::str::contains("skipped_stale: 2"))
        .stderr(predicate::str::contains("Query returned no rows").not());

    let raw_overview = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("overview")
        .output()?;
    if !raw_overview.status.success() {
        return Err(io::Error::other("overview after legacy import scan failed").into());
    }
    let overview_json: Value = serde_json::from_slice(&raw_overview.stdout)?;
    require_json_usize(&overview_json, &["files"], 2)?;
    require_json_usize(&overview_json, &["approved_purposes"], 4)?;
    Ok(())
}

#[test]
fn scan_does_not_overwrite_agent_purpose_with_legacy_header() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "// Purpose: Legacy header purpose that should only seed empty rows.\nfn main() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(repo.join(ATLAS_DIR_NAME).join("config.toml"))
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("imported: 1"));

    {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed Rust entry point for the scan preservation test.",
            PurposeSource::Agent,
        )?;
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(repo.join(ATLAS_DIR_NAME).join("config.toml"))
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("imported: 0"))
        .stdout(predicate::str::contains("skipped_existing: 1"));

    let nodes = AtlasStore::open(&db)?.load_nodes_by_paths(&["src/main.rs".to_string()])?;
    let node = nodes
        .first()
        .ok_or_else(|| io::Error::other("indexed source node missing after rescan"))?;
    if node.purpose.source != PurposeSource::Agent {
        return Err(io::Error::other("legacy import downgraded agent-reviewed purpose").into());
    }
    if node.purpose.purpose.as_deref()
        != Some("Agent-reviewed Rust entry point for the scan preservation test.")
    {
        return Err(io::Error::other("legacy import replaced agent-reviewed purpose text").into());
    }
    Ok(())
}

#[test]
fn mcp_config_discovers_flat_config_from_db_root() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let outside = temp.path().join("outside");
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&repo)?;
    fs::create_dir(&outside)?;
    fs::create_dir(&unrelated)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::write(
        outside.join("projectatlas.toml"),
        "[project]\nroot = \"../unrelated\"\n\n[scan]\nexclude_dir_names = [\"src\"]\n",
    )?;
    fs::write(
        repo.join("projectatlas.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"generated\"]\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("engine.rs"),
        "pub fn flat_config_engine() {}\n",
    )?;
    fs::write(
        repo.join("generated").join("noise.rs"),
        "pub fn flat_config_noise() {}\n",
    )?;
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir(&atlas_dir)?;
    let db = atlas_dir.join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let raw_config = Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("mcp-config")
        .output()?;
    if !raw_config.status.success() {
        return Err(io::Error::other("outside mcp-config command failed").into());
    }
    let config_json: Value = serde_json::from_slice(&raw_config.stdout)?;
    let args = config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("mcp args missing"))?;
    let config_arg = args
        .iter()
        .position(|value| value.as_str() == Some("--config"))
        .ok_or_else(|| io::Error::other("flat config was not emitted"))?;
    let emitted_config = args
        .get(config_arg + 1)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("flat config path missing"))?;
    if cfg!(windows) && (emitted_config.starts_with(r"\\?\") || emitted_config.starts_with("//?/"))
    {
        return Err(io::Error::other("mcp config path used a Windows extended path prefix").into());
    }
    if std::path::Path::new(emitted_config).canonicalize()?
        != repo.join("projectatlas.toml").canonicalize()?
    {
        return Err(io::Error::other("emitted config was not projectatlas.toml").into());
    }
    let cwd = config_json["mcpServers"]["projectatlas"]["cwd"]
        .as_str()
        .ok_or_else(|| io::Error::other("mcp cwd missing"))?;
    if std::path::Path::new(cwd).canonicalize()? != repo.canonicalize()? {
        return Err(io::Error::other("mcp cwd did not use DB project root").into());
    }
    Ok(())
}

#[test]
fn files_command_normalizes_windows_style_folder_filters() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("nested"))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("nested").join("handler.rs"),
        "fn handler() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["scan"])
        .arg(&repo)
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["files", "handler", "--folder", "src\\nested\\"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/nested/handler.rs"));
    Ok(())
}

#[test]
fn scan_does_not_exclude_repository_under_excluded_parent_name() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("target").join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "pub fn main_entry() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    let raw_scan = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["scan"])
        .arg(&repo)
        .output()?;
    if !raw_scan.status.success() {
        return Err(io::Error::other("scan under excluded parent failed").into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize(&scan_json, &["overview", "files"], 1)?;
    require_json_usize(&scan_json, &["text_index", "indexed"], 1)?;

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["files", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));
    Ok(())
}

#[test]
fn compiler_config_utf8_bom_refreshes_through_cli_and_mcp() -> Result<(), Box<dyn Error>> {
    const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
    const COMPILER_CONFIG: &[u8] =
        br#"{"compilerOptions":{"baseUrl":"src","paths":{"@/*":["*"]}}}"#;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("compiler-config-utf8-bom");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("controller.ts"),
        "export function useController(): string { return \"ok\"; }\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("page.ts"),
        "import { useController } from \"@/controller\";\nexport const value = useController();\n",
    )?;
    let config_path = repo.join(TS_CONFIG_FILE_NAME);
    fs::write(&config_path, [UTF8_BOM, COMPILER_CONFIG].concat())?;

    let executable = mcp_contract_executable();
    let init = Command::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "init"])
        .output()?;
    if !init.status.success() {
        return Err(io::Error::other(format!(
            "init rejected UTF-8 BOM compiler configuration: {}",
            String::from_utf8_lossy(&init.stderr)
        ))
        .into());
    }
    let db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    run_scan(&repo, &db)?;
    let current_generation = || -> Result<usize, Box<dyn Error>> {
        let generation = AtlasStore::open(&db)?
            .index_publication()?
            .ok_or_else(|| io::Error::other("compiler-config publication is missing"))?
            .generation;
        Ok(usize::try_from(generation.get())?)
    };
    let bom_relation = detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?;
    assert_detailed_resolution(&bom_relation, 1, "resolved")?;
    require_json_usize(
        &bom_relation,
        &["symbol_relations", "generation"],
        current_generation()?,
    )?;
    let expected_alias_semantics = detailed_resolution_semantics(&bom_relation)
        .ok_or_else(|| io::Error::other("initial alias relation omitted semantic fields"))?;

    fs::write(&config_path, COMPILER_CONFIG)?;
    run_watch_once(&repo, &db)?;
    let plain_relation =
        detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?;
    assert_detailed_resolution(&plain_relation, 1, "resolved")?;
    require_json_usize(
        &plain_relation,
        &["symbol_relations", "generation"],
        current_generation()?,
    )?;
    if detailed_resolution_semantics(&plain_relation) != Some(expected_alias_semantics) {
        return Err(io::Error::other(
            "CLI BOM-to-plain refresh changed configured alias semantics",
        )
        .into());
    }

    fs::write(&config_path, [UTF8_BOM, COMPILER_CONFIG].concat())?;
    let mut session = McpContractSession::spawn(&executable, &repo, &db)?;
    let report = session.call_tool(
        "atlas_watch_once",
        &serde_json::json!({"project_path": repo.as_path(), "path": repo.as_path()}),
    )?;
    let mcp_relation: Value = toon_format::decode_default(&session.call_tool(
        "atlas_symbol_relations",
        &serde_json::json!({
            "project_path": repo.as_path(),
            "view": "detailed",
            "compact": true,
            "file": "src/controller.ts",
            "direction": "inbound",
            "limit": 10
        }),
    )?)?;
    session.shutdown()?;
    if !report.contains("watch:") || !report.contains("single-refresh") {
        return Err(io::Error::other(format!(
            "MCP watch did not report a successful BOM refresh: {report}"
        ))
        .into());
    }
    assert_detailed_resolution(&mcp_relation, 1, "resolved")?;
    require_json_usize(
        &mcp_relation,
        &["symbol_relations", "generation"],
        current_generation()?,
    )?;
    if detailed_resolution_semantics(&mcp_relation) != Some(expected_alias_semantics) {
        return Err(io::Error::other(
            "MCP plain-to-BOM refresh changed configured alias semantics",
        )
        .into());
    }
    Ok(())
}

fn detailed_relation_payload(
    repo: &Path,
    db: &Path,
    file: &str,
    symbol: Option<&str>,
    direction: &str,
) -> Result<Value, Box<dyn Error>> {
    let mut command = Command::new(mcp_contract_executable());
    command
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(db)
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            file,
            "--direction",
            direction,
            "--limit",
            "50",
        ]);
    if let Some(symbol) = symbol {
        command.args(["--symbol", symbol]);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "configured alias relation query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn assert_detailed_resolution(
    payload: &Value,
    expected_total: usize,
    expected_resolution: &str,
) -> Result<(), Box<dyn Error>> {
    require_json_usize(
        payload,
        &["symbol_relations", "total", "value"],
        expected_total,
    )?;
    let rows = payload
        .pointer("/symbol_relations/rows")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("detailed relation rows are missing"))?;
    if rows.iter().any(|row| {
        row.pointer("/relation/resolution/status")
            .and_then(Value::as_str)
            != Some(expected_resolution)
    }) {
        return Err(io::Error::other(format!(
            "relation rows did not all retain {expected_resolution:?}: {rows:?}"
        ))
        .into());
    }
    Ok(())
}

fn detailed_resolution_semantics(payload: &Value) -> Option<(&Value, &Value)> {
    Some((
        payload.pointer("/symbol_relations/rows/0/relation/kind")?,
        payload.pointer("/symbol_relations/rows/0/relation/resolution/selector")?,
    ))
}

#[test]
fn csharp_symbol_identity_boundary_preserves_full_and_incremental_publication()
-> Result<(), Box<dyn Error>> {
    const REGISTRY_PATH: &str = "src/registry.cs";
    const UNRELATED_PATH: &str = "src/unrelated.rs";
    const GRAPH_IDENTITY_BYTES: usize = 4_096;
    const LEGACY_DECLARATION_BYTES: usize = 4_090;

    let declaration = |entry_count: usize, padding: &str| -> Result<String, std::fmt::Error> {
        let mut entries = String::new();
        for index in 0..entry_count {
            writeln!(entries, "        [\"k{index:03}\"]=\"v\",")?;
        }
        Ok(format!(
            "    public static readonly Dictionary<string, string> D = new()\n    {{\n        /*{padding}*/\n{entries}    }};\n"
        ))
    };
    let compact_bytes = |value: &str| {
        value
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| token.len() + usize::from(index > 0))
            .sum::<usize>()
    };
    let unpadded_declaration = declaration(224, "")?;
    let padding_bytes = LEGACY_DECLARATION_BYTES
        .checked_sub(compact_bytes(&unpadded_declaration))
        .ok_or_else(|| io::Error::other("C# boundary fixture exceeded its target size"))?;
    let padding = "x".repeat(padding_bytes);
    let overbound_parent = "P".repeat(GRAPH_IDENTITY_BYTES + 1);
    let registry_source = |entry_count: usize| -> Result<String, std::fmt::Error> {
        Ok(format!(
            "using System.Collections.Generic;\n\npublic class {overbound_parent}\n{{\n    public void Retained() {{}}\n}}\n\npublic class Registry\n{{\n{}    public int Sibling = 1;\n}}\n",
            declaration(entry_count, &padding)?,
        ))
    };
    let source_224 = registry_source(224)?;
    let source_225 = registry_source(225)?;
    let declaration_224 = declaration(224, &padding)?;
    let declaration_225 = declaration(225, &padding)?;
    if compact_bytes(&declaration_224) != LEGACY_DECLARATION_BYTES
        || compact_bytes(&declaration_225) <= GRAPH_IDENTITY_BYTES
    {
        return Err(io::Error::other(format!(
            "C# fixture did not cross the graph identity boundary: 224={}, 225={}, limit={}",
            compact_bytes(&declaration_224),
            compact_bytes(&declaration_225),
            GRAPH_IDENTITY_BYTES
        ))
        .into());
    }

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let db = temp.path().join("csharp-symbol-identity-boundary.db");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    let registry = repo.join(REGISTRY_PATH);
    fs::write(&registry, source_224)?;
    fs::write(
        repo.join(UNRELATED_PATH),
        "pub fn retained_entry() -> u32 { retained_helper() }\nfn retained_helper() -> u32 { 7 }\n",
    )?;

    let assert_published_facts = |snapshot: &DerivedResultSnapshot| -> Result<(), Box<dyn Error>> {
        let registry_symbols = snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.path == REGISTRY_PATH)
            .collect::<Vec<_>>();
        if !registry_symbols
            .iter()
            .any(|symbol| symbol.name == "D" && symbol.kind == SymbolKind::Value)
            || !registry_symbols
                .iter()
                .any(|symbol| symbol.name == "Sibling")
        {
            return Err(io::Error::other(format!(
                "C# publication omitted D or its valid sibling: {registry_symbols:#?}"
            ))
            .into());
        }
        if registry_symbols.iter().any(|symbol| {
            symbol.name.contains("Dictionary")
                || symbol.name.contains('=')
                || symbol.name == overbound_parent
        }) {
            return Err(
                io::Error::other("invalid C# declaration identity reached publication").into(),
            );
        }
        if !registry_symbols
            .iter()
            .any(|symbol| symbol.name == "Retained" && symbol.parent.is_none())
        {
            return Err(io::Error::other(
                "valid C# child was not retained without its invalid parent",
            )
            .into());
        }
        if !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.path == UNRELATED_PATH && symbol.name == "retained_entry")
            || !snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.path == UNRELATED_PATH && symbol.name == "retained_helper")
            || !snapshot.symbol_relations.iter().any(|relation| {
                relation.path == UNRELATED_PATH
                    && relation.source_name == "retained_entry"
                    && relation.target_name == "retained_helper"
                    && relation.kind == RelationKind::Calls
            })
        {
            return Err(io::Error::other(
                "publication omitted unrelated Rust symbols or their call relation",
            )
            .into());
        }
        Ok(())
    };

    run_scan(&repo, &db)?;
    let first_publication = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("initial C# publication missing"))?;
    let initial_snapshot = derived_result_snapshot(&db)?;
    assert_published_facts(&initial_snapshot)?;
    let unrelated_symbols = initial_snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>();
    let unrelated_relations = initial_snapshot
        .symbol_relations
        .iter()
        .filter(|relation| relation.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>();

    fs::write(&registry, source_225)?;
    run_watch_once(&repo, &db)?;
    let refreshed_publication = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("incremental C# publication missing"))?;
    if refreshed_publication.generation <= first_publication.generation {
        return Err(io::Error::other("C# boundary edit did not publish a new generation").into());
    }
    let refreshed_snapshot = derived_result_snapshot(&db)?;
    assert_published_facts(&refreshed_snapshot)?;
    if refreshed_snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>()
        != unrelated_symbols
        || refreshed_snapshot
            .symbol_relations
            .iter()
            .filter(|relation| relation.path == UNRELATED_PATH)
            .cloned()
            .collect::<Vec<_>>()
            != unrelated_relations
    {
        return Err(io::Error::other(
            "incremental C# publication changed unrelated Rust graph facts",
        )
        .into());
    }
    let listed_symbols = run_packaged_cli_json(
        &mcp_contract_executable(),
        &repo,
        &db,
        &["symbols", "list", "--file", REGISTRY_PATH, "--limit", "20"],
    )?;
    if !listed_symbols
        .as_array()
        .is_some_and(|symbols| symbols.iter().any(|symbol| symbol["name"] == "D"))
    {
        return Err(io::Error::other(format!(
            "CLI navigation did not return the exact D identity: {listed_symbols}"
        ))
        .into());
    }
    assert_clean_scan_convergence(&repo, &db, temp.path(), "csharp-symbol-identity-boundary")?;

    let before_failed_publication = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before failed C# refresh"))?;
    let before_failed_snapshot = derived_result_snapshot(&db)?;
    fs::write(&registry, registry_source(226)?)?;
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("index work deadline was reached"));
    let after_failed_publication = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after failed C# refresh"))?;
    if after_failed_publication != before_failed_publication
        || derived_result_snapshot(&db)? != before_failed_snapshot
    {
        return Err(io::Error::other("failed C# refresh changed the complete publication").into());
    }
    Ok(())
}

#[test]
fn deep_qualified_symbol_parents_preserve_full_and_incremental_publication()
-> Result<(), Box<dyn Error>> {
    const DEEP_PATH: &str = "src/deep.rs";
    const UNRELATED_PATH: &str = "src/unrelated.rs";

    let nested_source = |depth: usize| -> Result<String, std::fmt::Error> {
        let mut source = String::new();
        for index in 0..depth {
            let name = format!("scope{index:02}{}", "x".repeat(233));
            writeln!(source, "mod {name} {{")?;
        }
        source.push_str("pub fn deep_marker() {}\n");
        for _ in 0..depth {
            source.push_str("}\n");
        }
        Ok(source)
    };
    let assert_deep_publication = |snapshot: &DerivedResultSnapshot| -> Result<(), Box<dyn Error>> {
        if !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.path == DEEP_PATH && symbol.name == "deep_marker")
        {
            return Err(io::Error::other("deep Rust marker was not published").into());
        }
        if !snapshot.graph_entities.iter().any(|entity| {
            entity.contains("deep_marker") && entity.contains("@projectatlas.scope.v1:")
        }) {
            return Err(io::Error::other(
                "deep Rust marker did not receive a bounded qualified graph parent",
            )
            .into());
        }
        Ok(())
    };

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let database = temp.path().join("deep-qualified-symbol-parents.db");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    let deep = repo.join(DEEP_PATH);
    fs::write(&deep, nested_source(18)?)?;
    fs::write(
        repo.join(UNRELATED_PATH),
        "pub fn retained_entry() -> u32 { retained_helper() }\nfn retained_helper() -> u32 { 7 }\n",
    )?;

    run_scan(&repo, &database)?;
    let initial_publication = AtlasStore::open_read_only(&database)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("initial deep publication missing"))?;
    let initial = derived_result_snapshot(&database)?;
    assert_deep_publication(&initial)?;
    let unrelated_symbols = initial
        .symbols
        .iter()
        .filter(|symbol| symbol.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>();
    let unrelated_relations = initial
        .symbol_relations
        .iter()
        .filter(|relation| relation.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>();

    fs::write(&deep, nested_source(19)?)?;
    run_watch_once(&repo, &database)?;
    let refreshed_publication = AtlasStore::open_read_only(&database)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("incremental deep publication missing"))?;
    if refreshed_publication.generation <= initial_publication.generation {
        return Err(io::Error::other("deep-scope edit did not publish a new generation").into());
    }
    let refreshed = derived_result_snapshot(&database)?;
    assert_deep_publication(&refreshed)?;
    if refreshed
        .symbols
        .iter()
        .filter(|symbol| symbol.path == UNRELATED_PATH)
        .cloned()
        .collect::<Vec<_>>()
        != unrelated_symbols
        || refreshed
            .symbol_relations
            .iter()
            .filter(|relation| relation.path == UNRELATED_PATH)
            .cloned()
            .collect::<Vec<_>>()
            != unrelated_relations
    {
        return Err(io::Error::other(
            "deep-scope incremental publication changed unrelated graph facts",
        )
        .into());
    }
    let listed = run_packaged_cli_json(
        &mcp_contract_executable(),
        &repo,
        &database,
        &["symbols", "list", "--file", DEEP_PATH, "--limit", "50"],
    )?;
    if !listed
        .as_array()
        .is_some_and(|symbols| symbols.iter().any(|symbol| symbol["name"] == "deep_marker"))
    {
        return Err(io::Error::other("CLI navigation omitted the deep Rust marker").into());
    }
    assert_clean_scan_convergence(
        &repo,
        &database,
        temp.path(),
        "deep-qualified-symbol-parents",
    )
}

#[test]
fn partial_markdown_limit_persists_without_losing_complete_publication()
-> Result<(), Box<dyn Error>> {
    const DOCUMENT_PATH: &str = "docs/limited.md";

    let label = "l".repeat(projectatlas_symbols::MAX_MARKDOWN_LABEL_BYTES);
    let selector = format!(
        "src/{}.rs",
        "s".repeat(projectatlas_symbols::MAX_DOCUMENT_SELECTOR_BYTES - "src/".len() - ".rs".len())
    );
    let evidence_bytes = label.len() + selector.len();
    let limited_source = format!("[{label}]({selector})\n")
        .repeat(projectatlas_symbols::MAX_MARKDOWN_EVIDENCE_BYTES / evidence_bytes + 1);

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let database = temp.path().join("partial-markdown-limit.db");
    fs::create_dir_all(repo.join("docs"))?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    let document = repo.join(DOCUMENT_PATH);
    fs::write(&document, &limited_source)?;

    let assert_partial_coverage = || -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::open_read_only(&database)?;
        let publication = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("Markdown publication missing"))?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("Markdown project identity missing"))?;
        let coverage = store.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new(DOCUMENT_PATH))?,
            },
            100,
        )?;
        if publication.state != projectatlas_db::IndexPublicationState::Complete
            || !coverage.rows.iter().any(|row| {
                row.state() == CoverageState::Partial
                    && row.reached_limit() == Some(GraphLimitKind::IntermediateBytes)
            })
        {
            return Err(io::Error::other(format!(
                "Markdown intermediate-byte coverage was not durably published: {coverage:?}"
            ))
            .into());
        }
        Ok(())
    };

    run_scan(&repo, &database)?;
    assert_partial_coverage()?;
    let before_failed_publication = AtlasStore::open_read_only(&database)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before failed Markdown refresh"))?;
    let before_failed_snapshot = derived_result_snapshot(&database)?;
    fs::write(
        &document,
        format!("{limited_source}\n[extra](missing.md)\n"),
    )?;
    Command::new(mcp_contract_executable())
        .current_dir(&repo)
        .arg("--db")
        .arg(&database)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("index work deadline was reached"));
    let after_failed_publication = AtlasStore::open_read_only(&database)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after failed Markdown refresh"))?;
    if after_failed_publication != before_failed_publication
        || derived_result_snapshot(&database)? != before_failed_snapshot
    {
        return Err(io::Error::other(
            "failed Markdown refresh changed the last complete publication",
        )
        .into());
    }

    run_watch_once(&repo, &database)?;
    assert_partial_coverage()?;
    assert_clean_scan_convergence(&repo, &database, temp.path(), "partial-markdown-limit")
}

#[derive(Debug, Eq, PartialEq)]
struct DerivedResultSnapshot {
    nodes: Vec<String>,
    file_classifications: Vec<String>,
    unreviewed_purposes: BTreeMap<String, String>,
    texts: Vec<IndexedFileText>,
    parsers: Vec<String>,
    symbols: Vec<CodeSymbol>,
    symbol_relations: Vec<SymbolRelation>,
    graph_entities: BTreeSet<String>,
    graph_relations: BTreeSet<String>,
    graph_occurrences: BTreeSet<String>,
    graph_coverage: BTreeSet<String>,
    search_symbol_summaries: Vec<String>,
    resolution_keys: Vec<String>,
    entity_exports: Vec<String>,
    relation_dependencies: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct InternalDerivedSnapshot {
    search_symbol_summaries: Vec<String>,
    resolution_keys: Vec<String>,
    entity_exports: Vec<String>,
    relation_dependencies: Vec<String>,
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

fn run_watch_once(repo: &Path, db: &Path) -> Result<(), Box<dyn Error>> {
    let _report = run_watch_once_report(repo, db)?;
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

fn assert_clean_scan_convergence(
    repo: &Path,
    incremental_db: &Path,
    scratch: &Path,
    checkpoint: &str,
) -> Result<(), Box<dyn Error>> {
    let clean_db = scratch.join(format!("clean-{checkpoint}.db"));
    run_scan(repo, &clean_db)?;
    let incremental = derived_result_snapshot(incremental_db)?;
    let mut clean = derived_result_snapshot(&clean_db)?;
    for path in authored_purpose_paths(incremental_db)? {
        clean.unreviewed_purposes.remove(&path);
    }
    if incremental != clean {
        return Err(io::Error::other(format!(
            "incremental results diverged from clean scan at {checkpoint}:\nincremental={incremental:#?}\nclean={clean:#?}"
        ))
        .into());
    }
    Ok(())
}

fn derived_result_snapshot(db: &Path) -> Result<DerivedResultSnapshot, Box<dyn Error>> {
    const GRAPH_ROW_LIMIT: u32 = 10_000;
    const GRAPH_OCCURRENCE_LIMIT: u32 = 1_024;
    let store = AtlasStore::open_read_only(db)?;
    let publication = store
        .index_publication()?
        .ok_or_else(|| io::Error::other("convergence publication missing"))?;
    let project = store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("convergence project identity missing"))?;
    let indexed_nodes = store.load_nodes()?;
    let file_paths = indexed_nodes
        .iter()
        .filter(|indexed| indexed.node.kind == NodeKind::File)
        .map(|indexed| indexed.node.path.clone())
        .collect::<Vec<_>>();
    let file_classifications = store
        .file_content_classifications_for_paths(&file_paths)?
        .into_iter()
        .map(|row| format!("{}:{}", row.path, row.classification.as_str()))
        .collect::<Vec<_>>();
    let mut nodes = Vec::with_capacity(indexed_nodes.len());
    let mut unreviewed_purposes = BTreeMap::new();
    let mut parsers = Vec::new();
    let mut graph_entities = BTreeSet::new();
    let mut graph_coverage = BTreeSet::new();
    let project_coverage =
        store.repository_graph_coverage(project, &CoverageScope::Project, GRAPH_ROW_LIMIT)?;
    if project_coverage.truncated {
        return Err(io::Error::other("project coverage snapshot was truncated").into());
    }
    for coverage in project_coverage.rows {
        if coverage.generation() != publication.generation {
            return Err(io::Error::other("project coverage used a mixed generation").into());
        }
        graph_coverage.insert(coverage_semantics(&coverage)?);
    }
    for indexed in &indexed_nodes {
        nodes.push(serde_json::to_string(&serde_json::json!({
            "path": indexed.node.path,
            "kind": indexed.node.kind,
            "parent_path": indexed.node.parent_path,
            "extension": indexed.node.extension,
            "language": indexed.node.language,
            "size_bytes": indexed.node.size_bytes,
            "content_hash": indexed.node.content_hash,
            "summary": indexed.summary,
        }))?);
        if matches!(
            indexed.purpose.source,
            PurposeSource::Missing | PurposeSource::Generated
        ) {
            unreviewed_purposes.insert(
                indexed.purpose.path.clone(),
                serde_json::to_string(&serde_json::json!({
                "purpose": indexed.purpose.purpose,
                "source": indexed.purpose.source,
                "status": indexed.purpose.status,
                }))?,
            );
        }
        if let Some(metadata) = store.load_source_parse_metadata(&indexed.node.path)? {
            parsers.push(serde_json::to_string(&metadata)?);
        }
        let path = RepositoryNodePath::new(Path::new(&indexed.node.path))?;
        let entities = store.repository_graph_entities_by_path(project, &path, GRAPH_ROW_LIMIT)?;
        if entities.truncated {
            return Err(io::Error::other("entity snapshot was truncated").into());
        }
        for entity in entities.rows {
            if entity.generation() != publication.generation {
                return Err(io::Error::other("entity snapshot used a mixed generation").into());
            }
            graph_entities.insert(serde_json::to_string(entity.selector())?);
        }
        let path_coverage = store.repository_graph_coverage(
            project,
            &CoverageScope::Path { path },
            GRAPH_ROW_LIMIT,
        )?;
        if path_coverage.truncated {
            return Err(io::Error::other("path coverage snapshot was truncated").into());
        }
        for coverage in path_coverage.rows {
            if coverage.generation() != publication.generation {
                return Err(io::Error::other("path coverage used a mixed generation").into());
            }
            graph_coverage.insert(coverage_semantics(&coverage)?);
        }
    }

    let mut graph_relations = BTreeSet::new();
    let mut graph_occurrences = BTreeSet::new();
    for family in GraphRelationKind::ALL {
        let relations = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family { relation: family },
            GRAPH_ROW_LIMIT,
            None,
        )?;
        if relations.truncated {
            return Err(io::Error::other("relation snapshot was truncated").into());
        }
        for row in relations.rows {
            let relation = &row.relation;
            if relation.generation() != publication.generation {
                return Err(io::Error::other("relation snapshot used a mixed generation").into());
            }
            graph_entities.insert(serde_json::to_string(row.source.selector())?);
            if let Some(target) = &row.target {
                graph_entities.insert(serde_json::to_string(target.selector())?);
            }
            let semantics = relation_semantics(
                row.source.selector(),
                relation,
                row.document_unresolved_reason,
            )?;
            graph_relations.insert(semantics.clone());
            let occurrences =
                store.repository_graph_occurrences(relation, GRAPH_OCCURRENCE_LIMIT)?;
            if occurrences.truncated {
                return Err(io::Error::other("occurrence snapshot was truncated").into());
            }
            for occurrence in occurrences.rows {
                if occurrence.generation() != publication.generation {
                    return Err(
                        io::Error::other("occurrence snapshot used a mixed generation").into(),
                    );
                }
                let span = occurrence.span();
                graph_occurrences.insert(serde_json::to_string(&serde_json::json!({
                    "relation": semantics,
                    "file": occurrence.file().as_str(),
                    "start_line": span.start_line(),
                    "start_column": span.start_column(),
                    "end_line": span.end_line(),
                    "end_column": span.end_column(),
                }))?);
            }
        }
    }
    nodes.sort();
    parsers.sort();
    let graph_row_limit = usize::try_from(GRAPH_ROW_LIMIT)?;
    let symbols = store.load_symbols(None, None, graph_row_limit)?;
    if symbols.len() == graph_row_limit {
        return Err(io::Error::other("symbol snapshot reached its row ceiling").into());
    }
    let symbol_relations = store.load_symbol_relations(None, None, graph_row_limit)?;
    if symbol_relations.len() == graph_row_limit {
        return Err(io::Error::other("symbol-relation snapshot reached its row ceiling").into());
    }
    let project_hex = project.as_hex();
    let internal = internal_derived_snapshot(db, &project_hex)?;
    Ok(DerivedResultSnapshot {
        nodes,
        file_classifications,
        unreviewed_purposes,
        texts: store.load_file_texts_for_search(None, true)?,
        parsers,
        symbols,
        symbol_relations,
        graph_entities,
        graph_relations,
        graph_occurrences,
        graph_coverage,
        search_symbol_summaries: internal.search_symbol_summaries,
        resolution_keys: internal.resolution_keys,
        entity_exports: internal.entity_exports,
        relation_dependencies: internal.relation_dependencies,
    })
}

fn authored_purpose_paths(db: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    Ok(AtlasStore::open_read_only(db)?
        .load_nodes()?
        .into_iter()
        .filter(|indexed| {
            matches!(
                indexed.purpose.source,
                PurposeSource::Imported | PurposeSource::Agent
            )
        })
        .map(|indexed| indexed.node.path)
        .collect())
}

fn internal_derived_snapshot(
    db: &Path,
    project_hex: &str,
) -> Result<InternalDerivedSnapshot, Box<dyn Error>> {
    const ROW_LIMIT: usize = 4_096;
    const NORMALIZED_PROJECT: &str = "00000000000000000000000000000000";

    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let sql_limit = i64::try_from(ROW_LIMIT + 1)?;
    let search_symbol_summaries = {
        let mut statement = connection.prepare(
            "SELECT node.path, summary.summary_level, summary.subject, summary.summary
               FROM summaries AS summary
               JOIN nodes AS node ON node.id = summary.node_id
              WHERE node.exists_now = 1
                AND summary.summary_level = 'search'
                AND summary.subject = 'symbols'
              ORDER BY node.path
              LIMIT ?1",
        )?;
        let rows = statement.query_map([sql_limit], |row| {
            Ok(serde_json::json!({
                "path": row.get::<_, String>(0)?,
                "summary_level": row.get::<_, String>(1)?,
                "subject": row.get::<_, String>(2)?,
                "summary": row.get::<_, Option<String>>(3)?,
            })
            .to_string())
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    require_snapshot_below_limit(
        &search_symbol_summaries,
        ROW_LIMIT,
        "search symbol summaries",
    )?;

    let mut resolution_keys = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT project_instance_id, resolution_domain, key_digest, canonical_identity
               FROM graph_resolution_keys
              ORDER BY resolution_domain, canonical_identity
              LIMIT ?1",
        )?;
        let mut rows = statement.query([sql_limit])?;
        while let Some(row) = rows.next()? {
            require_selected_project(row.get::<_, Vec<u8>>(0)?, project_hex)?;
            let domain = row.get::<_, String>(1)?;
            let digest = row.get::<_, Vec<u8>>(2)?;
            let canonical = row.get::<_, String>(3)?;
            require_canonical_digest("resolution key", &digest, &canonical)?;
            let normalized =
                normalize_project_witness(&canonical, project_hex, NORMALIZED_PROJECT)?;
            resolution_keys.push(
                serde_json::json!({
                    "project": NORMALIZED_PROJECT,
                    "resolution_domain": domain,
                    "key_digest": blake3::hash(normalized.as_bytes()).to_hex().to_string(),
                    "canonical_identity": normalized,
                })
                .to_string(),
            );
        }
    }
    require_snapshot_below_limit(&resolution_keys, ROW_LIMIT, "resolution keys")?;

    let mut entity_exports = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT export.project_instance_id, export.entity_key, export.owner_path,
                    export.resolution_domain, export.key_digest,
                    entity.canonical_identity, resolution.canonical_identity
               FROM graph_entity_exports AS export
               JOIN graph_entities AS entity
                 ON entity.project_instance_id = export.project_instance_id
                AND entity.entity_key = export.entity_key
               JOIN graph_resolution_keys AS resolution
                 ON resolution.project_instance_id = export.project_instance_id
                AND resolution.resolution_domain = export.resolution_domain
                AND resolution.key_digest = export.key_digest
              ORDER BY export.owner_path, entity.canonical_identity,
                       export.resolution_domain, resolution.canonical_identity
              LIMIT ?1",
        )?;
        let mut rows = statement.query([sql_limit])?;
        while let Some(row) = rows.next()? {
            require_selected_project(row.get::<_, Vec<u8>>(0)?, project_hex)?;
            let entity_digest = row.get::<_, Vec<u8>>(1)?;
            let owner_path = row.get::<_, String>(2)?;
            let domain = row.get::<_, String>(3)?;
            let resolution_digest = row.get::<_, Vec<u8>>(4)?;
            let entity_canonical = row.get::<_, String>(5)?;
            let resolution_canonical = row.get::<_, String>(6)?;
            require_canonical_digest("export entity", &entity_digest, &entity_canonical)?;
            require_canonical_digest(
                "export resolution key",
                &resolution_digest,
                &resolution_canonical,
            )?;
            let entity =
                normalize_project_witness(&entity_canonical, project_hex, NORMALIZED_PROJECT)?;
            let resolution =
                normalize_project_witness(&resolution_canonical, project_hex, NORMALIZED_PROJECT)?;
            entity_exports.push(
                serde_json::json!({
                    "project": NORMALIZED_PROJECT,
                    "owner_path": owner_path,
                    "entity_key": blake3::hash(entity.as_bytes()).to_hex().to_string(),
                    "entity_canonical_identity": entity,
                    "resolution_domain": domain,
                    "key_digest": blake3::hash(resolution.as_bytes()).to_hex().to_string(),
                    "resolution_canonical_identity": resolution,
                })
                .to_string(),
            );
        }
    }
    require_snapshot_below_limit(&entity_exports, ROW_LIMIT, "entity exports")?;

    let mut relation_dependencies = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT dependency.project_instance_id, dependency.relation_key,
                    dependency.owner_path, dependency.resolution_domain,
                    dependency.key_digest, relation.canonical_identity,
                    resolution.canonical_identity
               FROM graph_relation_dependencies AS dependency
               JOIN graph_relations AS relation
                 ON relation.project_instance_id = dependency.project_instance_id
                AND relation.relation_key = dependency.relation_key
               JOIN graph_resolution_keys AS resolution
                 ON resolution.project_instance_id = dependency.project_instance_id
                AND resolution.resolution_domain = dependency.resolution_domain
                AND resolution.key_digest = dependency.key_digest
              ORDER BY dependency.owner_path, relation.canonical_identity,
                       dependency.resolution_domain, resolution.canonical_identity
              LIMIT ?1",
        )?;
        let mut rows = statement.query([sql_limit])?;
        while let Some(row) = rows.next()? {
            require_selected_project(row.get::<_, Vec<u8>>(0)?, project_hex)?;
            let relation_digest = row.get::<_, Vec<u8>>(1)?;
            let owner_path = row.get::<_, String>(2)?;
            let domain = row.get::<_, String>(3)?;
            let resolution_digest = row.get::<_, Vec<u8>>(4)?;
            let relation_canonical = row.get::<_, String>(5)?;
            let resolution_canonical = row.get::<_, String>(6)?;
            require_canonical_digest("dependency relation", &relation_digest, &relation_canonical)?;
            require_canonical_digest(
                "dependency resolution key",
                &resolution_digest,
                &resolution_canonical,
            )?;
            let relation =
                normalize_project_witness(&relation_canonical, project_hex, NORMALIZED_PROJECT)?;
            let resolution =
                normalize_project_witness(&resolution_canonical, project_hex, NORMALIZED_PROJECT)?;
            relation_dependencies.push(
                serde_json::json!({
                    "project": NORMALIZED_PROJECT,
                    "owner_path": owner_path,
                    "relation_key": blake3::hash(relation.as_bytes()).to_hex().to_string(),
                    "relation_canonical_identity": relation,
                    "resolution_domain": domain,
                    "key_digest": blake3::hash(resolution.as_bytes()).to_hex().to_string(),
                    "resolution_canonical_identity": resolution,
                })
                .to_string(),
            );
        }
    }
    require_snapshot_below_limit(&relation_dependencies, ROW_LIMIT, "relation dependencies")?;
    Ok(InternalDerivedSnapshot {
        search_symbol_summaries,
        resolution_keys,
        entity_exports,
        relation_dependencies,
    })
}

fn require_snapshot_below_limit<T>(
    rows: &[T],
    limit: usize,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    if rows.len() > limit {
        return Err(io::Error::other(format!("{label} snapshot exceeded its row ceiling")).into());
    }
    Ok(())
}

fn require_selected_project(project: Vec<u8>, project_hex: &str) -> Result<(), Box<dyn Error>> {
    let mut actual = String::with_capacity(project.len() * 2);
    for byte in project {
        write!(&mut actual, "{byte:02x}")?;
    }
    if actual != project_hex {
        return Err(io::Error::other("internal graph row belongs to another project").into());
    }
    Ok(())
}

fn require_canonical_digest(
    label: &str,
    digest: &[u8],
    canonical: &str,
) -> Result<(), Box<dyn Error>> {
    if digest != blake3::hash(canonical.as_bytes()).as_bytes() {
        return Err(io::Error::other(format!("{label} digest does not match its witness")).into());
    }
    Ok(())
}

fn normalize_project_witness(
    canonical: &str,
    project_hex: &str,
    normalized_project: &str,
) -> Result<String, Box<dyn Error>> {
    if project_hex.len() != normalized_project.len() {
        return Err(io::Error::other("normalized project identity changed field length").into());
    }
    let project_field = format!("|{}:{project_hex}", project_hex.len());
    if !canonical.contains(&project_field) {
        return Err(
            io::Error::other("canonical graph witness omitted its project identity").into(),
        );
    }
    let normalized_field = format!("|{}:{normalized_project}", normalized_project.len());
    Ok(canonical.replace(&project_field, &normalized_field))
}

fn relation_semantics(
    source: &projectatlas_core::graph::EntitySelector,
    relation: &projectatlas_core::graph::LogicalRelation,
    document_unresolved_reason: Option<projectatlas_core::graph::DocumentTargetUnresolvedReason>,
) -> Result<String, Box<dyn Error>> {
    let resolution = match relation.resolution() {
        RelationResolution::Resolved { selector, .. } => {
            serde_json::json!({"status": "resolved", "selector": selector})
        }
        RelationResolution::Ambiguous {
            reference,
            candidates,
        } => serde_json::json!({
            "status": "ambiguous",
            "reference": reference.as_str(),
            "candidates": candidates.get(),
        }),
        RelationResolution::Unresolved { reference } => serde_json::json!({
            "status": "unresolved",
            "reference": reference.as_str(),
        }),
        RelationResolution::External { external, .. } => {
            serde_json::json!({"status": "external", "external": external})
        }
    };
    Ok(serde_json::to_string(&serde_json::json!({
        "source": source,
        "kind": relation.kind(),
        "resolution": resolution,
        "confidence": relation.confidence(),
        "completeness": relation.completeness(),
        "document_unresolved_reason": document_unresolved_reason,
    }))?)
}

fn coverage_semantics(
    coverage: &projectatlas_core::graph::CoverageRecord,
) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(&serde_json::json!({
        "scope": coverage.scope(),
        "relation": coverage.relation(),
        "state": coverage.state(),
        "total": coverage.total(),
        "covered": coverage.covered(),
        "omitted": coverage.omitted(),
        "reason": coverage.reason().map(projectatlas_core::graph::GraphIdentityText::as_str),
        "reached_limit": coverage.reached_limit(),
    }))?)
}

fn git_success(root: &Path, arguments: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = git_command_for_root(root).args(arguments).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "git {arguments:?} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

#[test]
fn full_repository_intelligence_flow_indexes_database_and_commands() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join("crates").join("atlas_core").join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join("tmp"))?;
    fs::create_dir_all(repo.join("target"))?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"atlas-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
    )?;
    fs::write(
        repo.join("build.rs"),
        "fn main() {\n    println!(\"cargo:rerun-if-changed=build.rs\");\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "mod service;\nconst CONTENT_ONLY_ROUTE: &str = \"contentOnlyRoute\";\nfn main() {\n    service::run();\n}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("service.rs"),
        "pub struct Runner;\n\nimpl Runner {\n    pub fn execute(&self) {\n        helper();\n    }\n}\n\npub fn run() {\n    Runner.execute();\n}\n\nfn helper() {}\n",
    )?;
    fs::write(
        repo.join("crates")
            .join("atlas_core")
            .join(SRC_DIR_NAME)
            .join("lib.rs"),
        "pub fn library_entry() -> &'static str {\n    \"atlas\"\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("files:"))
        .stdout(predicate::str::contains("folders:"))
        .stdout(predicate::str::contains("symbols:"));

    if !db.exists() {
        return Err(io::Error::other("ProjectAtlas database was not created").into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["folders", "crates", "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("crates/atlas_core"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "service", "--folder", SRC_DIR_NAME, "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/service.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "files",
            "contentOnlyRoute",
            "--folder",
            SRC_DIR_NAME,
            "--file-pattern",
            "*.rs",
            "--include-content",
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--query", "serde", "--limit", "20"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dependency"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "list",
            "--file",
            "src/service.rs",
            "--limit",
            "20",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Runner"))
        .stdout(predicate::str::contains("execute"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "relations", "--query", "helper", "--limit", "20"])
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "search",
            "Runner",
            "--file-pattern",
            "src/*.rs",
            "--context-lines",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/service.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["slice", "src/service.rs", "--symbol", "execute"])
        .assert()
        .success()
        .stdout(predicate::str::contains("helper();"));

    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs", "--limit", "1"])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other("limited json summary command failed").into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    require_json_string(&summary_json, &["file_path"], "src/service.rs")?;
    require_json_usize(&summary_json, &["limit"], 1)?;
    require_json_bool(&summary_json, &["truncated"], true)?;
    require_json_usize(&summary_json, &["total_functions"], 2)?;
    require_json_usize(&summary_json, &["total_methods"], 1)?;
    require_json_usize(&summary_json, &["total_types"], 1)?;
    require_json_array_len(&summary_json, &["functions"], 1)?;
    require_json_array_len(&summary_json, &["methods"], 1)?;
    require_json_array_len(&summary_json, &["types"], 1)?;

    let cross_file_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs", "--limit", "5"])
        .output()?;
    if !cross_file_summary.status.success() {
        return Err(io::Error::other("cross-file json summary command failed").into());
    }
    let cross_file_json: Value = serde_json::from_slice(&cross_file_summary.stdout)?;
    require_json_string(
        &cross_file_json,
        &["functions", "0", "called_by", "0"],
        "src/main.rs::main",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("health_findings"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("token")
        .assert()
        .success()
        .stdout(predicate::str::contains("estimated_saved"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["parity", "report"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parity:"))
        .stdout(predicate::str::contains(
            "profile: \"repository-intelligence\"",
        ))
        .stdout(predicate::str::contains("5 suggested"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["parity", "--profile", "repository-intelligence"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parity:"))
        .stdout(predicate::str::contains("5 suggested"));

    Ok(())
}

#[test]
fn gradle_dsl_tasks_are_symbols_and_file_ranking_signals() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::write(
        repo.join("build.gradle.kts"),
        r#"
import org.springframework.boot.gradle.tasks.run.BootRun

fun loadDotEnv() = emptyMap<String, String>()

tasks.register<BootRun>("bootRunE2E") {
    group = "verification"
}

val verifyAtlas by tasks.registering {
    group = "verification"
}

tasks {
    register<Copy>("copyE2EReports") {
        group = "verification"
    }
}
"#,
    )?;
    fs::write(
        repo.join("build.gradle"),
        r"
plugins { id 'java' }

tasks.register('bootRunSmoke', BootRun) {
    group = 'verification'
}

task cleanE2E(type: Delete) {}

tasks {
    create('copyGroovyReports') {
        group = 'verification'
    }
}
",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "files",
            "bootRunE2E",
            "--file-pattern",
            "*.kts",
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("build.gradle.kts"));

    let kotlin_summary = json_summary_command(&repo, &db, "build.gradle.kts")?;
    require_json_string(
        &kotlin_summary,
        &["parser_kind"],
        "tree-sitter-symbol-graph",
    )?;
    require_json_contains(&kotlin_summary, &["content_summary"], "bootRunE2E")?;
    require_json_contains(&kotlin_summary, &["content_summary"], "copyE2EReports")?;
    require_json_contains(&kotlin_summary, &["content_summary"], "verifyAtlas")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "list",
            "--file",
            "build.gradle.kts",
            "--query",
            "bootRunE2E",
            "--limit",
            "20",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("bootRunE2E"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "files",
            "bootRunSmoke",
            "--file-pattern",
            "*.gradle",
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("build.gradle"));

    let groovy_summary = json_summary_command(&repo, &db, "build.gradle")?;
    require_json_string(&groovy_summary, &["parser_kind"], "fallback-symbol-graph")?;
    require_json_contains(&groovy_summary, &["content_summary"], "bootRunSmoke")?;
    require_json_contains(&groovy_summary, &["content_summary"], "copyGroovyReports")?;
    require_json_contains(&groovy_summary, &["content_summary"], "cleanE2E")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "list",
            "--file",
            "build.gradle",
            "--query",
            "copyGroovyReports",
            "--limit",
            "20",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("copyGroovyReports"));

    Ok(())
}

#[test]
fn parity_alias_passes_clean_repository() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn library_entry() -> &'static str {\n    \"atlas\"\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    for (path, purpose) in [
        (".", "Repository root for clean parity alias tests."),
        (
            SRC_DIR_NAME,
            "Rust source folder for clean parity alias tests.",
        ),
        (
            "src/lib.rs",
            "Rust library source file for clean parity alias tests.",
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

    for args in [
        vec!["parity", "report", "--profile", "repository-intelligence"],
        vec!["parity", "--profile", "repository-intelligence"],
    ] {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .arg("--format")
            .arg("json")
            .arg("--db")
            .arg(&db)
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "clean parity command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        let parity_json: Value = serde_json::from_slice(&output.stdout)?;
        require_json_bool(&parity_json, &["ok"], true)?;
    }

    Ok(())
}

#[test]
fn powershell_summary_preserves_hyphenated_function_names() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("scripts"))?;
    fs::write(
        repo.join("scripts").join("install-runtime.ps1"),
        "class RuntimeConfig {\n}\n\nfunction Resolve-DefaultProjectRoot {\n}\n\nfunction Get-ReleaseRuntimeInstallPath {\n}\n\nfunction Install-ReleaseBinary {\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let summary = json_summary_command(&repo, &db, "scripts/install-runtime.ps1")?;
    require_json_string(&summary, &["summary_status"], "ok")?;
    require_json_usize(&summary, &["total_classes"], 1)?;
    require_json_string(&summary, &["classes", "0", "name"], "RuntimeConfig")?;
    require_json_string(&summary, &["classes", "0", "kind"], "class")?;
    let function_names = summary
        .get("functions")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("PowerShell summary functions array missing"))?
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    for expected in [
        "Resolve-DefaultProjectRoot",
        "Get-ReleaseRuntimeInstallPath",
        "Install-ReleaseBinary",
    ] {
        if !function_names.contains(&expected) {
            return Err(io::Error::other(format!(
                "PowerShell summary missed full function name {expected}: {function_names:?}"
            ))
            .into());
        }
    }
    for truncated in ["Resolve", "Get", "Install"] {
        if function_names.contains(&truncated) {
            return Err(io::Error::other(format!(
                "PowerShell summary included truncated function name {truncated}: {function_names:?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn search_and_symbol_slice_are_bounded_and_identity_safe() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("a.rs"), "needle one café λ\n")?;
    fs::write(repo.join(SRC_DIR_NAME).join("b.rs"), "needle two\n")?;
    fs::write(
        repo.join(CARGO_LOCK_FILE_NAME),
        "[[package]]\nname = \"windows-sys\"\nversion = \"0.59.0\"\n\n[[package]]\nname = \"windows-sys\"\nversion = \"0.60.0\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "struct A;\nimpl A {\n    fn run(&self) {\n        a();\n    }\n}\nstruct B;\nimpl B {\n    fn run(&self) {\n        b();\n    }\n}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let raw_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "needle", "--file-pattern", "*.rs", "--limit", "1"])
        .output()?;
    if !raw_search.status.success() {
        return Err(io::Error::other("bounded search command failed").into());
    }
    let search_json: Value = serde_json::from_slice(&raw_search.stdout)?;
    require_json_string(&search_json, &["retrieval_mode"], "lexical")?;
    require_json_string(
        &search_json,
        &["strategy"],
        "fts5-bm25-candidates-exact-verified",
    )?;
    require_json_usize(&search_json, &["returned"], 1)?;
    require_json_usize(&search_json, &["searched_files"], 1)?;
    require_json_usize(&search_json, &["candidate_files"], 2)?;
    require_json_bool(&search_json, &["truncated"], true)?;

    let raw_fallback = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "search",
            "needle",
            "--regex",
            "--file-pattern",
            "*.rs",
            "--limit",
            "1",
        ])
        .output()?;
    if !raw_fallback.status.success() {
        return Err(io::Error::other("regex fallback search command failed").into());
    }
    let fallback_json: Value = serde_json::from_slice(&raw_fallback.stdout)?;
    require_json_string(&fallback_json, &["retrieval_mode"], "lexical")?;
    require_json_string(&fallback_json, &["strategy"], "persisted-text-fallback")?;
    require_json_usize(&fallback_json, &["candidate_files"], 0)?;
    if search_json["results"] != fallback_json["results"] {
        return Err(io::Error::other("FTS and fallback CLI results diverged").into());
    }

    let semantic = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "needle", "--retrieval-mode", "semantic"])
        .output()?;
    if semantic.status.success() {
        return Err(io::Error::other("unavailable semantic search unexpectedly succeeded").into());
    }
    let semantic_error: Value = serde_json::from_slice(&semantic.stderr)?;
    require_json_string(
        &semantic_error,
        &["error", "kind"],
        "search_capability_unavailable",
    )?;
    require_json_string(
        &semantic_error,
        &["error", "search_capability", "requested_mode"],
        "semantic",
    )?;
    require_json_string(
        &semantic_error,
        &["error", "search_capability", "state"],
        "not-installed",
    )?;

    let json_slice = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            "src/a.rs",
            "--start-line",
            "1",
            "--output-bytes",
            "1024",
        ])
        .output()?;
    if !json_slice.status.success() || json_slice.stdout.len() > 1_024 {
        return Err(io::Error::other(format!(
            "bounded JSON slice failed or exceeded its envelope: {}",
            String::from_utf8_lossy(&json_slice.stderr)
        ))
        .into());
    }
    let json_slice_payload: Value = serde_json::from_slice(&json_slice.stdout)?;
    require_json_string(&json_slice_payload, &["content"], "needle one café λ")?;
    if json_slice_payload.get("output_budget").is_some() {
        return Err(io::Error::other("slice budget changed the compatibility JSON schema").into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            "src/a.rs",
            "--start-line",
            "1",
            "--output-bytes",
            "64",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slice output exceeds"));

    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let mcp_messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-search-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_search","arguments":{"pattern":"needle","file_pattern":"*.rs","limit":1}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_search","arguments":{"pattern":"needle","retrieval_mode":"semantic"}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_search","arguments":{"pattern":"needle","retrieval_mode":"hybrid"}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/a.rs","start_line":1,"output_bytes":1024}}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/a.rs","start_line":1,"output_bytes":64}}}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/lib.rs","symbol":"run","symbol_parent":"B","output_bytes":1024}}}"#,
    ];
    let mcp_stdout = run_mcp_stdio(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &mcp_messages,
    )?;
    let lexical_mcp = mcp_tool_text(&mcp_stdout, 2)?;
    if !lexical_mcp.contains("retrieval_mode: lexical")
        || !lexical_mcp.contains("fts5-bm25-candidates-exact-verified")
        || !lexical_mcp.contains("needle")
    {
        return Err(io::Error::other(format!(
            "MCP lexical search did not use the bounded exact-verified path: {lexical_mcp}"
        ))
        .into());
    }
    for (id, mode) in [(3, "semantic"), (4, "hybrid")] {
        let unavailable = mcp_tool_text(&mcp_stdout, id)?;
        if !unavailable.contains("search_capability_unavailable")
            || !unavailable.contains("requested_mode")
            || !unavailable.contains(mode)
            || !unavailable.contains("not-installed")
            || !unavailable.contains("recovery")
        {
            return Err(io::Error::other(format!(
                "MCP {mode} search lost typed unavailable state: {unavailable}"
            ))
            .into());
        }
    }
    let line_slice = mcp_tool_text(&mcp_stdout, 5)?;
    if line_slice.len() > 1_024 || !line_slice.contains("needle one café λ") {
        return Err(io::Error::other(format!(
            "bounded MCP line slice lost UTF-8 or exceeded its envelope: {line_slice}"
        ))
        .into());
    }
    let rejected_slice = mcp_tool_text(&mcp_stdout, 6)?;
    if !rejected_slice.contains("slice output exceeds") {
        return Err(io::Error::other(format!(
            "MCP accepted an oversized slice envelope: {rejected_slice}"
        ))
        .into());
    }
    let symbol_slice = mcp_tool_text(&mcp_stdout, 7)?;
    if symbol_slice.len() > 1_024 || !symbol_slice.contains("b();") || symbol_slice.contains("a();")
    {
        return Err(io::Error::other(format!(
            "bounded MCP symbol slice lost selection or exceeded its envelope: {symbol_slice}"
        ))
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "slice", "src/lib.rs", "run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous"))
        .stderr(predicate::str::contains("parent=A"))
        .stderr(predicate::str::contains("parent=B"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "slice",
            "src/lib.rs",
            "run",
            "--symbol-parent",
            "B",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("b();"))
        .stdout(predicate::str::contains("a();").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "slice",
            "src/lib.rs",
            "run",
            "--symbol-parent",
            "B",
            "--output-bytes",
            "64",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slice output exceeds"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "slice",
            CARGO_LOCK_FILE_NAME,
            "windows-sys",
            "--symbol-kind",
            "dependency",
            "--symbol-line",
            "6",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("start_line: 6"))
        .stdout(predicate::str::contains(
            "content: \"name = \\\"windows-sys\\\"\"",
        ))
        .stdout(predicate::str::contains("start_line: 2").not());

    Ok(())
}

#[test]
fn skipped_and_failed_symbol_builds_keep_a_consistent_projection() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    let source = repo.join(SRC_DIR_NAME).join("main.rs");
    fs::write(&source, "pub fn old_too_large_symbol() {}\n")?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"skip-summary\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    fs::write(&source, "pub fn new_too_large_symbol() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "build", ".", "--max-bytes", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refresh_required"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "build", ".", "--max-bytes", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("too_large: 2"));

    let store = AtlasStore::open(&db)?;
    let skipped_symbols = store.load_symbols(Some("src/main.rs"), None, 10)?;
    if !skipped_symbols.is_empty() {
        return Err(io::Error::other(
            "oversized symbol rebuild retained a stale symbol projection",
        )
        .into());
    }

    fs::write(&source, "pub fn old_timeout_symbol() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    let timeout_publication = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("pre-timeout publication missing"))?;

    fs::write(&source, "pub fn new_timeout_symbol() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("index work deadline was reached"));

    let retained = AtlasStore::open_read_only(&db)?;
    let retained_publication = retained
        .index_publication()?
        .ok_or_else(|| io::Error::other("retained publication missing"))?;
    if retained_publication.generation != timeout_publication.generation {
        return Err(io::Error::other("timed-out refresh advanced the generation").into());
    }
    let retained_symbols = retained.load_symbols(Some("src/main.rs"), None, 10)?;
    if !retained_symbols
        .iter()
        .any(|symbol| symbol.name == "old_timeout_symbol")
        || retained_symbols
            .iter()
            .any(|symbol| symbol.name == "new_timeout_symbol")
    {
        return Err(io::Error::other("timed-out refresh replaced the last-valid symbols").into());
    }

    Ok(())
}

/// Expected summary behavior for one checked-in language fixture.
struct LanguageSummaryBaseline {
    /// Repository-relative fixture path.
    path: String,
    /// Expected detected language or file family.
    language: String,
    /// Expected summary parser family.
    parser_kind: String,
    /// Expected quality status for agent consumers.
    status: String,
    /// Expected deterministic content summary.
    summary: String,
    /// Minimum expected symbol count.
    minimum_symbol_count: usize,
}

/// Decode exact baseline summaries for representative supported language families.
fn language_summary_baselines() -> Result<Vec<LanguageSummaryBaseline>, Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| io::Error::other("workspace root not found"))?;
    let baseline_text = fs::read_to_string(
        workspace_root
            .join("fixtures")
            .join("languages")
            .join("baselines.toon"),
    )?;
    let normalized_baseline_text = baseline_text.replace("\r\n", "\n").replace('\r', "\n");
    let decoded: Value = toon_format::decode_default(&normalized_baseline_text)
        .map_err(|error| io::Error::other(format!("baseline TOON decode failed: {error}")))?;
    let rows = decoded
        .get("summaries")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("baseline TOON missing summaries array"))?;
    rows.iter()
        .map(|row| {
            let min_symbols = row
                .get("min_symbols")
                .and_then(Value::as_u64)
                .ok_or_else(|| io::Error::other("baseline row missing min_symbols"))?;
            Ok(LanguageSummaryBaseline {
                path: required_baseline_string(row, "path")?,
                language: required_baseline_string(row, "language")?,
                parser_kind: required_baseline_string(row, "parser_kind")?,
                status: required_baseline_string(row, "status")?,
                summary: required_baseline_string(row, "summary")?,
                minimum_symbol_count: usize::try_from(min_symbols)?,
            })
        })
        .collect()
}

/// Return a required string from a decoded baseline row.
fn required_baseline_string(row: &Value, field: &str) -> Result<String, Box<dyn Error>> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| io::Error::other(format!("baseline row missing {field}")).into())
}

/// Return path-based language fixtures without ordinary extensions.
fn special_language_fixtures() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "special/Cargo.toml",
            "cargo-manifest",
            "[package]\nname = \"all-language-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        ),
        (
            "special/Cargo.lock",
            "cargo-lock",
            "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"all-language-fixture\"\nversion = \"0.1.0\"\n",
        ),
        ("special/build.rs", "rust-build-script", "fn main() {}\n"),
        ("special/Dockerfile", "dockerfile", "FROM scratch\n"),
        ("special/Makefile", "makefile", "all:\n\t@echo ok\n"),
    ]
}

/// Return minimal valid fixture content for one supported extension.
fn fixture_content_for_extension(extension: &str) -> &'static str {
    let normalized = extension.to_ascii_lowercase();
    match normalized.as_str() {
        ".py" | ".pyw" => "def fixture():\n    return \"ok\"\n",
        ".js" | ".jsx" | ".mjs" | ".cjs" => "export function fixture() { return \"ok\"; }\n",
        ".ts" => "export function fixture(): string { return \"ok\"; }\n",
        ".tsx" => "export function Fixture() { return <div />; }\n",
        ".d.ts" => "export interface Fixture { value: string }\n",
        ".java" => "class Fixture { void run() {} }\n",
        ".c" => "void fixture(void) {}\n",
        ".cpp" | ".cxx" | ".cc" => "class Fixture { void run() {} };\n",
        ".h" => "void fixture_header(void);\n",
        ".hpp" | ".hxx" | ".hh" => "class FixtureHeader { void run(); };\n",
        ".cs" => "class Fixture { void Run() {} }\n",
        ".go" => "package fixture\nfunc Run() {}\n",
        ".m" | ".mm" => {
            "@interface Fixture\n- (void)run;\n@end\n@implementation Fixture\n- (void)run {}\n@end\n"
        }
        ".rb" => "def fixture\n  :ok\nend\n",
        ".php" => "<?php function fixture() { return 'ok'; }\n",
        ".swift" => "func fixture() -> String { \"ok\" }\n",
        ".kt" | ".kts" => "class Fixture { fun run() = \"ok\" }\n",
        ".rs" => "pub fn fixture() {}\n",
        ".scala" => "object Fixture { def run(): String = \"ok\" }\n",
        ".sh" | ".bash" | ".zsh" => "#!/usr/bin/env sh\necho ok\n",
        ".ps1" | ".psm1" | ".psd1" => "function Invoke-Fixture { 'ok' }\n",
        ".bat" | ".cmd" => "@echo off\necho ok\n",
        ".r" => "fixture <- function() { \"ok\" }\n",
        ".pl" | ".pm" => "sub fixture { return 'ok'; }\n",
        ".lua" => "function fixture() return 'ok' end\n",
        ".dart" => "String fixture() => 'ok';\n",
        ".hs" => "fixture = \"ok\"\n",
        ".ml" | ".mli" | ".fs" | ".fsx" => "let fixture = \"ok\"\n",
        ".clj" | ".cljs" => "(defn fixture [] \"ok\")\n",
        ".vim" => "function! Fixture()\nendfunction\n",
        ".zig" | ".zon" => "pub fn fixture() void {}\n",
        ".html" | ".htm" => "<!doctype html><title>Fixture</title><h1>Fixture</h1>\n",
        ".css" | ".scss" | ".sass" | ".less" | ".styl" | ".stylus" => ":root { --fixture: ok; }\n",
        ".md" | ".mdx" => "# Fixture\n\n## Usage\n",
        ".json" => "{\"name\":\"fixture\"}\n",
        ".jsonc" => "{// comment\n\"name\":\"fixture\"}\n",
        ".xml" => "<fixture />\n",
        ".yml" | ".yaml" => "name: fixture\n",
        ".toml" => "name = \"fixture\"\n",
        ".toon" => "fixture:\n  name: fixture\n",
        ".txt" => "fixture text\n",
        ".ini" | ".cfg" | ".conf" | ".properties" => "name=fixture\n",
        ".vue" => "<script setup>\nconst fixture = 'ok'\n</script>\n",
        ".svelte" => "<script>let fixture = 'ok';</script>\n",
        ".astro" => "---\nconst fixture = 'ok';\n---\n<div>{fixture}</div>\n",
        ".jsp" | ".jspx" | ".jspf" | ".tag" | ".tagx" => "<%@ page language=\"java\" %>\n",
        ".gsp" => "<html><body>${fixture}</body></html>\n",
        ".gradle" | ".groovy" => "def fixture = 'ok'\n",
        ".proto" => "syntax = \"proto3\";\nmessage Fixture {}\n",
        ".hbs" | ".handlebars" | ".ejs" | ".pug" | ".ftl" | ".mustache" | ".liquid" | ".erb" => {
            "fixture {{name}}\n"
        }
        ".sql" | ".ddl" | ".dml" | ".mysql" | ".postgresql" | ".psql" | ".sqlite" | ".mssql"
        | ".oracle" | ".ora" | ".db2" | ".proc" | ".procedure" | ".func" | ".function"
        | ".view" | ".trigger" | ".index" | ".migration" | ".seed" | ".fixture" | ".schema"
        | ".cql" | ".cypher" | ".sparql" | ".gql" | ".liquibase" | ".flyway" => "SELECT 1;\n",
        _ => "fixture\n",
    }
}

/// Return whether a summary is only the scanner byte-count fallback.
fn is_scanner_byte_summary(summary: &str) -> bool {
    let trimmed = summary.trim_end_matches('.');
    let Some((_, tail)) = trimmed.rsplit_once(", ") else {
        return false;
    };
    let Some(number) = tail.strip_suffix(" bytes") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

/// Return the generated Python baseline source used only inside temporary repos.
fn python_baseline_fixture_source() -> &'static str {
    "\"\"\"Python fixture module for ProjectAtlas language coverage.\"\"\"\n\n\nclass Builder:\n    \"\"\"Builds atlas state.\"\"\"\n\n    def build(self):\n        \"\"\"Build the atlas.\"\"\"\n        return helper()\n\n\ndef helper():\n    return \"atlas\"\n"
}

/// Copy a fixture directory tree into a temporary repository.
fn copy_directory_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Run one release-candidate CLI command and decode its typed JSON output.
fn run_mcp_contract_json(
    executable: &Path,
    cwd: &Path,
    arguments: &[String],
) -> Result<Value, Box<dyn Error>> {
    let output = StdCommand::new(executable)
        .current_dir(cwd)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "MCP contract CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Run one normal packaged JSON command with telemetry disabled and an exact version guard.
fn run_packaged_cli_json(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    arguments: &[&str],
) -> Result<Value, Box<dyn Error>> {
    let mut command = vec![
        "--require-version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        "--db".to_string(),
        database.display().to_string(),
    ];
    command.extend(arguments.iter().map(|argument| (*argument).to_string()));
    run_mcp_contract_json(executable, cwd, &command)
}

/// Return the exact advertised inventory from a real release-candidate stdio process.
fn run_mcp_contract_inventory(
    executable: &Path,
    cwd: &Path,
    database: &Path,
) -> Result<String, Box<dyn Error>> {
    let (mut session, initialized) = McpContractSession::spawn_initialized(
        executable,
        cwd,
        database,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
    )?;
    let operation_result = (|| -> Result<String, Box<dyn Error>> {
        let tools = session.request("tools/list", &serde_json::json!({}))?;
        Ok(format!(
            "{}\n{}\n",
            serde_json::to_string(&initialized)?,
            serde_json::to_string(&tools)?
        ))
    })();
    complete_mcp_test_after_shutdown(operation_result, || session.shutdown())
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

/// Return one complete MCP response envelope by numeric request id.
fn mcp_response(stdout: &str, id: i64) -> Result<Value, Box<dyn Error>> {
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let response: Value = serde_json::from_str(line)?;
        if response.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(response);
        }
    }
    Err(io::Error::other(format!("MCP response {id} is missing")).into())
}

/// Prove that every frozen v0.3.26 and v0.4.0 MCP contract remains intact.
fn assert_frozen_mcp_surfaces_compatible(stdout: &str) -> Result<(), Box<dyn Error>> {
    let baseline: Value = serde_json::from_str(include_str!("fixtures/mcp-v0.3.26-tools.json"))?;
    let baseline_tools = baseline
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("frozen MCP fixture has no tools array"))?;
    let response = mcp_response(stdout, 2)?;
    let current_tools = response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("current MCP tools/list response has no tools array"))?;
    let baseline_by_name = mcp_tools_by_name(baseline_tools)?;
    let current_by_name = mcp_tools_by_name(current_tools)?;
    let missing_tools = baseline_by_name
        .keys()
        .filter(|name| !current_by_name.contains_key(**name))
        .copied()
        .collect::<Vec<_>>();
    if !missing_tools.is_empty() {
        return Err(io::Error::other(format!(
            "MCP inventory removed frozen v0.3.26 tools: missing={missing_tools:?} current={:?}",
            current_by_name.keys().collect::<Vec<_>>()
        ))
        .into());
    }
    for (name, baseline_tool) in baseline_by_name {
        let current_tool = current_by_name
            .get(name)
            .ok_or_else(|| io::Error::other(format!("current MCP tool {name} is missing")))?;
        if baseline_tool.get("description") != current_tool.get("description") {
            return Err(io::Error::other(format!(
                "MCP tool description drifted for {name}: baseline={:?}, current={:?}",
                baseline_tool.get("description"),
                current_tool.get("description")
            ))
            .into());
        }
        let baseline_schema = baseline_tool
            .get("inputSchema")
            .ok_or_else(|| io::Error::other(format!("baseline schema missing for {name}")))?;
        let normalized_schema = (name == "atlas_purpose_review")
            .then(|| inline_legacy_purpose_review_item_schema(baseline_schema))
            .transpose()?;
        assert_json_contract_subset(
            &format!("{name}.inputSchema"),
            normalized_schema.as_ref().unwrap_or(baseline_schema),
            current_tool
                .get("inputSchema")
                .ok_or_else(|| io::Error::other(format!("current schema missing for {name}")))?,
        )?;
    }
    let v040: Value = serde_json::from_str(include_str!("fixtures/mcp-v0.4.0-schema-delta.json"))?;
    for (field, expected) in [
        ("base", "v0.3.26"),
        ("release", "v0.4.0"),
        (
            "windows_release_sha256",
            "09423b83011ab14fc2254f7ee29edae4c1fc797df167d345aefa1ef9d8bfbcda",
        ),
    ] {
        require_json_string(&v040, &[field], expected)?;
    }
    if v040.get("normalization")
        != Some(&json!([
            "atlas_purpose_review.inputSchema.properties.items.items",
            "atlas_root_set.inputSchema.properties.transition",
            "atlas_search.inputSchema.properties.retrieval_mode"
        ]))
    {
        return Err(io::Error::other(
            "frozen v0.4.0 MCP delta does not declare its exact representation-only normalizations",
        )
        .into());
    }
    let v040_tools = v040
        .get("tools")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("frozen v0.4.0 MCP delta has no tools object"))?;
    for (name, baseline_schema) in v040_tools {
        let current_schema = current_by_name
            .get(name.as_str())
            .and_then(|tool| tool.get("inputSchema"))
            .ok_or_else(|| io::Error::other(format!("current MCP tool {name} is missing")))?;
        assert_json_contract_subset(
            &format!("{name}.inputSchema"),
            baseline_schema,
            current_schema,
        )?;
    }
    for name in [
        "atlas_session_brief",
        "atlas_file_summary",
        "atlas_symbol_relations",
    ] {
        let properties = current_by_name[name]
            .get("inputSchema")
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object)
            .ok_or_else(|| io::Error::other(format!("current properties missing for {name}")))?;
        if !properties.contains_key("compact") {
            return Err(io::Error::other(format!(
                "additive compact opt-in is missing from {name}"
            ))
            .into());
        }
    }
    for name in [
        "atlas_init",
        "atlas_map",
        "atlas_root",
        "atlas_config",
        "atlas_ignore_list",
        "atlas_ignore_init_gitignore",
        "atlas_ignore_add",
        "atlas_ignore_remove",
        "atlas_scan",
        "atlas_overview",
        "atlas_folders",
        "atlas_files",
        "atlas_next",
        "atlas_outline",
        "atlas_file_summary",
        "atlas_search",
        "atlas_slice",
        "atlas_symbols_build",
        "atlas_symbols",
        "atlas_symbol_relations",
        "atlas_health",
        "atlas_health_resolve",
        "atlas_lint",
        "atlas_token_report",
        "atlas_parity_report",
        "atlas_settings",
        "atlas_watch_status",
        "atlas_watch_once",
        "atlas_strip_legacy_purpose",
        "atlas_reset_index",
        "atlas_mcp_config",
        "atlas_session_brief",
        "atlas_purpose_queue",
        "atlas_purpose_set",
        "atlas_purpose_review",
    ] {
        let schema_properties = current_by_name
            .get(name)
            .and_then(|tool| tool.get("inputSchema"))
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object)
            .ok_or_else(|| io::Error::other(format!("current MCP tool {name} is missing")))?;
        for property in ["project_path", "worktree"] {
            if !schema_properties.contains_key(property) {
                return Err(io::Error::other(format!(
                    "root-scoped MCP tool {name} omitted {property}"
                ))
                .into());
            }
        }
    }
    for (name, properties) in [
        ("atlas_worktree_list", &["include_retired"][..]),
        ("atlas_worktree_add", &["worktree", "alias"][..]),
        ("atlas_worktree_remove", &["worktree"][..]),
    ] {
        let schema_properties = current_by_name
            .get(name)
            .and_then(|tool| tool.get("inputSchema"))
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object)
            .ok_or_else(|| io::Error::other(format!("current MCP tool {name} is missing")))?;
        for property in properties {
            if !schema_properties.contains_key(*property) {
                return Err(io::Error::other(format!(
                    "current MCP tool {name} omitted {property}"
                ))
                .into());
            }
        }
    }
    Ok(())
}

/// Index MCP tool definitions by their stable public name.
fn mcp_tools_by_name(tools: &[Value]) -> Result<BTreeMap<&str, &Value>, Box<dyn Error>> {
    let mut indexed = BTreeMap::new();
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("MCP tool has no name"))?;
        if indexed.insert(name, tool).is_some() {
            return Err(io::Error::other(format!("duplicate MCP tool name {name}")).into());
        }
    }
    Ok(indexed)
}

/// Require the concrete JSON Schema subset consumed by the supported Codex bridge.
fn assert_codex_bridge_compatible_input_schemas(tools: &[Value]) -> Result<(), Box<dyn Error>> {
    let tools_by_name = mcp_tools_by_name(tools)?;
    for (name, tool) in &tools_by_name {
        let schema = tool
            .get("inputSchema")
            .ok_or_else(|| io::Error::other(format!("{name} omitted inputSchema")))?;
        assert_self_contained_input_schema(&format!("{name}.inputSchema"), schema)?;
    }

    let item_schema = tools_by_name
        .get("atlas_purpose_review")
        .and_then(|tool| tool.get("inputSchema"))
        .and_then(|schema| schema.pointer("/properties/items/items"))
        .ok_or_else(|| io::Error::other("atlas_purpose_review omitted its item schema"))?;
    if item_schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(io::Error::other(format!(
            "atlas_purpose_review item schema is not a concrete object: {item_schema}"
        ))
        .into());
    }
    let required = item_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    if required != BTreeSet::from(["path"]) {
        return Err(io::Error::other(format!(
            "atlas_purpose_review item required fields drifted: {required:?}"
        ))
        .into());
    }
    let properties = item_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("atlas_purpose_review item omitted properties"))?;
    for (name, expected_type) in [
        ("path", "string"),
        ("purpose", "string"),
        ("confirm_existing", "boolean"),
        ("task", "string"),
        ("work_key", "string"),
        ("state_token", "string"),
    ] {
        let property = properties.get(name).ok_or_else(|| {
            io::Error::other(format!(
                "atlas_purpose_review item omitted property {name:?}"
            ))
        })?;
        if !schema_declares_type(property, expected_type) {
            return Err(io::Error::other(format!(
                "atlas_purpose_review item property {name:?} omitted type {expected_type:?}: {property}"
            ))
            .into());
        }
    }
    if required_members_present(item_schema, &serde_json::json!({}))
        || !required_members_present(item_schema, &serde_json::json!({"path": "src/lib.rs"}))
    {
        return Err(io::Error::other(
            "atlas_purpose_review item schema does not make missing path host-rejectable",
        )
        .into());
    }
    Ok(())
}

/// Reject definitions or references anywhere in a self-contained input schema.
fn assert_self_contained_input_schema(path: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if matches!(key.as_str(), "$defs" | "definitions" | "$ref") {
                    return Err(io::Error::other(format!(
                        "Codex-facing schema retained reference member {path}.{key}"
                    ))
                    .into());
                }
                assert_self_contained_input_schema(&format!("{path}.{key}"), child)?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                assert_self_contained_input_schema(&format!("{path}[{index}]"), child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Return whether one field schema admits the expected JSON primitive type.
fn schema_declares_type(schema: &Value, expected: &str) -> bool {
    schema.get("type").is_some_and(|value| match value {
        Value::String(value) => value == expected,
        Value::Array(values) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    })
}

/// Apply the advertised required-object members to one host-side candidate.
fn required_members_present(schema: &Value, candidate: &Value) -> bool {
    let Some(candidate) = candidate.as_object() else {
        return false;
    };
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .all(|required| candidate.contains_key(required))
}

/// Normalize the one frozen v0.3.26 nested schema whose representation is now inline.
fn inline_legacy_purpose_review_item_schema(schema: &Value) -> Result<Value, Box<dyn Error>> {
    let mut schema = schema.clone();
    let object = schema
        .as_object_mut()
        .ok_or_else(|| io::Error::other("legacy purpose-review schema is not an object"))?;
    let definitions = object
        .remove("$defs")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| io::Error::other("legacy purpose-review schema omitted $defs"))?;
    let item = definitions
        .get("AtlasPurposeReviewItem")
        .cloned()
        .ok_or_else(|| io::Error::other("legacy purpose-review item definition is missing"))?;
    let item_schema = schema
        .pointer_mut("/properties/items/items")
        .ok_or_else(|| io::Error::other("legacy purpose-review item reference is missing"))?;
    *item_schema = item;
    Ok(schema)
}

/// Require every frozen JSON contract member while permitting additive object properties.
fn assert_json_contract_subset(
    path: &str,
    baseline: &Value,
    current: &Value,
) -> Result<(), Box<dyn Error>> {
    match (baseline, current) {
        (Value::Object(baseline), Value::Object(current)) => {
            for (key, baseline_value) in baseline {
                let current_value = current.get(key).ok_or_else(|| {
                    io::Error::other(format!("legacy MCP schema member {path}.{key} is missing"))
                })?;
                assert_json_contract_subset(
                    &format!("{path}.{key}"),
                    baseline_value,
                    current_value,
                )?;
            }
            Ok(())
        }
        _ if baseline == current => Ok(()),
        _ => Err(io::Error::other(format!(
            "legacy MCP schema value drifted at {path}: baseline={baseline}, current={current}"
        ))
        .into()),
    }
}

/// Require that a real CLI summary reports a caller for a named function.
fn assert_summary_called_by(
    repo: &std::path::Path,
    db: &std::path::Path,
    file_path: &str,
    function_name: &str,
    expected_caller: &str,
) -> Result<(), Box<dyn Error>> {
    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .args(["summary", file_path, "--limit", "10"])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other(format!(
            "summary command failed for {file_path}: {}",
            String::from_utf8_lossy(&raw_summary.stderr)
        ))
        .into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    let function = summary_json["functions"]
        .as_array()
        .and_then(|functions| {
            functions
                .iter()
                .find(|function| function["name"].as_str() == Some(function_name))
        })
        .ok_or_else(|| io::Error::other(format!("function {function_name} missing")))?;
    let called_by = function["called_by"]
        .as_array()
        .ok_or_else(|| io::Error::other(format!("called_by missing for {function_name}")))?;
    if called_by
        .iter()
        .any(|caller| caller.as_str() == Some(expected_caller))
    {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {function_name} in {file_path} to be called by {expected_caller}, found {called_by:?}"
        ))
        .into())
    }
}

/// Generate one harness-specific MCP config document.
fn mcp_config_for_harness(repo: &Path, db: &Path, harness: &str) -> Result<Value, Box<dyn Error>> {
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .arg("mcp-config")
        .arg("--harness")
        .arg(harness)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "mcp-config --harness {harness} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Generate one harness-specific MCP config document with nearest-project startup routing.
fn mcp_config_for_harness_with_nearest(
    repo: &Path,
    db: &Path,
    harness: &str,
) -> Result<Value, Box<dyn Error>> {
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .arg("mcp-config")
        .arg("--harness")
        .arg(harness)
        .arg("--nearest-project")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "mcp-config --harness {harness} --nearest-project failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Extract a launchable MCP command and arguments from a generated config.
fn mcp_command_and_args(
    config: &Value,
) -> Result<(std::path::PathBuf, Vec<String>), Box<dyn Error>> {
    let command = json_string_at(config, &["mcpServers", "projectatlas", "command"])?;
    let args = json_at(config, &["mcpServers", "projectatlas", "args"])?
        .as_array()
        .ok_or_else(|| io::Error::other("mcp args missing"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .ok_or_else(|| io::Error::other("mcp arg was not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((std::path::PathBuf::from(command), args))
}

/// Return a nested `JSON` string.
fn json_string_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, Box<dyn Error>> {
    json_at(value, path)?
        .as_str()
        .ok_or_else(|| io::Error::other(format!("expected string at {path:?}")).into())
}

/// Require a nested signed JSON integer value to be greater than a threshold.
fn require_json_i64_greater_than(
    value: &Value,
    path: &[&str],
    threshold: i64,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_i64()
        .ok_or_else(|| io::Error::other(format!("expected signed integer at {path:?}")))?;
    if actual > threshold {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to be greater than {threshold}, found {actual}"
        ))
        .into())
    }
}
