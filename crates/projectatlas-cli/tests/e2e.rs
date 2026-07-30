//! Purpose: Validate `ProjectAtlas` 3 CLI end-to-end behavior.

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
use projectatlas_core::PurposeSource;
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
    READ_AVOIDANCE_CONFIDENCE_MODELED, READ_AVOIDANCE_SCOPE, usage_from_estimates,
};
use projectatlas_db::{
    AtlasStore, HealthResolution, IndexedFileText, PlannerStatisticsPolicy, PlannerStatisticsState,
    RepositoryGraphRelationQuery, TelemetryCheckpointState,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
#[cfg(feature = "optional-parser-supervisor")]
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
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";
const SRC_DIR_NAME: &str = "src";
const TESTS_DIR_NAME: &str = "tests";
const ALPHA_RS_FILE_NAME: &str = "alpha.rs";
const CREATED_RS_FILE_NAME: &str = "created.rs";
const DUPLICATE_RS_FILE_NAME: &str = "duplicate.rs";
const HIDDEN_RS_FILE_NAME: &str = "hidden.rs";
const IGNORED_DIR_NAME: &str = "ignored";
const INSTALLER_RS_FILE_NAME: &str = "installer.rs";
const LIB_RS_FILE_NAME: &str = "lib.rs";
const SCANNED_RS_FILE_NAME: &str = "scanned.rs";
const GIT_DIR_NAME: &str = ".git";
const OUTSIDE_CANARY_FILE_NAME: &str = "outside-canary.txt";
const PARENT_CANARY_FILE_NAME: &str = "parent-canary.txt";
const ATLAS_DIR_NAME: &str = ".projectatlas";
const GITHOOKS_DIR_NAME: &str = ".githooks";
const ISSUE_TEMPLATE_DIR_NAME: &str = "ISSUE_TEMPLATE";
const VERSIONS_DIR_NAME: &str = "versions";
const PRE_PUSH_HOOK_FILE_NAME: &str = "pre-push";
const GIT_REPOSITORY_ENVIRONMENT_VARIABLES: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];
const OPENSPEC_DIR_NAME: &str = "openspec";
const WORKFLOW_DOC_FILE_NAME: &str = "workflow.md";
const CARGO_LOCK_FILE_NAME: &str = "Cargo.lock";
const CODEX_CONFIG_DIR: &str = ".codex";
const CODEX_PLUGIN_MANIFEST_DIR: &str = ".codex-plugin";
const FAKE_CODEX_LOG_FILE: &str = "fake-codex.log";
const FAKE_CODEX_PLUGIN_CACHE_DIR: &str = "plugin-cache";
const FAKE_CODEX_PLUGIN_ADD_FAILURE_MARKER_FILE: &str = "plugin-add-failed.marker";
const FAKE_CODEX_PLUGIN_ADD_FAILURE_MARKER_ENV: &str = "PROJECTATLAS_FAKE_FAILURE_MARKER";
const FAKE_CODEX_SKILL_CONTENT: &str = "# ProjectAtlas\n";
const FAKE_PATH_DIR: &str = "fake-path";
const IGNORED_FIXTURE_DIR: &str = "ignored-dir";
const ISOLATED_HOME_DIR: &str = "isolated-home";
#[cfg(windows)]
const WINDOWS_SYSTEM32_DIR: &str = "System32";
#[cfg(feature = "optional-parser-supervisor")]
const OPTIONAL_PARSER_ARCHIVE_ENV: &str = "PROJECTATLAS_OPTIONAL_PARSER_ARCHIVE";
#[cfg(feature = "optional-parser-supervisor")]
const PARSER_PACK_TEST_HOME_DIR: &str = "home";
#[cfg(feature = "optional-parser-supervisor")]
const PARSER_PACK_TEST_LOCAL_APP_DATA_DIR: &str = "local-app-data";
#[cfg(feature = "optional-parser-supervisor")]
const PARSER_PACK_TEST_XDG_DATA_DIR: &str = "xdg-data";
#[cfg(feature = "optional-parser-supervisor")]
const OPTIONAL_PARSER_PACKS_DIR_NAME: &str = "parser-packs";
const PROJECTATLAS_SKILL_DIR: &str = "skills";
const PROJECTATLAS_SKILL_NAME: &str = "projectatlas";
const SKILL_FILE_NAME: &str = "SKILL.md";
const MCP_CONTRACT_EXECUTABLE_ENV: &str = "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE";
const MCP_CONTRACT_PLUGIN_ROOT_ENV: &str = "PROJECTATLAS_MCP_CONTRACT_PLUGIN_ROOT";
const MCP_CONTRACT_METADATA_CANARY: &str = "mcp_contract_metadata_canary";
const MCP_V041_TOOLS_SHA256: &str =
    "26674d7134973a8f5abdb870a29db6d11e19d4287ec20add04f08653e50dec73";
const AGENT_EFFICIENCY_BENCHMARK_PATH: &str =
    "../../docs/benchmarks/v0.4-agent-navigation-results.json";
const AGENT_EFFICIENCY_PARTIAL_FILE: &str = "partial.json";
const SUBDIR_CONFIG_DIR: &str = "config";
const SESSION_TEST_FILE_NAME: &str = "session.rs";
#[cfg(any(
    windows,
    all(target_os = "macos", feature = "optional-parser-supervisor")
))]
const PROJECTATLAS_LOCAL_APPDATA_DIR: &str = "ProjectAtlas";
#[cfg(all(not(windows), feature = "optional-parser-supervisor"))]
const PROJECTATLAS_XDG_DATA_DIR: &str = "projectatlas";

/// Return one `SQLite` sidecar path for exact no-mutation assertions.
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum McpSqliteEffect {
    None,
    Telemetry,
    DerivedSourceAdvance,
    DerivedGraphAdvance,
    PurposeAdvance(&'static str),
    HealthResolution,
}

struct McpToolContractCase {
    name: &'static str,
    arguments: Value,
    expected_marker: &'static str,
    payload_key: Option<&'static str>,
    effect: McpSqliteEffect,
    telemetry_enabled: bool,
}

#[derive(Clone, Copy)]
enum CliContractOutput {
    JsonObject,
    JsonArray,
    Empty,
    Mcp,
}

struct CliContractCase {
    name: &'static str,
    arguments: Vec<String>,
    output: CliContractOutput,
    effect: McpSqliteEffect,
    expected_exit_code: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct McpDatabaseSnapshot {
    authoritative: BTreeMap<String, String>,
    usage: BTreeMap<String, String>,
    authored_purposes: BTreeMap<String, String>,
    metadata_canary: Option<String>,
    project_instance_id: Option<String>,
    usage_calls: usize,
    usage_events: Vec<String>,
    active_usage_instances: usize,
    sealed_mcp_instances: usize,
    generation: u64,
    purpose_revision: u64,
    publication_state: String,
}

#[test]
fn runtime_info_does_not_create_projectatlas_directory() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "runtime-info"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "runtime-info command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let runtime_json: Value = serde_json::from_slice(&output.stdout)?;
    require_json_string(&runtime_json, &["project"], "ProjectAtlas")?;
    require_json_usize(&runtime_json, &["major_version"], 3)?;
    if runtime_json["executable"].as_str().is_none() {
        return Err(io::Error::other("runtime-info executable path missing").into());
    }
    if runtime_json.get("mcp_nearest_project").is_some() {
        return Err(io::Error::other("CLI runtime-info leaked MCP startup policy").into());
    }
    if atlas_dir.exists() {
        return Err(io::Error::other("runtime-info created .projectatlas").into());
    }
    let required_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--require-version",
            required_version.as_str(),
            "runtime-info",
        ])
        .assert()
        .success();
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--require-version", "0.0.0", "runtime-info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "does not satisfy required version",
        ));
    Ok(())
}

#[cfg(feature = "derived-snapshot")]
#[test]
fn derived_snapshot_cli_round_trips_without_replacing_authored_state() -> Result<(), Box<dyn Error>>
{
    const FIXTURE_SOURCE_PATH: &str = "src/lib.rs";
    const PROJECT_DB_PATH: &str = ".projectatlas/projectatlas.db";

    let temp = tempfile::tempdir()?;
    let source = temp.path().join("snapshot-source");
    let destination = temp.path().join("snapshot-destination");
    for root in [&source, &destination] {
        fs::create_dir_all(root.join(SRC_DIR_NAME))?;
        fs::write(
            root.join(FIXTURE_SOURCE_PATH),
            "pub fn answer() -> u32 { 42 }\n",
        )?;
    }
    let source_db = source.join(PROJECT_DB_PATH);
    let destination_db = destination.join(PROJECT_DB_PATH);
    for (root, db) in [(&source, &source_db), (&destination, &destination_db)] {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(root)
            .args(["--format", "json", "--db"])
            .arg(db)
            .arg("scan")
            .arg(root)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "snapshot fixture scan failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    let source_secret = "TOP_SECRET_CLI_SNAPSHOT_SOURCE";
    Command::cargo_bin("projectatlas")?
        .current_dir(&source)
        .arg("--db")
        .arg(&source_db)
        .args(["purpose", "set", FIXTURE_SOURCE_PATH, source_secret])
        .assert()
        .success();
    let destination_purpose = "Destination-authored purpose survives import";
    Command::cargo_bin("projectatlas")?
        .current_dir(&destination)
        .arg("--db")
        .arg(&destination_db)
        .args(["purpose", "set", FIXTURE_SOURCE_PATH, destination_purpose])
        .assert()
        .success();

    let destination_identity_before =
        Connection::open_with_flags(&destination_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
    let archive = temp.path().join("derived-graph.tar.zst");
    let export = Command::cargo_bin("projectatlas")?
        .current_dir(&source)
        .args(["--format", "json", "--db"])
        .arg(&source_db)
        .args(["snapshot", "export"])
        .arg(&archive)
        .output()?;
    if !export.status.success() {
        return Err(io::Error::other(format!(
            "snapshot export failed: {}",
            String::from_utf8_lossy(&export.stderr)
        ))
        .into());
    }
    let export_json: Value = serde_json::from_slice(&export.stdout)?;
    let digest = export_json["snapshot_digest"]
        .as_str()
        .ok_or_else(|| io::Error::other("snapshot export digest is missing"))?;

    let archive_file = fs::File::open(&archive)?;
    let decoder = zstd::stream::read::Decoder::new(archive_file)?;
    let mut tar = tar::Archive::new(decoder);
    let mut payload = None;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("projectatlas-derived-snapshot/graph.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            payload = Some(bytes);
        }
    }
    let payload = String::from_utf8(
        payload.ok_or_else(|| io::Error::other("snapshot archive payload is missing"))?,
    )?;
    if payload.contains(source_secret) {
        return Err(io::Error::other("snapshot payload leaked an authored purpose").into());
    }

    let import = Command::cargo_bin("projectatlas")?
        .current_dir(&destination)
        .args(["--format", "json", "--db"])
        .arg(&destination_db)
        .args(["snapshot", "import"])
        .arg(&archive)
        .args(["--require-digest", digest])
        .output()?;
    if !import.status.success() {
        return Err(io::Error::other(format!(
            "snapshot import failed: {}",
            String::from_utf8_lossy(&import.stderr)
        ))
        .into());
    }
    let connection =
        Connection::open_with_flags(&destination_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let destination_identity_after = connection.query_row(
        "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
        [],
        |row| row.get::<_, Vec<u8>>(0),
    )?;
    if destination_identity_before != destination_identity_after {
        return Err(io::Error::other("snapshot import replaced destination identity").into());
    }
    let retained_purpose = connection.query_row(
        "SELECT purpose.purpose
           FROM purposes AS purpose
           JOIN nodes AS node ON node.id = purpose.node_id
          WHERE node.path = 'src/lib.rs'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    if retained_purpose != destination_purpose {
        return Err(io::Error::other("snapshot import replaced destination purpose").into());
    }
    let generation = connection.query_row(
        "SELECT value FROM metadata WHERE key = 'index_publication_generation'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    if generation != "2" {
        return Err(io::Error::other(format!(
            "snapshot import published generation {generation}, expected 2"
        ))
        .into());
    }
    Ok(())
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
            "50",
            "--analysis-mode",
            "impact",
            "--vcs",
            "working-tree",
        ])
        .output()?;
    if !impact.status.success() {
        return Err(io::Error::other(format!(
            "public impact analysis CLI failed: {}",
            String::from_utf8_lossy(&impact.stderr)
        ))
        .into());
    }
    let impact_payload: Value = serde_json::from_slice(&impact.stdout)?;
    require_json_string(&impact_payload, &["symbol_relations", "mode"], "impact")?;
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

#[cfg(feature = "optional-parser-supervisor")]
#[test]
fn parser_pack_disable_does_not_require_default_user_storage() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let selection = repo.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
    fs::create_dir_all(
        selection
            .parent()
            .ok_or_else(|| io::Error::other("parser-pack selection has no parent"))?,
    )?;
    fs::write(&selection, b"stale-selection")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env_remove("HOME")
        .env_remove("LOCALAPPDATA")
        .env_remove("XDG_DATA_HOME")
        .args(["--format", "json", "parser-pack", "disable"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"operation\": \"disable\""));

    if selection.exists() {
        return Err(io::Error::other(
            "parser-pack disable retained project selection without user storage",
        )
        .into());
    }
    Ok(())
}

#[cfg(all(target_os = "macos", feature = "optional-parser-supervisor"))]
#[test]
fn parser_pack_supported_only_commands_refuse_unsupported_macos_before_state_access()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let home = temp.path().join(PARSER_PACK_TEST_HOME_DIR);
    let missing_archive = temp.path().join("missing-parser-pack.tar.zst");
    let selection = repo.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
    let storage = home
        .join("Library")
        .join("Application Support")
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join(OPTIONAL_PARSER_PACKS_DIR_NAME);
    fs::create_dir(&repo)?;
    fs::create_dir(&home)?;

    let commands = [
        (
            "verify",
            vec![
                OsString::from("parser-pack"),
                OsString::from("verify"),
                OsString::from("--archive"),
                missing_archive.as_os_str().to_owned(),
            ],
        ),
        (
            "install",
            vec![
                OsString::from("parser-pack"),
                OsString::from("install"),
                OsString::from("--archive"),
                missing_archive.as_os_str().to_owned(),
            ],
        ),
        (
            "enable",
            vec![
                OsString::from("parser-pack"),
                OsString::from("enable"),
                OsString::from("--artifact"),
                OsString::from("a".repeat(64)),
            ],
        ),
        (
            "update",
            vec![
                OsString::from("parser-pack"),
                OsString::from("update"),
                OsString::from("--archive"),
                missing_archive.as_os_str().to_owned(),
            ],
        ),
    ];
    for (operation, arguments) in commands {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env("HOME", &home)
            .env_remove("LOCALAPPDATA")
            .env_remove("XDG_DATA_HOME")
            .args(["--format", "json"])
            .args(arguments)
            .output()?;
        if output.status.success() {
            return Err(io::Error::other(format!(
                "unsupported macOS parser-pack {operation} unexpectedly succeeded"
            ))
            .into());
        }
        let error: Value = serde_json::from_slice(&output.stderr)?;
        require_json_string(&error, &["error", "kind"], "unsupported_containment")?;
        if missing_archive.exists() || selection.exists() || storage.exists() {
            return Err(io::Error::other(format!(
                "unsupported macOS parser-pack {operation} touched archive or lifecycle state"
            ))
            .into());
        }
    }

    let source = repo.join(SRC_DIR_NAME).join("main.rs");
    let selection_bytes = b"malformed selection must not be inspected";
    let source_bytes = b"pub fn untouched() {}\n";
    fs::create_dir_all(
        selection
            .parent()
            .ok_or_else(|| io::Error::other("parser-pack selection has no parent"))?,
    )?;
    fs::create_dir_all(
        source
            .parent()
            .ok_or_else(|| io::Error::other("macOS source path has no parent"))?,
    )?;
    fs::write(&selection, selection_bytes)?;
    fs::write(&source, source_bytes)?;
    let scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("HOME", &home)
        .env_remove("LOCALAPPDATA")
        .env_remove("XDG_DATA_HOME")
        .args(["--format", "json", "scan"])
        .output()?;
    if scan.status.success() {
        return Err(io::Error::other(
            "unsupported macOS normal scan unexpectedly accepted parser-pack state",
        )
        .into());
    }
    let scan_error: Value = serde_json::from_slice(&scan.stderr)?;
    require_json_string(&scan_error, &["error", "kind"], "unsupported_containment")?;
    if fs::read(&selection)? != selection_bytes
        || fs::read(&source)? != source_bytes
        || storage.exists()
    {
        return Err(io::Error::other(
            "unsupported macOS normal scan touched parser selection, storage, or source",
        )
        .into());
    }

    for expected_changed in [true, false] {
        let remove = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env("HOME", &home)
            .env_remove("LOCALAPPDATA")
            .env_remove("XDG_DATA_HOME")
            .args(["--format", "json", "parser-pack", "remove"])
            .output()?;
        if !remove.status.success() {
            return Err(io::Error::other(format!(
                "unsupported macOS parser-pack remove failed: {}",
                String::from_utf8_lossy(&remove.stderr)
            ))
            .into());
        }
        let report: Value = serde_json::from_slice(&remove.stdout)?;
        require_json_string(&report, &["operation"], "remove")?;
        require_json_string(&report, &["state"], "unsupported_containment")?;
        require_json_bool(&report, &["changed"], expected_changed)?;
        if selection.exists() || storage.exists() || fs::read(&source)? != source_bytes {
            return Err(io::Error::other(
                "unsupported macOS parser-pack remove touched storage or source",
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(feature = "optional-parser-supervisor")]
#[test]
#[ignore = "requires one exact workflow-built optional parser-pack archive"]
fn optional_parser_pack_real_archive_normal_runtime_lifecycle() -> Result<(), Box<dyn Error>> {
    let archive = std::env::var_os(OPTIONAL_PARSER_ARCHIVE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("real optional parser archive environment is absent"))?;
    let archive = archive.canonicalize()?;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let source_dir = repo.join(SRC_DIR_NAME);
    let optional_source = source_dir.join("main.awk");
    let host_state = temp.path().join("host-state");
    let local_app_data = host_state.join(PARSER_PACK_TEST_LOCAL_APP_DATA_DIR);
    let xdg_data = host_state.join(PARSER_PACK_TEST_XDG_DATA_DIR);
    let isolated_home = host_state.join(PARSER_PACK_TEST_HOME_DIR);
    #[cfg(windows)]
    let storage = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join(OPTIONAL_PARSER_PACKS_DIR_NAME);
    #[cfg(not(windows))]
    let storage = xdg_data
        .join(PROJECTATLAS_XDG_DATA_DIR)
        .join(OPTIONAL_PARSER_PACKS_DIR_NAME);
    let logical_pack_root = storage.join(OPTIONAL_PARSER_PACK_ID);
    let db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let selection = repo.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
    fs::create_dir_all(&source_dir)?;
    fs::create_dir_all(&local_app_data)?;
    fs::create_dir_all(&xdg_data)?;
    fs::create_dir_all(&isolated_home)?;
    fs::write(source_dir.join("lib.rs"), "pub fn built_in() {}\n")?;
    fs::write(&optional_source, "# Hello\n{}\n")?;

    projectatlas_json(&repo, &host_state, &[OsStr::new("init")])?;
    if storage.exists() {
        return Err(io::Error::other("default-core init touched optional pack storage").into());
    }
    let inactive = AtlasStore::open_read_only(&db)?
        .load_node_by_path("src/main.awk")?
        .ok_or_else(|| io::Error::other("inactive optional source node missing"))?;
    if inactive.node.language.is_some()
        || AtlasStore::open_read_only(&db)?
            .load_source_parse_metadata("src/main.awk")?
            .is_some()
    {
        return Err(
            io::Error::other("default-core scan admitted optional catalog source work").into(),
        );
    }

    let verified = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("verify"),
            OsStr::new("--archive"),
            archive.as_os_str(),
        ],
    )?;
    let artifact = json_string_at(&verified, &["artifact", "artifact"])?.to_owned();
    require_json_string(&verified, &["operation"], "verify")?;
    if logical_pack_root.exists() || selection.exists() {
        return Err(io::Error::other("parser-pack verification mutated installed state").into());
    }
    #[cfg(windows)]
    {
        let entries = fs::read_dir(&storage)?.collect::<Result<Vec<_>, _>>()?;
        if entries.len() != 1 || !entries[0].file_type()?.is_file() {
            return Err(io::Error::other(
                "parser-pack verification retained state beyond its stable coordination lease",
            )
            .into());
        }
    }
    #[cfg(not(windows))]
    if storage.exists() {
        return Err(
            io::Error::other("parser-pack verification created unsupported host state").into(),
        );
    }

    let installed = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("install"),
            OsStr::new("--archive"),
            archive.as_os_str(),
        ],
    )?;
    require_json_string(&installed, &["operation"], "install")?;
    if !logical_pack_root.is_dir() || selection.exists() {
        return Err(io::Error::other(
            "parser-pack installation did not remain installed but disabled",
        )
        .into());
    }
    let enabled = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("enable"),
            OsStr::new("--artifact"),
            OsStr::new(&artifact),
        ],
    )?;
    require_json_string(&enabled, &["operation"], "enable")?;
    require_json_string(&enabled, &["selected", "artifact"], &artifact)?;
    if !selection.is_file() {
        return Err(io::Error::other("parser-pack enable did not persist selection").into());
    }

    #[cfg(debug_assertions)]
    {
        const CURRENTNESS_DELAY: Duration = Duration::from_secs(2);
        const PRE_SPAWN_DELAY: Duration = Duration::from_secs(14);
        const PRE_READY_NO_PROGRESS: Duration = Duration::from_secs(15);

        let lifecycle = OptionalParserPackLifecycle::new(&repo, Some(storage))?;
        let mut runtime_selection = lifecycle
            .resolve_selected_pack()?
            .ok_or_else(|| io::Error::other("enabled parser pack did not resolve"))?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};

            let artifact_manifest = logical_pack_root
                .join(VERSIONS_DIR_NAME)
                .join(OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION)
                .join(&artifact)
                .join("artifact-manifest.json");
            let before = fs::metadata(&artifact_manifest)?;
            let original = before.permissions();
            let epoch = |metadata: &fs::Metadata| {
                (
                    metadata.len(),
                    metadata.modified().ok(),
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            };
            let before_epoch = epoch(&before);
            let drift_deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let mut changed = original.clone();
                changed.set_mode(original.mode() ^ 0o200);
                fs::set_permissions(&artifact_manifest, changed)?;
                fs::set_permissions(&artifact_manifest, original.clone())?;
                if epoch(&fs::metadata(&artifact_manifest)?) != before_epoch {
                    break;
                }
                if Instant::now() >= drift_deadline {
                    return Err(io::Error::other(
                        "parser-pack manifest did not enter a new Unix change epoch",
                    )
                    .into());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        let currentness_seen = Arc::new(AtomicBool::new(false));
        let currentness_hook_seen = Arc::clone(&currentness_seen);
        install_currentness_test_hook(move || {
            currentness_hook_seen.store(true, Ordering::Release);
            thread::sleep(CURRENTNESS_DELAY);
        })?;
        let pre_spawn_seen = Arc::new(AtomicBool::new(false));
        let pre_spawn_hook_seen = Arc::clone(&pre_spawn_seen);
        install_pre_spawn_test_hook(move || {
            pre_spawn_hook_seen.store(true, Ordering::Release);
            thread::sleep(PRE_SPAWN_DELAY);
        })?;

        let parser_source = fs::read(&optional_source)?;
        let request_limits = ParserRequestLimits::new(PARSER_MAX_OUTPUT_BYTES, 100_000, 512)?;
        let cumulative_result = runtime_selection.supervisor_mut().parse(
            "awk",
            &parser_source,
            request_limits,
            Instant::now() + Duration::from_secs(60),
            PRE_READY_NO_PROGRESS,
            &IndexCancellation::new(),
        );
        if !currentness_seen.load(Ordering::Acquire) || !pre_spawn_seen.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "real parser-pack cumulative epoch did not traverse both bounded phases",
            )
            .into());
        }
        match cumulative_result {
            Err(ParserSupervisorError::NoProgress {
                phase: "process launch",
            }) => {}
            other => {
                return Err(io::Error::other(format!(
                    "real parser-pack cumulative epoch returned the wrong result: {other:?}"
                ))
                .into());
            }
        }
        runtime_selection.supervisor_mut().shutdown()?;
        runtime_selection.supervisor_mut().parse(
            "awk",
            &parser_source,
            request_limits,
            Instant::now() + Duration::from_secs(30),
            Duration::from_secs(10),
            &IndexCancellation::new(),
        )?;
        runtime_selection.supervisor_mut().shutdown()?;
        drop(runtime_selection);
    }

    projectatlas_json(&repo, &host_state, &[OsStr::new("scan")])?;
    let store = AtlasStore::open_read_only(&db)?;
    let selected_node = store
        .load_node_by_path("src/main.awk")?
        .ok_or_else(|| io::Error::other("selected optional source node missing"))?;
    if selected_node.node.language.as_deref() != Some("awk") {
        return Err(io::Error::other("selected optional source was not admitted as AWK").into());
    }
    let source_metadata = store
        .load_source_parse_metadata("src/main.awk")?
        .ok_or_else(|| io::Error::other("optional source parse metadata missing"))?;
    if source_metadata.parser != ParserKind::TreeSitter {
        return Err(
            io::Error::other("normal scan did not retain Tree-sitter source provenance").into(),
        );
    }
    let graphs = store.load_symbol_graphs_for_paths(&["src/main.awk".to_string()])?;
    if graphs.len() != 1 || graphs[0].parser != ParserKind::Fallback {
        return Err(io::Error::other(
            "normal scan did not retain independent fallback fact provenance",
        )
        .into());
    }
    let first_generation = store
        .index_publication()?
        .ok_or_else(|| io::Error::other("selected scan publication missing"))?
        .generation;
    drop(store);

    let updated_source = b"BEGIN { print \"atlas\" }\n";
    fs::write(&optional_source, updated_source)?;
    projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("watch"), OsStr::new("--once")],
    )?;
    let refreshed = AtlasStore::open_read_only(&db)?;
    let second_generation = refreshed
        .index_publication()?
        .ok_or_else(|| io::Error::other("watch publication missing"))?
        .generation;
    if second_generation <= first_generation
        || refreshed
            .load_source_parse_metadata("src/main.awk")?
            .is_none_or(|metadata| metadata.parser != ParserKind::TreeSitter)
    {
        return Err(io::Error::other(
            "optional source watcher refresh did not publish new Tree-sitter provenance",
        )
        .into());
    }
    let refreshed_node = refreshed
        .load_node_by_path("src/main.awk")?
        .ok_or_else(|| io::Error::other("refreshed optional source node missing"))?;
    if refreshed_node.node.content_hash.as_deref()
        != Some(blake3::hash(updated_source).to_hex().as_str())
    {
        return Err(io::Error::other(
            "optional source watcher refresh did not index the updated source bytes",
        )
        .into());
    }
    let refreshed_graphs = refreshed.load_symbol_graphs_for_paths(&["src/main.awk".to_string()])?;
    if refreshed_graphs.len() != 1 || refreshed_graphs[0].parser != ParserKind::Fallback {
        return Err(io::Error::other(
            "optional source watcher refresh did not retain independent fallback fact provenance",
        )
        .into());
    }
    drop(refreshed);

    let selection_before_failed_update = fs::read(&selection)?;
    let mut corrupt_bytes = fs::read(&archive)?;
    if corrupt_bytes.is_empty() {
        return Err(io::Error::other("real optional parser archive is empty").into());
    }
    corrupt_bytes.truncate(corrupt_bytes.len() / 2);
    let corrupt_archive = temp.path().join("corrupt-parser-pack.tar.zst");
    fs::write(&corrupt_archive, corrupt_bytes)?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("HOME", &isolated_home)
        .env("LOCALAPPDATA", &local_app_data)
        .env("XDG_DATA_HOME", &xdg_data)
        .arg("--format")
        .arg("json")
        .arg("parser-pack")
        .arg("update")
        .arg("--archive")
        .arg(&corrupt_archive)
        .assert()
        .failure();
    if fs::read(&selection)? != selection_before_failed_update
        || AtlasStore::open_read_only(&db)?
            .index_publication()?
            .is_none_or(|publication| publication.generation != second_generation)
    {
        return Err(io::Error::other(
            "failed real-archive update changed selection or active generation",
        )
        .into());
    }

    let replacement_dir = temp.path().join("replacement");
    fs::create_dir(&replacement_dir)?;
    let replacement_archive = replacement_dir.join(
        archive
            .file_name()
            .ok_or_else(|| io::Error::other("real optional parser archive has no file name"))?,
    );
    let replacement_artifact =
        derive_whitespace_distinct_parser_archive(&archive, &replacement_archive)?;
    if replacement_artifact == artifact {
        return Err(io::Error::other("replacement parser artifact identity did not change").into());
    }
    let updated = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("update"),
            OsStr::new("--archive"),
            replacement_archive.as_os_str(),
        ],
    )?;
    require_json_bool(&updated, &["changed"], true)?;
    require_json_string(&updated, &["state"], "rollback_ready")?;
    require_json_string(&updated, &["selected", "artifact"], &replacement_artifact)?;
    require_json_string(&updated, &["rollback", "artifact"], &artifact)?;
    let release_version = json_string_at(&updated, &["selected", "projectatlas_version"])?;
    let versions_root = logical_pack_root
        .join(VERSIONS_DIR_NAME)
        .join(release_version);
    if !versions_root.join(&artifact).is_dir()
        || !versions_root.join(&replacement_artifact).is_dir()
    {
        return Err(io::Error::other(
            "successful parser-pack update did not retain both exact immutable slots",
        )
        .into());
    }

    let replacement_source = b"BEGIN { print \"replacement\" }\n";
    fs::write(&optional_source, replacement_source)?;
    projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("watch"), OsStr::new("--once")],
    )?;
    let replacement_store = AtlasStore::open_read_only(&db)?;
    let replacement_generation = replacement_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("replacement watch publication missing"))?
        .generation;
    let replacement_node = replacement_store
        .load_node_by_path("src/main.awk")?
        .ok_or_else(|| io::Error::other("replacement optional source node missing"))?;
    if replacement_generation <= second_generation
        || replacement_node.node.content_hash.as_deref()
            != Some(blake3::hash(replacement_source).to_hex().as_str())
        || replacement_store
            .load_source_parse_metadata("src/main.awk")?
            .is_none_or(|metadata| metadata.parser != ParserKind::TreeSitter)
    {
        return Err(io::Error::other(
            "replacement parser artifact did not publish the exact updated source with Tree-sitter provenance",
        )
        .into());
    }
    let replacement_graphs =
        replacement_store.load_symbol_graphs_for_paths(&["src/main.awk".to_string()])?;
    if replacement_graphs.len() != 1 || replacement_graphs[0].parser != ParserKind::Fallback {
        return Err(io::Error::other(
            "replacement parser artifact did not publish independent fallback fact provenance",
        )
        .into());
    }
    drop(replacement_store);

    let idempotent_update = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("update"),
            OsStr::new("--archive"),
            replacement_archive.as_os_str(),
        ],
    )?;
    require_json_bool(&idempotent_update, &["changed"], false)?;

    let rolled_back = projectatlas_json(
        &repo,
        &host_state,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("enable"),
            OsStr::new("--artifact"),
            OsStr::new(&artifact),
        ],
    )?;
    require_json_string(&rolled_back, &["operation"], "enable")?;
    require_json_string(&rolled_back, &["state"], "rollback_ready")?;
    require_json_string(&rolled_back, &["selected", "artifact"], &artifact)?;
    require_json_string(
        &rolled_back,
        &["rollback", "artifact"],
        &replacement_artifact,
    )?;
    let rollback_source = b"BEGIN { print \"rollback\" }\n";
    fs::write(&optional_source, rollback_source)?;
    projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("watch"), OsStr::new("--once")],
    )?;
    let rollback_store = AtlasStore::open_read_only(&db)?;
    let rollback_generation = rollback_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("rollback watch publication missing"))?
        .generation;
    let rollback_node = rollback_store
        .load_node_by_path("src/main.awk")?
        .ok_or_else(|| io::Error::other("rollback optional source node missing"))?;
    if rollback_generation <= replacement_generation
        || rollback_node.node.language.as_deref() != Some("awk")
        || rollback_node.node.content_hash.as_deref()
            != Some(blake3::hash(rollback_source).to_hex().as_str())
        || rollback_store
            .load_source_parse_metadata("src/main.awk")?
            .is_none_or(|metadata| metadata.parser != ParserKind::TreeSitter)
    {
        return Err(io::Error::other(
            "explicit parser-pack rollback did not publish the updated source with Tree-sitter provenance",
        )
        .into());
    }
    let rollback_graphs =
        rollback_store.load_symbol_graphs_for_paths(&["src/main.awk".to_string()])?;
    if rollback_graphs.len() != 1 || rollback_graphs[0].parser != ParserKind::Fallback {
        return Err(io::Error::other(
            "explicit parser-pack rollback did not retain independent fallback fact provenance",
        )
        .into());
    }
    drop(rollback_store);

    let disabled = projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("parser-pack"), OsStr::new("disable")],
    )?;
    require_json_string(&disabled, &["operation"], "disable")?;
    if selection.exists() {
        return Err(io::Error::other("parser-pack disable retained project selection").into());
    }
    projectatlas_json(&repo, &host_state, &[OsStr::new("scan")])?;
    let disabled_store = AtlasStore::open_read_only(&db)?;
    if disabled_store
        .load_node_by_path("src/main.awk")?
        .is_none_or(|node| node.node.language.is_some())
        || disabled_store
            .load_source_parse_metadata("src/main.awk")?
            .is_some()
    {
        return Err(io::Error::other(
            "disabled optional pack retained catalog language or parse metadata",
        )
        .into());
    }
    drop(disabled_store);

    let removed = projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("parser-pack"), OsStr::new("remove")],
    )?;
    require_json_string(&removed, &["operation"], "remove")?;
    if logical_pack_root.exists() {
        return Err(io::Error::other("parser-pack removal retained the logical pack root").into());
    }
    let removed_again = projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("parser-pack"), OsStr::new("remove")],
    )?;
    require_json_bool(&removed_again, &["changed"], false)?;
    if selection.exists() {
        return Err(io::Error::other("parser-pack removal retained project selection").into());
    }
    Ok(())
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
fn settings_reports_content_free_telemetry_without_recording() -> Result<(), Box<dyn Error>> {
    const SESSION_SENTINEL: &str = "private-session-sentinel";
    const QUERY_SENTINEL: &str = "private-query-sentinel";
    const SOURCE_SENTINEL: &str = "private_source_sentinel";
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        format!("pub fn {SOURCE_SENTINEL}() {{}}\n"),
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env_remove("PROJECTATLAS_NO_TELEMETRY")
        .args([
            "--session",
            SESSION_SENTINEL,
            "files",
            QUERY_SENTINEL,
            "--folder",
            SRC_DIR_NAME,
        ])
        .assert()
        .success();

    let db_path = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let connection = Connection::open(&db_path)?;
    let instances_before: i64 =
        connection.query_row("SELECT COUNT(*) FROM usage_instances", [], |row| row.get(0))?;
    drop(connection);

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env_remove("PROJECTATLAS_NO_TELEMETRY")
        .args(["--format", "json", "settings"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "settings failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let settings: Value = serde_json::from_slice(&output.stdout)?;
    let language_registry = settings
        .get("language_registry")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted language registry state"))?;
    for field in [
        "registry_version",
        "accepted_set_version",
        "detection_policy_version",
    ] {
        if language_registry
            .get(field)
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(io::Error::other(format!(
                "settings language registry omitted numeric field {field:?}"
            ))
            .into());
        }
    }
    for field in [
        "registry_digest",
        "accepted_set_digest",
        "semantic_provider_digest",
    ] {
        if language_registry
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(io::Error::other(format!(
                "settings language registry omitted digest field {field:?}"
            ))
            .into());
        }
    }
    let capability_counts = language_registry
        .get("counts")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted language capability counts"))?;
    let accepted = capability_counts
        .get("accepted")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other("accepted language count was not numeric"))?;
    let built_in = capability_counts
        .get("built_in")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other("built-in language count was not numeric"))?;
    let optional_candidates = capability_counts
        .get("optional_candidates")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other("optional language count was not numeric"))?;
    if built_in + optional_candidates != accepted {
        return Err(io::Error::other(
            "built-in plus optional language rows did not reconcile with accepted rows",
        )
        .into());
    }
    if language_registry.get("capabilities").is_some() {
        return Err(io::Error::other(
            "default settings embedded the complete language capability matrix",
        )
        .into());
    }
    if output.stdout.len() > 64_000 {
        return Err(io::Error::other(format!(
            "default JSON settings response was not compact: {} bytes",
            output.stdout.len()
        ))
        .into());
    }
    let optional_catalog = language_registry
        .get("optional_catalog")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted optional catalog provenance"))?;
    for field in ["name", "version", "revision", "metadata_license"] {
        if optional_catalog
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(
                io::Error::other(format!("settings optional catalog omitted {field:?}")).into(),
            );
        }
    }
    for axis in ["detected", "parsed", "symbols", "semantic", "benchmarked"] {
        let levels = capability_counts
            .get(axis)
            .and_then(Value::as_object)
            .ok_or_else(|| io::Error::other(format!("missing language axis {axis:?}")))?;
        let total = ["unavailable", "fallback", "supported"]
            .into_iter()
            .map(|level| {
                levels
                    .get(level)
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
            })
            .sum::<u64>();
        if total != accepted {
            return Err(io::Error::other(format!(
                "language axis {axis:?} total {total} did not match accepted rows {accepted}"
            ))
            .into());
        }
    }
    if settings
        .get("semantic_relation_contract_digest")
        .and_then(Value::as_str)
        .is_none_or(|digest| digest.len() != 64)
    {
        return Err(io::Error::other(
            "settings omitted the bounded semantic relation contract digest",
        )
        .into());
    }
    let relation_inventory = settings
        .get("relation_family_inventory")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted accepted relation-family inventory"))?;
    if relation_inventory.get("version").and_then(Value::as_u64) != Some(1)
        || relation_inventory
            .get("digest")
            .and_then(Value::as_str)
            .is_none_or(|digest| digest.len() != 64)
    {
        return Err(io::Error::other("settings omitted accepted relation-family identity").into());
    }
    let active_rows = RELATION_FAMILY_CAPABILITIES
        .iter()
        .filter(|row| row.state == RelationFamilyState::Active)
        .count() as u64;
    let disabled_rows = RELATION_FAMILY_CAPABILITIES
        .iter()
        .filter(|row| row.state == RelationFamilyState::OptionalDisabled)
        .count() as u64;
    if relation_inventory
        .get("active_families")
        .and_then(Value::as_u64)
        != Some(active_rows)
        || relation_inventory
            .get("optional_disabled_families")
            .and_then(Value::as_u64)
            != Some(disabled_rows)
    {
        return Err(io::Error::other(
            "settings relation-family counts did not derive from their rows",
        )
        .into());
    }
    let search = settings
        .get("search")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted typed search readiness"))?;
    if search.get("default_mode").and_then(Value::as_str) != Some("lexical")
        || search
            .get("lexical")
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            != Some("ready")
        || search
            .get("lexical")
            .and_then(|value| value.get("source"))
            .and_then(Value::as_str)
            != Some("persisted_text")
        || search
            .get("lexical")
            .and_then(|value| value.get("exact_verification"))
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(io::Error::other("settings misstated lexical search readiness").into());
    }
    if search
        .get("fts")
        .and_then(|value| value.get("state"))
        .and_then(Value::as_str)
        != Some("ready")
    {
        return Err(io::Error::other("settings omitted ready FTS acceleration").into());
    }
    for mode in ["semantic", "hybrid"] {
        if search
            .get(mode)
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            != Some("unavailable")
        {
            return Err(io::Error::other(format!(
                "settings overstated unavailable search mode {mode:?}"
            ))
            .into());
        }
    }
    let optional_parser_pack = settings
        .get("optional_parser_pack")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted optional parser lifecycle"))?;
    if optional_parser_pack
        .get("compiled")
        .and_then(Value::as_bool)
        != Some(true)
        || optional_parser_pack
            .get("lifecycle")
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(
            io::Error::other("settings omitted compiled optional parser lifecycle truth").into(),
        );
    }
    let database = settings
        .get("database")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted database diagnostics"))?;
    let schema = database
        .get("schema")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted schema compatibility"))?;
    if schema.get("compatibility").and_then(Value::as_str) != Some("current")
        || schema.get("runtime_version").and_then(Value::as_i64)
            != schema.get("stored_version").and_then(Value::as_i64)
        || schema.get("migration_required").and_then(Value::as_bool) != Some(false)
        || schema.get("migration_supported").and_then(Value::as_bool) != Some(true)
        || schema
            .get("migration_steps_remaining")
            .and_then(Value::as_u64)
            != Some(0)
    {
        return Err(io::Error::other("settings misstated current schema/migration state").into());
    }
    let sqlite = database
        .get("sqlite")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted linked SQLite identity"))?;
    let compile_options = sqlite
        .get("compile_options")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted SQLite compile-option identity"))?;
    if sqlite
        .get("version")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || sqlite
            .get("version_number")
            .and_then(Value::as_i64)
            .is_none()
        || compile_options
            .get("count")
            .and_then(Value::as_u64)
            .is_none_or(|count| count == 0)
        || compile_options
            .get("digest")
            .and_then(Value::as_str)
            .is_none_or(|digest| digest.len() != 64)
        || compile_options.len() != 2
    {
        return Err(io::Error::other("settings emitted an invalid SQLite identity").into());
    }
    if database.get("filesystem").and_then(Value::as_str) != Some("supported_local") {
        return Err(
            io::Error::other("settings did not report the validated local filesystem").into(),
        );
    }
    let operating_profile = database
        .get("operating_profile")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted the SQLite operating profile"))?;
    for field in [
        "required_journal_mode",
        "observed_journal_mode",
        "required_synchronous_mode",
        "observed_synchronous_mode",
    ] {
        if operating_profile
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(
                io::Error::other(format!("settings operating profile omitted {field:?}")).into(),
            );
        }
    }
    let publication = database
        .get("publication")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted active publication state"))?;
    if publication.get("state").and_then(Value::as_str) != Some("complete")
        || publication.get("generation").is_none()
        || publication
            .get("contract_fingerprint_state")
            .and_then(Value::as_str)
            != Some("valid")
        || publication
            .get("contract_fingerprint")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(io::Error::other("settings misstated active publication state").into());
    }
    let coverage = database
        .get("coverage")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("settings omitted bounded coverage state"))?;
    if coverage.get("returned").and_then(Value::as_u64).is_none()
        || coverage.get("inspected").and_then(Value::as_u64).is_none()
        || coverage.get("truncated").and_then(Value::as_bool).is_none()
        || coverage
            .get("total_state")
            .and_then(Value::as_str)
            .is_none()
        || coverage.get("next_call").and_then(Value::as_str) != Some("atlas_file_summary")
    {
        return Err(io::Error::other("settings omitted bounded actionable coverage truth").into());
    }
    let diagnostics_text = serde_json::to_string(&serde_json::json!({
        "database": database,
        "language_registry": language_registry,
        "semantic_relation_contract_digest": settings["semantic_relation_contract_digest"],
        "relation_family_inventory": relation_inventory,
        "search": search,
        "optional_parser_pack": optional_parser_pack,
    }))?;
    let repo_sentinel = repo.to_string_lossy().into_owned();
    for forbidden_value in [
        SESSION_SENTINEL,
        QUERY_SENTINEL,
        SOURCE_SENTINEL,
        repo_sentinel.as_str(),
    ] {
        if diagnostics_text.contains(forbidden_value) {
            return Err(io::Error::other(format!(
                "settings diagnostics exposed private value {forbidden_value:?}"
            ))
            .into());
        }
    }
    for forbidden_field in ["mount_point", "device", "probe_path", "environment"] {
        if diagnostics_text.contains(forbidden_field) {
            return Err(io::Error::other(format!(
                "settings diagnostics exposed forbidden field {forbidden_field:?}"
            ))
            .into());
        }
    }
    let telemetry = settings
        .get("telemetry")
        .ok_or_else(|| io::Error::other("settings omitted telemetry retention state"))?;
    let telemetry_object = telemetry
        .as_object()
        .ok_or_else(|| io::Error::other("settings telemetry state was not an object"))?;
    for field in [
        "policy_version",
        "logical_byte_version",
        "raw_rows",
        "max_raw_rows",
        "max_raw_age_seconds",
        "raw_logical_bytes",
        "max_raw_logical_bytes",
        "baseline_rows",
        "max_baselines_per_instance",
        "max_active_baseline_rows",
        "baseline_logical_bytes",
        "max_baseline_logical_bytes",
        "dimension_rows",
        "max_dimensions",
        "instance_rows",
        "active_instance_rows",
        "max_active_instances",
        "max_retained_instances",
        "retained_label_rows",
        "max_retained_labels",
        "daily_rows",
        "max_daily_rows",
        "retained_trend_days",
        "label_tombstone_rows",
        "max_label_tombstones",
        "instance_tombstone_rows",
        "max_instance_tombstones",
        "pruned_raw_rows",
        "pruned_instance_rows",
        "evicted_tombstones",
        "prune_batch_rows",
        "writes_since_checkpoint",
        "checkpoint_write_interval",
        "last_checkpoint_epoch",
        "wal_autocheckpoint_pages",
        "freelist_pages",
        "page_count",
        "page_size",
        "connection_busy_timeout_ms",
        "normal_busy_timeout_ms",
        "telemetry_busy_timeout_ms",
    ] {
        if telemetry_object
            .get(field)
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(io::Error::other(format!(
                "settings telemetry state omitted numeric lifecycle field {field:?}"
            ))
            .into());
        }
    }
    let checkpoint_state = telemetry_object
        .get("checkpoint_state")
        .ok_or_else(|| io::Error::other("settings omitted typed checkpoint state"))?;
    let checkpoint_states = [
        TelemetryCheckpointState::NotDue,
        TelemetryCheckpointState::Completed,
        TelemetryCheckpointState::Busy,
        TelemetryCheckpointState::Error,
    ]
    .map(serde_json::to_value)
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if !checkpoint_states.contains(checkpoint_state) {
        return Err(io::Error::other("settings emitted an unknown checkpoint state").into());
    }
    if telemetry_object.get("statistics_policy")
        != Some(&serde_json::to_value(
            PlannerStatisticsPolicy::NotConfigured,
        )?)
    {
        return Err(io::Error::other("settings overstated planner maintenance policy").into());
    }
    let statistics_state = telemetry_object
        .get("statistics_state")
        .ok_or_else(|| io::Error::other("settings omitted typed statistics state"))?;
    let statistics_states = [
        PlannerStatisticsState::NotInitialized,
        PlannerStatisticsState::Available,
    ]
    .map(serde_json::to_value)
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if !statistics_states.contains(statistics_state) {
        return Err(io::Error::other("settings emitted an unknown statistics state").into());
    }
    let connection_busy_timeout = telemetry_object["connection_busy_timeout_ms"]
        .as_u64()
        .ok_or_else(|| io::Error::other("connection busy timeout was not numeric"))?;
    let normal_busy_timeout = telemetry_object["normal_busy_timeout_ms"]
        .as_u64()
        .ok_or_else(|| io::Error::other("normal busy timeout was not numeric"))?;
    let telemetry_busy_timeout = telemetry_object["telemetry_busy_timeout_ms"]
        .as_u64()
        .ok_or_else(|| io::Error::other("telemetry busy timeout was not numeric"))?;
    if connection_busy_timeout != normal_busy_timeout
        || telemetry_busy_timeout == 0
        || telemetry_busy_timeout >= normal_busy_timeout
    {
        return Err(io::Error::other(
            "settings did not distinguish bounded telemetry and normal busy policies",
        )
        .into());
    }
    for field in ["maintenance_pending", "clock_anomaly"] {
        if telemetry_object
            .get(field)
            .and_then(Value::as_bool)
            .is_none()
        {
            return Err(io::Error::other(format!(
                "settings telemetry state omitted boolean lifecycle field {field:?}"
            ))
            .into());
        }
    }
    for field in [
        "checkpoint_state",
        "journal_mode",
        "synchronous_mode",
        "statistics_policy",
        "statistics_state",
    ] {
        if telemetry_object
            .get(field)
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(io::Error::other(format!(
                "settings telemetry state omitted storage truth field {field:?}"
            ))
            .into());
        }
    }
    if telemetry_object
        .get("spill_cleanup")
        .and_then(Value::as_str)
        != Some("not_applicable")
    {
        return Err(io::Error::other(
            "settings telemetry state did not report spill cleanup as not applicable",
        )
        .into());
    }
    if !telemetry_object.contains_key("oldest_retained_epoch") {
        return Err(
            io::Error::other("settings telemetry state omitted retained-detail age truth").into(),
        );
    }
    let telemetry_text = serde_json::to_string(telemetry)?;
    for forbidden in [
        "caller_label",
        "runtime_instance",
        "project_instance_id",
        "baseline_identity",
        "query",
        "path",
        "source_content",
    ] {
        if telemetry_text.contains(forbidden) {
            return Err(io::Error::other(format!(
                "settings telemetry state exposed forbidden field {forbidden:?}"
            ))
            .into());
        }
    }
    for forbidden_value in [
        SESSION_SENTINEL,
        QUERY_SENTINEL,
        SOURCE_SENTINEL,
        repo_sentinel.as_str(),
    ] {
        if telemetry_text.contains(forbidden_value) {
            return Err(io::Error::other(format!(
                "settings telemetry state exposed private value {forbidden_value:?}"
            ))
            .into());
        }
    }

    let connection = Connection::open(db_path)?;
    let instances_after: i64 =
        connection.query_row("SELECT COUNT(*) FROM usage_instances", [], |row| row.get(0))?;
    if instances_after != instances_before {
        return Err(io::Error::other("settings recorded telemetry for its own read").into());
    }
    Ok(())
}

#[test]
fn settings_rejects_untrusted_publication_with_retained_text() -> Result<(), Box<dyn Error>> {
    const PRIVATE_FINGERPRINT_SENTINEL: &str = "private-publication-fingerprint-sentinel";
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn retained_text() {}\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("init")
        .assert()
        .success();

    let db_path = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let connection = Connection::open(&db_path)?;
    connection.execute(
        "UPDATE metadata SET value = ?1 WHERE key = 'index_publication_fingerprint'",
        [PRIVATE_FINGERPRINT_SENTINEL],
    )?;
    drop(connection);

    let invalid_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "settings"])
        .output()?;
    if !invalid_output.status.success() {
        return Err(io::Error::other(format!(
            "settings failed for invalid publication metadata: {}",
            String::from_utf8_lossy(&invalid_output.stderr)
        ))
        .into());
    }
    let invalid: Value = serde_json::from_slice(&invalid_output.stdout)?;
    if invalid_output
        .stdout
        .windows(PRIVATE_FINGERPRINT_SENTINEL.len())
        .any(|window| window == PRIVATE_FINGERPRINT_SENTINEL.as_bytes())
        || invalid["database"]["publication"]["contract_fingerprint_state"] != "invalid"
        || !invalid["database"]["publication"]["contract_fingerprint"].is_null()
        || !invalid["database"]["coverage"].is_null()
        || invalid["search"]["lexical"]["state"] != "unavailable"
        || invalid["search"]["fts"]["state"] != "unavailable"
        || !invalid["index"].is_null()
        || !invalid["telemetry"].is_null()
    {
        return Err(
            io::Error::other("settings exposed or trusted invalid publication metadata").into(),
        );
    }

    let connection = Connection::open(&db_path)?;
    connection.execute(
        "DELETE FROM metadata
          WHERE key IN (
              'index_publication_state',
              'index_publication_fingerprint',
              'index_publication_generation'
          )",
        [],
    )?;
    let retained_text_rows: i64 =
        connection.query_row("SELECT COUNT(*) FROM file_texts", [], |row| row.get(0))?;
    drop(connection);
    if retained_text_rows == 0 {
        return Err(io::Error::other("fixture did not retain indexed text rows").into());
    }

    let missing_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "settings"])
        .output()?;
    if !missing_output.status.success() {
        return Err(io::Error::other(format!(
            "settings failed for missing publication metadata: {}",
            String::from_utf8_lossy(&missing_output.stderr)
        ))
        .into());
    }
    let missing: Value = serde_json::from_slice(&missing_output.stdout)?;
    if !missing["database"]["publication"].is_null()
        || missing["search"]["lexical"]["state"] != "unavailable"
        || missing["search"]["fts"]["state"] != "unavailable"
        || missing["index"]["indexed_text_files"].as_i64() != Some(retained_text_rows)
    {
        return Err(io::Error::other(
            "settings trusted retained text without an active publication",
        )
        .into());
    }
    Ok(())
}

#[test]
fn settings_reports_supported_predecessor_without_migration() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let db_path = atlas_dir.join("projectatlas.db");
    let connection = Connection::open(&db_path)?;
    connection.execute_batch(include_str!(
        "../../projectatlas-db/tests/fixtures/released-schema-8.sql"
    ))?;
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('schema_version', '8')",
        [],
    )?;
    let project_root = repo.to_string_lossy().into_owned();
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('project_root', ?1)",
        [project_root],
    )?;
    drop(connection);
    let bytes_before = fs::read(&db_path)?;

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "settings"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "settings failed for a supported predecessor: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let settings: Value = serde_json::from_slice(&output.stdout)?;
    let schema = settings
        .get("database")
        .and_then(|value| value.get("schema"))
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("predecessor settings omitted schema state"))?;
    if schema.get("stored_version").and_then(Value::as_i64) != Some(8)
        || schema.get("compatibility").and_then(Value::as_str) != Some("supported_predecessor")
        || schema.get("migration_required").and_then(Value::as_bool) != Some(true)
        || schema.get("migration_supported").and_then(Value::as_bool) != Some(true)
        || schema
            .get("migration_steps_remaining")
            .and_then(Value::as_u64)
            != Some(8)
        || !settings.get("index").is_some_and(Value::is_null)
        || !settings.get("telemetry").is_some_and(Value::is_null)
    {
        return Err(io::Error::other("settings misstated the supported predecessor").into());
    }
    if settings
        .get("search")
        .and_then(|value| value.get("lexical"))
        .and_then(|value| value.get("state"))
        .and_then(Value::as_str)
        != Some("unavailable")
    {
        return Err(io::Error::other("predecessor settings overstated lexical readiness").into());
    }
    if fs::read(&db_path)? != bytes_before {
        return Err(
            io::Error::other("settings migrated or mutated the predecessor database").into(),
        );
    }
    Ok(())
}

#[test]
fn mcp_clean_shutdown_seals_runtime_instances_across_restarts() -> Result<(), Box<dyn Error>> {
    const RESTART_COUNT: usize = 2;
    if std::env::var_os("PROJECTATLAS_NO_TELEMETRY").is_some() {
        return Ok(());
    }
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
    let config = mcp_config_for_harness(&repo, &db_path, "mcp-json")?;
    let (command, args) = mcp_command_and_args(&config)?;
    let initial_calls = AtlasStore::open_for_project(&db_path, &repo)?
        .token_overview(None)?
        .calls;
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-telemetry-restart-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#,
    ];
    for _ in 0..RESTART_COUNT {
        let stdout = run_mcp_stdio(&command, &repo, &args, &messages)?;
        if !mcp_tool_text(&stdout, 2)?.contains("overview:") {
            return Err(io::Error::other("restarted MCP overview did not succeed").into());
        }
    }

    let store = AtlasStore::open_for_project(&db_path, &repo)?;
    let final_calls = store.token_overview(None)?.calls;
    if final_calls != initial_calls + RESTART_COUNT {
        return Err(io::Error::other(format!(
            "MCP restart telemetry was not exact: expected {}, found {final_calls}",
            initial_calls + RESTART_COUNT
        ))
        .into());
    }
    let retention = store.telemetry_retention_state()?;
    if retention.active_instance_rows != 0 {
        return Err(io::Error::other(format!(
            "clean MCP shutdown left {} active runtime instances",
            retention.active_instance_rows
        ))
        .into());
    }
    let connection = Connection::open(db_path)?;
    let sealed_instances: usize = connection.query_row(
        "SELECT COUNT(*) FROM usage_instances WHERE owner = 'mcp_process' AND state = 'sealed'",
        [],
        |row| row.get(0),
    )?;
    if sealed_instances != RESTART_COUNT {
        return Err(io::Error::other(format!(
            "expected {RESTART_COUNT} persisted sealed MCP instances, found {sealed_instances}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn init_bootstrap_creates_db_scan_report_and_host_configs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {}\n",
    )?;

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "init command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let report: Value = serde_json::from_slice(&output.stdout)?;
    require_json_string(&report, &["project_dir", "status"], "created")?;
    require_json_string(&report, &["config", "status"], "created")?;
    require_json_string(&report, &["nonsource_files", "status"], "created")?;
    require_json_string(&report, &["db", "status"], "created")?;
    require_json_string(&report, &["scan", "status"], "verified")?;
    require_json_bool(&report, &["scan", "requested"], true)?;
    require_json_usize_at_least(&report, &["scan", "report", "overview", "files"], 1)?;
    require_json_string(
        &report,
        &["purpose_handoff", "recommended_subagent_reasoning"],
        "lowest_host_enforced",
    )?;
    require_json_string(
        &report,
        &["purpose_handoff", "execution_owner"],
        "agent_host",
    )?;
    require_json_bool(&report, &["purpose_handoff", "main_agent_fallback"], true)?;
    require_json_bool(
        &report,
        &["purpose_handoff", "server_started_curator"],
        false,
    )?;
    require_json_string(
        &report,
        &["purpose_handoff", "queue", "curation_scope"],
        "low",
    )?;
    require_json_array_len(&report, &["host_configs"], 3)?;

    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    for file_name in [
        "projectatlas.mcp.json",
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
    ] {
        let config_path = atlas_dir.join(file_name);
        if !config_path.is_file() {
            return Err(io::Error::other(format!("{file_name} was not generated")).into());
        }
        let config_text = fs::read_to_string(&config_path)?;
        if config_text.contains("--nearest-project") {
            return Err(io::Error::other(format!(
                "init-generated {file_name} enabled nearest-project routing by default"
            ))
            .into());
        }
    }
    if !atlas_dir.join("projectatlas.db").is_file() {
        return Err(io::Error::other("projectatlas.db was not created").into());
    }

    Ok(())
}

#[test]
fn init_no_scan_preserves_existing_config_and_is_idempotent() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let escaped_root = repo.to_string_lossy().replace('\\', "/");
    let config_path = atlas_dir.join("config.toml");
    let sentinel_config =
        format!("[project]\nroot = \"{escaped_root}\"\n\n[scan]\nmax_file_bytes = 12345\n");
    fs::write(&config_path, &sentinel_config)?;

    let first_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if !first_output.status.success() {
        return Err(io::Error::other(format!(
            "init --no-scan command failed: {}",
            String::from_utf8_lossy(&first_output.stderr)
        ))
        .into());
    }
    let first_report: Value = serde_json::from_slice(&first_output.stdout)?;
    require_json_string(&first_report, &["config", "status"], "exists")?;
    require_json_string(&first_report, &["db", "status"], "created")?;
    require_json_string(&first_report, &["scan", "status"], "skipped")?;
    require_json_bool(&first_report, &["scan", "requested"], false)?;
    if fs::read_to_string(&config_path)? != sentinel_config {
        return Err(io::Error::other("init --no-scan rewrote existing config").into());
    }

    let second_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if !second_output.status.success() {
        return Err(io::Error::other(format!(
            "second init --no-scan command failed: {}",
            String::from_utf8_lossy(&second_output.stderr)
        ))
        .into());
    }
    let second_report: Value = serde_json::from_slice(&second_output.stdout)?;
    require_json_string(&second_report, &["project_dir", "status"], "exists")?;
    require_json_string(&second_report, &["config", "status"], "exists")?;
    require_json_string(&second_report, &["db", "status"], "exists")?;
    require_json_string(&second_report, &["scan", "status"], "skipped")?;

    let force_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init", "--force-rescan"])
        .output()?;
    if !force_output.status.success() {
        return Err(io::Error::other(format!(
            "init --force-rescan command failed: {}",
            String::from_utf8_lossy(&force_output.stderr)
        ))
        .into());
    }
    let force_report: Value = serde_json::from_slice(&force_output.stdout)?;
    require_json_string(&force_report, &["scan", "status"], "verified")?;
    require_json_bool(&force_report, &["scan", "requested"], true)?;
    require_json_bool(&force_report, &["scan", "force_rescan"], true)?;
    if fs::read_to_string(&config_path)? != sentinel_config {
        return Err(io::Error::other("init --force-rescan rewrote existing config").into());
    }

    Ok(())
}

#[test]
fn init_reports_host_config_failure_before_nonzero_exit() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;

    let first_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if !first_output.status.success() {
        return Err(io::Error::other(format!(
            "initial init --no-scan command failed: {}",
            String::from_utf8_lossy(&first_output.stderr)
        ))
        .into());
    }

    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let blocked_config_path = atlas_dir.join("projectatlas.mcp.json");
    fs::remove_file(&blocked_config_path)?;
    fs::create_dir(&blocked_config_path)?;

    let failed_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if failed_output.status.success() {
        return Err(io::Error::other("init succeeded despite blocked host config path").into());
    }

    let report: Value = serde_json::from_slice(&failed_output.stdout)?;
    require_json_bool(&report, &["ok"], false)?;
    require_json_string(&report, &["host_configs", "0", "harness"], "mcp_json")?;
    require_json_string(&report, &["host_configs", "0", "status"], "failed")?;
    require_json_contains(&report, &["host_configs", "0", "error"], "io error")?;
    let next_steps = report["next_steps"]
        .as_array()
        .ok_or_else(|| io::Error::other("init report next_steps missing"))?;
    if !next_steps.iter().any(|step| {
        step.as_str()
            .is_some_and(|text| text.contains("Fix generated host MCP config errors"))
    }) {
        return Err(
            io::Error::other("init report did not include host config recovery step").into(),
        );
    }

    Ok(())
}

#[test]
fn init_preserves_flat_config_and_uses_it_for_first_scan() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(IGNORED_FIXTURE_DIR))?;
    fs::write(repo.join(SRC_DIR_NAME).join("lib.rs"), "pub fn kept() {}\n")?;
    fs::write(
        repo.join(IGNORED_FIXTURE_DIR).join(HIDDEN_RS_FILE_NAME),
        "pub fn hidden() {}\n",
    )?;
    let flat_config_path = repo.join("projectatlas.toml");
    let flat_config = "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"ignored-dir\"]\n";
    fs::write(&flat_config_path, flat_config)?;

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "init"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "init command with flat config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let report: Value = serde_json::from_slice(&output.stdout)?;
    require_json_string(&report, &["config", "status"], "exists")?;
    let reported_config = json_string_at(&report, &["config", "path"])?;
    if !reported_config
        .replace('\\', "/")
        .ends_with("repo/projectatlas.toml")
    {
        return Err(io::Error::other(format!(
            "init did not report flat projectatlas.toml as config path: {reported_config}"
        ))
        .into());
    }
    if repo.join(ATLAS_DIR_NAME).join("config.toml").exists() {
        return Err(io::Error::other("init created nested config that shadows flat config").into());
    }
    if fs::read_to_string(&flat_config_path)? != flat_config {
        return Err(io::Error::other("init rewrote flat projectatlas.toml").into());
    }

    let store = AtlasStore::open(&repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))?;
    let node_paths = store
        .load_nodes()?
        .into_iter()
        .map(|node| node.node.path)
        .collect::<Vec<_>>();
    if !node_paths.iter().any(|path| path == "src/lib.rs") {
        return Err(io::Error::other("init scan did not index the non-ignored source file").into());
    }
    if node_paths
        .iter()
        .any(|path| path == "ignored-dir/hidden.rs")
    {
        return Err(io::Error::other("init scan ignored the flat config exclude_dir_names").into());
    }

    Ok(())
}

#[test]
fn init_explicit_config_creates_selected_config_and_reports_it() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let custom_config_path = repo.join("custom.toml");

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--format",
            "json",
            "--config",
            "custom.toml",
            "init",
            "--no-scan",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "init --config custom.toml --no-scan failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    let report: Value = serde_json::from_slice(&output.stdout)?;
    require_json_string(&report, &["config", "status"], "created")?;
    let reported_config = json_string_at(&report, &["config", "path"])?;
    if std::path::Path::new(reported_config).canonicalize()? != custom_config_path.canonicalize()? {
        return Err(io::Error::other(format!(
            "init reported the wrong custom config path: {reported_config}"
        ))
        .into());
    }
    if !custom_config_path.is_file() {
        return Err(io::Error::other("init did not create selected custom.toml").into());
    }
    if repo.join(ATLAS_DIR_NAME).join("config.toml").exists() {
        return Err(
            io::Error::other("init created nested config despite explicit custom config").into(),
        );
    }

    let generated_mcp_config =
        fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.mcp.json"))?;
    let generated_mcp_config_json: Value = serde_json::from_str(&generated_mcp_config)?;
    let args = generated_mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("generated mcp args missing"))?;
    let config_arg = args
        .iter()
        .position(|value| value.as_str() == Some("--config"))
        .ok_or_else(|| io::Error::other("generated mcp config omitted --config"))?;
    let emitted_config = args
        .get(config_arg + 1)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("generated mcp config path missing"))?;
    if std::path::Path::new(emitted_config).canonicalize()? != custom_config_path.canonicalize()? {
        return Err(io::Error::other("generated mcp config did not use custom.toml").into());
    }

    Ok(())
}

#[test]
fn init_explicit_subdir_config_scans_the_repo_root() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(SUBDIR_CONFIG_DIR))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {}\n",
    )?;
    let custom_config_path = repo.join(SUBDIR_CONFIG_DIR).join("projectatlas.toml");

    let output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--format",
            "json",
            "--config",
            "config/projectatlas.toml",
            "init",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "init --config config/projectatlas.toml failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    let report: Value = serde_json::from_slice(&output.stdout)?;
    require_json_string(&report, &["config", "status"], "created")?;
    let reported_config = json_string_at(&report, &["config", "path"])?;
    if std::path::Path::new(reported_config).canonicalize()? != custom_config_path.canonicalize()? {
        return Err(io::Error::other(format!(
            "init reported the wrong subdir config path: {reported_config}"
        ))
        .into());
    }
    let config_text = fs::read_to_string(&custom_config_path)?;
    if !config_text.contains("root = \"..\"") {
        return Err(io::Error::other(format!(
            "subdir init config did not point back to repo root:\n{config_text}"
        ))
        .into());
    }
    let store = AtlasStore::open(&repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))?;
    let node_paths = store
        .load_nodes()?
        .into_iter()
        .map(|node| node.node.path)
        .collect::<Vec<_>>();
    if !node_paths.iter().any(|path| path == "src/lib.rs") {
        return Err(io::Error::other(
            "init scan with explicit subdir config did not index repo source",
        )
        .into());
    }

    Ok(())
}

#[test]
fn root_set_preserves_flat_config_for_generated_mcp_configs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let flat_config_path = repo.join("projectatlas.toml");
    fs::write(
        &flat_config_path,
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("root")
        .arg("set")
        .arg(&repo)
        .assert()
        .success();

    if repo.join(ATLAS_DIR_NAME).join("config.toml").exists() {
        return Err(
            io::Error::other("root set created nested config that shadows flat config").into(),
        );
    }

    let root_set_mcp_config_text =
        fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("projectatlas.mcp.json"))?;
    let root_set_mcp_config_json: Value = serde_json::from_str(&root_set_mcp_config_text)?;
    let args = root_set_mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("root set mcp args missing"))?;
    let config_arg = args
        .iter()
        .position(|value| value.as_str() == Some("--config"))
        .ok_or_else(|| io::Error::other("root set mcp config omitted --config"))?;
    let emitted_config = args
        .get(config_arg + 1)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("root set mcp config path missing"))?;
    if std::path::Path::new(emitted_config).canonicalize()? != flat_config_path.canonicalize()? {
        return Err(io::Error::other("root set mcp config did not use projectatlas.toml").into());
    }

    Ok(())
}

#[test]
fn plugin_installers_require_matching_runtime_version() -> Result<(), Box<dyn Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| io::Error::other("workspace root not found"))?;
    let powershell_installer = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join("scripts")
            .join("install-runtime.ps1"),
    )?;
    let posix_installer = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join("scripts")
            .join("install-runtime.sh"),
    )?;
    let release_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join("release.yml"),
    )?;
    let ci_workflow = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("workflows")
            .join("ci.yml"),
    )?;
    let readme = fs::read_to_string(workspace_root.join("README.md"))?;
    let agent_integration =
        fs::read_to_string(workspace_root.join("docs").join("agent-integration.md"))?;
    let architecture = fs::read_to_string(
        workspace_root
            .join("docs")
            .join("projectatlas-3-architecture.md"),
    )?;
    let skill_guidance = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
    )?;
    let codex_fallback_mcp = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join(".mcp.json");
    let claude_manifest = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join(".claude-plugin")
            .join("plugin.json"),
    )?;
    let codex_manifest = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join(CODEX_PLUGIN_MANIFEST_DIR)
            .join("plugin.json"),
    )?;
    let opencode_template = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join("opencode")
            .join("opencode.json"),
    )?;
    let opencode_native_plugin_dir = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join(".opencode-plugin");

    for required in [
        "Convert-ProjectAtlasVersionTag",
        "$runtime.version -eq $expectedRuntimeVersion",
        "Sync-ProjectAtlasRuntimeToLocalAppData",
        "Get-ReleaseRuntimeInstallPath",
        r"ProjectAtlas\runtimes\$safeVersion\x86_64-pc-windows-msvc",
        "ProjectAtlas LocalAppData mirror skipped",
        "PROJECTATLAS_SKIP_USER_PATH_UPDATE",
        "Set-ProjectAtlasProcessPathPrecedence",
        "Confirm-ProjectAtlasBareCommandResolution",
        "Active process resolves bare projectatlas to verified runtime",
        "Restart Codex or the shell",
        "Resolve-ProjectAtlasCodexCommand",
        "Update-ProjectAtlasCodexPlugin",
        "PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE",
        "Codex ProjectAtlas plugin marketplace updated",
        "Confirm-ProjectAtlasCodexSkillArtifact",
        "Codex ProjectAtlas plugin skill verified",
        "Codex does not expose the active in-process ProjectAtlas skill path",
        "plugin marketplace add styler-ai/ProjectAtlas --ref",
        "Update-ProjectAtlasCodexMcpRegistry",
        "PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE",
        "PROJECTATLAS_CODEX_COMMAND",
        "PROJECTATLAS_CODEX_COMMAND does not resolve",
        "Codex MCP registry updated to ProjectAtlas runtime",
        "mcp\", \"add\", \"projectatlas\", \"--\"",
        "Get-KnownProjectAtlasShimPaths",
        "Quarantine-ProjectAtlasStaleShims",
        "Test-ProjectAtlasRuntime $candidate $null",
        "[string]$RuntimePath",
        "PROJECTATLAS_RUNTIME_PATH",
        "Find-ProjectAtlas $ProjectAtlasVersion",
        "System.Text.UTF8Encoding",
        "Confirm-ReleaseArchiveChecksum",
        "Get-ProjectAtlasSha256",
        "SHA256SUMS",
        "[System.Security.Cryptography.SHA256]::Create()",
        "Checksum mismatch for ${Asset}",
        r#"$installArgs += @("projectatlas-cli", "--locked", "--force")"#,
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
        r#"Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code""#,
        r#"Write-ProjectAtlasMcpConfig $opencodeConfigPath "opencode""#,
        "Confirm-ProjectAtlasGeneratedMcpConfig",
        "ProjectAtlas generated MCP config verified",
        "Write-ProjectAtlasWorkflowPinReport",
        "Stale ProjectAtlas workflow release pin",
        "Claude Code ProjectAtlas integration verified through generated MCP config",
        "OpenCode ProjectAtlas integration verified through generated MCP config",
    ] {
        if !powershell_installer.contains(required) {
            return Err(io::Error::other(format!(
                "PowerShell installer is missing runtime version guard {required:?}"
            ))
            .into());
        }
    }
    if powershell_installer.contains("\"--package\", \"projectatlas-cli\"") {
        return Err(io::Error::other(
            "PowerShell installer uses invalid cargo install --git --package syntax",
        )
        .into());
    }
    let release_binary_function = powershell_installer
        .split("function Install-ReleaseBinary")
        .nth(1)
        .and_then(|tail| tail.split("if (-not $ProjectRoot)").next())
        .ok_or_else(|| io::Error::other("PowerShell release-binary installer block missing"))?;
    if release_binary_function.contains(r"ProjectAtlas\bin") {
        return Err(io::Error::other(
            "release-binary install must not write directly to the stable LocalAppData bin path",
        )
        .into());
    }
    for required in [
        "expected_runtime_version()",
        "known_projectatlas_shim_paths()",
        "is_projectatlas_runtime_contract",
        "quarantine_known_stale_projectatlas_shims",
        "quarantine_stale_projectatlas_shim",
        "prepend_projectatlas_process_path",
        "confirm_bare_projectatlas_resolution",
        "Active process resolves bare projectatlas to verified runtime",
        "restart the host shell",
        "resolve_codex_command",
        "update_codex_plugin",
        "PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE",
        "Codex ProjectAtlas plugin marketplace updated",
        "verify_codex_projectatlas_skill_artifact",
        "Codex ProjectAtlas plugin skill verified",
        "Codex does not expose the active in-process ProjectAtlas skill path",
        "plugin marketplace add styler-ai/ProjectAtlas --ref",
        "update_codex_mcp_registry",
        "PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE",
        "PROJECTATLAS_CODEX_COMMAND",
        "Codex MCP registry updated to ProjectAtlas runtime",
        "mcp add projectatlas --",
        "runtime_override=${PROJECTATLAS_RUNTIME_PATH:-}",
        "runtime_version=$(printf",
        "[ \"$runtime_version\" = \"$expected_version\" ]",
        "command -v realpath",
        "readlink -f",
        "Path(sys.argv[1]).resolve()",
        "download_release_file()",
        "archive_sha256()",
        "verify_release_checksum()",
        "SHA256SUMS did not contain an entry for $asset",
        "Checksum mismatch for $asset",
        "cargo install --git \"$repository\" --tag \"$projectatlas_version\" projectatlas-cli --locked --force",
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
        "write_mcp_config \"$claude_mcp_config_path\" claude-code",
        "write_mcp_config \"$opencode_config_path\" opencode",
        "verify_generated_mcp_config",
        "require_json_parser()",
        "ProjectAtlas generated MCP config verification requires jq or python3",
        "ProjectAtlas generated MCP config verified",
        "report_projectatlas_workflow_pins",
        "Stale ProjectAtlas workflow release pin",
        "Claude Code ProjectAtlas integration verified through generated MCP config",
        "OpenCode ProjectAtlas integration verified through generated MCP config",
    ] {
        if !posix_installer.contains(required) {
            return Err(io::Error::other(format!(
                "POSIX installer is missing runtime version guard {required:?}"
            ))
            .into());
        }
    }
    for forbidden in [
        r#"sed -n 's/.*"mcpServers""#,
        r#"sed -n 's/.*"args""#,
        r#"sed -n 's/.*"mcp""#,
        r#"sed -n 's/.*"enabled""#,
    ] {
        if posix_installer.contains(forbidden) {
            return Err(io::Error::other(format!(
                "POSIX generated MCP config verification must use a real JSON parser, found {forbidden:?}"
            ))
            .into());
        }
    }
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    for required in [
        format!("releases/tag/{release_tag}"),
        format!("badge/release-{release_tag}-blue"),
        format!("--ref {release_tag}"),
        format!("--tag {release_tag}"),
        format!("`{release_tag}` ships through the full release matrix"),
    ] {
        if !readme.contains(&required) {
            return Err(io::Error::other(format!(
                "README release/install docs are missing current version reference {required:?}"
            ))
            .into());
        }
    }
    for (job, expected, forbidden) in [
        (
            "prepublish-installer-smoke-unix",
            "bash ./plugins/projectatlas/scripts/install-runtime.sh \"$project_root\"",
            "install-runtime.ps1",
        ),
        (
            "installer-smoke-unix",
            "PROJECTATLAS_VERSION=\"$RELEASE_VERSION\" bash ./plugins/projectatlas/scripts/install-runtime.sh \"$project_root\"",
            "install-runtime.ps1",
        ),
        (
            "prepublish-installer-smoke-windows",
            ".\\plugins\\projectatlas\\scripts\\install-runtime.ps1",
            "install-runtime.sh",
        ),
        (
            "installer-smoke-windows",
            ".\\plugins\\projectatlas\\scripts\\install-runtime.ps1",
            "install-runtime.sh",
        ),
    ] {
        let block = workflow_job_block(&release_workflow, job)?;
        if !block.contains(expected) {
            return Err(io::Error::other(format!(
                "release workflow job {job} is missing platform-native installer route {expected:?}"
            ))
            .into());
        }
        if block.contains(forbidden) {
            return Err(io::Error::other(format!(
                "release workflow job {job} contains forbidden installer route {forbidden:?}"
            ))
            .into());
        }
    }
    let e2e_smoke = workflow_job_block(&ci_workflow, "e2e-smoke")?;
    if !e2e_smoke.contains("plugin_update_replaces_stale_runtime_configs_and_launches_new_mcp") {
        return Err(io::Error::other(
            "multi-OS CI smoke must run the plugin update stale-shim regression",
        )
        .into());
    }
    for required in [
        "plugin_update_skips_non_official_codex_marketplace",
        "plugin_update_leaves_current_codex_marketplace_untouched",
        "plugin_update_repairs_current_codex_plugin_with_stale_source_manifest",
        "plugin_update_restores_current_ref_marketplace_when_plugin_reinstall_fails",
    ] {
        if !e2e_smoke.contains(required) {
            return Err(io::Error::other(format!(
                "multi-OS CI smoke must run the Codex plugin update regression {required}"
            ))
            .into());
        }
    }
    if !e2e_smoke.contains(
        "windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked",
    ) {
        return Err(io::Error::other(
            "Windows CI smoke must run the locked stable mirror release-binary regression",
        )
        .into());
    }
    if !e2e_smoke
        .contains("windows_release_binary_installer_repairs_stale_mirror_without_registering_it")
    {
        return Err(io::Error::other(
            "Windows CI smoke must run the stale Codex MCP registry repair regression",
        )
        .into());
    }
    if posix_installer.contains("--package projectatlas-cli") {
        return Err(io::Error::other(
            "POSIX installer uses invalid cargo install --git --package syntax",
        )
        .into());
    }
    if codex_fallback_mcp.exists() {
        return Err(io::Error::other(
            "plugin must not ship a Codex fallback .mcp.json; generated project-local MCP configs use absolute runtime paths across supported operating systems",
        )
        .into());
    }
    if opencode_native_plugin_dir.exists() {
        return Err(io::Error::other(
            "ProjectAtlas OpenCode support is an MCP config template, not a native OpenCode plugin directory",
        )
        .into());
    }
    if !readme.contains("OpenCode MCP config template")
        || !agent_integration
            .contains("ProjectAtlas does not ship a native OpenCode JavaScript/TypeScript plugin")
        || !architecture.contains("not a native OpenCode JavaScript/TypeScript plugin")
    {
        return Err(io::Error::other(
            "docs must distinguish Claude Code plugin packaging from OpenCode MCP config support",
        )
        .into());
    }
    for forbidden in ["OpenCode plugin assets", "Claude Code / OpenCode plugins"] {
        if readme.contains(forbidden)
            || agent_integration.contains(forbidden)
            || architecture.contains(forbidden)
        {
            return Err(io::Error::other(format!(
                "docs still imply native OpenCode plugin packaging through {forbidden:?}"
            ))
            .into());
        }
    }
    for (document_name, document) in [
        ("README.md", readme.as_str()),
        ("docs/agent-integration.md", agent_integration.as_str()),
        (
            "plugins/projectatlas/skills/projectatlas/SKILL.md",
            skill_guidance.as_str(),
        ),
    ] {
        for required in [
            "codex plugin marketplace upgrade projectatlas --json",
            "codex plugin remove projectatlas --marketplace projectatlas",
            "codex plugin add projectatlas --marketplace projectatlas",
            "codex plugin list --marketplace projectatlas --available --json",
            "pinned to an older release tag",
            "dedicated `styler-ai/ProjectAtlas` source",
            "codex plugin marketplace remove projectatlas",
            "codex plugin marketplace add styler-ai/ProjectAtlas --ref",
            "codex mcp get projectatlas",
            "PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1",
        ] {
            if !document.contains(required) {
                return Err(io::Error::other(format!(
                    "{document_name} is missing Codex plugin cache/update guidance {required:?}"
                ))
                .into());
            }
        }
    }
    let windows_release_smoke = workflow_job_block(&release_workflow, "installer-smoke-windows")?;
    for required in [
        "[System.IO.FileShare]::None",
        r"ProjectAtlas\runtimes\$expectedVersion\x86_64-pc-windows-msvc\projectatlas.exe",
        "LocalAppData stable mirror unexpectedly changed while locked",
    ] {
        if !windows_release_smoke.contains(required) {
            return Err(io::Error::other(format!(
                "windows release smoke does not validate locked stable mirror behavior with {required:?}"
            ))
            .into());
        }
    }
    let claude_manifest_json: Value = serde_json::from_str(&claude_manifest)?;
    require_json_string(&claude_manifest_json, &["name"], "projectatlas")?;
    require_json_string(
        &claude_manifest_json,
        &["version"],
        env!("CARGO_PKG_VERSION"),
    )?;
    let codex_manifest_json: Value = serde_json::from_str(&codex_manifest)?;
    require_json_string(&codex_manifest_json, &["name"], "projectatlas")?;
    require_json_string(
        &codex_manifest_json,
        &["version"],
        env!("CARGO_PKG_VERSION"),
    )?;
    let default_prompts = codex_manifest_json["interface"]["defaultPrompt"]
        .as_array()
        .ok_or_else(|| io::Error::other("Codex plugin defaultPrompt must be an array"))?;
    if default_prompts.len() > 3 {
        return Err(
            io::Error::other("Codex plugin defaultPrompt must contain at most 3 prompts").into(),
        );
    }
    for prompt in default_prompts {
        let prompt = prompt.as_str().ok_or_else(|| {
            io::Error::other("Codex plugin defaultPrompt entries must be strings")
        })?;
        if prompt.trim().is_empty() {
            return Err(
                io::Error::other("Codex plugin defaultPrompt entries must not be empty").into(),
            );
        }
        if prompt.chars().count() > 128 {
            return Err(io::Error::other(format!(
                "Codex plugin defaultPrompt entry is longer than 128 characters: {prompt}"
            ))
            .into());
        }
    }
    let opencode_json: Value = serde_json::from_str(&opencode_template)?;
    require_json_string(
        &opencode_json,
        &["$schema"],
        "https://opencode.ai/config.json",
    )?;
    require_json_string(&opencode_json, &["mcp", "projectatlas", "type"], "local")?;
    require_json_bool(&opencode_json, &["mcp", "projectatlas", "enabled"], false)?;
    require_json_string(
        &opencode_json,
        &["mcp", "projectatlas", "command", "0"],
        "/absolute/path/to/projectatlas",
    )?;
    require_json_string(
        &opencode_json,
        &["mcp", "projectatlas", "command", "1"],
        "--require-version",
    )?;
    require_json_string(
        &opencode_json,
        &["mcp", "projectatlas", "command", "2"],
        env!("CARGO_PKG_VERSION"),
    )?;
    require_json_string(
        &opencode_json,
        &["mcp", "projectatlas", "command", "4"],
        "/absolute/path/to/project/.projectatlas/projectatlas.db",
    )?;
    require_json_string(
        &opencode_json,
        &["mcp", "projectatlas", "cwd"],
        "/absolute/path/to/project",
    )?;
    Ok(())
}

#[test]
fn packaged_skill_routes_task_startup_through_session_brief() -> Result<(), Box<dyn Error>> {
    let skill = fs::read_to_string(
        workspace_root()?
            .join("plugins")
            .join("projectatlas")
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
    )?;
    for required in [
        "For task-directed work in an existing indexed repository",
        "On first use in each distinct project root",
        "Every project root owns its own `.projectatlas/projectatlas.db`",
        "**Fresh existing index:** make no indexing call",
        "**Changed files:** use `atlas_watch_once`",
        "**Deep symbol/graph rebuild:** use `atlas_symbols_build` only when",
        "`atlas_session_brief` once at task-oriented startup",
        "`atlas_session_brief` once at task-oriented startup with `query`, `project_path` when needed, and `compact: true`",
        "start with `file_limit: 3`, `folder_limit: 3`, `blocker_limit: 1`, and `purpose_limit: 1`",
        "do not restart the brief",
        "returned `atlas_file_summary` recommendation with `compact: true`",
        "compact summary's crisp connections for an ordinary direct caller",
        "Do not add a relation call merely to reconfirm a trusted `called_by` or call row",
        "Request occurrences only when the call-site span itself is needed",
        "Public exposure is not an inbound-caller question",
        "a reviewed purpose and nested `pub` declaration are selection evidence, not exposure proof",
        "Follow its typed next call directly",
        "`connections_truncated` describes the compact sample",
        "Do not guess a symbol line or other disambiguator",
        "Fall back to `atlas_overview` only when the session-brief MCP tool is unavailable",
        "partition a large queue into bounded, non-overlapping batches",
        "becomes `approved`, `source: agent`, and `agent_reviewed: true` immediately",
        "add one durable pointer to the nearest harness instruction file",
        "a runtime `version` matching the selected plugin release",
        "resolve the installer from the installed, version-matched ProjectAtlas plugin root",
        "-ProjectRoot \"<target-project-root>\"",
        "Do not assume an unrelated target repository contains `plugins/projectatlas/scripts`",
    ] {
        if !skill.contains(required) {
            return Err(io::Error::other(format!(
                "packaged skill is missing task-oriented session-brief guidance {required:?}"
            ))
            .into());
        }
    }
    for stale in [
        "otherwise call `atlas_overview`",
        "New session after scan: call `atlas_overview`",
        "`atlas_overview` at startup",
    ] {
        if skill.contains(stale) {
            return Err(io::Error::other(format!(
                "packaged skill still contains stale startup routing {stale:?}"
            ))
            .into());
        }
    }
    for path in [
        "README.md",
        "templates/AGENTS.md",
        "docs/agent-integration.md",
        "docs/index.md",
        "docs/workflow.md",
    ] {
        let guidance = fs::read_to_string(workspace_root()?.join(path))?;
        for required in ["atlas_session_brief", "compact: true"] {
            if !guidance.contains(required) {
                return Err(io::Error::other(format!(
                    "{path} must route task-directed agent startup through one compact session brief; missing {required:?}"
                ))
                .into());
            }
        }
        for stale in [
            "1. Build or refresh the local atlas.",
            "This is the workflow the agent runs for you:",
            "3. Run `projectatlas overview`",
            "`atlas_scan` if stale, then `atlas_overview`",
            "Run `projectatlas overview`, `projectatlas folders <query>`, and `projectatlas files <query>` before broad source reads",
        ] {
            if guidance.contains(stale) {
                return Err(io::Error::other(format!(
                    "{path} still contains mandatory pre-session-brief startup routing {stale:?}"
                ))
                .into());
            }
        }
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
    let gitignore = fs::read_to_string(workspace_root.join(".gitignore"))?;
    for required in [
        "Rust-native local code index and atlas",
        "complete SQLite-backed index",
        "fast local SQLite index",
    ] {
        if !readme.contains(required) {
            return Err(io::Error::other(format!(
                "README must present ProjectAtlas as a complete local code index; missing {required:?}"
            ))
            .into());
        }
    }
    for (workflow_name, workflow) in [("ci", &ci_workflow), ("release", &release_workflow)] {
        let verify = workflow_job_block(workflow, "verify")?;
        if verify.contains("projectatlas.toon") || verify.contains("map --force") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must not require the legacy committed TOON map artifact"
            ))
            .into());
        }
        if verify.contains("--strict-folders") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must not require legacy folder .purpose linting"
            ))
            .into());
        }
        if !verify.contains("projectatlas-lints") || !verify.contains("strict-strings") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must run strict ProjectAtlas source string lints"
            ))
            .into());
        }
        for run in workflow_job_runs(workflow, "verify")? {
            if command_runs_projectatlas_maintenance(&run) {
                return Err(io::Error::other(format!(
                    "{workflow_name} verify job must keep ProjectAtlas init, scan, purpose, parity, and lint maintenance local"
                ))
                .into());
            }
        }
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
        "cargo test --locked -p projectatlas-cli --test e2e",
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
        "AGENTS.md",
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
    let auto_release_workflow = fs::read_to_string(workflow_dir.join("03-auto-release.yml"))?;
    let optional_parser_handoff_resolver = fs::read_to_string(
        workspace_root
            .join(".github")
            .join("scripts")
            .join("resolve-optional-parser-handoff.py"),
    )?;
    let optional_parser_workflow =
        fs::read_to_string(workflow_dir.join("optional-parser-pack.yml"))?;
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
            ("target-branch", update["target-branch"].as_str(), "dev"),
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
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
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
    for required in [
        "gh release create \"$RELEASE_VERSION\"",
        "gh release upload \"$RELEASE_VERSION\" release-assets/* --clobber",
        "--target \"$GITHUB_SHA\"",
        "PROJECTATLAS_RELEASE_EXISTS",
        "SHA256SUMS",
        "No release archives matched projectatlas-${RELEASE_VERSION}-*",
        "already points to",
        "exists without a GitHub release; continuing recovery publish",
        "continuing asset repair publish",
    ] {
        if !release_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "release workflow is missing recoverable publish/checksum guard {required:?}"
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
        "git rev-parse HEAD^2",
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
fn issueops_and_workflows_use_behavior_focused_quality_gates() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let github = workspace_root.join(".github");
    let workflows = github.join("workflows");
    let issueops = fs::read_to_string(github.join("scripts").join("issue-checklists.py"))?;
    let ci = fs::read_to_string(workflows.join("ci.yml"))?;
    let release = fs::read_to_string(workflows.join("release.yml"))?;
    let hook = fs::read_to_string(
        workspace_root
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
    )?;
    let template = fs::read_to_string(github.join("pull_request_template.md"))?;
    let bug_issue_template =
        fs::read_to_string(github.join(ISSUE_TEMPLATE_DIR_NAME).join("bug_report.yml"))?;
    let chore_issue_template =
        fs::read_to_string(github.join(ISSUE_TEMPLATE_DIR_NAME).join("chore.yml"))?;
    let improvement_issue_template = fs::read_to_string(
        github
            .join(ISSUE_TEMPLATE_DIR_NAME)
            .join("improvement_request.yml"),
    )?;
    let agent_rules = fs::read_to_string(workspace_root.join("AGENTS.md"))?;
    let workflow_docs =
        fs::read_to_string(workspace_root.join("docs").join(WORKFLOW_DOC_FILE_NAME))?;
    let toolchain = fs::read_to_string(workspace_root.join("rust-toolchain.toml"))?;
    let issue_map = fs::read_to_string(
        workspace_root
            .join(OPENSPEC_DIR_NAME)
            .join("issue-map.json"),
    )?;
    let tasks = fs::read_to_string(
        workspace_root
            .join(OPENSPEC_DIR_NAME)
            .join("changes")
            .join("enforce-rust-test-quality-gates")
            .join("tasks.md"),
    )?;

    for required in [
        "validate_unique_issue_ownership",
        "owner_slices",
        "visible_markdown",
        "remote != expected",
        "milestone_issue_failures",
        "REQUIRED_OPEN_ISSUE_HEADINGS",
        "architecture_diagram_link_failures",
        "MITIGATION_RE",
        "issue_contract_failures",
    ] {
        if !issueops.contains(required) {
            return Err(io::Error::other(format!(
                "IssueOps is missing lean checklist behavior {required:?}"
            ))
            .into());
        }
    }
    if !issue_map.contains(r#""schema_version": 2"#)
        || !issue_map.contains(r#""enforce-rust-test-quality-gates": 309"#)
    {
        return Err(io::Error::other("#309 must be mapped by the schema-2 issue map").into());
    }
    for required in [
        "label: Why",
        "label: What Changes",
        "label: Capabilities",
        "label: Architecture Diagrams",
        "blob/dev/docs/",
        "label: Release Scope",
        "label: Acceptance criteria",
        "label: Non-Goals",
        "label: Pre-Mortem",
        "OpenSpec tasks:",
    ] {
        for (name, content) in [
            ("bug", bug_issue_template.as_str()),
            ("chore", chore_issue_template.as_str()),
            ("improvement", improvement_issue_template.as_str()),
        ] {
            if !content.contains(required) {
                return Err(io::Error::other(format!(
                    "{name} issue form is missing v0.3.26 issue contract field {required:?}"
                ))
                .into());
            }
        }
    }
    for required in [
        "Pre-Mortem",
        "Architecture Diagrams",
        "dev/docs/*.md",
        "OpenSpec tasks:",
        "commit/SHA permalink evidence",
    ] {
        if !agent_rules.contains(required) || !workflow_docs.contains(required) {
            return Err(io::Error::other(format!(
                "agent and workflow guidance are missing lean issue contract {required:?}"
            ))
            .into());
        }
    }

    for required in [
        "cargo fmt --all --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo test --workspace --all-features --locked",
        "cargo test --doc --workspace --all-features --locked",
        "RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --all-features --locked",
        "cargo deny --locked --all-features check -D warnings",
        "issue-checklists.py --self-test",
        "test-optional-parser-proof-inputs.py",
    ] {
        if !hook.contains(required) || !workflow_docs.contains(required) {
            return Err(io::Error::other(format!(
                "hook and workflow docs must share ordinary gate {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "cargo fmt --all --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo test --workspace --all-features --locked",
        "cargo test --locked -p projectatlas-cli --all-features task_errors_classify_only_typed_cancellation_as_canceled",
        "cargo test --doc --workspace --all-features --locked",
        "cargo deny --locked --all-features check -D warnings",
        "test-optional-parser-proof-inputs.py",
        "--issue-map openspec/issue-map.json",
    ] {
        if !ci.contains(required) {
            return Err(io::Error::other(format!(
                "ordinary CI is missing blocking gate {required:?}"
            ))
            .into());
        }
    }
    let checklist_step = ci
        .split("- name: Issue checklist check")
        .nth(1)
        .and_then(|tail| tail.split("- name:").next())
        .ok_or_else(|| io::Error::other("ordinary IssueOps step is missing"))?;
    if checklist_step.contains("--milestone") {
        return Err(io::Error::other(
            "ordinary pull requests must not require full milestone completion",
        )
        .into());
    }
    if !release.contains("--milestone \"${RELEASE_VERSION}-00\"")
        || !release.contains("cargo fmt --all --check")
        || !release.contains(
            "cargo test --locked -p projectatlas-cli --all-features task_errors_classify_only_typed_cancellation_as_canceled",
        )
        || !release.contains("test-optional-parser-proof-inputs.py")
    {
        return Err(io::Error::other(
            "release must retain milestone completion, ordinary gates, and a non-publishing package-proof mode",
        )
        .into());
    }
    let prepublish_input = release
        .split("      prepublish_only:")
        .nth(1)
        .and_then(|tail| tail.split("\n\npermissions:").next())
        .ok_or_else(|| io::Error::other("release omitted the prepublish-only input"))?;
    for required in ["required: false", "default: false", "type: boolean"] {
        if !prepublish_input.contains(required) {
            return Err(io::Error::other(format!(
                "prepublish-only input omitted fail-closed field {required:?}"
            ))
            .into());
        }
    }
    let prepublish_guard = "if: ${{ !inputs.prepublish_only }}";
    let checklist_gate = release
        .split("      - name: Release issue checklist gate")
        .nth(1)
        .and_then(|tail| tail.split("\n  package-unix:").next())
        .ok_or_else(|| io::Error::other("release omitted the checklist gate step"))?;
    if !checklist_gate.contains(prepublish_guard) {
        return Err(io::Error::other(
            "release checklist gate is not owned by the prepublish-only guard",
        )
        .into());
    }
    let publish_job = release
        .split("\n  publish:\n")
        .nth(1)
        .ok_or_else(|| io::Error::other("release omitted the publish job"))?;
    let publish_header = publish_job
        .split("    steps:")
        .next()
        .ok_or_else(|| io::Error::other("release publish job omitted its header"))?;
    if !publish_header.contains(prepublish_guard) || release.matches(prepublish_guard).count() != 2
    {
        return Err(io::Error::other(
            "prepublish-only guard must own exactly the checklist step and publish job",
        )
        .into());
    }
    for job in [
        "verify",
        "package-unix",
        "package-windows",
        "prepublish-installer-smoke-unix",
        "prepublish-installer-smoke-windows",
    ] {
        let marker = format!("\n  {job}:\n");
        let header = release
            .split(&marker)
            .nth(1)
            .and_then(|tail| tail.split("    steps:").next())
            .ok_or_else(|| io::Error::other(format!("release omitted the {job} job header")))?;
        if header.contains(prepublish_guard) {
            return Err(io::Error::other(format!(
                "prepublish-only mode incorrectly suppresses the {job} job"
            ))
            .into());
        }
    }
    for required in [
        "packaged-contract-runner-${{ matrix.suffix }}",
        "packaged-contract-runner-x86_64-pc-windows-msvc",
        "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE",
        "[prepublish-installer-smoke-unix, prepublish-installer-smoke-windows, parser-pack-assets]",
        "pattern: projectatlas-*",
    ] {
        if !release.contains(required) {
            return Err(io::Error::other(format!(
                "release omitted packaged CLI contract wiring {required:?}"
            ))
            .into());
        }
    }
    let unix_prepublish = release
        .split("  prepublish-installer-smoke-unix:")
        .nth(1)
        .and_then(|tail| tail.split("  prepublish-installer-smoke-windows:").next())
        .ok_or_else(|| io::Error::other("release omitted the Unix prepublish job"))?;
    let windows_prepublish = release
        .split("  prepublish-installer-smoke-windows:")
        .nth(1)
        .and_then(|tail| tail.split("  parser-pack-assets:").next())
        .ok_or_else(|| io::Error::other("release omitted the Windows prepublish job"))?;
    for (job, body) in [("Unix", unix_prepublish), ("Windows", windows_prepublish)] {
        for contract in [
            "mcp_advertised_tools_own_their_real_sqlite_effects",
            "packaged_cli_surface_preserves_v0326_routes_and_defaults",
            "packaged_cli_commands_own_their_real_sqlite_effects",
        ] {
            if !body.contains(contract) {
                return Err(io::Error::other(format!(
                    "{job} prepublish omitted packaged contract {contract:?}"
                ))
                .into());
            }
        }
    }
    for suffix in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        if unix_prepublish.matches(suffix).count() != 1 {
            return Err(io::Error::other(format!(
                "Unix prepublish must own exactly one {suffix:?} target"
            ))
            .into());
        }
    }
    if !windows_prepublish.contains("x86_64-pc-windows-msvc") {
        return Err(io::Error::other(
            "Windows prepublish omitted the x86_64-pc-windows-msvc target",
        )
        .into());
    }

    if !template.contains("Refs #NNN")
        || !template.contains("Use `Closes #NNN` only when this pull request completes the issue.")
        || template.contains("every OpenSpec task is checked off before merge")
    {
        return Err(io::Error::other(
            "pull request template must allow meaningful incremental dev slices",
        )
        .into());
    }
    if !toolchain.contains("channel = \"1.93.1\"") {
        return Err(io::Error::other("Rust toolchain must be repository-owned and pinned").into());
    }

    let rejected_terms = [
        "TQG-UT",
        "OpenSpec-Task:",
        "task-verification",
        "task-evidence",
        "cargo nextest",
        "cargo llvm-cov",
        "cargo mutants",
        "issue sealing",
    ];
    for (name, content) in [
        ("CI", ci.as_str()),
        ("hook", hook.as_str()),
        ("PR template", template.as_str()),
    ] {
        for rejected in rejected_terms {
            if content.contains(rejected) {
                return Err(io::Error::other(format!(
                    "{name} retains rejected evidence ceremony {rejected:?}"
                ))
                .into());
            }
        }
    }
    if tasks.contains("TQG-UT") || tasks.contains("[UT:") {
        return Err(io::Error::other(
            "OpenSpec tasks must not assign one test identifier per task",
        )
        .into());
    }
    for rejected in [
        "OpenSpec Commit Links",
        "exact-commit OpenSpec links",
        "required committed OpenSpec permalinks",
    ] {
        for (name, content) in [
            ("AGENTS", agent_rules.as_str()),
            ("workflow docs", workflow_docs.as_str()),
            ("#309 tasks", tasks.as_str()),
        ] {
            if content.contains(rejected) {
                return Err(io::Error::other(format!(
                    "{name} retains issue-level commit evidence {rejected:?}"
                ))
                .into());
            }
        }
    }
    for removed in [
        ".cargo/mutants.toml",
        ".config/nextest.toml",
        ".github/workflows/05-full-mutation.yml",
        ".github/workflows/06-task-evidence-render.yml",
        ".github/workflows/07-quality-failure-smoke.yml",
        "openspec/task-evidence.json",
        "openspec/task-verification-plan.json",
        "openspec/task-verification.json",
        "test-quality.toml",
    ] {
        if workspace_root.join(removed).exists() {
            return Err(io::Error::other(format!(
                "rejected evidence artifact still exists: {removed}"
            ))
            .into());
        }
    }

    Ok(())
}

#[test]
fn plugin_installer_writes_real_harness_configs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer(&workspace_root, &repo, &runtime)?;
    let installer_output_text = format!(
        "{}{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output_text.contains("Claude Code ProjectAtlas generated MCP config verified")
        || !installer_output_text.contains("OpenCode ProjectAtlas generated MCP config verified")
    {
        return Err(io::Error::other(format!(
            "installer did not verify generated Claude/OpenCode configs:\n{installer_output_text}"
        ))
        .into());
    }

    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
    let claude_config = read_json_file(&atlas_dir.join("projectatlas.claude.mcp.json"))?;
    let opencode_config = read_json_file(&atlas_dir.join("projectatlas.opencode.json"))?;

    require_same_executable(
        json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "codex",
    )?;
    require_json_string(
        &codex_config,
        &["mcpServers", "projectatlas", "args", "0"],
        "--require-version",
    )?;
    require_json_string(
        &codex_config,
        &["mcpServers", "projectatlas", "args", "6"],
        "mcp",
    )?;
    let codex_cwd = json_string_at(&codex_config, &["mcpServers", "projectatlas", "cwd"])?;
    require_same_directory(codex_cwd, &repo, "codex cwd")?;

    require_same_executable(
        json_string_at(&claude_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "claude",
    )?;
    if claude_config["mcpServers"]["projectatlas"]
        .get("cwd")
        .is_some()
    {
        return Err(io::Error::other("Claude Code MCP config should not rely on cwd").into());
    }
    require_json_string(
        &claude_config,
        &["mcpServers", "projectatlas", "args", "6"],
        "mcp",
    )?;

    require_json_string(
        &opencode_config,
        &["$schema"],
        "https://opencode.ai/config.json",
    )?;
    require_json_string(&opencode_config, &["mcp", "projectatlas", "type"], "local")?;
    require_json_bool(&opencode_config, &["mcp", "projectatlas", "enabled"], true)?;
    require_same_executable(
        json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "0"])?,
        &runtime,
        "opencode",
    )?;
    require_json_string(
        &opencode_config,
        &["mcp", "projectatlas", "command", "7"],
        "mcp",
    )?;
    require_same_directory(
        json_string_at(&opencode_config, &["mcp", "projectatlas", "cwd"])?,
        &repo,
        "opencode cwd",
    )?;

    Ok(())
}

#[test]
#[cfg(unix)]
fn posix_installer_accepts_symlinked_runtime_path() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let runtime_link = temp.path().join("projectatlas-runtime-link");
    symlink(&runtime, &runtime_link)?;

    let installer_output =
        run_projectatlas_plugin_installer(&workspace_root, &repo, &runtime_link)?;
    let installer_output_text = format!(
        "{}{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output.status.success() {
        return Err(io::Error::other(format!(
            "POSIX installer rejected symlinked runtime path:\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Claude Code ProjectAtlas generated MCP config verified")
        || !installer_output_text.contains("OpenCode ProjectAtlas generated MCP config verified")
    {
        return Err(io::Error::other(format!(
            "installer did not verify generated configs with symlinked runtime:\n{installer_output_text}"
        ))
        .into());
    }

    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
    let claude_config = read_json_file(&atlas_dir.join("projectatlas.claude.mcp.json"))?;
    let opencode_config = read_json_file(&atlas_dir.join("projectatlas.opencode.json"))?;

    require_same_executable(
        json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "codex symlink",
    )?;
    require_same_executable(
        json_string_at(&claude_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "claude symlink",
    )?;
    require_same_executable(
        json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "0"])?,
        &runtime,
        "opencode symlink",
    )?;

    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;

    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex = isolated_home.join("codex.cmd");
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  echo projectatlas\r\n  echo   command: %LOCALAPPDATA%\\ProjectAtlas\\bin\\projectatlas.exe\r\n  echo   args: --require-version 0.3.15 --db C:\\old\\.projectatlas\\projectatlas.db mcp\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
    )?;

    let stable_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("bin")
        .join("projectatlas.exe");
    fs::create_dir_all(
        stable_runtime
            .parent()
            .ok_or_else(|| io::Error::other("stable runtime parent missing"))?,
    )?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    fs::copy(&runtime, &stable_runtime)?;

    let db = atlas_dir.join("projectatlas.db");
    let mut locked_runtime = StdCommand::new(&stable_runtime)
        .arg("--require-version")
        .arg(env!("CARGO_PKG_VERSION"))
        .arg("--db")
        .arg(&db)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::sleep(Duration::from_millis(300));
    if let Some(status) = locked_runtime.try_wait()? {
        return Err(io::Error::other(format!(
            "fixture runtime exited before it could lock the stable mirror: {status}"
        ))
        .into());
    }

    let test_result = (|| -> Result<(), Box<dyn Error>> {
        let release_archive = create_windows_release_archive(temp.path(), &runtime)?;
        let (release_base_url, release_server) = serve_release_assets(&release_archive, None)?;
        let workspace_root = workspace_root()?;
        let installer = workspace_root
            .join("plugins")
            .join("projectatlas")
            .join("scripts")
            .join("install-runtime.ps1");
        let output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-ReleaseBaseUrl")
            .arg(&release_base_url)
            .arg("-ReleaseBinaryOnly")
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let server_result = release_server.join().map_err(|panic_payload| {
            let message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
                *message
            } else if let Some(message) = panic_payload.downcast_ref::<String>() {
                message.as_str()
            } else {
                "unknown panic payload"
            };
            io::Error::other(format!("release asset test server panicked: {message}"))
        })?;
        server_result?;
        let installer_output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "release-binary installer failed\n{installer_output_text}"
            ))
            .into());
        }
        let normalized_installer_output = installer_output_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for required in [
            "ProjectAtlas LocalAppData mirror skipped",
            "Close any running ProjectAtlas or Codex session using that file",
            "then rerun this installer",
            "Codex MCP and generated configs continue to use verified",
        ] {
            if !normalized_installer_output.contains(required) {
                return Err(io::Error::other(format!(
                    "installer did not provide locked LocalAppData mirror guidance {required:?}\n{installer_output_text}"
                ))
                .into());
            }
        }
        if !installer_output_text
            .contains("Active process resolves bare projectatlas to verified runtime")
        {
            return Err(io::Error::other(format!(
                "installer did not make its active process prefer the verified runtime\n{installer_output_text}"
            ))
            .into());
        }

        let versioned_runtime = local_app_data
            .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
            .join("runtimes")
            .join(env!("CARGO_PKG_VERSION"))
            .join("x86_64-pc-windows-msvc")
            .join("projectatlas.exe");
        if !versioned_runtime.exists() {
            return Err(io::Error::other(format!(
                "release binary was not installed to the versioned runtime path: {}",
                versioned_runtime.display()
            ))
            .into());
        }
        let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
        if !fake_codex_calls.contains("mcp add projectatlas --")
            || !fake_codex_calls.contains(versioned_runtime.to_string_lossy().as_ref())
            || fake_codex_calls.contains(stable_runtime.to_string_lossy().as_ref())
        {
            return Err(io::Error::other(format!(
                "locked mirror Codex MCP registry was not repaired to the versioned runtime:\n{fake_codex_calls}"
            ))
            .into());
        }
        if !installer_output_text.contains("Codex MCP registry updated to ProjectAtlas runtime") {
            return Err(io::Error::other(format!(
                "installer did not report locked mirror Codex registry repair\n{installer_output_text}"
            ))
            .into());
        }

        let runtime_info = StdCommand::new(&versioned_runtime)
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--format")
            .arg("json")
            .arg("runtime-info")
            .output()?;
        if !runtime_info.status.success() {
            return Err(io::Error::other(format!(
                "versioned runtime failed runtime-info: {}",
                String::from_utf8_lossy(&runtime_info.stderr)
            ))
            .into());
        }

        let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
        let claude_config = read_json_file(&atlas_dir.join("projectatlas.claude.mcp.json"))?;
        let opencode_config = read_json_file(&atlas_dir.join("projectatlas.opencode.json"))?;
        require_same_executable(
            json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
            &versioned_runtime,
            "locked mirror codex",
        )?;
        require_json_string(
            &codex_config,
            &["mcpServers", "projectatlas", "args", "1"],
            env!("CARGO_PKG_VERSION"),
        )?;
        require_same_directory(
            json_string_at(&codex_config, &["mcpServers", "projectatlas", "cwd"])?,
            &repo,
            "locked mirror codex cwd",
        )?;
        require_same_executable(
            json_string_at(&claude_config, &["mcpServers", "projectatlas", "command"])?,
            &versioned_runtime,
            "locked mirror claude",
        )?;
        require_json_string(
            &claude_config,
            &["mcpServers", "projectatlas", "args", "1"],
            env!("CARGO_PKG_VERSION"),
        )?;
        require_same_executable(
            json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "0"])?,
            &versioned_runtime,
            "locked mirror opencode",
        )?;
        require_json_string(
            &opencode_config,
            &["mcp", "projectatlas", "command", "2"],
            env!("CARGO_PKG_VERSION"),
        )?;
        require_same_directory(
            json_string_at(&opencode_config, &["mcp", "projectatlas", "cwd"])?,
            &repo,
            "locked mirror opencode cwd",
        )?;

        let (mcp_command, mcp_args) = mcp_command_and_args(&codex_config)?;
        let messages = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-locked-runtime-e2e","version":"0.1.0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        ];
        let mcp_stdout = run_mcp_stdio(&mcp_command, &repo, &mcp_args, &messages)?;
        let expected_server_info = format!(
            r#""serverInfo":{{"name":"ProjectAtlas","version":"{}"}}"#,
            env!("CARGO_PKG_VERSION")
        );
        if !mcp_stdout.contains(&expected_server_info) {
            return Err(io::Error::other(format!(
                "locked mirror MCP config did not launch the versioned runtime: {mcp_stdout}"
            ))
            .into());
        }

        Ok(())
    })();

    let kill_result = locked_runtime.kill();
    let wait_result = locked_runtime.wait();
    if let Err(error) = kill_result
        && test_result.is_ok()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        return Err(error.into());
    }
    if let Err(error) = wait_result
        && test_result.is_ok()
    {
        return Err(error.into());
    }
    test_result
}

#[test]
fn plugin_update_replaces_stale_runtime_configs_and_launches_new_mcp() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("a.rs"), "pub fn a() {}\n")?;
    fs::write(repo.join(SRC_DIR_NAME).join("b.rs"), "pub fn b() {}\n")?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        atlas_dir.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n",
    )?;
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let workflow_dir = repo.join(".github").join("workflows");
    fs::create_dir_all(&workflow_dir)?;
    fs::write(
        workflow_dir.join("ci.yml"),
        format!(
            "jobs:\n  smoke:\n    steps:\n      - run: curl -fsSL https://github.com/styler-ai/ProjectAtlas/releases/download/v0.0.1/projectatlas-v0.0.1-x86_64-unknown-linux-gnu.tar.gz -o projectatlas.tar.gz\n      - run: curl -fsSL https://github.com/styler-ai/ProjectAtlas/releases/download/{expected_release_tag}/projectatlas-{expected_release_tag}-x86_64-unknown-linux-gnu.tar.gz -o projectatlas-current.tar.gz\n      - run: curl -fsSL https://github.com/example/ProjectAtlas/releases/download/v9.9.9/projectatlas-v9.9.9-x86_64-unknown-linux-gnu.tar.gz -o projectatlas-fork.tar.gz\n"
        ),
    )?;
    fs::write(
        atlas_dir.join("kept-state.txt"),
        "existing project-local state must survive plugin updates\n",
    )?;
    let db = atlas_dir.join("projectatlas.db");
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(atlas_dir.join("config.toml"))
        .args(["scan", "."])
        .assert()
        .success();
    let preserved_health_id = {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            "src/a.rs",
            "Shared plugin update state",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/b.rs",
            "Shared plugin update state",
            PurposeSource::Agent,
        )?;
        store.record_usage(&usage_from_estimates(
            "plugin-update",
            "summary",
            Some("src/a.rs".to_string()),
            None,
            200,
            50,
        ))?;
        let duplicate = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        let id = duplicate.id.clone();
        store.resolve_health_finding(&HealthResolution {
            finding_id: id.clone(),
            category: duplicate.category,
            path: duplicate.path,
            related_path: duplicate.related_path,
            rationale: "Plugin update preservation fixture.".to_string(),
        })?;
        id
    };
    let token_calls_before = AtlasStore::open(&db)?.token_overview(None)?.calls;
    let stale_runtime_dir = temp.path().join("old-plugin");
    let stale_runtime = stale_runtime_dir.join(if cfg!(windows) {
        "projectatlas.cmd"
    } else {
        "projectatlas"
    });
    let stale_runtime_text = stale_runtime.to_string_lossy();
    fs::create_dir_all(&stale_runtime_dir)?;
    let stale_runtime_script = if cfg!(windows) {
        "@echo off\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.0.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n"
    } else {
        "#!/usr/bin/env sh\nprintf '%s\\n' '{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.0.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}'\n"
    };
    fs::write(&stale_runtime, stale_runtime_script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&stale_runtime)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&stale_runtime, permissions)?;
    }
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex = stale_runtime_dir.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let fake_plugin_cache = isolated_home
        .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
        .join("projectatlas");
    fs::create_dir_all(fake_plugin_cache.join(CODEX_PLUGIN_MANIFEST_DIR))?;
    fs::create_dir_all(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME),
    )?;
    fs::write(
        fake_plugin_cache
            .join(CODEX_PLUGIN_MANIFEST_DIR)
            .join("plugin.json"),
        format!(
            r#"{{"name":"projectatlas","version":"{}"}}"#,
            env!("CARGO_PKG_VERSION")
        ),
    )?;
    fs::write(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let fake_plugin_cache_json =
        serde_json::to_string(&fake_plugin_cache.to_string_lossy().to_string())?;
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_cache_json
    );
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {plugin_list_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  echo projectatlas\r\n  echo   command: C:\\stale\\ProjectAtlas\\bin\\projectatlas.exe\r\n  echo   args: --require-version 0.0.1 --db C:\\stale-repo\\.projectatlas\\projectatlas.db mcp\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{plugin_list_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  printf '%s\\n' 'projectatlas'\n  printf '%s\\n' '  command: /stale/ProjectAtlas/bin/projectatlas'\n  printf '%s\\n' '  args: --require-version 0.0.1 --db /stale-repo/.projectatlas/projectatlas.db mcp'\n  exit 0\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;
    let safe_stale_runtime = if cfg!(windows) {
        isolated_home
            .join("AppData")
            .join("Roaming")
            .join("npm")
            .join("projectatlas.cmd")
    } else {
        isolated_home
            .join(".cargo")
            .join("bin")
            .join("projectatlas")
    };
    fs::create_dir_all(
        safe_stale_runtime
            .parent()
            .ok_or_else(|| io::Error::other("safe stale runtime parent missing"))?,
    )?;
    fs::write(&safe_stale_runtime, stale_runtime_script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&safe_stale_runtime)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&safe_stale_runtime, permissions)?;
    }
    let non_project_runtime = if cfg!(windows) {
        isolated_home
            .join(".cargo")
            .join("bin")
            .join("projectatlas.cmd")
    } else {
        isolated_home.join(".npm").join("bin").join("projectatlas")
    };
    let non_project_script = if cfg!(windows) {
        "@echo off\r\necho {\"project\":\"NotProjectAtlas\",\"major_version\":3,\"version\":\"0.0.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n"
    } else {
        "#!/usr/bin/env sh\nprintf '%s\\n' '{\"project\":\"NotProjectAtlas\",\"major_version\":3,\"version\":\"0.0.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}'\n"
    };
    fs::create_dir_all(
        non_project_runtime
            .parent()
            .ok_or_else(|| io::Error::other("non-project runtime parent missing"))?,
    )?;
    fs::write(&non_project_runtime, non_project_script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&non_project_runtime)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&non_project_runtime, permissions)?;
    }
    fs::write(
        atlas_dir.join("projectatlas.mcp.json"),
        format!(
            r#"{{"mcpServers":{{"projectatlas":{{"command":{},"args":["--require-version","0.0.1","mcp"],"cwd":{}}}}}}}"#,
            serde_json::to_string(&stale_runtime_text)?,
            serde_json::to_string(&repo.to_string_lossy())?
        ),
    )?;
    fs::write(
        atlas_dir.join("projectatlas.claude.mcp.json"),
        format!(
            r#"{{"mcpServers":{{"projectatlas":{{"command":{},"args":["--require-version","0.0.1","mcp"]}}}}}}"#,
            serde_json::to_string(&stale_runtime_text)?
        ),
    )?;
    fs::write(
        atlas_dir.join("projectatlas.opencode.json"),
        format!(
            r#"{{"$schema":"https://opencode.ai/config.json","mcp":{{"projectatlas":{{"type":"local","enabled":true,"command":[{},"--require-version","0.0.1","mcp"],"cwd":{}}}}}}}"#,
            serde_json::to_string(&stale_runtime_text)?,
            serde_json::to_string(&repo.to_string_lossy())?
        ),
    )?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer_with_path_shadow_and_home(
        &workspace_root,
        &repo,
        &runtime,
        &stale_runtime_dir,
        &isolated_home,
    )?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output_text
        .contains("Active process resolves bare projectatlas to verified runtime")
    {
        return Err(io::Error::other(format!(
            "plugin update installer did not make its active process prefer the verified runtime:\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Codex MCP registry updated to ProjectAtlas runtime") {
        return Err(io::Error::other(format!(
            "plugin update installer did not repair stale global Codex MCP registry:\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains(&format!(
        "Codex ProjectAtlas plugin marketplace updated to {expected_release_tag}."
    )) {
        return Err(io::Error::other(format!(
            "plugin update installer did not repair stale Codex ProjectAtlas plugin marketplace:\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Codex ProjectAtlas plugin skill verified at") {
        return Err(io::Error::other(format!(
            "plugin update installer did not verify the refreshed Codex ProjectAtlas skill artifact:\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Stale ProjectAtlas workflow release pin")
        || !installer_output_text.contains("v0.0.1")
        || !installer_output_text.contains(&expected_release_tag)
    {
        return Err(io::Error::other(format!(
            "plugin update installer did not report stale downstream workflow release pins:\n{installer_output_text}"
        ))
        .into());
    }
    if installer_output_text.contains("v9.9.9") {
        return Err(io::Error::other(format!(
            "plugin update installer reported a non-official fork workflow release pin:\n{installer_output_text}"
        ))
        .into());
    }
    let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
    let required_codex_call_fragments = vec![
        "plugin marketplace list --json".to_string(),
        "plugin list --marketplace projectatlas --json".to_string(),
        "plugin marketplace remove projectatlas --json".to_string(),
        format!(
            "plugin marketplace add styler-ai/ProjectAtlas --ref {expected_release_tag} --json"
        ),
        "plugin remove projectatlas --marketplace projectatlas --json".to_string(),
        "plugin add projectatlas --marketplace projectatlas --json".to_string(),
        "mcp get projectatlas".to_string(),
        "mcp remove projectatlas".to_string(),
        "mcp add projectatlas --".to_string(),
        runtime.to_string_lossy().into_owned(),
        "--require-version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        "--db".to_string(),
        db.to_string_lossy().into_owned(),
        "--config".to_string(),
        atlas_dir.join("config.toml").to_string_lossy().into_owned(),
    ];
    for required in required_codex_call_fragments {
        if !fake_codex_calls.contains(&required) {
            return Err(io::Error::other(format!(
                "fake Codex MCP registry did not receive expected argument {required:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    let safe_stale_quarantine = stale_shim_quarantine_path(&safe_stale_runtime, "0.0.1");
    if !installer_output_text.contains("Quarantined stale ProjectAtlas shim") {
        return Err(io::Error::other(format!(
            "plugin update did not quarantine a known user-local stale shim:\n{installer_output_text}"
        ))
        .into());
    }
    if safe_stale_runtime.exists() {
        return Err(io::Error::other(format!(
            "known user-local stale shim was not moved out of PATH: {}",
            safe_stale_runtime.display()
        ))
        .into());
    }
    if !safe_stale_quarantine.exists() {
        return Err(io::Error::other(format!(
            "known user-local stale shim was not preserved at quarantine path: {}",
            safe_stale_quarantine.display()
        ))
        .into());
    }
    if !non_project_runtime.exists() {
        return Err(io::Error::other(format!(
            "installer removed a non-ProjectAtlas command from a known shim path: {}",
            non_project_runtime.display()
        ))
        .into());
    }
    if !stale_runtime.exists() {
        return Err(io::Error::other(
            "installer removed an unknown external PATH shadow instead of only warning",
        )
        .into());
    }
    if !installer_output_text.contains("Obsolete ProjectAtlas runtime")
        && !installer_output_text.contains("obsolete ProjectAtlas runtime")
    {
        return Err(io::Error::other(format!(
            "plugin update did not report shadowed stale runtime:\n{installer_output_text}"
        ))
        .into());
    }

    let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
    let claude_config = read_json_file(&atlas_dir.join("projectatlas.claude.mcp.json"))?;
    let opencode_config = read_json_file(&atlas_dir.join("projectatlas.opencode.json"))?;

    require_same_executable(
        json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "updated codex",
    )?;
    require_json_string(
        &codex_config,
        &["mcpServers", "projectatlas", "args", "1"],
        env!("CARGO_PKG_VERSION"),
    )?;
    require_same_executable(
        json_string_at(&claude_config, &["mcpServers", "projectatlas", "command"])?,
        &runtime,
        "updated claude",
    )?;
    require_json_string(
        &claude_config,
        &["mcpServers", "projectatlas", "args", "1"],
        env!("CARGO_PKG_VERSION"),
    )?;
    require_same_executable(
        json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "0"])?,
        &runtime,
        "updated opencode",
    )?;
    require_json_string(
        &opencode_config,
        &["mcp", "projectatlas", "command", "2"],
        env!("CARGO_PKG_VERSION"),
    )?;
    let codex_text = fs::read_to_string(atlas_dir.join("projectatlas.mcp.json"))?;
    if codex_text.contains("0.0.1") || codex_text.contains(stale_runtime_text.as_ref()) {
        return Err(
            io::Error::other("updated plugin config still contains stale runtime data").into(),
        );
    }
    if !atlas_dir.join("kept-state.txt").exists() {
        return Err(io::Error::other("plugin update removed existing project-local state").into());
    }
    if fs::read_to_string(atlas_dir.join("projectatlas-nonsource-files.toon"))?
        != "nonsource_files[]:\n  # path,summary\n"
    {
        return Err(io::Error::other("plugin update rewrote nonsource metadata").into());
    }
    let preserved_store = AtlasStore::open(&db)?;
    let token_calls_after = preserved_store.token_overview(None)?.calls;
    if token_calls_after < token_calls_before {
        return Err(io::Error::other(format!(
            "plugin update lost token telemetry: before {token_calls_before}, after {token_calls_after}"
        ))
        .into());
    }
    let nodes = preserved_store.load_nodes()?;
    let preserved_purpose = nodes
        .iter()
        .find(|node| node.node.path == "src/a.rs")
        .ok_or_else(|| io::Error::other("plugin update lost indexed source node"))?;
    if preserved_purpose.purpose.purpose.as_deref() != Some("Shared plugin update state")
        || preserved_purpose.purpose.source != PurposeSource::Agent
        || !preserved_purpose.purpose.agent_reviewed()
    {
        return Err(io::Error::other(format!(
            "plugin update lost approved purpose metadata: {:?}",
            preserved_purpose.purpose
        ))
        .into());
    }
    if !preserved_store
        .resolved_health_ids()?
        .contains(&preserved_health_id)
    {
        return Err(io::Error::other("plugin update lost health resolution metadata").into());
    }

    let (mcp_command, mcp_args) = mcp_command_and_args(&codex_config)?;
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-plugin-update-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
    ];
    let mcp_stdout = run_mcp_stdio(&mcp_command, &repo, &mcp_args, &messages)?;
    let expected_server_info = format!(
        r#""serverInfo":{{"name":"ProjectAtlas","version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    );
    if !mcp_stdout.contains(&expected_server_info) {
        return Err(io::Error::other(format!(
            "updated plugin MCP config did not launch current runtime: {mcp_stdout}"
        ))
        .into());
    }

    Ok(())
}

#[test]
fn plugin_update_skips_non_official_codex_marketplace() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let stale_plugin_json = r#"{"installed":[{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1"}],"available":[]}"#;
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://internal.example/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {stale_plugin_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://internal.example/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{stale_plugin_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer_with_path_shadow_and_home(
        &workspace_root,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output_text
        .contains("projectatlas marketplace is not the official styler-ai/ProjectAtlas source")
    {
        return Err(io::Error::other(format!(
            "installer did not explain the non-official marketplace skip:\n{installer_output_text}"
        ))
        .into());
    }
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    for forbidden in [
        "plugin marketplace remove projectatlas",
        "plugin marketplace add styler-ai/ProjectAtlas",
        "plugin remove projectatlas",
        "plugin add projectatlas",
    ] {
        if fake_codex_calls.contains(forbidden) {
            return Err(io::Error::other(format!(
                "non-official Codex marketplace was mutated by forbidden call {forbidden:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn plugin_update_leaves_current_codex_marketplace_untouched() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{expected_release_tag}\"\n"
        ),
    )?;
    let fake_plugin_cache = isolated_home
        .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
        .join("projectatlas");
    fs::create_dir_all(fake_plugin_cache.join(CODEX_PLUGIN_MANIFEST_DIR))?;
    fs::create_dir_all(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME),
    )?;
    fs::write(
        fake_plugin_cache
            .join(CODEX_PLUGIN_MANIFEST_DIR)
            .join("plugin.json"),
        format!(
            r#"{{"name":"projectatlas","version":"{}"}}"#,
            env!("CARGO_PKG_VERSION")
        ),
    )?;
    fs::write(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let fake_plugin_cache_json =
        serde_json::to_string(&fake_plugin_cache.to_string_lossy().to_string())?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_cache_json
    );
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {plugin_list_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{plugin_list_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer_with_path_shadow_and_home(
        &workspace_root,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output_text.contains(&format!(
        "Codex ProjectAtlas plugin marketplace already points to {expected_release_tag}."
    )) {
        return Err(io::Error::other(format!(
            "installer did not report the already-current marketplace/plugin state:\n{installer_output_text}"
        ))
        .into());
    }
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    for forbidden in [
        "plugin marketplace remove projectatlas",
        "plugin marketplace add styler-ai/ProjectAtlas",
        "plugin remove projectatlas",
        "plugin add projectatlas",
    ] {
        if fake_codex_calls.contains(forbidden) {
            return Err(io::Error::other(format!(
                "current Codex marketplace/plugin state was mutated by forbidden call {forbidden:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn plugin_update_repairs_current_codex_plugin_with_stale_source_manifest()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{expected_release_tag}\"\n"
        ),
    )?;
    let fake_plugin_cache = isolated_home
        .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
        .join("projectatlas");
    fs::create_dir_all(fake_plugin_cache.join(CODEX_PLUGIN_MANIFEST_DIR))?;
    fs::create_dir_all(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME),
    )?;
    let manifest_path = fake_plugin_cache
        .join(CODEX_PLUGIN_MANIFEST_DIR)
        .join("plugin.json");
    fs::write(
        &manifest_path,
        r#"{"name":"projectatlas","version":"0.0.1"}"#,
    )?;
    fs::write(
        fake_plugin_cache
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let fake_plugin_cache_json =
        serde_json::to_string(&fake_plugin_cache.to_string_lossy().to_string())?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_cache_json
    );
    let current_manifest_json = format!(
        r#"{{"name":"projectatlas","version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {plugin_list_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"add\" (\r\n  >\"%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%\" echo {current_manifest_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{plugin_list_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"add\" ]; then\n  printf '%s\\n' '{current_manifest_json}' > \"$PROJECTATLAS_FAKE_PLUGIN_MANIFEST\"\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer_with_path_shadow_and_home(
        &workspace_root,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    if !installer_output_text
        .contains("Codex ProjectAtlas plugin source manifest version '0.0.1' does not match")
        || !installer_output_text.contains(&format!(
            "Codex ProjectAtlas plugin marketplace updated to {expected_release_tag}."
        ))
        || !installer_output_text.contains("Codex ProjectAtlas plugin skill verified at")
    {
        return Err(io::Error::other(format!(
            "installer did not repair stale reported Codex plugin source manifest:\n{installer_output_text}"
        ))
        .into());
    }
    let manifest_after = fs::read_to_string(&manifest_path)?;
    if !manifest_after.contains(env!("CARGO_PKG_VERSION")) {
        return Err(io::Error::other(format!(
            "fake Codex plugin source manifest was not refreshed:\n{manifest_after}"
        ))
        .into());
    }
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    for required in [
        "plugin remove projectatlas --marketplace projectatlas",
        "plugin add projectatlas --marketplace projectatlas",
    ] {
        if !fake_codex_calls.contains(required) {
            return Err(io::Error::other(format!(
                "installer did not repair current-ref plugin source manifest with call {required:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    for forbidden in [
        "plugin marketplace remove projectatlas",
        "plugin marketplace add styler-ai/ProjectAtlas",
    ] {
        if fake_codex_calls.contains(forbidden) {
            return Err(io::Error::other(format!(
                "current-ref source manifest repair mutated marketplace by forbidden call {forbidden:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn plugin_update_restores_current_ref_marketplace_when_plugin_reinstall_fails()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{expected_release_tag}\"\n"
        ),
    )?;
    let failure_marker = isolated_home.join(FAKE_CODEX_PLUGIN_ADD_FAILURE_MARKER_FILE);
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let stale_plugin_json = r#"{"installed":[{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1"}],"available":[]}"#;
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" (\r\n  echo {stale_plugin_json}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" (\r\n  if exist \"%PROJECTATLAS_FAKE_FAILURE_MARKER%\" exit /b 0\r\n  echo failed>\"%PROJECTATLAS_FAKE_FAILURE_MARKER%\"\r\n  goto plugin_add_failure\r\n)\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n:plugin_add_failure\r\nexit /b 1\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{stale_plugin_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"add\" ]; then\n  if [ ! -f \"$PROJECTATLAS_FAKE_FAILURE_MARKER\" ]; then\n    printf '%s\\n' failed > \"$PROJECTATLAS_FAKE_FAILURE_MARKER\"\n    exit 1\n  fi\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_projectatlas_plugin_installer_with_path_shadow_and_home(
        &workspace_root,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&installer_output.stdout),
        String::from_utf8_lossy(&installer_output.stderr)
    );
    let normalized_installer_output = installer_output_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    if !failure_marker.exists() {
        return Err(io::Error::other(format!(
            "fake Codex plugin add failure was not exercised:\n{fake_codex_calls}"
        ))
        .into());
    }
    if !normalized_installer_output
        .contains("Codex ProjectAtlas plugin update failed: could not install projectatlas plugin")
    {
        return Err(io::Error::other(format!(
            "installer did not report the failed plugin reinstall:\n{installer_output_text}\nfake Codex calls:\n{fake_codex_calls}"
        ))
        .into());
    }
    if fake_codex_calls
        .matches("plugin add projectatlas --marketplace projectatlas --json")
        .count()
        < 2
    {
        return Err(io::Error::other(format!(
            "installer did not retry plugin add during restore:\n{fake_codex_calls}"
        ))
        .into());
    }
    let restore_marketplace_call = format!(
        "plugin marketplace add https://github.com/styler-ai/ProjectAtlas.git --ref {expected_release_tag} --json"
    );
    for required in [
        "plugin marketplace remove projectatlas --json",
        restore_marketplace_call.as_str(),
    ] {
        if !fake_codex_calls.contains(required) {
            return Err(io::Error::other(format!(
                "installer did not restore marketplace with call {required:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_without_codex_reports_clean_skip() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let system_root = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?,
    );
    let powershell_dir = system_root
        .join(WINDOWS_SYSTEM32_DIR)
        .join("WindowsPowerShell")
        .join("v1.0");
    let restricted_path = std::env::join_paths([
        system_root.join(WINDOWS_SYSTEM32_DIR),
        powershell_dir.clone(),
    ])?;
    let output = StdCommand::new(powershell_dir.join("powershell.exe"))
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(
            workspace_root()?
                .join("plugins")
                .join("projectatlas")
                .join("scripts")
                .join("install-runtime.ps1"),
        )
        .arg("-ProjectRoot")
        .arg(&repo)
        .arg("-RuntimePath")
        .arg(assert_cmd::cargo::cargo_bin("projectatlas"))
        .env("HOME", &isolated_home)
        .env("USERPROFILE", &isolated_home)
        .env("APPDATA", &app_data)
        .env("LOCALAPPDATA", &local_app_data)
        .env("PATH", restricted_path)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .env_remove("PROJECTATLAS_CODEX_COMMAND")
        .env_remove("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE")
        .env_remove("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE")
        .output()?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "clean-host installer failed without Codex:\n{output_text}"
        ))
        .into());
    }
    for required in [
        "Codex ProjectAtlas plugin update skipped: codex command not found.",
        "Codex MCP registry update skipped: codex command not found.",
    ] {
        if !output_text.contains(required) {
            return Err(io::Error::other(format!(
                "clean-host installer omitted {required:?}:\n{output_text}"
            ))
            .into());
        }
    }
    for forbidden in [
        "is not recognized as the name",
        "Codex ProjectAtlas plugin update failed",
        "Codex MCP registry update failed",
    ] {
        if output_text.contains(forbidden) {
            return Err(io::Error::other(format!(
                "clean-host installer emitted {forbidden:?}:\n{output_text}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_release_binary_installer_repairs_stale_mirror_without_registering_it()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let stable_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("bin")
        .join("projectatlas.exe");
    fs::create_dir_all(
        stable_runtime
            .parent()
            .ok_or_else(|| io::Error::other("stable runtime parent missing"))?,
    )?;
    fs::write(&stable_runtime, b"stale 0.3.10 stable mirror")?;

    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex = isolated_home.join("codex.cmd");
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  echo projectatlas\r\n  echo   command: C:\\Users\\shaun_tyler\\AppData\\Local\\ProjectAtlas\\bin\\projectatlas.exe\r\n  echo   args: --require-version 0.3.10 --db C:\\projects\\io.pasx.kai\\.projectatlas\\projectatlas.db mcp\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
    )?;

    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let release_archive = create_windows_release_archive(temp.path(), &runtime)?;
    let (release_base_url, release_server) = serve_release_assets(&release_archive, None)?;
    let workspace_root = workspace_root()?;
    let installer = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(&repo)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .arg("-ReleaseBaseUrl")
        .arg(&release_base_url)
        .arg("-ReleaseBinaryOnly")
        .env("HOME", &isolated_home)
        .env("USERPROFILE", &isolated_home)
        .env("APPDATA", &app_data)
        .env("LOCALAPPDATA", &local_app_data)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
        .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let server_result = release_server.join().map_err(|panic_payload| {
        let message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
            *message
        } else if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic payload"
        };
        io::Error::other(format!("release asset test server panicked: {message}"))
    })?;
    server_result?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "release-binary installer failed\n{installer_output_text}"
        ))
        .into());
    }

    let versioned_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("runtimes")
        .join(env!("CARGO_PKG_VERSION"))
        .join("x86_64-pc-windows-msvc")
        .join("projectatlas.exe");
    if !versioned_runtime.exists() {
        return Err(io::Error::other(format!(
            "release binary was not installed to the versioned runtime path: {}",
            versioned_runtime.display()
        ))
        .into());
    }
    for runtime_path in [&versioned_runtime, &stable_runtime] {
        let runtime_info = StdCommand::new(runtime_path)
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--format")
            .arg("json")
            .arg("runtime-info")
            .output()?;
        if !runtime_info.status.success() {
            return Err(io::Error::other(format!(
                "runtime failed runtime-info after install: {}\n{}",
                runtime_path.display(),
                String::from_utf8_lossy(&runtime_info.stderr)
            ))
            .into());
        }
    }

    let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
    require_same_executable(
        json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
        &versioned_runtime,
        "repaired mirror codex",
    )?;
    let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
    if !fake_codex_calls.contains("mcp add projectatlas --")
        || !fake_codex_calls.contains(versioned_runtime.to_string_lossy().as_ref())
        || fake_codex_calls.contains(stable_runtime.to_string_lossy().as_ref())
    {
        return Err(io::Error::other(format!(
            "Codex MCP registry was not repaired to the versioned runtime:\n{fake_codex_calls}"
        ))
        .into());
    }
    if !installer_output_text.contains("Codex MCP registry updated to ProjectAtlas runtime") {
        return Err(io::Error::other(format!(
            "installer did not report Codex registry repair:\n{installer_output_text}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_release_binary_installer_rejects_checksum_mismatch() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let release_archive = create_windows_release_archive(temp.path(), &runtime)?;
    let wrong_hash = "0".repeat(64);
    let (release_base_url, release_server) =
        serve_release_assets(&release_archive, Some(wrong_hash.as_str()))?;
    let workspace_root = workspace_root()?;
    let installer = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(&repo)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .arg("-ReleaseBaseUrl")
        .arg(&release_base_url)
        .arg("-ReleaseBinaryOnly")
        .env("HOME", &isolated_home)
        .env("USERPROFILE", &isolated_home)
        .env("APPDATA", &app_data)
        .env("LOCALAPPDATA", &local_app_data)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let server_result = release_server.join().map_err(|panic_payload| {
        let message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
            *message
        } else if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic payload"
        };
        io::Error::other(format!("release asset test server panicked: {message}"))
    })?;
    server_result?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        return Err(io::Error::other(format!(
            "release-binary installer accepted a checksum mismatch\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Checksum mismatch") {
        return Err(io::Error::other(format!(
            "installer failure did not report checksum mismatch\n{installer_output_text}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_release_binary_only_rejects_invalid_runtime_without_fallback()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let stable_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("bin")
        .join("projectatlas.exe");
    fs::create_dir_all(
        stable_runtime
            .parent()
            .ok_or_else(|| io::Error::other("stable runtime parent missing"))?,
    )?;
    let valid_runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    fs::copy(&valid_runtime, &stable_runtime)?;

    let invalid_runtime = temp.path().join("invalid-projectatlas.exe");
    fs::write(
        &invalid_runtime,
        b"this is not a valid ProjectAtlas Windows executable",
    )?;
    let release_archive = create_windows_release_archive(temp.path(), &invalid_runtime)?;
    let (release_base_url, release_server) = serve_release_assets(&release_archive, None)?;
    let workspace_root = workspace_root()?;
    let installer = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(&repo)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .arg("-ReleaseBaseUrl")
        .arg(&release_base_url)
        .arg("-ReleaseBinaryOnly")
        .env("HOME", &isolated_home)
        .env("USERPROFILE", &isolated_home)
        .env("APPDATA", &app_data)
        .env("LOCALAPPDATA", &local_app_data)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let server_result = release_server.join().map_err(|panic_payload| {
        let message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
            *message
        } else if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic payload"
        };
        io::Error::other(format!("release asset test server panicked: {message}"))
    })?;
    server_result?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        return Err(io::Error::other(format!(
            "release-binary installer fell back to an ambient runtime after installing an invalid asset\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("produced an invalid runtime") {
        return Err(io::Error::other(format!(
            "installer failure did not report invalid release runtime\n{installer_output_text}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn posix_release_binary_installer_rejects_checksum_mismatch() -> Result<(), Box<dyn Error>> {
    let Some(_suffix) = posix_release_suffix() else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    fs::create_dir_all(&isolated_home)?;

    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let release_archive = create_posix_release_archive(temp.path(), &runtime)?;
    let wrong_hash = "0".repeat(64);
    let (release_base_url, release_server) =
        serve_release_assets(&release_archive, Some(wrong_hash.as_str()))?;
    let workspace_root = workspace_root()?;
    let installer = workspace_root
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.sh");
    let output = StdCommand::new("bash")
        .arg(installer)
        .arg(&repo)
        .env(
            "PROJECTATLAS_VERSION",
            format!("v{}", env!("CARGO_PKG_VERSION")),
        )
        .env("PROJECTATLAS_RELEASE_BASE_URL", &release_base_url)
        .env("PROJECTATLAS_RELEASE_BINARY_ONLY", "1")
        .env("HOME", &isolated_home)
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let server_result = release_server.join().map_err(|panic_payload| {
        let message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
            *message
        } else if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic payload"
        };
        io::Error::other(format!("release asset test server panicked: {message}"))
    })?;
    server_result?;
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        return Err(io::Error::other(format!(
            "release-binary installer accepted a checksum mismatch\n{installer_output_text}"
        ))
        .into());
    }
    if !installer_output_text.contains("Checksum mismatch") {
        return Err(io::Error::other(format!(
            "installer failure did not report checksum mismatch\n{installer_output_text}"
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
fn explicit_database_binding_is_used_by_cli_and_mcp_admin_surfaces() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    let config_path = atlas_dir.join("config.toml");
    let selected_database = temp.path().join("selected-projectatlas.db");
    let canonical_database = atlas_dir.join("projectatlas.db");
    let canonical_sentinel = b"protected canonical database sentinel";
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        &config_path,
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\n",
    )?;
    fs::write(
        atlas_dir.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "pub fn selected_database_marker() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&selected_database)
        .arg("--config")
        .arg(&config_path)
        .args(["scan", "."])
        .assert()
        .success();
    if canonical_database.exists() {
        return Err(io::Error::other("explicit scan created the canonical database").into());
    }
    fs::write(&canonical_database, canonical_sentinel)?;

    let default_config_output = Command::cargo_bin("projectatlas")?
        .current_dir(temp.path())
        .arg("--format")
        .arg("json")
        .arg("--config")
        .arg(&config_path)
        .args(["config", "--print"])
        .output()?;
    if !default_config_output.status.success() {
        return Err(io::Error::other(format!(
            "config-root database resolution failed: {}",
            String::from_utf8_lossy(&default_config_output.stderr)
        ))
        .into());
    }
    let default_config_json: Value = serde_json::from_slice(&default_config_output.stdout)?;
    require_json_string(
        &default_config_json,
        &["db_path"],
        canonical_database.to_string_lossy().as_ref(),
    )?;

    let config_output = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&selected_database)
        .arg("--config")
        .arg(&config_path)
        .args(["config", "--print"])
        .output()?;
    if !config_output.status.success() {
        return Err(io::Error::other(format!(
            "config --print rejected the selected database: {}",
            String::from_utf8_lossy(&config_output.stderr)
        ))
        .into());
    }
    let config_json: Value = serde_json::from_slice(&config_output.stdout)?;
    require_json_string(
        &config_json,
        &["db_path"],
        selected_database.to_string_lossy().as_ref(),
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&selected_database)
        .arg("--config")
        .arg(&config_path)
        .args(["map", "--force"])
        .assert()
        .success();
    let map = fs::read_to_string(atlas_dir.join("projectatlas.toon"))?;
    if !map.contains("src/main.rs") {
        return Err(io::Error::other("selected-database map omitted indexed source").into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&selected_database)
        .arg("--config")
        .arg(&config_path)
        .args(["lint", "--report-untracked", "--purpose-level", "low"])
        .assert()
        .success();

    let mcp_config = mcp_config_for_harness(&repo, &selected_database, "mcp-json")?;
    let (mcp_command, mcp_args) = mcp_command_and_args(&mcp_config)?;
    let mcp_output = run_mcp_stdio(
        &mcp_command,
        temp.path(),
        &mcp_args,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_config","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_map","arguments":{"force":true}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_lint","arguments":{"report_untracked":true,"purpose_level":"low"}}}"#,
        ],
    )?;
    let mcp_config_text = mcp_tool_text(&mcp_output, 2)?;
    let selected_database_toon = selected_database.to_string_lossy().replace('\\', "\\\\");
    if !mcp_config_text.contains(&selected_database_toon) {
        return Err(io::Error::other(format!(
            "MCP config did not report the selected database: {mcp_config_text}"
        ))
        .into());
    }
    let mcp_map_text = mcp_tool_text(&mcp_output, 3)?;
    if !mcp_map_text.contains("written: true") {
        return Err(io::Error::other(format!(
            "MCP map did not use the selected database: {mcp_map_text}"
        ))
        .into());
    }
    let mcp_lint_text = mcp_tool_text(&mcp_output, 4)?;
    if !mcp_lint_text.contains("ok: true") {
        return Err(io::Error::other(format!(
            "MCP lint did not use the selected database: {mcp_lint_text}"
        ))
        .into());
    }

    if fs::read(&canonical_database)? != canonical_sentinel {
        return Err(io::Error::other("admin surfaces mutated the canonical database").into());
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if sqlite_sidecar_path(&canonical_database, suffix).exists() {
            return Err(io::Error::other(format!(
                "admin surfaces created canonical database sidecar {suffix}"
            ))
            .into());
        }
    }
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
    require_json_i64_greater_than(&token_json, &["tokens_avoided"], 0)?;
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
    let tokens_avoided = token_json["tokens_avoided"]
        .as_i64()
        .ok_or_else(|| io::Error::other("tokens_avoided missing"))?;
    if measured_saved.saturating_add(deduped_modeled_saved) != tokens_avoided {
        return Err(io::Error::other(format!(
            "tokens_avoided does not reconcile: {measured_saved} + {deduped_modeled_saved} != {tokens_avoided}"
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
    let calibrated_token = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["token", "--tokenizer", "o200k_base"])
        .output()?;
    if !calibrated_token.status.success() {
        return Err(io::Error::other("json token calibration command failed").into());
    }
    let calibrated_json: Value = serde_json::from_slice(&calibrated_token.stdout)?;
    require_json_string(
        &calibrated_json,
        &["calibration", "tokenizer"],
        "o200k_base",
    )?;
    require_json_usize_greater_than(&calibrated_json, &["calibration", "files"], 0)?;
    require_json_usize_greater_than(&calibrated_json, &["calibration", "calibrated_tokens"], 0)?;
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
            "T O T A L   T O K E N S   A V O I D E D",
        ))
        .stdout(predicate::str::contains("Without ProjectAtlas"))
        .stdout(predicate::str::contains("With ProjectAtlas"))
        .stdout(predicate::str::contains("Saved by ProjectAtlas"))
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

#[test]
fn root_and_metadata_validation_flow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("a.rs"), "pub fn a() {}\n")?;
    fs::write(repo.join(SRC_DIR_NAME).join("b.rs"), "pub fn b() {}\n")?;

    let root_set = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&repo)
        .output()?;
    if !root_set.status.success() {
        return Err(io::Error::other(format!(
            "root set failed: {}",
            String::from_utf8_lossy(&root_set.stderr)
        ))
        .into());
    }
    let root_set_json: Value = serde_json::from_slice(&root_set.stdout)?;
    require_json_string(&root_set_json, &["transition"], "bind")?;
    require_json_bool(&root_set_json, &["identity_changed"], true)?;
    require_json_bool(&root_set_json, &["publication_invalidated"], false)?;
    let source_identity = root_set_json["project_instance_id"]
        .as_str()
        .filter(|identity| !identity.is_empty())
        .ok_or_else(|| io::Error::other("root set did not report project identity"))?
        .to_string();

    let db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let config = repo.join(ATLAS_DIR_NAME).join("config.toml");
    let root_show = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(&config)
        .args(["root", "show"])
        .output()?;
    if !root_show.status.success() {
        return Err(io::Error::other("root show failed").into());
    }
    let root_json: Value = serde_json::from_slice(&root_show.stdout)?;
    require_json_bool(&root_json, &["verified"], true)?;
    require_json_string(&root_json, &["detection_source"], "config")?;
    require_json_string(&root_json, &["project_instance_id"], &source_identity)?;

    let copied_repo = temp.path().join("copied-repo");
    let copied_atlas_dir = copied_repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&copied_atlas_dir)?;
    let copied_db = copied_atlas_dir.join("projectatlas.db");
    fs::copy(&db, &copied_db)?;
    let copied_config = copied_atlas_dir.join("config.toml");

    Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&copied_repo)
        .assert()
        .failure()
        .stderr(predicate::str::contains("project_mismatch"));
    if copied_config.exists()
        || copied_atlas_dir.join("projectatlas.mcp.json").exists()
        || copied_atlas_dir
            .join("projectatlas.claude.mcp.json")
            .exists()
        || copied_atlas_dir.join("projectatlas.opencode.json").exists()
    {
        return Err(io::Error::other(
            "rejected copied-database bind created destination config files",
        )
        .into());
    }

    let blocked_mcp_config = copied_atlas_dir.join("projectatlas.mcp.json");
    fs::create_dir(&blocked_mcp_config)?;
    let detached = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&copied_repo)
        .args(["--transition", "detach"])
        .output()?;
    if detached.status.success() {
        return Err(io::Error::other(
            "detach unexpectedly succeeded through a blocked generated config path",
        )
        .into());
    }
    let detach_error = String::from_utf8_lossy(&detached.stderr);
    for required in [
        "root transition Detach committed",
        "generated project configuration is incomplete",
        "default bind transition",
    ] {
        if !detach_error.contains(required) {
            return Err(io::Error::other(format!(
                "partial detach failure omitted {required:?}: {detach_error}"
            ))
            .into());
        }
    }
    let committed_show = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&copied_db)
        .arg("--config")
        .arg(&copied_config)
        .args(["root", "show"])
        .output()?;
    if !committed_show.status.success() {
        return Err(io::Error::other(format!(
            "committed detach could not be inspected: {}",
            String::from_utf8_lossy(&committed_show.stderr)
        ))
        .into());
    }
    let committed_json: Value = serde_json::from_slice(&committed_show.stdout)?;
    let detached_identity = committed_json["project_instance_id"]
        .as_str()
        .ok_or_else(|| io::Error::other("committed detach did not retain project identity"))?
        .to_string();
    if detached_identity == source_identity {
        return Err(io::Error::other("detach preserved the copied project identity").into());
    }
    fs::remove_dir(&blocked_mcp_config)?;
    let repaired = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&copied_repo)
        .output()?;
    if !repaired.status.success() {
        return Err(io::Error::other(format!(
            "bind repair after committed detach failed: {}",
            String::from_utf8_lossy(&repaired.stderr)
        ))
        .into());
    }
    let repaired_json: Value = serde_json::from_slice(&repaired.stdout)?;
    require_json_string(&repaired_json, &["transition"], "bind")?;
    require_json_bool(&repaired_json, &["identity_changed"], false)?;
    require_json_bool(&repaired_json, &["publication_invalidated"], false)?;
    require_json_string(&repaired_json, &["project_instance_id"], &detached_identity)?;
    for generated in [
        copied_config.clone(),
        blocked_mcp_config,
        copied_atlas_dir.join("projectatlas.claude.mcp.json"),
        copied_atlas_dir.join("projectatlas.opencode.json"),
    ] {
        if !generated.exists() {
            return Err(io::Error::other(format!(
                "detach did not generate {}",
                generated.display()
            ))
            .into());
        }
    }
    let copied_root_show = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&copied_db)
        .arg("--config")
        .arg(&copied_config)
        .args(["root", "show"])
        .output()?;
    if !copied_root_show.status.success() {
        return Err(io::Error::other("detached root show failed").into());
    }
    let copied_root_json: Value = serde_json::from_slice(&copied_root_show.stdout)?;
    require_json_bool(&copied_root_json, &["verified"], true)?;
    require_json_string(
        &copied_root_json,
        &["project_instance_id"],
        &detached_identity,
    )?;
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(&config)
        .arg("root")
        .assert()
        .success()
        .stdout(predicate::str::contains("root:"))
        .stdout(predicate::str::contains("detection_source: config"));

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(&config)
        .args(["scan"])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["purpose", "set", "no/such/file.rs", "Missing file"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not indexed"))
        .stderr(predicate::str::contains("sqlite error").not());

    for file in ["src/a.rs", "src/b.rs"] {
        Command::cargo_bin("projectatlas")?
            .arg("--db")
            .arg(&db)
            .args(["purpose", "set", file, "Shared purpose"])
            .assert()
            .success();
    }
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args([
            "health",
            "resolve",
            "missing-id",
            "duplicate-purpose",
            "no/such/file.rs",
            "--rationale",
            "typo",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not active"));

    let other_repo = temp.path().join("other-repo");
    fs::create_dir(&other_repo)?;
    Command::cargo_bin("projectatlas")?
        .args(["root", "set"])
        .arg(&other_repo)
        .assert()
        .success();
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(other_repo.join(ATLAS_DIR_NAME).join("projectatlas.db"))
        .arg("--config")
        .arg(&config)
        .args(["root", "verify"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("mismatches"));

    let other_config = other_repo.join(ATLAS_DIR_NAME).join("config.toml");
    let copied_db = temp.path().join("copied-projectatlas.db");
    fs::copy(&db, &copied_db)?;
    Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&copied_db)
        .arg("--config")
        .arg(&other_config)
        .args(["purpose", "set", "src/a.rs", "Wrong project mutation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("project_mismatch"));

    let moved_repo = temp.path().join("moved-test-repo");
    fs::rename(&repo, &moved_repo)?;
    let moved_db = moved_repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let moved_config = moved_repo.join(ATLAS_DIR_NAME).join("config.toml");
    let moved_root = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&moved_repo)
        .args(["--transition", "move"])
        .output()?;
    if !moved_root.status.success() {
        return Err(io::Error::other(format!(
            "identity-preserving CLI move failed: {}",
            String::from_utf8_lossy(&moved_root.stderr)
        ))
        .into());
    }
    let moved_json: Value = serde_json::from_slice(&moved_root.stdout)?;
    require_json_string(&moved_json, &["transition"], "move")?;
    require_json_string(&moved_json, &["project_instance_id"], &source_identity)?;
    require_json_bool(&moved_json, &["identity_changed"], false)?;
    require_json_bool(&moved_json, &["publication_invalidated"], true)?;
    if !fs::read_to_string(&moved_config)?.contains("root = \".\"") {
        return Err(
            io::Error::other("moved relative config no longer remained relocatable").into(),
        );
    }

    let moved_atlas_dir = moved_repo.join(ATLAS_DIR_NAME);
    let codex_config = read_json_file(&moved_atlas_dir.join("projectatlas.mcp.json"))?;
    let claude_config = read_json_file(&moved_atlas_dir.join("projectatlas.claude.mcp.json"))?;
    let opencode_config = read_json_file(&moved_atlas_dir.join("projectatlas.opencode.json"))?;
    for (actual, expected, label) in [
        (
            json_string_at(&codex_config, &["mcpServers", "projectatlas", "args", "3"])?,
            moved_db.as_path(),
            "moved codex database",
        ),
        (
            json_string_at(&codex_config, &["mcpServers", "projectatlas", "args", "5"])?,
            moved_config.as_path(),
            "moved codex config",
        ),
        (
            json_string_at(&claude_config, &["mcpServers", "projectatlas", "args", "3"])?,
            moved_db.as_path(),
            "moved Claude database",
        ),
        (
            json_string_at(&claude_config, &["mcpServers", "projectatlas", "args", "5"])?,
            moved_config.as_path(),
            "moved Claude config",
        ),
        (
            json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "4"])?,
            moved_db.as_path(),
            "moved OpenCode database",
        ),
        (
            json_string_at(&opencode_config, &["mcp", "projectatlas", "command", "6"])?,
            moved_config.as_path(),
            "moved OpenCode config",
        ),
    ] {
        require_same_canonical_path(actual, expected, label)?;
    }
    require_same_directory(
        json_string_at(&codex_config, &["mcpServers", "projectatlas", "cwd"])?,
        &moved_repo,
        "moved codex cwd",
    )?;
    require_same_directory(
        json_string_at(&opencode_config, &["mcp", "projectatlas", "cwd"])?,
        &moved_repo,
        "moved OpenCode cwd",
    )?;

    let moved_show = Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&moved_db)
        .arg("--config")
        .arg(&moved_config)
        .args(["root", "show"])
        .output()?;
    if !moved_show.status.success() {
        return Err(io::Error::other("moved root show failed").into());
    }
    let moved_show_json: Value = serde_json::from_slice(&moved_show.stdout)?;
    require_json_bool(&moved_show_json, &["verified"], true)?;
    require_json_string(&moved_show_json, &["project_instance_id"], &source_identity)?;
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&moved_db)
        .arg("--config")
        .arg(&moved_config)
        .arg("scan")
        .assert()
        .success();

    Ok(())
}

#[test]
fn mcp_server_stays_bound_to_one_project_database() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    let db_a = temp.path().join("repo-a.db");
    let db_b = temp.path().join("repo-b.db");
    for (repo, marker) in [(&repo_a, "alpha_marker"), (&repo_b, "beta_marker")] {
        fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
        fs::write(
            repo.join(SRC_DIR_NAME).join("main.rs"),
            format!("pub fn {marker}() -> &'static str {{\n    \"{marker}\"\n}}\n"),
        )?;
    }

    for (repo, db) in [(&repo_a, &db_a), (&repo_b, &db_b)] {
        Command::cargo_bin("projectatlas")?
            .current_dir(repo)
            .arg("--db")
            .arg(db)
            .args(["scan", "."])
            .assert()
            .success();
    }

    let config_a = mcp_config_for_harness(&repo_a, &db_a, "mcp-json")?;
    let (command_a, args_a) = mcp_command_and_args(&config_a)?;
    let outside_purpose = format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"atlas_purpose_set","arguments":{{"path":{},"purpose":"Wrong repository file."}}}}}}"#,
        serde_json::to_string(&repo_b.join(SRC_DIR_NAME).join("main.rs").to_string_lossy())?
    );
    let messages_a = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":5}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/main.rs","start_line":1,"end_line":3}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{}}}"#,
        outside_purpose.as_str(),
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_purpose_set","arguments":{"path":".","purpose":"Repository root for repo A."}}}"#,
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"atlas_purpose_set","arguments":{"path":"","purpose":"Empty path should fail."}}}"#,
    ];
    let output_a = run_mcp_stdio(&command_a, &repo_b, &args_a, &messages_a)?;
    if !output_a.contains("alpha_marker") {
        return Err(io::Error::other(format!(
            "repo A MCP server did not return repo A marker when launched from repo B cwd: {output_a}"
        ))
        .into());
    }
    if output_a.contains("beta_marker") {
        return Err(
            io::Error::other(format!("repo A MCP server leaked repo B data: {output_a}")).into(),
        );
    }
    if !output_a.contains("token_savings:")
        || !output_a.contains("estimate_kind: heuristic")
        || !output_a.contains("detail_availability: retained")
        || !output_a.contains("read_avoidance:")
        || !output_a.contains("likely_file_reads_avoided")
    {
        return Err(io::Error::other(format!(
            "repo A MCP token report did not include heuristic read-avoidance telemetry: {output_a}"
        ))
        .into());
    }
    if !output_a.contains("absolute paths are not allowed") {
        return Err(io::Error::other(format!(
            "repo A MCP purpose_set did not reject an outside absolute path: {output_a}"
        ))
        .into());
    }
    if !output_a.contains("purpose_set:")
        || !output_a.contains("path: .")
        || !output_a.contains("source: agent")
        || !output_a.contains("agent_reviewed: true")
    {
        return Err(io::Error::other(format!(
            "repo A MCP purpose_set did not accept explicit repository root: {output_a}"
        ))
        .into());
    }
    if !output_a.contains("a path is required") {
        return Err(io::Error::other(format!(
            "repo A MCP purpose_set did not reject an empty path: {output_a}"
        ))
        .into());
    }

    let config_b = mcp_config_for_harness(&repo_b, &db_b, "mcp-json")?;
    let (command_b, args_b) = mcp_command_and_args(&config_b)?;
    let messages_b = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":5}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/main.rs","start_line":1,"end_line":3}}}"#,
    ];
    let output_b = run_mcp_stdio(&command_b, &repo_a, &args_b, &messages_b)?;
    if !output_b.contains("beta_marker") || output_b.contains("alpha_marker") {
        return Err(io::Error::other(format!(
            "repo B MCP server did not stay bound to repo B when launched from repo A cwd: {output_b}"
        ))
        .into());
    }

    Ok(())
}

#[test]
fn no_telemetry_readonly_cli_smoke() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "pub fn main_entry() -> &'static str {\n    \"atlas\"\n}\n",
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
        (".", "Repository root for no-telemetry CLI smoke."),
        (
            SRC_DIR_NAME,
            "Rust source folder for no-telemetry CLI smoke.",
        ),
        (
            "src/main.rs",
            "Rust source file for no-telemetry CLI smoke.",
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

    let calls_before = token_call_count(&repo, &db)?;
    for args in [
        vec!["overview"],
        vec!["folders", SRC_DIR_NAME, "--limit", "5"],
        vec!["files", "main", "--folder", SRC_DIR_NAME, "--limit", "5"],
        vec!["summary", "src/main.rs", "--limit", "5"],
        vec![
            "search",
            "main_entry",
            "--file-pattern",
            "src/*.rs",
            "--limit",
            "5",
        ],
        vec!["parity", "report", "--profile", "repository-intelligence"],
        vec!["parity", "--profile", "repository-intelligence"],
        vec!["token"],
        vec!["token", "--view", "tui"],
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .arg("--db")
            .arg(&db)
            .args(args)
            .assert()
            .success();
    }
    let calls_after = token_call_count(&repo, &db)?;
    if calls_before != calls_after {
        return Err(io::Error::other(format!(
            "read-only no-telemetry smoke mutated token calls: before {calls_before}, after {calls_after}"
        ))
        .into());
    }

    let purpose_review = temp.path().join("purpose-review.json");
    fs::write(
        &purpose_review,
        serde_json::to_string_pretty(&serde_json::json!({
            "items": [{
                "path": "src/main.rs",
                "purpose": "Rust source file for no-telemetry CLI smoke."
            }]
        }))?,
    )?;
    let mcp_config = mcp_config_for_harness(&repo, &db, "mcp-json")?;
    let (mcp_command, mcp_args) = mcp_command_and_args(&mcp_config)?;
    let connection = Connection::open(&db)?;
    // Stamp the current layout as a predecessor to prove every read-only adapter
    // rejects schema lookalikes without migrating or repairing them.
    connection.execute(
        "UPDATE metadata SET value = '8' WHERE key = 'schema_version'",
        [],
    )?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    drop(connection);
    let incompatible_schema_bytes = fs::read(&db)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        if sqlite_sidecar_path(&db, suffix).exists() {
            return Err(io::Error::other("schema-lookalike fixture retained a sidecar").into());
        }
    }

    for args in [
        vec!["settings"],
        vec!["token"],
        vec!["parity", "report", "--profile", "repository-intelligence"],
        vec!["lint", "--purpose-level", "low"],
    ] {
        Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .arg("--db")
            .arg(&db)
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("incompatible schema object"));
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&db)
        .args(["purpose", "review", "--from-file"])
        .arg(&purpose_review)
        .assert()
        .failure()
        .stderr(predicate::str::contains("incompatible schema object"));

    let mcp_messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_parity_report","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_purpose_review","arguments":{"items":[{"path":"src/main.rs","purpose":"Rust source file for no-telemetry CLI smoke."}]}}}"#,
    ];
    let mcp_output = run_mcp_stdio(&mcp_command, &repo, &mcp_args, &mcp_messages)?;
    if mcp_output.matches("incompatible schema object").count() < 3 {
        return Err(io::Error::other(format!(
            "MCP pure reports did not refuse a schema lookalike: {mcp_output}"
        ))
        .into());
    }
    if fs::read(&db)? != incompatible_schema_bytes {
        return Err(io::Error::other("pure reports migrated or rewrote a schema lookalike").into());
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if sqlite_sidecar_path(&db, suffix).exists() {
            return Err(io::Error::other("pure report created a SQLite sidecar").into());
        }
    }
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
fn mcp_tools_list_exposes_self_contained_codex_input_schemas_without_index_state()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let executable = mcp_contract_executable();

    let inventory = run_mcp_contract_inventory(&executable, &repo, &database)?;
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
fn symbols_watch_and_legacy_cleanup_flow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub struct Atlas;\n\nimpl Atlas {\n    pub fn sail(&self) {\n        helper();\n    }\n}\n\nfn helper() {}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(".purpose"),
        "Rust source folder\n",
    )?;
    fs::create_dir_all(repo.join("node_modules").join("pkg"))?;
    fs::write(
        repo.join("node_modules").join("pkg").join(".purpose"),
        "Ignored dependency purpose\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("symbols:"))
        .stdout(predicate::str::contains("purpose_suggestions:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Atlas"))
        .stdout(predicate::str::contains("helper"));

    let legacy_default = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "relations", "--file", "src/lib.rs"])
        .output()?;
    if !legacy_default.status.success()
        || !String::from_utf8_lossy(&legacy_default.stdout).contains("helper")
    {
        return Err(io::Error::other("default legacy relation command failed").into());
    }
    let legacy_explicit = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "relations",
            "--view",
            "legacy",
            "--file",
            "src/lib.rs",
        ])
        .output()?;
    if !legacy_explicit.status.success() || legacy_default.stdout != legacy_explicit.stdout {
        return Err(io::Error::other(
            "explicit legacy relation view changed default output bytes or ordering",
        )
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "relations",
            "--file",
            "src/lib.rs",
            "--limit",
            "0",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("relations[1]"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/lib.rs",
            "--symbol",
            "sail",
            "--relation",
            "calls",
            "--include-occurrences",
            "--depth",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol_relations\""))
        .stdout(predicate::str::contains("helper"))
        .stdout(predicate::str::contains("symbol_slice"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args([
            "symbols",
            "build",
            ".",
            "--max-workers",
            "2",
            "--timeout-seconds",
            "30",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("max_workers: 1"))
        .stdout(predicate::str::contains("timeout_seconds: 30"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "slice", "src/lib.rs", "sail"])
        .assert()
        .success()
        .stdout(predicate::str::contains("helper();"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("watch:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["strip-legacy-purpose", ".", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/.purpose"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["strip-legacy-purpose", ".", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_files_removed: 1"));
    if repo.join(SRC_DIR_NAME).join(".purpose").exists() {
        return Err(io::Error::other("legacy .purpose file was not removed").into());
    }
    if !repo
        .join("node_modules")
        .join("pkg")
        .join(".purpose")
        .exists()
    {
        return Err(io::Error::other("excluded .purpose file was removed").into());
    }
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
fn packaged_cli_surface_preserves_v0326_routes_and_defaults() -> Result<(), Box<dyn Error>> {
    let executable = mcp_contract_executable();
    assert_mcp_contract_runtime_and_skill(&executable)?;
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cli-surfaces.json"))?;
    let current_key = format!("v{}", env!("CARGO_PKG_VERSION"));
    let current = fixture
        .get(&current_key)
        .ok_or_else(|| io::Error::other(format!("CLI fixture omitted {current_key}")))?;
    let legacy = json_at(&fixture, &["v0.3.26"])?;
    let current_commands = cli_surface_strings(current, &["commands"])?;
    let root_help = cli_help_surface(&executable, "")?;
    if !root_help.help_present {
        return Err(io::Error::other("packaged CLI root omitted Clap's help command").into());
    }
    let advertised_commands = root_help.subcommands;
    if advertised_commands != current_commands {
        return Err(io::Error::other(format!(
            "packaged CLI command inventory drifted: advertised={advertised_commands:?} expected={current_commands:?}"
        ))
        .into());
    }

    let current_subcommands = json_at(current, &["subcommands"])?
        .as_object()
        .ok_or_else(|| io::Error::other("current CLI subcommands fixture was not an object"))?;
    for (parent, expected) in current_subcommands {
        let expected = cli_value_strings(expected, parent)?;
        let help = cli_help_surface(&executable, parent)?;
        if !help.help_present {
            return Err(io::Error::other(format!(
                "packaged CLI group {parent} omitted Clap's help command"
            ))
            .into());
        }
        let advertised = help.subcommands;
        if advertised != expected {
            return Err(io::Error::other(format!(
                "packaged CLI subcommand inventory drifted for {parent}: advertised={advertised:?} expected={expected:?}"
            ))
            .into());
        }
    }
    let current_actions = json_at(current, &["actions"])?
        .as_object()
        .ok_or_else(|| io::Error::other("current CLI actions fixture was not an object"))?;
    for (route, expected) in current_actions {
        let expected = cli_value_strings(expected, route)?;
        let advertised = cli_help_surface(&executable, route)?.possible_values;
        if advertised != expected {
            return Err(io::Error::other(format!(
                "packaged CLI positional action inventory drifted for {route}: advertised={advertised:?} expected={expected:?}"
            ))
            .into());
        }
    }

    let current_defaults = json_at(current, &["defaults"])?
        .as_object()
        .ok_or_else(|| io::Error::other("current CLI defaults fixture was not an object"))?;
    let mut current_routes = vec![String::new()];
    for command in &current_commands {
        current_routes.push(command.clone());
        if let Some(subcommands) = current_subcommands.get(command) {
            for subcommand in cli_value_strings(subcommands, command)? {
                current_routes.push(format!("{command} {subcommand}"));
            }
        }
    }
    for route in &current_routes {
        let expected = current_defaults
            .get(route)
            .map(|value| cli_value_strings(value, route))
            .transpose()?
            .unwrap_or_default();
        let advertised = cli_help_surface(&executable, route)?.defaults;
        if advertised != expected {
            return Err(io::Error::other(format!(
                "packaged CLI defaults drifted for {route:?}: advertised={advertised:?} expected={expected:?}"
            ))
            .into());
        }
    }

    let legacy_commands = cli_surface_strings(legacy, &["commands"])?;
    if !legacy_commands
        .iter()
        .all(|command| current_commands.contains(command))
    {
        return Err(io::Error::other(format!(
            "packaged CLI removed a v0.3.26 command: current={current_commands:?} legacy={legacy_commands:?}"
        ))
        .into());
    }
    let legacy_subcommands = json_at(legacy, &["subcommands"])?
        .as_object()
        .ok_or_else(|| io::Error::other("legacy CLI subcommands fixture was not an object"))?;
    for (parent, expected) in legacy_subcommands {
        let expected = cli_value_strings(expected, parent)?;
        let advertised = cli_help_surface(&executable, parent)?.subcommands;
        if !expected
            .iter()
            .all(|subcommand| advertised.contains(subcommand))
        {
            return Err(io::Error::other(format!(
                "packaged CLI removed a v0.3.26 {parent} route: advertised={advertised:?} legacy={expected:?}"
            ))
            .into());
        }
    }
    let legacy_defaults = json_at(legacy, &["defaults"])?
        .as_object()
        .ok_or_else(|| io::Error::other("legacy CLI defaults fixture was not an object"))?;
    for (route, expected) in legacy_defaults {
        let expected = cli_value_strings(expected, route)?;
        let advertised = cli_help_surface(&executable, route)?.defaults;
        if !ordered_subsequence(&expected, &advertised) {
            return Err(io::Error::other(format!(
                "packaged CLI removed or reordered a v0.3.26 default for {route:?}: advertised={advertised:?} legacy={expected:?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn packaged_cli_commands_own_their_real_sqlite_effects() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("cli-contract");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(DUPLICATE_RS_FILE_NAME),
        "pub fn duplicate_contract() {}\n",
    )?;
    fs::write(repo.join(OUTSIDE_CANARY_FILE_NAME), "preserve me\n")?;
    fs::write(
        repo.join(".gitignore"),
        ".projectatlas/\nprojectatlas.toon\n",
    )?;
    let parent_canary = temp.path().join(PARENT_CANARY_FILE_NAME);
    fs::write(&parent_canary, "preserve parent\n")?;
    for arguments in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "ProjectAtlas CLI Contract"],
        vec!["config", "user.email", "cli-contract@projectatlas.invalid"],
        vec!["add", "."],
        vec!["commit", "--quiet", "-m", "baseline"],
    ] {
        let output = git_command_for_root(&repo).args(arguments).output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "CLI contract Git setup failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    let executable = mcp_contract_executable();
    assert_mcp_contract_runtime_and_skill(&executable)?;
    assert_packaged_cli_first_init_filesystem(&executable, temp.path())?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    for arguments in [
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    ] {
        run_mcp_contract_json(&executable, &repo, &arguments)?;
    }
    {
        let store = AtlasStore::open(&database)?;
        for (path, purpose) in [
            (".", "CLI contract fixture repository."),
            (SRC_DIR_NAME, "CLI contract fixture Rust sources."),
            ("src/lib.rs", "CLI contract fixture Rust library."),
            ("src/duplicate.rs", "CLI contract fixture Rust library."),
        ] {
            store.set_purpose(path, purpose, PurposeSource::Agent)?;
        }
    }
    Connection::open(&database)?.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
        (MCP_CONTRACT_METADATA_CANARY, "preserve"),
    )?;
    let clean_status = git_command_for_root(&repo)
        .args(["status", "--porcelain"])
        .output()?;
    if !clean_status.status.success() || !clean_status.stdout.is_empty() {
        return Err(io::Error::other(format!(
            "CLI contract clean Git fixture was dirty: {}",
            String::from_utf8_lossy(&clean_status.stdout)
        ))
        .into());
    }

    let snapshot_archive = temp.path().join("cli-contract-snapshot.tar.zst");
    let parser_storage = temp.path().join("cli-contract-parser-pack");
    let health_finding_id = projectatlas_core::health::finding_id(
        "duplicate-purpose",
        "src/lib.rs",
        Some("src/duplicate.rs"),
    );
    let cases = vec![
        CliContractCase {
            name: "init",
            arguments: vec!["init".to_string(), "--no-scan".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "map",
            arguments: vec!["map".to_string(), "--force".to_string()],
            output: CliContractOutput::Empty,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "scan",
            arguments: vec!["scan".to_string(), ".".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::DerivedSourceAdvance,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "overview",
            arguments: vec!["overview".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "folders",
            arguments: vec![
                "folders".to_string(),
                "source".to_string(),
                "--limit".to_string(),
                "2".to_string(),
            ],
            output: CliContractOutput::JsonArray,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "files",
            arguments: vec![
                "files".to_string(),
                "contract".to_string(),
                "--folder".to_string(),
                SRC_DIR_NAME.to_string(),
                "--limit".to_string(),
                "2".to_string(),
            ],
            output: CliContractOutput::JsonArray,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "next",
            arguments: vec![
                "next".to_string(),
                "contract".to_string(),
                "--limit".to_string(),
                "2".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "outline",
            arguments: vec![
                "outline".to_string(),
                "src/lib.rs".to_string(),
                "--lines".to_string(),
                "3".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "summary",
            arguments: vec![
                "summary".to_string(),
                "src/lib.rs".to_string(),
                "--limit".to_string(),
                "5".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::DerivedSourceAdvance,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "search",
            arguments: vec![
                "search".to_string(),
                "contract".to_string(),
                "--file-pattern".to_string(),
                "src/*.rs".to_string(),
                "--limit".to_string(),
                "1".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "slice",
            arguments: vec![
                "slice".to_string(),
                "src/lib.rs".to_string(),
                "--start-line".to_string(),
                "1".to_string(),
                "--end-line".to_string(),
                "2".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "symbols",
            arguments: vec!["symbols".to_string(), "build".to_string(), ".".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::DerivedGraphAdvance,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "settings",
            arguments: vec!["settings".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "snapshot",
            arguments: vec![
                "snapshot".to_string(),
                "export".to_string(),
                snapshot_archive.display().to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "parser-pack",
            arguments: vec![
                "parser-pack".to_string(),
                "--storage-root".to_string(),
                parser_storage.display().to_string(),
                "status".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "root",
            arguments: vec!["root".to_string(), "show".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "config",
            arguments: vec!["config".to_string(), "--print".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "ignore",
            arguments: vec!["ignore".to_string(), "list".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "watch-status",
            arguments: vec!["watch-status".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "watch",
            arguments: vec!["watch".to_string(), ".".to_string(), "--once".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::DerivedSourceAdvance,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "health-check",
            arguments: vec![
                "health-check".to_string(),
                "--source-only".to_string(),
                "--limit".to_string(),
                "5".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "health",
            arguments: vec![
                "health".to_string(),
                "resolve".to_string(),
                health_finding_id,
                "duplicate-purpose".to_string(),
                "src/lib.rs".to_string(),
                "--related-path".to_string(),
                "src/duplicate.rs".to_string(),
                "--rationale".to_string(),
                "CLI contract resolution.".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::HealthResolution,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "lint",
            arguments: vec![
                "lint".to_string(),
                "--purpose-level".to_string(),
                "low".to_string(),
            ],
            output: CliContractOutput::Empty,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "token",
            arguments: vec!["token".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "parity",
            arguments: vec!["parity".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "strip-legacy-purpose",
            arguments: vec![
                "strip-legacy-purpose".to_string(),
                ".".to_string(),
                "--dry-run".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "reset-index",
            arguments: vec!["reset-index".to_string(), "--dry-run".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "mcp",
            arguments: vec!["mcp".to_string()],
            output: CliContractOutput::Mcp,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "mcp-config",
            arguments: vec!["mcp-config".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "runtime-info",
            arguments: vec!["runtime-info".to_string()],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::None,
            expected_exit_code: 0,
        },
        CliContractCase {
            name: "purpose",
            arguments: vec![
                "purpose".to_string(),
                "set".to_string(),
                "src/watched.rs".to_string(),
                "CLI contract watched source.".to_string(),
            ],
            output: CliContractOutput::JsonObject,
            effect: McpSqliteEffect::PurposeAdvance("src/watched.rs"),
            expected_exit_code: 0,
        },
    ];
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cli-surfaces.json"))?;
    let current_key = format!("v{}", env!("CARGO_PKG_VERSION"));
    let expected_commands =
        cli_surface_strings(json_at(&fixture, &[&current_key])?, &["commands"])?;
    let tested_commands = cases
        .iter()
        .map(|case| case.name.to_string())
        .collect::<Vec<_>>();
    if tested_commands != expected_commands {
        return Err(io::Error::other(format!(
            "packaged CLI behavior table drifted from the frozen inventory: tested={tested_commands:?} expected={expected_commands:?}"
        ))
        .into());
    }

    for case in &cases {
        match case.name {
            "scan" => fs::write(
                repo.join(SRC_DIR_NAME).join(SCANNED_RS_FILE_NAME),
                "pub fn scanned_contract() {}\n",
            )?,
            "summary" => fs::write(
                repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
                "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n\npub fn dirty_contract() {}\n",
            )?,
            "watch" => fs::write(
                repo.join("src/watched.rs"),
                "pub fn watched_contract() {}\n",
            )?,
            _ => {}
        }
        let before = mcp_database_snapshot(&database)?;
        let filesystem_before = repository_filesystem_snapshot(&repo)?;
        let outer_filesystem_before = repository_filesystem_snapshot(temp.path())?;
        let output = run_packaged_cli_contract_case(&executable, &repo, &database, case)?;
        assert_cli_contract_filesystem_effect(case.name, &repo)?;
        let filesystem_after = repository_filesystem_snapshot(&repo)?;
        assert_cli_contract_filesystem_delta(case.name, &filesystem_before, &filesystem_after)?;
        let outer_filesystem_after = repository_filesystem_snapshot(temp.path())?;
        assert_cli_contract_outer_filesystem_delta(
            case.name,
            &outer_filesystem_before,
            &outer_filesystem_after,
        )?;
        if case.name == "search" {
            require_json_bool(
                output
                    .as_ref()
                    .ok_or_else(|| io::Error::other("CLI search omitted typed output"))?,
                &["truncated"],
                true,
            )?;
        }
        let after = mcp_database_snapshot(&database)?;
        assert_contract_sqlite_effect(case.name, case.effect, &before, &after)?;
        if matches!(
            case.effect,
            McpSqliteEffect::DerivedSourceAdvance | McpSqliteEffect::DerivedGraphAdvance
        ) {
            assert_mcp_matches_clean_packaged_scan(
                &executable,
                &repo,
                &database,
                temp.path(),
                &format!("cli-{}", case.name),
            )?;
        }
        if fs::read_to_string(&parent_canary)? != "preserve parent\n"
            || fs::read_to_string(repo.join(OUTSIDE_CANARY_FILE_NAME))? != "preserve me\n"
        {
            return Err(io::Error::other(format!(
                "{} escaped the CLI contract repository boundary",
                case.name
            ))
            .into());
        }
    }
    if !snapshot_archive.is_file() {
        return Err(io::Error::other("packaged CLI snapshot export omitted its archive").into());
    }
    assert_cli_snapshot_archive(&snapshot_archive)?;

    assert_packaged_cli_legacy_leaf_contracts(&executable, &repo, &database, temp.path())?;
    assert_packaged_cli_edge_contracts(&executable, &repo, &database)?;
    assert_cli_non_git_freshness(&executable)?;

    let reopened = run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "summary".to_string(),
            "src/watched.rs".to_string(),
            "--limit".to_string(),
            "5".to_string(),
        ],
    )?;
    require_json_contains(&reopened, &["content_summary"], "watched_contract")?;
    require_json_string(&reopened, &["file_purpose"], "CLI contract watched source.")?;
    let reopen_case = McpToolContractCase {
        name: "atlas_file_summary",
        arguments: serde_json::json!({
            "project_path": repo,
            "file": "src/watched.rs",
            "compact": true
        }),
        expected_marker: "file_summary:",
        payload_key: Some("file_summary"),
        effect: McpSqliteEffect::None,
        telemetry_enabled: false,
    };
    let reopened_text = run_mcp_contract_call(&executable, &repo, &database, &reopen_case)?;
    let reopened_mcp: Value = toon_format::decode_default(&reopened_text)?;
    require_json_contains(
        &reopened_mcp,
        &["file_summary", "content_summary"],
        "watched_contract",
    )?;
    require_json_string(
        &reopened_mcp,
        &["file_summary", "file_purpose"],
        "CLI contract watched source.",
    )?;
    Ok(())
}

#[test]
fn mcp_advertised_tools_own_their_real_sqlite_effects() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n",
    )?;
    fs::write(repo.join(OUTSIDE_CANARY_FILE_NAME), "preserve me\n")?;
    fs::write(
        repo.join(".gitignore"),
        ".projectatlas/\nprojectatlas.toon\n",
    )?;
    let parent_canary = temp.path().join(PARENT_CANARY_FILE_NAME);
    fs::write(&parent_canary, "preserve parent\n")?;
    for arguments in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "ProjectAtlas Contract"],
        vec!["config", "user.email", "contract@projectatlas.invalid"],
        vec!["add", "."],
        vec!["commit", "--quiet", "-m", "baseline"],
    ] {
        let output = git_command_for_root(&repo).args(arguments).output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "MCP contract Git setup failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }
    let db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let executable = mcp_contract_executable();
    assert_mcp_contract_runtime_and_skill(&executable)?;
    run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
    )?;
    run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    )?;
    {
        let store = AtlasStore::open(&db)?;
        for (path, purpose) in [
            (".", "Contract fixture repository."),
            (SRC_DIR_NAME, "Contract fixture Rust sources."),
            ("src/lib.rs", "Contract fixture Rust library."),
        ] {
            store.set_purpose(path, purpose, PurposeSource::Agent)?;
        }
    }
    Connection::open(&db)?.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
        (MCP_CONTRACT_METADATA_CANARY, "preserve"),
    )?;
    let clean_status = git_command_for_root(&repo)
        .args(["status", "--porcelain"])
        .output()?;
    if !clean_status.status.success() || !clean_status.stdout.is_empty() {
        return Err(io::Error::other(format!(
            "MCP contract clean Git fixture was dirty: {}",
            String::from_utf8_lossy(&clean_status.stdout)
        ))
        .into());
    }

    let repo_argument = repo.to_string_lossy().to_string();
    let suggested_purpose_id =
        projectatlas_core::health::finding_id("suggested-purpose-review", "src/scanned.rs", None);
    let cases = vec![
        McpToolContractCase {
            name: "atlas_set_project_path",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "project:",
            payload_key: Some("project"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_init",
            arguments: serde_json::json!({"project_path": repo_argument, "force_rescan": true}),
            expected_marker: "init:",
            payload_key: Some("init"),
            effect: McpSqliteEffect::DerivedSourceAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_map",
            arguments: serde_json::json!({"project_path": repo_argument, "force": true}),
            expected_marker: "map:",
            payload_key: Some("map"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_root",
            arguments: serde_json::json!({"project_path": repo_argument, "verify": true}),
            expected_marker: "root:",
            payload_key: Some("root"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_root_set",
            arguments: serde_json::json!({"root": repo_argument}),
            expected_marker: "root:",
            payload_key: Some("root"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_config",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "config:",
            payload_key: Some("config"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_ignore_list",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "ignore:",
            payload_key: Some("ignore"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_ignore_init_gitignore",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "gitignore:",
            payload_key: Some("gitignore"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_ignore_add",
            arguments: serde_json::json!({"project_path": repo_argument, "kind": "path-prefix", "value": "generated"}),
            expected_marker: "ignore:",
            payload_key: Some("ignore"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_ignore_remove",
            arguments: serde_json::json!({"project_path": repo_argument, "kind": "path-prefix", "value": "generated"}),
            expected_marker: "ignore:",
            payload_key: Some("ignore"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_scan",
            arguments: serde_json::json!({"project_path": repo_argument, "path": repo_argument, "max_workers": 1}),
            expected_marker: "scan:",
            payload_key: Some("scan"),
            effect: McpSqliteEffect::DerivedSourceAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_overview",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "overview:",
            payload_key: Some("overview"),
            effect: McpSqliteEffect::Telemetry,
            telemetry_enabled: true,
        },
        McpToolContractCase {
            name: "atlas_folders",
            arguments: serde_json::json!({"project_path": repo_argument, "query": SRC_DIR_NAME, "limit": 2}),
            expected_marker: "folders",
            payload_key: Some("folders"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_files",
            arguments: serde_json::json!({"project_path": repo_argument, "query": "indexed", "folder": SRC_DIR_NAME, "limit": 2}),
            expected_marker: "files",
            payload_key: Some("files"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_next",
            arguments: serde_json::json!({"project_path": repo_argument, "query": "indexed", "limit": 2}),
            expected_marker: "next:",
            payload_key: Some("next"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_outline",
            arguments: serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "lines": 4}),
            expected_marker: "outline:",
            payload_key: Some("outline"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_file_summary",
            arguments: serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "compact": true}),
            expected_marker: "file_summary:",
            payload_key: Some("file_summary"),
            effect: McpSqliteEffect::DerivedSourceAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_search",
            arguments: serde_json::json!({"project_path": repo_argument, "pattern": "helper", "file_pattern": "src/*.rs", "limit": 1}),
            expected_marker: "search:",
            payload_key: Some("search"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_slice",
            arguments: serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "start_line": 1, "end_line": 2, "output_bytes": 4096}),
            expected_marker: "slice:",
            payload_key: Some("slice"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_symbols_build",
            arguments: serde_json::json!({"project_path": repo_argument, "path": repo_argument, "max_workers": 1}),
            expected_marker: "symbols_build:",
            payload_key: Some("symbols_build"),
            effect: McpSqliteEffect::DerivedGraphAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_symbols",
            arguments: serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "query": "indexed", "limit": 2}),
            expected_marker: "symbols",
            payload_key: Some("symbols"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_symbol_relations",
            arguments: serde_json::json!({"project_path": repo_argument, "view": "detailed", "compact": true, "file": "src/lib.rs", "symbol": "indexed", "direction": "outbound", "limit": 2, "output_bytes": 65536}),
            expected_marker: "symbol_relations:",
            payload_key: Some("symbol_relations"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_health",
            arguments: serde_json::json!({"project_path": repo_argument, "limit": 2}),
            expected_marker: "health:",
            payload_key: Some("health"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_health_resolve",
            arguments: serde_json::json!({"project_path": repo_argument, "finding_id": suggested_purpose_id, "category": "suggested-purpose-review", "path": "src/scanned.rs", "rationale": "Contract-owned resolution."}),
            expected_marker: "health_resolution:",
            payload_key: Some("health_resolution"),
            effect: McpSqliteEffect::HealthResolution,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_lint",
            arguments: serde_json::json!({"project_path": repo_argument, "purpose_level": "low"}),
            expected_marker: "lint:",
            payload_key: Some("lint"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_token_report",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "token_savings:",
            payload_key: Some("token_savings"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_parity_report",
            arguments: serde_json::json!({"project_path": repo_argument, "profile": "repository-intelligence"}),
            expected_marker: "parity:",
            payload_key: Some("parity"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_settings",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "settings:",
            payload_key: Some("settings"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_watch_status",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "watch_status:",
            payload_key: Some("watch_status"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_watch_once",
            arguments: serde_json::json!({"project_path": repo_argument, "path": repo_argument, "max_workers": 1}),
            expected_marker: "watch:",
            payload_key: Some("watch"),
            effect: McpSqliteEffect::DerivedSourceAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_strip_legacy_purpose",
            arguments: serde_json::json!({"project_path": repo_argument, "path": repo_argument, "dry_run": true}),
            expected_marker: "legacy_purpose_migration:",
            payload_key: Some("legacy_purpose_migration"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_reset_index",
            arguments: serde_json::json!({"project_path": repo_argument, "dry_run": true}),
            expected_marker: "reset_index:",
            payload_key: Some("reset_index"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_mcp_config",
            arguments: serde_json::json!({"project_path": repo_argument, "harness": "mcp-json"}),
            expected_marker: "mcp_config:",
            payload_key: Some("mcp_config"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_runtime_info",
            arguments: serde_json::json!({"project_path": repo_argument}),
            expected_marker: "runtime:",
            payload_key: Some("runtime"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_session_brief",
            arguments: serde_json::json!({"project_path": repo_argument, "query": "indexed", "compact": true, "folder_limit": 2, "file_limit": 2, "blocker_limit": 1, "purpose_limit": 1}),
            expected_marker: "session_brief:",
            payload_key: Some("session_brief"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_task_status",
            arguments: serde_json::json!({"task_id": "task-progress-contract"}),
            expected_marker: "task_status:",
            payload_key: Some("task_status"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_task_cancel",
            arguments: serde_json::json!({"task_id": "task-progress-contract"}),
            expected_marker: "task_cancel:",
            payload_key: Some("task_cancel"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_purpose_queue",
            arguments: serde_json::json!({"project_path": repo_argument, "limit": 2, "task": "mcp-contract"}),
            expected_marker: "purpose_curation:",
            payload_key: Some("purpose_curation"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_purpose_set",
            arguments: serde_json::json!({"project_path": repo_argument, "path": "src/lib.rs", "purpose": "MCP contract Rust library."}),
            expected_marker: "purpose_set:",
            payload_key: Some("purpose_set"),
            effect: McpSqliteEffect::PurposeAdvance("src/lib.rs"),
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_purpose_review",
            arguments: serde_json::json!({"project_path": repo_argument, "apply": true, "items": [{"path": "src/lib.rs", "purpose": "Reviewed MCP contract Rust library."}]}),
            expected_marker: "purpose_review:",
            payload_key: Some("purpose_review"),
            effect: McpSqliteEffect::PurposeAdvance("src/lib.rs"),
            telemetry_enabled: false,
        },
    ];

    let inventory = run_mcp_contract_inventory(&executable, &repo, &db)?;
    assert_legacy_mcp_surface_compatible(&inventory)?;
    let tools_response = mcp_response(&inventory, 2)?;
    let tools = tools_response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("MCP contract tools/list omitted tools"))?;
    assert_codex_bridge_compatible_input_schemas(tools)?;
    let tools_by_name = mcp_tools_by_name(tools)?;
    let advertised_names = tools_by_name.keys().copied().collect::<BTreeSet<_>>();
    let case_names = cases.iter().map(|case| case.name).collect::<BTreeSet<_>>();
    if advertised_names != case_names || cases.len() != case_names.len() {
        return Err(io::Error::other(format!(
            "MCP contract table did not exactly own the advertised inventory: advertised={advertised_names:?} cases={case_names:?}"
        ))
        .into());
    }
    let tools_digest = sha256_hex(&serde_json::to_vec(tools)?);
    if tools_digest != MCP_V041_TOOLS_SHA256 {
        return Err(io::Error::other(format!(
            "frozen v0.4.1 MCP inventory/schema digest drifted: expected {MCP_V041_TOOLS_SHA256}, found {tools_digest}"
        ))
        .into());
    }
    for case in &cases {
        let schema = tools_by_name[case.name]
            .get("inputSchema")
            .ok_or_else(|| io::Error::other(format!("{} omitted inputSchema", case.name)))?;
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| io::Error::other(format!("{} schema omitted properties", case.name)))?;
        let arguments = case.arguments.as_object().ok_or_else(|| {
            io::Error::other(format!("{} arguments were not an object", case.name))
        })?;
        for key in arguments.keys() {
            if !properties.contains_key(key) {
                return Err(io::Error::other(format!(
                    "{} contract argument {key:?} is absent from the advertised schema",
                    case.name
                ))
                .into());
            }
        }
        for required in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !arguments.contains_key(required) {
                return Err(io::Error::other(format!(
                    "{} contract row omitted required schema member {required:?}",
                    case.name
                ))
                .into());
            }
        }
    }

    for case in &cases {
        match case.name {
            "atlas_init" => fs::write(
                repo.join(SRC_DIR_NAME).join(SCANNED_RS_FILE_NAME),
                "pub fn scanned_contract() {}\n",
            )?,
            "atlas_file_summary" => fs::write(
                repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
                "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n\npub fn dirty_contract() {}\n",
            )?,
            "atlas_watch_once" => fs::write(
                repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
                "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n\npub fn dirty_contract() {}\n\npub fn watched_contract() {}\n",
            )?,
            _ => {}
        }
        let before = mcp_database_snapshot(&db)?;
        let text = run_mcp_contract_call(&executable, &repo, &db, case)?;
        if !text.contains(case.expected_marker) {
            return Err(io::Error::other(format!(
                "{} response omitted marker {:?}: {text}",
                case.name, case.expected_marker
            ))
            .into());
        }
        let decoded: Value = toon_format::decode_default(&text).map_err(|error| {
            io::Error::other(format!(
                "{} returned invalid typed TOON: {error}",
                case.name
            ))
        })?;
        let object = decoded.as_object().ok_or_else(|| {
            io::Error::other(format!("{} TOON did not decode to an object", case.name))
        })?;
        if let Some(payload_key) = case.payload_key
            && !object.contains_key(payload_key)
        {
            return Err(io::Error::other(format!(
                "{} typed TOON omitted top-level {payload_key:?}: {decoded}",
                case.name
            ))
            .into());
        }
        let after = mcp_database_snapshot(&db)?;
        assert_contract_sqlite_effect(case.name, case.effect, &before, &after)?;
        assert_mcp_typed_payload(case, &decoded, &text, &after)?;
        if matches!(
            case.effect,
            McpSqliteEffect::DerivedSourceAdvance | McpSqliteEffect::DerivedGraphAdvance
        ) {
            assert_mcp_matches_clean_packaged_scan(
                &executable,
                &repo,
                &db,
                temp.path(),
                case.name,
            )?;
        }
        if matches!(case.name, "atlas_init" | "atlas_watch_once") {
            let file = if case.name == "atlas_init" {
                "src/scanned.rs"
            } else {
                "src/lib.rs"
            };
            let expected = if case.name == "atlas_init" {
                "scanned_contract"
            } else {
                "watched_contract"
            };
            let reopened = run_mcp_contract_json(
                &executable,
                &repo,
                &[
                    "--db".to_string(),
                    db.display().to_string(),
                    "summary".to_string(),
                    file.to_string(),
                    "--limit".to_string(),
                    "25".to_string(),
                ],
            )?;
            require_json_contains(&reopened, &["content_summary"], expected)?;
        }
        if case.name == "atlas_file_summary" {
            let status = git_command_for_root(&repo)
                .args(["status", "--porcelain", "--", "src/lib.rs"])
                .output()?;
            if !status.status.success()
                || !String::from_utf8_lossy(&status.stdout).contains("src/lib.rs")
            {
                return Err(io::Error::other(
                    "saved-dirty MCP freshness fixture did not remain visibly Git-dirty",
                )
                .into());
            }
        }
        if fs::read_to_string(repo.join(OUTSIDE_CANARY_FILE_NAME))? != "preserve me\n"
            || fs::read_to_string(&parent_canary)? != "preserve parent\n"
        {
            return Err(io::Error::other(format!(
                "{} changed an unrelated filesystem canary",
                case.name
            ))
            .into());
        }
    }

    for (name, arguments, expected_error) in [
        (
            "atlas_slice",
            serde_json::json!({"project_path": repo_argument}),
            "file",
        ),
        (
            "atlas_slice",
            serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "start_line": 1, "end_line": 2, "output_bytes": 1}),
            "output",
        ),
        (
            "atlas_purpose_set",
            serde_json::json!({"project_path": repo_argument, "path": "../parent-canary.txt", "purpose": "Must not escape."}),
            "project",
        ),
        (
            "atlas_purpose_review",
            serde_json::json!({"project_path": repo_argument, "apply": false, "items": [{}]}),
            "missing field `path`",
        ),
    ] {
        assert_mcp_contract_failure_no_mutation(
            &executable,
            &repo,
            &db,
            name,
            &arguments,
            expected_error,
        )?;
    }
    assert_mcp_non_git_freshness(&executable)?;
    assert_mcp_active_cancellation_preserves_generation(&executable)?;

    let reopened = run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "summary".to_string(),
            "src/lib.rs".to_string(),
            "--limit".to_string(),
            "2".to_string(),
        ],
    )?;
    require_json_string(
        &reopened,
        &["file_purpose"],
        "Reviewed MCP contract Rust library.",
    )?;
    require_json_contains(&reopened, &["content_summary"], "watched_contract")?;
    let reopened_mcp = run_mcp_contract_call(
        &executable,
        &repo,
        &db,
        &McpToolContractCase {
            name: "atlas_file_summary",
            arguments: serde_json::json!({"project_path": repo_argument, "file": "src/lib.rs", "compact": true}),
            expected_marker: "file_summary:",
            payload_key: Some("file_summary"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
    )?;
    if !reopened_mcp.contains("Reviewed MCP contract Rust library.")
        || !reopened_mcp.contains("watched_contract")
    {
        return Err(io::Error::other(
            "reopened MCP did not preserve authored purpose and watched source facts",
        )
        .into());
    }
    Ok(())
}

#[test]
fn mcp_stdio_serves_toon_tool_payloads() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {\n    helper();\n}\n\nfn helper() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["init", "--no-scan"])
        .assert()
        .success();
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
            "src/lib.rs",
            "Reviewed Rust library purpose for MCP navigation.",
            PurposeSource::Agent,
        )?;
    }

    let repo_argument = repo.to_string_lossy().to_string();
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#.to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_init","arguments":{"project_path":repo_argument}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_config","arguments":{"project_path":repo_argument}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_root","arguments":{"project_path":repo_argument,"verify":true}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"atlas_mcp_config","arguments":{"project_path":repo_argument,"nearest_project":true}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_ignore_list","arguments":{"project_path":repo_argument}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"atlas_lint","arguments":{"project_path":repo_argument,"purpose_level":"low"}}}).to_string(),
        r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"atlas_runtime_info","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"atlas_health","arguments":{"category":"missing-purpose","path_prefix":".","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"include_chart":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"atlas_purpose_review","arguments":{"apply":true,"items":[{"path":"src/lib.rs","confirm_existing":true}]}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"atlas_next","arguments":{"query":"indexed","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"atlas_settings","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"atlas_session_brief","arguments":{"query":"src/lib.rs","folder_limit":1,"file_limit":1,"blocker_limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"atlas_task_status","arguments":{"task_id":"task-progress-contract"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"atlas_task_cancel","arguments":{"task_id":"task-progress-contract"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"atlas_health","arguments":{"coverage":true,"path_prefix":"src/lib.rs","parser":"tree-sitter","provider":"tree-sitter","coverage_state":"complete","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","file":"src/lib.rs","symbol":"indexed","direction":"outbound","depth":2,"limit":50,"output_bytes":65536,"include_communities":true,"include_cycles":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/lib.rs","symbol":"helper","symbol_kind":"function","symbol_signature":"fn helper ( )"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs","compact":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","analysis_mode":"impact","vcs":"working_tree","file":"src/lib.rs","symbol":"indexed","direction":"outbound","depth":2,"limit":50,"output_bytes":65536}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","analysis_mode":"trace","file":"src/lib.rs","symbol":"indexed","direction":"outbound","depth":2,"limit":50,"output_bytes":65536,"trace_target":"helper","trace_target_file":"src/lib.rs","trace_target_kind":"function","trace_target_signature":"fn helper ( )"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":28,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs","limit":25}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":29,"method":"tools/call","params":{"name":"atlas_session_brief","arguments":{"query":"src/lib.rs","compact":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"atlas_session_brief","arguments":{"query":"src/lib.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"detailed","compact":true,"file":"src/lib.rs","symbol":"indexed","symbol_parent":"","direction":"outbound","include_occurrences":true,"limit":10,"output_bytes":65536}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"detailed","compact":true,"file":"src/lib.rs","symbol":"indexed","direction":"outbound","include_occurrences":true,"limit":10,"output_bytes":2048}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"compact":true,"file":"src/lib.rs","symbol":"indexed","direction":"outbound","limit":10}}}"#.to_string(),
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
    assert_legacy_mcp_surface_compatible(&stdout)?;
    let session_brief_text = mcp_tool_text(&stdout, 19)?;
    let analysis_text = mcp_tool_text(&stdout, 23)?;
    let next_call_text = mcp_tool_text(&stdout, 24)?;
    let recommended_summary_text = mcp_tool_text(&stdout, 25)?;
    let impact_text = mcp_tool_text(&stdout, 26)?;
    let trace_text = mcp_tool_text(&stdout, 27)?;
    let expanded_summary_text = mcp_tool_text(&stdout, 28)?;
    let compact_session_brief_text = mcp_tool_text(&stdout, 29)?;
    let legacy_default_brief_text = mcp_tool_text(&stdout, 30)?;
    let legacy_default_summary_text = mcp_tool_text(&stdout, 31)?;
    let compact_relations_text = mcp_tool_text(&stdout, 32)?;
    let bounded_compact_relations_text = mcp_tool_text(&stdout, 33)?;
    let rejected_legacy_compact_text = mcp_tool_text(&stdout, 34)?;
    let session_brief_has_ready_call = session_brief_text.contains("target: atlas_file_summary")
        && session_brief_text.contains("file: src/lib.rs");
    if !stdout.contains(r#""id":1"#)
        || !stdout.contains(r#""serverInfo":{"name":"ProjectAtlas","version":"#)
        || !stdout.contains(r#""name":"atlas_files""#)
        || !stdout.contains(r#""name":"atlas_next""#)
        || !stdout.contains(r#""name":"atlas_session_brief""#)
        || !stdout.contains("Return selected project identity, index state, ranked candidates, blockers, and typed next-call recommendations for agent startup.")
        || !stdout.contains(r#""project_path":{"description":""#)
        || !stdout.contains(r#""name":"atlas_task_status""#)
        || !stdout.contains(r#""name":"atlas_task_cancel""#)
        || !stdout.contains("overview:")
        || !stdout.contains("files[1]")
        || !stdout.contains("next:")
        || !stdout.contains("mcp_session:")
        || !stdout.contains("path_scope: selected_project")
        || !stdout.contains("session_brief:")
        || !session_brief_has_ready_call
        || session_brief_text.contains("target: atlas_folders")
        || session_brief_text.contains("target: atlas_files")
        || !compact_session_brief_text.contains("target: atlas_file_summary")
        || !compact_session_brief_text.contains("compact: true")
        || compact_session_brief_text.contains("\n    db:")
        || compact_session_brief_text.contains("\n    config:")
        || !legacy_default_brief_text.contains("folder_limit: 5")
        || !legacy_default_brief_text.contains("file_limit: 5")
        || !legacy_default_brief_text.contains("blocker_limit: 5")
        || !legacy_default_brief_text.contains("purpose_limit: 5")
        || !legacy_default_brief_text.contains("\n    db:")
        || !legacy_default_brief_text.contains("\n  policy:")
        || !legacy_default_summary_text.contains("source_status: \"live-source\"")
        || !legacy_default_summary_text.contains("total_functions:")
        || !legacy_default_summary_text.contains("coverage:")
        || !stdout.contains("task_status:")
        || !stdout.contains("task_cancel:")
        || !stdout.contains("task-progress-contract")
        || !stdout.contains("already_finished")
        || !stdout.contains("health:")
        || !stdout.contains("health_findings[1]")
        || !stdout.contains("coverage:")
        || !stdout.contains("output_bytes:")
        || !stdout.contains("next_start_index: 1")
        || !stdout.contains("ProjectAtlas")
        || !stdout.contains("Token Impact")
        || !stdout.contains("T O T A L   T O K E N S   A V O I D E D")
        || !stdout.contains("N A V I G A T I O N   W O R K   A V O I D E D")
        || !stdout.to_ascii_lowercase().contains("file reads avoided")
        || stdout.contains("Broad folder walks skipped")
        || stdout.contains("Candidate files not opened")
        || stdout.contains("source steps account for")
        || !stdout.contains("S I G N A L")
        || !stdout.contains("purpose_review:")
        || !stdout.contains("failed: 0")
        || !stdout.contains("src/lib.rs")
        || !analysis_text.contains("symbol_relations:")
        || !analysis_text.contains("mode: architecture")
        || !analysis_text.contains("findings[")
        || !analysis_text.contains("next_call:")
        || !analysis_text.contains("symbol_slice")
        || !next_call_text.contains("fn helper()")
        || !recommended_summary_text.contains("file_summary:")
        || !recommended_summary_text.contains("src/lib.rs")
        || !recommended_summary_text.contains("indexed")
        || !recommended_summary_text.contains("file_purpose_agent_reviewed: true")
        || recommended_summary_text.contains("source_status:")
        || recommended_summary_text.contains("total_functions:")
        || recommended_summary_text.contains("coverage:")
        || recommended_summary_text.contains("documentation: \"\"")
        || recommended_summary_text.contains("parent: \"\"")
        || recommended_summary_text.contains("called_by[0]")
        || !expanded_summary_text.contains("source_status: \"live-source\"")
        || !expanded_summary_text.contains("total_functions:")
        || !expanded_summary_text.contains("coverage:")
        || !impact_text.contains("mode: impact")
        || (!impact_text.contains("state: available")
            && !impact_text.contains("state: unavailable"))
        || !trace_text.contains("mode: trace")
        || !trace_text.contains("kind: static_trace")
        || !trace_text.contains("status: confirmed")
        || !trace_text.contains("name: helper")
        || !trace_text.contains("capability: symbol_slice")
        || !compact_relations_text.contains("symbol_relations:")
        || !compact_relations_text.contains("returned: 1")
        || !compact_relations_text.contains("status: resolved")
        || !compact_relations_text.contains("confidence: exact")
        || !compact_relations_text.contains("completeness: complete")
        || !compact_relations_text.contains("next_call:")
        || !bounded_compact_relations_text
            .contains("graph output byte limit is too small for the empty response envelope")
        || !rejected_legacy_compact_text
            .contains("compact symbol relations require view=detailed")
    {
        return Err(io::Error::other(format!(
            "mcp stdout did not include expected payloads: {stdout}"
        ))
        .into());
    }
    if stdout.contains("\u{1b}[") || stdout.contains("\\u001b") || stdout.contains("\\x1b") {
        return Err(io::Error::other(
            "atlas_token_report include_chart leaked ANSI escape sequences into MCP stdout",
        )
        .into());
    }
    let reviewed_summary = json_summary_command(&repo, &db, "src/lib.rs")?;
    require_json_string(&reviewed_summary, &["file_purpose_source"], "agent")?;
    require_json_bool(&reviewed_summary, &["file_purpose_agent_reviewed"], true)?;
    require_json_string(
        &reviewed_summary,
        &["file_purpose"],
        "Reviewed Rust library purpose for MCP navigation.",
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

#[test]
fn cli_federation_is_explicit_read_only_and_fails_closed_on_a_stale_late_root()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let mut projects = Vec::new();
    for index in 0..3 {
        let root = temp.path().join(format!("federated-{index}"));
        let database = create_federation_navigation_project(&root)?;
        projects.push((root, database));
    }

    let before = projects
        .iter()
        .map(|(_, database)| fs::read(database))
        .collect::<Result<Vec<_>, _>>()?;
    let mut command = Command::cargo_bin("projectatlas")?;
    command
        .current_dir(&projects[0].0)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&projects[0].1)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/navigation_owner.rs",
            "--relation",
            "imports",
            "--resolution",
            "external",
            "--limit",
            "10",
        ]);
    for (root, _) in &projects {
        command.arg("--root").arg(root);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "federated CLI relation query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let report: Value = serde_json::from_slice(&output.stdout)?;
    require_json_array_len(&report, &["symbol_relations", "participants"], 3)?;
    require_json_array_len(&report, &["symbol_relations", "rendezvous"], 1)?;
    require_json_array_len(
        &report,
        &["symbol_relations", "rendezvous", "0", "evidence"],
        3,
    )?;
    let after = projects
        .iter()
        .map(|(_, database)| fs::read(database))
        .collect::<Result<Vec<_>, _>>()?;
    if before != after {
        return Err(io::Error::other("federated CLI call changed a database").into());
    }

    fs::write(
        projects[2].0.join("src/navigation_owner.rs"),
        "pub fn navigation_fixture_changed() {}\n",
    )?;
    let mut stale = Command::cargo_bin("projectatlas")?;
    stale
        .current_dir(&projects[0].0)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&projects[0].1)
        .args([
            "symbols",
            "relations",
            "--view",
            "detailed",
            "--file",
            "src/navigation_owner.rs",
            "--limit",
            "10",
        ]);
    for (root, _) in &projects {
        stale.arg("--root").arg(root);
    }
    let stale = stale.output()?;
    if stale.status.success()
        || !String::from_utf8_lossy(&stale.stderr).contains("refresh_required")
    {
        return Err(io::Error::other(format!(
            "stale late root did not fail closed: {}",
            String::from_utf8_lossy(&stale.stderr)
        ))
        .into());
    }
    let after_stale = projects
        .iter()
        .map(|(_, database)| fs::read(database))
        .collect::<Result<Vec<_>, _>>()?;
    if after != after_stale {
        return Err(io::Error::other("stale federation repaired or changed a database").into());
    }
    for (index, (_, database)) in projects.iter().enumerate() {
        let moved = database.with_extension(format!("closed-{index}"));
        fs::rename(database, &moved)?;
        fs::rename(moved, database)?;
    }
    Ok(())
}

#[test]
fn mcp_federation_uses_the_existing_relation_tool_without_telemetry_writes()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let first = temp.path().join("mcp-federated-0");
    let second = temp.path().join("mcp-federated-1");
    let first_db = create_federation_navigation_project(&first)?;
    let second_db = create_federation_navigation_project(&second)?;
    let before = [fs::read(&first_db)?, fs::read(&second_db)?];
    let roots = [first.display().to_string(), second.display().to_string()];
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-federation-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "atlas_symbol_relations",
                "arguments": {
                    "view": "detailed",
                    "file": "src/navigation_owner.rs",
                    "relation": "imports",
                    "resolution": "external",
                    "limit": 10,
                    "roots": roots
                }
            }
        })
        .to_string(),
    ];
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let stdout = run_mcp_stdio(
        &executable,
        &first,
        &[
            "--db".to_string(),
            first_db.display().to_string(),
            "mcp".to_string(),
        ],
        &messages,
    )?;
    let text = mcp_tool_text(&stdout, 2)?;
    for expected in [
        "symbol_relations:",
        "participants[2]",
        "rendezvous[1]",
        "evidence[2]",
    ] {
        if !text.contains(expected) {
            return Err(io::Error::other(format!(
                "federated MCP response omitted {expected:?}: {text}"
            ))
            .into());
        }
    }
    if before != [fs::read(&first_db)?, fs::read(&second_db)?] {
        return Err(
            io::Error::other("federated MCP call wrote telemetry or database state").into(),
        );
    }
    Ok(())
}

/// Scan and publish one conventional root used by federation adapter tests.
fn create_federation_navigation_project(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    fs::create_dir_all(root.join(SRC_DIR_NAME))?;
    fs::create_dir_all(root.join(TESTS_DIR_NAME))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"adapter-navigation\"\nversion = \"0.1.0\"\n",
    )?;
    for path in [
        "src/navigation_owner.rs",
        "src/navigation_local.rs",
        "src/navigation_unresolved.rs",
        "tests/navigation_owner.rs",
    ] {
        fs::write(root.join(path), "pub fn navigation_fixture() {}\n")?;
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(root)
        .args(["scan", "."])
        .assert()
        .success();
    let database = root.join(ATLAS_DIR_NAME).join("projectatlas.db");
    publish_cli_navigation_graph(&database)?;
    Ok(database)
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
fn indexed_reads_use_scanned_project_root_from_any_cwd() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let outside = temp.path().join("outside");
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&repo)?;
    fs::create_dir(&outside)?;
    fs::create_dir(&unrelated)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        outside.join("projectatlas.toml"),
        "[project]\nroot = \"../unrelated\"\n\n[scan]\nexclude_dir_names = [\"src\"]\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "/// Demo API.\npub fn from_scanned_root() {\n    helper();\n}\n\nfn helper() {}\n",
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
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["outline", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from_scanned_root"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Demo API"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["search", "helper", "--file-pattern", "*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args([
            "slice",
            "src/lib.rs",
            "--start-line",
            "2",
            "--end-line",
            "4",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("from_scanned_root"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "build"])
        .assert()
        .success()
        .stdout(predicate::str::contains("symbols_build:"));

    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "/// Demo API.\npub fn from_scanned_root() {\n    helper();\n}\n\npub fn after_outside_watch() {}\n\nfn helper() {}\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["watch", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("watch:"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("after_outside_watch"));

    let raw_settings = Command::cargo_bin("projectatlas")?
        .current_dir(&outside)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .arg("settings")
        .output()?;
    if !raw_settings.status.success() {
        return Err(io::Error::other("outside-cwd settings command failed").into());
    }
    let settings_json: Value = serde_json::from_slice(&raw_settings.stdout)?;
    let settings_root = settings_json["repo_root"]
        .as_str()
        .ok_or_else(|| io::Error::other("settings repo root missing"))?;
    if std::path::Path::new(settings_root).canonicalize()? != repo.canonicalize()? {
        return Err(io::Error::other(format!(
            "outside-cwd settings root mismatch: {settings_root}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn scan_honors_configured_excludes_and_cli_fuzzy_search() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("api"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::create_dir_all(repo.join("metadata.egg-info"))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\", \"generated\"]\nexclude_dir_suffixes = [\".egg-info\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("engine.rs"),
        "pub fn build_project_atlas() {}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("api").join("live.rs"),
        "pub fn live_api() {}\n",
    )?;
    fs::write(
        repo.join("docs").join("api").join("noise.rs"),
        "pub fn generated_doc_noise() {}\n",
    )?;
    fs::write(
        repo.join("generated").join("noise.rs"),
        "pub fn generated_noise() {}\n",
    )?;
    fs::write(
        repo.join("metadata.egg-info").join("PKG-INFO"),
        "suffix_excluded_package_metadata\n",
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
        return Err(io::Error::other("configured scan command failed").into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize(&scan_json, &["overview", "files"], 3)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "noise"])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated/noise.rs").not())
        .stdout(predicate::str::contains("docs/api/noise.rs").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/api/live.rs"))
        .stdout(predicate::str::contains("docs/api/noise.rs").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/engine.rs"))
        .stdout(predicate::str::contains("src/api/live.rs"))
        .stdout(predicate::str::contains("generated/noise.rs").not())
        .stdout(predicate::str::contains("docs/api/noise.rs").not());

    let raw_excluded_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "generated_doc_noise", "--file-pattern", "*.rs"])
        .output()?;
    if !raw_excluded_search.status.success() {
        return Err(io::Error::other("excluded-prefix search command failed").into());
    }
    let excluded_search_json: Value = serde_json::from_slice(&raw_excluded_search.stdout)?;
    require_json_usize(&excluded_search_json, &["returned"], 0)?;

    let raw_suffix_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "suffix_excluded_package_metadata"])
        .output()?;
    if !raw_suffix_search.status.success() {
        return Err(io::Error::other("excluded-suffix search command failed").into());
    }
    let suffix_search_json: Value = serde_json::from_slice(&raw_suffix_search.stdout)?;
    require_json_usize(&suffix_search_json, &["returned"], 0)?;

    let raw_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "bpa", "--fuzzy", "--file-pattern", "*.rs"])
        .output()?;
    if !raw_search.status.success() {
        return Err(io::Error::other("fuzzy search command failed").into());
    }
    let search_json: Value = serde_json::from_slice(&raw_search.stdout)?;
    require_json_string(&search_json, &["mode"], "fuzzy")?;
    require_json_usize(&search_json, &["returned"], 1)?;
    require_json_string(&search_json, &["results", "0", "path"], "src/engine.rs")?;
    Ok(())
}

#[test]
fn ignore_commands_preserve_manual_layer_while_gitignore_updates_apply()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::create_dir_all(repo.join("local-cache"))?;
    fs::write(repo.join(SRC_DIR_NAME).join("main.rs"), "fn main() {}\n")?;
    fs::write(
        repo.join("generated").join("noise.rs"),
        "fn generated_noise() {}\n",
    )?;
    fs::write(
        repo.join("docs").join("api").join("noise.rs"),
        "fn docs_noise() {}\n",
    )?;
    fs::write(
        repo.join("local-cache").join("noise.rs"),
        "fn ignored_by_gitignore() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    let raw_missing_gitignore = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "list"])
        .output()?;
    if !raw_missing_gitignore.status.success() {
        return Err(io::Error::other("ignore list without .gitignore failed").into());
    }
    let missing_gitignore_json: Value = serde_json::from_slice(&raw_missing_gitignore.stdout)?;
    require_json_bool(&missing_gitignore_json, &["gitignore_present"], false)?;
    require_json_string(
        &missing_gitignore_json,
        &["gitignore_mode"],
        "inherited-when-present",
    )?;
    require_json_string(
        &missing_gitignore_json,
        &["manual_layer_order"],
        "after-gitignore",
    )?;

    let raw_init_gitignore = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "init-gitignore"])
        .output()?;
    if !raw_init_gitignore.status.success() {
        return Err(io::Error::other("ignore init-gitignore failed").into());
    }
    let init_gitignore_json: Value = serde_json::from_slice(&raw_init_gitignore.stdout)?;
    require_json_bool(&init_gitignore_json, &["created"], true)?;
    require_json_bool(&init_gitignore_json, &["existed"], false)?;
    require_json_bool(&init_gitignore_json, &["gitignore_inherited"], true)?;
    let gitignore_path = repo.join(".gitignore");
    let gitignore_text = fs::read_to_string(&gitignore_path)?;
    for required in [
        ".projectatlas/*.db",
        ".projectatlas/*.lock",
        ".projectatlas/graph-stage-*/",
        ".projectatlas/optional-parser-pack.json",
        ".projectatlas/projectatlas.toon",
        ".projectatlas/projectatlas-purpose-review.json",
        ".projectatlas/projectatlas.mcp.json",
        ".projectatlas/projectatlas.claude.mcp.json",
        ".projectatlas/projectatlas.opencode.json",
    ] {
        if !gitignore_text.lines().any(|line| line == required) {
            return Err(io::Error::other(format!(
                "created .gitignore did not protect ProjectAtlas runtime state {required:?}: {gitignore_text}"
            ))
            .into());
        }
    }

    let raw_existing_gitignore = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "init-gitignore"])
        .output()?;
    if !raw_existing_gitignore.status.success() {
        return Err(io::Error::other("repeat ignore init-gitignore failed").into());
    }
    let existing_gitignore_json: Value = serde_json::from_slice(&raw_existing_gitignore.stdout)?;
    require_json_bool(&existing_gitignore_json, &["created"], false)?;
    require_json_bool(&existing_gitignore_json, &["existed"], true)?;

    fs::write(gitignore_path, format!("{gitignore_text}local-cache/\n"))?;

    let raw_add_dir = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "add", "--kind", "dir-name", "generated"])
        .output()?;
    if !raw_add_dir.status.success() {
        return Err(io::Error::other("ignore add dir-name failed").into());
    }
    let add_dir_json: Value = serde_json::from_slice(&raw_add_dir.stdout)?;
    require_json_bool(&add_dir_json, &["gitignore_present"], true)?;
    require_json_string(&add_dir_json, &["gitignore_mode"], "inherited-when-present")?;
    require_json_string(&add_dir_json, &["manual_layer_order"], "after-gitignore")?;
    require_json_string(&add_dir_json, &["kind"], "dir-name")?;
    require_json_string(&add_dir_json, &["value"], "generated")?;
    require_json_bool(&add_dir_json, &["changed"], true)?;

    let raw_add_prefix = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "add", "--kind", "path-prefix", "docs/api"])
        .output()?;
    if !raw_add_prefix.status.success() {
        return Err(io::Error::other("ignore add path-prefix failed").into());
    }
    let add_prefix_json: Value = serde_json::from_slice(&raw_add_prefix.stdout)?;
    require_json_string(&add_prefix_json, &["kind"], "path-prefix")?;
    require_json_string(&add_prefix_json, &["value"], "docs/api")?;
    require_json_bool(&add_prefix_json, &["changed"], true)?;

    let config_text = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if !config_text.contains(r"exclude_dir_names = [")
        || !config_text.contains(r#""generated""#)
        || !config_text.contains(r#""docs/api""#)
    {
        return Err(io::Error::other(format!(
            "ignore add did not persist manual excludes: {config_text}"
        ))
        .into());
    }
    if config_text.contains("local-cache") {
        return Err(
            io::Error::other(".gitignore entry was copied into ProjectAtlas config").into(),
        );
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
        return Err(io::Error::other("ignore-policy scan command failed").into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize_at_least(&scan_json, &["overview", "files"], 1)?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "**/*", "--limit", "10"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("generated/noise.rs").not())
        .stdout(predicate::str::contains("docs/api/noise.rs").not())
        .stdout(predicate::str::contains("local-cache/noise.rs").not());

    let nested = repo.join("nested").join("work");
    fs::create_dir_all(&nested)?;
    let raw_nested_add = Command::cargo_bin("projectatlas")?
        .current_dir(&nested)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "add", "--kind", "dir-name", "nested-generated"])
        .output()?;
    if !raw_nested_add.status.success() {
        return Err(io::Error::other("nested ignore add with explicit DB failed").into());
    }
    let nested_add_json: Value = serde_json::from_slice(&raw_nested_add.stdout)?;
    require_json_string(&nested_add_json, &["value"], "nested-generated")?;
    require_json_bool(&nested_add_json, &["changed"], true)?;
    if nested.join(ATLAS_DIR_NAME).join("config.toml").exists() {
        return Err(io::Error::other("nested ignore command created a nested config").into());
    }
    let nested_config_text = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if !nested_config_text.contains(r#""nested-generated""#) {
        return Err(io::Error::other("nested ignore command did not edit project config").into());
    }

    fs::write(repo.join(".gitignore"), "local-cache/\nsrc/\n")?;
    let raw_rescan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .output()?;
    if !raw_rescan.status.success() {
        return Err(io::Error::other("ignore-policy rescan command failed").into());
    }
    let rescan_json: Value = serde_json::from_slice(&raw_rescan.stdout)?;
    require_json_usize_at_least(&rescan_json, &["overview", "files"], 1)?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "**/*", "--limit", "10"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs").not())
        .stdout(predicate::str::contains("generated/noise.rs").not())
        .stdout(predicate::str::contains("docs/api/noise.rs").not())
        .stdout(predicate::str::contains("local-cache/noise.rs").not());

    let updated_config_text = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if updated_config_text.contains("local-cache") || updated_config_text.contains(r"SRC_DIR_NAME")
    {
        return Err(
            io::Error::other(".gitignore update was copied into ProjectAtlas config").into(),
        );
    }
    if !updated_config_text.contains(r#""generated""#)
        || !updated_config_text.contains(r#""docs/api""#)
    {
        return Err(io::Error::other("manual ProjectAtlas excludes were not preserved").into());
    }

    let raw_ignored_src_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "fn main", "--file-pattern", "*.rs"])
        .output()?;
    if !raw_ignored_src_search.status.success() {
        return Err(io::Error::other("ignored src search failed").into());
    }
    let ignored_src_search_json: Value = serde_json::from_slice(&raw_ignored_src_search.stdout)?;
    require_json_usize(&ignored_src_search_json, &["returned"], 0)?;

    let raw_remove_prefix = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "remove", "--kind", "path-prefix", "docs/api"])
        .output()?;
    if !raw_remove_prefix.status.success() {
        return Err(io::Error::other("ignore remove path-prefix failed").into());
    }
    let remove_prefix_json: Value = serde_json::from_slice(&raw_remove_prefix.stdout)?;
    require_json_bool(&remove_prefix_json, &["changed"], true)?;
    require_json_string(&remove_prefix_json, &["kind"], "path-prefix")?;
    require_json_string(&remove_prefix_json, &["value"], "docs/api")?;
    let removed_config_text = fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if !removed_config_text.contains(r#""generated""#)
        || removed_config_text.contains(r#""docs/api""#)
    {
        return Err(io::Error::other(format!(
            "manual ignore remove did not edit only the requested ProjectAtlas rule: {removed_config_text}"
        ))
        .into());
    }

    let windows_prefix_config = removed_config_text.replace(
        "exclude_path_prefixes = []",
        "exclude_path_prefixes = ['docs\\api']",
    );
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        windows_prefix_config,
    )?;
    let raw_remove_windows_prefix = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["ignore", "remove", "--kind", "path-prefix", "docs/api"])
        .output()?;
    if !raw_remove_windows_prefix.status.success() {
        return Err(io::Error::other("ignore remove Windows-style path-prefix failed").into());
    }
    let remove_windows_prefix_json: Value =
        serde_json::from_slice(&raw_remove_windows_prefix.stdout)?;
    require_json_bool(&remove_windows_prefix_json, &["changed"], true)?;
    let normalized_removed_config_text =
        fs::read_to_string(repo.join(ATLAS_DIR_NAME).join("config.toml"))?;
    if normalized_removed_config_text.contains("docs\\api")
        || normalized_removed_config_text.contains("docs/api")
    {
        return Err(io::Error::other(format!(
            "Windows-style path-prefix survived normalized ignore remove: {normalized_removed_config_text}"
        ))
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["ignore", "add", "--kind", "path-prefix", "../outside"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("parent traversal is not allowed"));

    Ok(())
}

#[test]
fn default_scan_drops_stale_nodes_after_prefix_exclude_config_change() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("api"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("engine.rs"),
        "pub fn active_engine() {}\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("api").join("live.rs"),
        "pub fn live_api() {}\n",
    )?;
    fs::write(
        repo.join("docs").join("api").join("noise.rs"),
        "pub fn generated_doc_noise() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("files: 3"));

    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", ".", "--text-index-max-bytes", "2000000"])
        .assert()
        .success()
        .stdout(predicate::str::contains("files: 3"))
        .stdout(predicate::str::contains("folders: 5"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["folders", "api", "--limit", "10"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/api"))
        .stdout(predicate::str::contains("docs/api").not());

    let raw_search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--format",
            "json",
            "search",
            "generated_doc_noise",
            "--file-pattern",
            "*.rs",
        ])
        .output()?;
    if !raw_search.status.success() {
        return Err(io::Error::other("excluded stale search command failed").into());
    }
    let search_json: Value = serde_json::from_slice(&raw_search.stdout)?;
    require_json_usize(&search_json, &["returned"], 0)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("health_findings"))
        .stdout(predicate::str::contains("docs/api").not());
    Ok(())
}

#[test]
fn vue_composition_api_summary_uses_bindings() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("ProductPanel.vue"),
        r#"
<template><article>{{ currentPriceLabel }}</article></template>
<script setup lang="ts">
import { computed, ref } from "vue";

const props = withDefaults(defineProps<{
  title: string;
}>(), { title: "Product" });
const emit = defineEmits<{
  select: [id: string];
}>();
const productTitleId = computed(() => props.title.toLowerCase());
const currentPriceLabel = computed(() => `$${props.title}`);
const retryCount = ref(0);
</script>
"#,
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", "."])
        .assert()
        .success();

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["summary", "src/ProductPanel.vue", "--limit", "10"])
        .assert()
        .success()
        .stdout(predicate::str::contains("vue source defining bindings"))
        .stdout(predicate::str::contains("currentPriceLabel"))
        .stdout(predicate::str::contains("vue file,").not());

    let summary_json = json_summary_command(
        &repo,
        &repo.join(ATLAS_DIR_NAME).join("projectatlas.db"),
        "src/ProductPanel.vue",
    )?;
    require_json_string(&summary_json, &["parser_kind"], "structural-symbol-graph")?;
    require_json_string(&summary_json, &["summary_status"], "ok")?;
    Ok(())
}

#[test]
fn javascript_summary_ignores_locals_and_object_stub_methods() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("app").join("scripts"))?;
    fs::write(
        repo.join("app")
            .join("scripts")
            .join("generate-dataset-manifest.mjs"),
        r#"
import path from "node:path";
import { createHash } from "node:crypto";

const DATA_DIRECTORY = path.resolve("app/public/data");
const OUTPUT_FILE = path.join(DATA_DIRECTORY, "datasets.manifest.json");
const CACHE_NAME = (() => `sw-${Date.now()}`)();
const listenerStub = {
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {}
};

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function readDatasetEntry(filePath) {
  return sha256(filePath);
}

async function main() {
  const datasetEntries = await Promise.all(["a"].map((file) => readDatasetEntry(file)));
  const versionSeed = datasetEntries.map((entry) => entry.id).join("\n");
  return versionSeed;
}
"#,
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["scan", "."])
        .assert()
        .success();

    let raw_summary = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args([
            "--format",
            "json",
            "summary",
            "app/scripts/generate-dataset-manifest.mjs",
            "--limit",
            "20",
        ])
        .output()?;
    if !raw_summary.status.success() {
        return Err(io::Error::other("javascript summary command failed").into());
    }
    let summary_json: Value = serde_json::from_slice(&raw_summary.stdout)?;
    require_json_string(
        &summary_json,
        &["content_summary"],
        "javascript source defining functions main, readDatasetEntry, sha256 with imports import path from \"node:path\";, import { createHash } from \"node:crypto\";.",
    )?;
    require_json_usize(&summary_json, &["total_functions"], 3)?;
    require_json_usize(&summary_json, &["total_methods"], 0)?;
    let function_names = json_symbol_names(&summary_json, "functions")?;
    for expected in ["main", "readDatasetEntry", "sha256"] {
        if !function_names.iter().any(|name| name == expected) {
            return Err(io::Error::other(format!("missing function {expected}")).into());
        }
    }
    for incidental in [
        "DATA_DIRECTORY",
        "OUTPUT_FILE",
        "CACHE_NAME",
        "datasetEntries",
        "versionSeed",
    ] {
        if function_names.iter().any(|name| name == incidental) {
            return Err(io::Error::other(format!(
                "incidental binding {incidental} must not appear as a function"
            ))
            .into());
        }
    }
    let method_names = json_symbol_names(&summary_json, "methods")?;
    for stub in [
        "addListener",
        "removeListener",
        "addEventListener",
        "removeEventListener",
    ] {
        if method_names.iter().any(|name| name == stub) {
            return Err(io::Error::other(format!(
                "object literal stub {stub} must not appear as a method"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn structural_summaries_cover_declarative_files_and_projectatlas_inputs()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir_all(repo.join(".github").join("workflows"))?;
    fs::create_dir_all(repo.join("app").join("styles"))?;
    fs::create_dir_all(repo.join("app").join("public").join("data"))?;
    fs::create_dir_all(repo.join("public"))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME)
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n",
    )?;
    fs::write(repo.join(ATLAS_DIR_NAME).join("projectatlas.db"), b"db")?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("projectatlas.toon"),
        "generated map\n",
    )?;
    fs::write(
        repo.join(ATLAS_DIR_NAME).join("projectatlas.mcp.json"),
        "{}\n",
    )?;
    fs::write(
        repo.join("README.md"),
        "# ProjectAtlas Demo\n\n## Install\n## Usage\n",
    )?;
    fs::write(
        repo.join("package.json"),
        r#"{"name":"demo","scripts":{"test":"vitest"},"dependencies":{"react":"1.0.0"}}"#,
    )?;
    fs::write(
        repo.join(".github").join("workflows").join("ci.yml"),
        "name: CI\non:\n  push:\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n",
    )?;
    fs::write(
        repo.join("app").join("styles").join("tokens.css"),
        ":root { --brand: #fff; }\n.card, .panel { color: red; }\n@media (min-width: 40rem) { .card { display: grid; } }\n",
    )?;
    fs::write(
        repo.join("app")
            .join("public")
            .join("data")
            .join("datasets.manifest.json"),
        r#"{
  "generated_at": "2026-06-28T00:00:00Z",
  "version": "2026.06.28",
  "datasets": {
    "catalog.primary": {"path": "primary.json"},
    "catalog.secondary": {"path": "secondary.json"},
    "catalog.archive": {"path": "archive.json"}
  }
}"#,
    )?;
    fs::write(
        repo.join("public").join("index.html"),
        "<html><head><title>Home</title><meta name=\"description\" content=\"Welcome page\"><link rel=\"canonical\" href=\"https://example.test/\"><link rel=\"manifest\" href=\"/site.webmanifest\"><link rel=\"alternate\" href=\"/de/\"></head><body><h1>Hello</h1><script type=\"application/ld+json\">{}</script></body></html>",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("empty.rs"),
        "// no declarations yet\n",
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
        return Err(io::Error::other("structural scan command failed").into());
    }
    let scan_json: Value = serde_json::from_slice(&raw_scan.stdout)?;
    require_json_usize_at_least(&scan_json, &["structural_summaries", "summarized"], 8)?;
    require_json_usize_at_least(
        &scan_json,
        &["structural_summaries", "purpose_suggestions"],
        5,
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("config")
        .arg("--print")
        .assert()
        .success()
        .stdout(predicate::str::contains("exclude_path_prefixes"))
        .stdout(predicate::str::contains("docs/api"))
        .stdout(predicate::str::contains("source_extensions"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "projectatlas", "--limit", "20"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".projectatlas/config.toml"))
        .stdout(predicate::str::contains(
            ".projectatlas/projectatlas-nonsource-files.toon",
        ))
        .stdout(predicate::str::contains(".projectatlas/projectatlas.db").not())
        .stdout(predicate::str::contains(".projectatlas/projectatlas.toon").not())
        .stdout(predicate::str::contains(".projectatlas/projectatlas.mcp.json").not());

    let readme_summary = json_summary_command(&repo, &db, "README.md")?;
    require_json_string(
        &readme_summary,
        &["content_summary"],
        "markdown document titled ProjectAtlas Demo with sections Install, Usage.",
    )?;
    require_json_string(&readme_summary, &["parser_kind"], "structural")?;
    require_json_string(&readme_summary, &["summary_status"], "ok")?;
    require_json_string(&readme_summary, &["file_purpose_status"], "suggested")?;

    let package_summary = json_summary_command(&repo, &db, "package.json")?;
    require_json_string(
        &package_summary,
        &["content_summary"],
        "package manifest for demo with scripts test and 1 dependencies.",
    )?;

    let workflow_summary = json_summary_command(&repo, &db, ".github/workflows/ci.yml")?;
    require_json_string(
        &workflow_summary,
        &["content_summary"],
        "yaml workflow CI triggered by pull_request, push with jobs test.",
    )?;
    require_json_string(&workflow_summary, &["file_purpose_status"], "suggested")?;

    let config_summary = json_summary_command(&repo, &db, ".projectatlas/config.toml")?;
    require_json_string(
        &config_summary,
        &["content_summary"],
        "ProjectAtlas config with tables project, scan and 5 scan excludes.",
    )?;
    require_json_string(&config_summary, &["file_purpose_status"], "approved")?;

    let css_summary = json_summary_command(&repo, &db, "app/styles/tokens.css")?;
    require_json_contains(
        &css_summary,
        &["content_summary"],
        "css stylesheet with selectors .card, .panel, :root",
    )?;

    let manifest_summary =
        json_summary_command(&repo, &db, "app/public/data/datasets.manifest.json")?;
    require_json_string(
        &manifest_summary,
        &["content_summary"],
        "json dataset manifest with 3 datasets including catalog.archive, catalog.primary, catalog.secondary and keys datasets, generated_at, version.",
    )?;
    require_json_string(&manifest_summary, &["file_purpose_status"], "suggested")?;
    require_json_contains(
        &manifest_summary,
        &["file_purpose"],
        "catalog.archive, catalog.primary, catalog.secondary",
    )?;

    let html_summary = json_summary_command(&repo, &db, "public/index.html")?;
    require_json_contains(
        &html_summary,
        &["content_summary"],
        "html document with title Home, meta description Welcome page",
    )?;
    require_json_contains(
        &html_summary,
        &["content_summary"],
        "link rels alternate, canonical, manifest",
    )?;

    let rust_summary = json_summary_command(&repo, &db, "src/empty.rs")?;
    require_json_string(
        &rust_summary,
        &["content_summary"],
        "rust source file with no declarations found.",
    )?;
    require_json_string(&rust_summary, &["parser_kind"], "tree-sitter-symbol-graph")?;
    require_json_string(&rust_summary, &["summary_status"], "ok")?;

    Ok(())
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
fn notify_watch_refreshes_symbols_after_file_change() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn initial() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let mut child = StdCommand::new(executable)
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--poll-seconds", "1", "--max-cycles", "2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    thread::sleep(Duration::from_millis(750));
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn changed() {\n    initial();\n}\n\npub fn initial() {}\n",
    )?;

    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(15) {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            match child.wait() {
                Ok(_status) => {}
                Err(error) => return Err(error.into()),
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "projectatlas watch did not exit after file change",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(200));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "projectatlas watch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    if !stdout.contains("watch:") || !stdout.contains("mode: notify") {
        return Err(io::Error::other(format!(
            "projectatlas watch did not report notify mode: {stdout}"
        ))
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("changed"));
    Ok(())
}

#[test]
fn normal_reads_do_not_serve_offline_stale_index_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    for with_git_metadata in [false, true] {
        let repository_name = if with_git_metadata {
            "git-repository"
        } else {
            "local-source"
        };
        let repo = temp.path().join(repository_name);
        let db = temp.path().join(format!("{repository_name}.db"));
        exercise_normal_read_freshness(&repo, &db, with_git_metadata)?;
    }
    Ok(())
}

#[test]
fn dependency_aware_refresh_re_resolves_unchanged_inbound_callers() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let db = temp.path().join("dependency-refresh.db");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"dependency-refresh\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "mod caller;\nmod target;\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("caller.rs"),
        "pub fn caller() { target(); }\n",
    )?;
    let target = repo.join(SRC_DIR_NAME).join("target.rs");
    let duplicate = repo.join(SRC_DIR_NAME).join(DUPLICATE_RS_FILE_NAME);
    fs::write(&target, "pub fn target() {}\n")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    assert_caller_call_resolution(&db, "src/caller.rs", ExpectedCallResolution::Resolved)?;

    fs::write(&duplicate, "pub fn target() {}\n")?;
    refresh_summary_once(&repo, &db, "src/caller.rs")?;
    assert_caller_call_resolution(&db, "src/caller.rs", ExpectedCallResolution::Ambiguous(2))?;

    fs::remove_file(&duplicate)?;
    refresh_summary_once(&repo, &db, "src/caller.rs")?;
    assert_caller_call_resolution(&db, "src/caller.rs", ExpectedCallResolution::Resolved)?;

    fs::write(&target, "pub fn renamed() {}\n")?;
    refresh_summary_once(&repo, &db, "src/caller.rs")?;
    assert_caller_call_resolution(&db, "src/caller.rs", ExpectedCallResolution::Unresolved)?;

    fs::write(&duplicate, "pub fn target() {}\n")?;
    refresh_summary_once(&repo, &db, "src/caller.rs")?;
    assert_caller_call_resolution(&db, "src/caller.rs", ExpectedCallResolution::Resolved)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ExpectedCallResolution {
    Resolved,
    Ambiguous(u32),
    Unresolved,
}

fn refresh_summary_once(repo: &Path, db: &Path, file: &str) -> Result<(), Box<dyn Error>> {
    let before = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before dependency refresh"))?;
    let _summary = json_summary_command(repo, db, file)?;
    let after = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after dependency refresh"))?;
    let expected = before
        .generation
        .checked_next()
        .ok_or_else(|| io::Error::other("dependency refresh generation overflow"))?;
    if after.generation != expected {
        return Err(io::Error::other(format!(
            "dependency-aware refresh advanced from {} to {} instead of {}",
            before.generation, after.generation, expected
        ))
        .into());
    }
    Ok(())
}

fn assert_caller_call_resolution(
    db: &Path,
    caller_path: &str,
    expected: ExpectedCallResolution,
) -> Result<(), Box<dyn Error>> {
    let store = AtlasStore::open(db)?;
    let project = store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("dependency refresh project identity missing"))?;
    let caller_path = RepositoryNodePath::new(Path::new(caller_path))?;
    let caller_entities = store.repository_graph_entities_by_path(project, &caller_path, 100)?;
    let call_relations = store.repository_graph_relations(
        RepositoryGraphRelationQuery::Family {
            relation: GraphRelationKind::Legacy(RelationKind::Calls),
        },
        100,
    )?;
    let matching = call_relations
        .rows
        .iter()
        .filter(|relation| {
            caller_entities
                .rows
                .iter()
                .any(|entity| entity.key() == relation.source())
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(io::Error::other(format!(
            "expected one caller relation, found {}",
            matching.len()
        ))
        .into());
    }
    let matches = match (expected, matching[0].resolution()) {
        (ExpectedCallResolution::Resolved, RelationResolution::Resolved { .. })
        | (ExpectedCallResolution::Unresolved, RelationResolution::Unresolved { .. }) => true,
        (
            ExpectedCallResolution::Ambiguous(expected_candidates),
            RelationResolution::Ambiguous { candidates, .. },
        ) => candidates.get() == expected_candidates,
        _ => false,
    };
    if !matches {
        return Err(io::Error::other(format!(
            "unexpected caller resolution: {:?}; {}",
            matching[0].resolution(),
            graph_resolution_debug(db)?
        ))
        .into());
    }
    Ok(())
}

fn graph_resolution_debug(db: &Path) -> Result<String, Box<dyn Error>> {
    let connection = Connection::open(db)?;
    let mut statement = connection.prepare(
        "SELECT 'export', export.owner_path, key.canonical_identity
           FROM graph_entity_exports AS export
           JOIN graph_resolution_keys AS key
             ON key.project_instance_id = export.project_instance_id
            AND key.resolution_domain = export.resolution_domain
            AND key.key_digest = export.key_digest
          UNION ALL
         SELECT 'dependency', dependency.owner_path, key.canonical_identity
           FROM graph_relation_dependencies AS dependency
           JOIN graph_resolution_keys AS key
             ON key.project_instance_id = dependency.project_instance_id
            AND key.resolution_domain = dependency.resolution_domain
            AND key.key_digest = dependency.key_digest
          ORDER BY 1, 2, 3",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("resolution bindings: {rows:?}"))
}

#[test]
fn incremental_refreshes_converge_with_clean_scan_results() -> Result<(), Box<dyn Error>> {
    const CONFIG: &str = "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n";
    const CHANGED_CONFIG: &str = "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\ntext_index_max_bytes = 8192\n";

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let db = temp.path().join("incremental-convergence.db");
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(TESTS_DIR_NAME))?;
    fs::create_dir_all(repo.join(IGNORED_DIR_NAME))?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"convergence\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "mod alpha;\npub fn entry() { alpha::answer(); }\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(ALPHA_RS_FILE_NAME),
        "pub fn answer() -> u32 { 1 }\n",
    )?;
    fs::write(
        repo.join(IGNORED_DIR_NAME).join(HIDDEN_RS_FILE_NAME),
        "pub fn hidden() {}\n",
    )?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\nignored/\n")?;
    let config = repo.join(ATLAS_DIR_NAME).join("config.toml");
    fs::write(&config, CONFIG)?;

    run_scan(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "initial")?;

    let created = repo.join(SRC_DIR_NAME).join(CREATED_RS_FILE_NAME);
    fs::write(&created, "pub fn created() -> u32 { 2 }\n")?;
    let _created_summary = json_summary_command(&repo, &db, "src/created.rs")?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "create")?;

    fs::write(
        &created,
        "pub fn created() -> u32 { helper() }\nfn helper() -> u32 { 3 }\n",
    )?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "modify")?;

    let moved = repo.join(TESTS_DIR_NAME).join(CREATED_RS_FILE_NAME);
    fs::rename(&created, &moved)?;
    let moved_files = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "tests/*.rs"])
        .output()?;
    if !moved_files.status.success()
        || !String::from_utf8_lossy(&moved_files.stdout).contains("tests/created.rs")
    {
        return Err(io::Error::other(format!(
            "normal read did not reconcile the move: {}",
            String::from_utf8_lossy(&moved_files.stderr)
        ))
        .into());
    }
    assert_clean_scan_convergence(&repo, &db, temp.path(), "move")?;

    let renamed = repo.join(TESTS_DIR_NAME).join("renamed.rs");
    fs::rename(&moved, &renamed)?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "rename")?;

    fs::remove_file(repo.join(SRC_DIR_NAME).join(ALPHA_RS_FILE_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn entry() {}\n",
    )?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "delete")?;

    fs::write(
        repo.join(".gitignore"),
        ".projectatlas/\nignored/\ntests/\n",
    )?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "ignore")?;

    fs::write(repo.join(".gitignore"), ".projectatlas/\nignored/\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "unignore")?;

    let python = repo.join(TESTS_DIR_NAME).join("renamed.py");
    fs::rename(&renamed, &python)?;
    fs::write(
        &python,
        "def renamed():\n    return helper()\n\ndef helper():\n    return 3\n",
    )?;
    fs::write(&config, CHANGED_CONFIG)?;
    let stale_read = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(&db)
        .args(["summary", "tests/renamed.py"])
        .output()?;
    if stale_read.status.success()
        || !String::from_utf8_lossy(&stale_read.stderr).contains("policy_drift")
    {
        return Err(io::Error::other(format!(
            "parser/configuration change did not require a full refresh: {}",
            String::from_utf8_lossy(&stale_read.stderr)
        ))
        .into());
    }
    run_scan(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "parser-config")?;

    let before_failed_refresh = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before failed refresh"))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn entry() {}\npub fn after_retry() {}\n",
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("index work deadline was reached"));
    let after_failed_refresh = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after failed refresh"))?;
    if after_failed_refresh != before_failed_refresh {
        return Err(
            io::Error::other("failed watcher work changed the complete publication").into(),
        );
    }
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "retry")?;

    let generation_before_dirty_noop = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before dirty no-op"))?
        .generation;
    let current_source = repo.join(SRC_DIR_NAME).join("lib.rs");
    let unchanged = fs::read(&current_source)?;
    fs::write(&current_source, unchanged)?;
    let dirty_report = run_watch_once_report(&repo, &db)?;
    let generation_after_dirty_noop = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after dirty no-op"))?
        .generation;
    if generation_after_dirty_noop != generation_before_dirty_noop {
        return Err(io::Error::other(format!(
            "unchanged dirty source advanced the generation from {generation_before_dirty_noop} to {generation_after_dirty_noop}; report={dirty_report}"
        ))
        .into());
    }
    assert_clean_scan_convergence(&repo, &db, temp.path(), "dirty-noop")?;

    let repeated_report = run_watch_once_report(&repo, &db)?;
    let generation_after_repeated_noop = AtlasStore::open_read_only(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after repeated no-op"))?
        .generation;
    if generation_after_repeated_noop != generation_after_dirty_noop {
        return Err(io::Error::other(format!(
            "repeated no-change watch advanced the generation from {generation_after_dirty_noop} to {generation_after_repeated_noop}; dirty={dirty_report}; repeated={repeated_report}"
        ))
        .into());
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct DerivedResultSnapshot {
    nodes: Vec<String>,
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
    let output = Command::cargo_bin("projectatlas")?
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
    let output = Command::cargo_bin("projectatlas")?
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
        let relations = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family { relation: family },
            GRAPH_ROW_LIMIT,
        )?;
        if relations.truncated {
            return Err(io::Error::other("relation snapshot was truncated").into());
        }
        for relation in relations.rows {
            if relation.generation() != publication.generation {
                return Err(io::Error::other("relation snapshot used a mixed generation").into());
            }
            let source = store
                .repository_graph_entity(relation.source())?
                .ok_or_else(|| io::Error::other("relation source entity missing"))?;
            graph_entities.insert(serde_json::to_string(source.selector())?);
            let semantics = relation_semantics(source.selector(), &relation)?;
            graph_relations.insert(semantics.clone());
            let occurrences =
                store.repository_graph_occurrences(&relation, GRAPH_OCCURRENCE_LIMIT)?;
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

/// Exercise the same local-source freshness contract with and without Git metadata.
fn exercise_normal_read_freshness(
    repo: &Path,
    db: &Path,
    with_git_metadata: bool,
) -> Result<(), Box<dyn Error>> {
    const REQUEST_TEXT_INDEX_MAX_BYTES: &str = "65536";
    const CONFIG: &str = "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n";
    const CONTRACT_CHANGED_CONFIG: &str = "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\ntext_index_max_bytes = 1\n";
    const LEGACY_SOURCE: &str =
        "pub fn session_route() { legacy_store(); }\n\npub fn legacy_store() {}\n";
    const CURRENT_SOURCE: &str =
        "pub fn session_route() { active_store(); }\n\npub fn active_store() {}\n";

    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(repo.join(TESTS_DIR_NAME))?;
    fs::create_dir_all(repo.join(SUBDIR_CONFIG_DIR))?;
    fs::create_dir_all(repo.join(ATLAS_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("lib.rs"), LEGACY_SOURCE)?;
    fs::write(
        repo.join(TESTS_DIR_NAME).join(SESSION_TEST_FILE_NAME),
        "#[test]\nfn legacy_session_test() {}\n",
    )?;
    fs::write(
        repo.join(SUBDIR_CONFIG_DIR).join("runtime.toml"),
        "mode = \"legacy\"\n",
    )?;
    fs::write(repo.join(".gitignore"), "local-cache/\n")?;
    let config_path = repo.join(ATLAS_DIR_NAME).join("config.toml");
    fs::write(&config_path, CONFIG)?;
    if with_git_metadata {
        let output = git_command_for_root(repo)
            .args(["init", "--quiet"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "git init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args([
            "scan",
            ".",
            "--text-index-max-bytes",
            REQUEST_TEXT_INDEX_MAX_BYTES,
        ])
        .assert()
        .success();

    let baseline_summary = json_summary_command(repo, db, "src/lib.rs")?;
    let baseline_summary_text = serde_json::to_string(&baseline_summary)?;
    if !baseline_summary_text.contains("legacy_store") {
        return Err(io::Error::other(format!(
            "baseline summary did not contain legacy symbol facts: {baseline_summary_text}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["symbols", "relations", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("legacy_store"));

    fs::write(&config_path, CONTRACT_CHANGED_CONFIG)?;
    let policy_changed_read = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(db)
        .args(["summary", "src/lib.rs"])
        .output()?;
    if policy_changed_read.status.success()
        || String::from_utf8_lossy(&policy_changed_read.stdout).contains("legacy_store")
    {
        return Err(io::Error::other(format!(
            "normal read did not reject a changed index policy: stdout={} stderr={}",
            String::from_utf8_lossy(&policy_changed_read.stdout),
            String::from_utf8_lossy(&policy_changed_read.stderr)
        ))
        .into());
    }
    let policy_changed_json: Value = serde_json::from_slice(&policy_changed_read.stderr)?;
    require_json_string(&policy_changed_json, &["error", "kind"], "refresh_required")?;
    require_json_string(
        &policy_changed_json,
        &["error", "refresh_required", "reason"],
        "policy_drift",
    )?;
    require_json_string(
        &policy_changed_json,
        &["error", "refresh_required", "scope"],
        "full",
    )?;
    let refused_symbol_build = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(db)
        .args(["symbols", "build", "."])
        .output()?;
    if refused_symbol_build.status.success()
        || String::from_utf8_lossy(&refused_symbol_build.stdout).contains("legacy_store")
    {
        return Err(io::Error::other(format!(
            "symbol-only build certified a changed full-index contract: stdout={} stderr={}",
            String::from_utf8_lossy(&refused_symbol_build.stdout),
            String::from_utf8_lossy(&refused_symbol_build.stderr)
        ))
        .into());
    }
    let refused_symbol_build_json: Value = serde_json::from_slice(&refused_symbol_build.stderr)?;
    require_json_string(
        &refused_symbol_build_json,
        &["error", "kind"],
        "verification_incomplete",
    )?;
    require_json_string(
        &refused_symbol_build_json,
        &["error", "verification_incomplete", "reason"],
        "publication_contract_mismatch",
    )?;

    fs::write(&config_path, CONFIG)?;
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let capped_symbol_messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_symbols_build","arguments":{"text_index_max_bytes":1}}}"#.to_string(),
    ];
    let capped_symbol_stdout = run_mcp_stdio(
        &executable,
        repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &capped_symbol_messages,
    )?;
    let capped_symbol_text = mcp_tool_text(&capped_symbol_stdout, 2)?;
    if capped_symbol_text.contains("kind: verification_incomplete")
        || capped_symbol_text.contains("reason: publication_contract_mismatch")
    {
        return Err(io::Error::other(format!(
            "MCP symbol-only request cap redefined the full publication contract: {capped_symbol_text}"
        ))
        .into());
    }
    let restored_summary = json_summary_command(repo, db, "src/lib.rs")?;
    require_json_contains(&restored_summary, &["content_summary"], "legacy_store")?;

    fs::write(&config_path, "[scan\n")?;
    let incomplete_cli = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(db)
        .args(["summary", "src/lib.rs"])
        .output()?;
    if incomplete_cli.status.success()
        || !String::from_utf8_lossy(&incomplete_cli.stderr).contains("verification_incomplete")
        || String::from_utf8_lossy(&incomplete_cli.stdout).contains("legacy_store")
    {
        return Err(io::Error::other(format!(
            "incomplete CLI verification did not fail closed: stdout={} stderr={}",
            String::from_utf8_lossy(&incomplete_cli.stdout),
            String::from_utf8_lossy(&incomplete_cli.stderr)
        ))
        .into());
    }
    let incomplete_cli_json: Value = serde_json::from_slice(&incomplete_cli.stderr)?;
    require_json_string(
        &incomplete_cli_json,
        &["error", "kind"],
        "verification_incomplete",
    )?;
    let incomplete_messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
    ];
    let incomplete_stdout = run_mcp_stdio(
        &executable,
        repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &incomplete_messages,
    )?;
    let incomplete_text = mcp_tool_text(&incomplete_stdout, 2)?;
    if !incomplete_text.contains("kind: verification_incomplete")
        || !incomplete_text.contains("status: verification_incomplete")
        || incomplete_text.contains("legacy_store")
    {
        return Err(io::Error::other(format!(
            "incomplete MCP verification did not return the typed fail-closed state: {incomplete_text}"
        ))
        .into());
    }

    fs::write(&config_path, CONFIG)?;
    let source_path = repo.join(SRC_DIR_NAME).join("lib.rs");
    let indexed_modified = fs::metadata(&source_path)?.modified()?;
    fs::write(&source_path, CURRENT_SOURCE)?;
    fs::File::options()
        .write(true)
        .open(&source_path)?
        .set_times(fs::FileTimes::new().set_modified(indexed_modified))?;
    let changed_metadata = fs::metadata(&source_path)?;
    if changed_metadata.len() != u64::try_from(LEGACY_SOURCE.len())?
        || changed_metadata.modified()? != indexed_modified
    {
        return Err(io::Error::other(
            "freshness fixture did not preserve indexed size and modification time",
        )
        .into());
    }

    let publication_before_automatic_refresh = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before automatic refresh"))?;
    let automatically_refreshed_text = if with_git_metadata {
        let messages = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
        ];
        let stdout = run_mcp_stdio(
            &executable,
            repo,
            &[
                "--db".to_string(),
                db.display().to_string(),
                "mcp".to_string(),
            ],
            &messages,
        )?;
        mcp_tool_text(&stdout, 2)?
    } else {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(repo)
            .args(["--format", "json"])
            .arg("--db")
            .arg(db)
            .args(["summary", "src/lib.rs"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "safe stale CLI read did not reconcile automatically: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        String::from_utf8(output.stdout)?
    };
    if !automatically_refreshed_text.contains("active_store")
        || automatically_refreshed_text.contains("legacy_store")
        || automatically_refreshed_text.contains("refresh_required")
    {
        return Err(io::Error::other(format!(
            "safe normal read did not return current local source after reconciliation: {automatically_refreshed_text}"
        ))
        .into());
    }
    let publication_after_automatic_refresh = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after automatic refresh"))?;
    if publication_after_automatic_refresh.generation
        != publication_before_automatic_refresh
            .generation
            .checked_next()
            .ok_or_else(|| io::Error::other("publication generation overflow"))?
    {
        return Err(io::Error::other(
            "safe automatic refresh did not publish exactly one generation",
        )
        .into());
    }

    let publication_before_automatic_rename = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before automatic rename refresh"))?;
    fs::rename(
        repo.join(TESTS_DIR_NAME).join(SESSION_TEST_FILE_NAME),
        repo.join(TESTS_DIR_NAME).join("current_session.rs"),
    )?;
    let renamed_files = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["files", "--file-pattern", "tests/*.rs"])
        .output()?;
    let renamed_files_text = String::from_utf8(renamed_files.stdout)?;
    if !renamed_files.status.success()
        || !renamed_files_text.contains("tests/current_session.rs")
        || renamed_files_text.contains("tests/session.rs")
        || renamed_files_text.contains("refresh_required")
    {
        return Err(io::Error::other(format!(
            "safe rename did not reconcile automatically: {renamed_files_text}"
        ))
        .into());
    }
    let publication_after_automatic_rename = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after automatic rename refresh"))?;
    if publication_after_automatic_rename.generation
        != publication_before_automatic_rename
            .generation
            .checked_next()
            .ok_or_else(|| io::Error::other("publication generation overflow"))?
    {
        return Err(io::Error::other(
            "safe automatic rename refresh did not publish exactly one generation",
        )
        .into());
    }
    fs::write(repo.join(".gitignore"), "config/\n")?;

    let deleted_absolute_selector = repo
        .join(TESTS_DIR_NAME)
        .join(SESSION_TEST_FILE_NAME)
        .to_string_lossy()
        .to_string();
    let stale_messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_search","arguments":{"pattern":"legacy_store","file_pattern":"*.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"tests/*.rs"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"atlas_slice","arguments":{"file":"src/lib.rs","symbol":"legacy_store"}}}"#.to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":deleted_absolute_selector}}}).to_string(),
    ];
    let stale_stdout = run_mcp_stdio(
        &executable,
        repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &stale_messages,
    )?;
    for id in 2..=7 {
        let tool_text = mcp_tool_text(&stale_stdout, id)?;
        if !tool_text.contains("kind: refresh_required")
            || !tool_text.contains("status: refresh_required")
            || !tool_text.contains("tool: atlas_watch_once")
            || !tool_text.contains("changed:")
            || tool_text.contains("changed: 0")
            || tool_text.contains("legacy_store")
        {
            return Err(io::Error::other(format!(
                "stale MCP read {id} did not return the typed fail-closed state: {tool_text}"
            ))
            .into());
        }
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("watch:"));

    let current_summary = json_summary_command(repo, db, "src/lib.rs")?;
    let current_summary_text = serde_json::to_string(&current_summary)?;
    if !current_summary_text.contains("active_store")
        || current_summary_text.contains("legacy_store")
    {
        return Err(io::Error::other(format!(
            "refreshed summary did not reflect current local source: {current_summary_text}"
        ))
        .into());
    }
    Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["symbols", "relations", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active_store"))
        .stdout(predicate::str::contains("legacy_store").not());
    Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--db")
        .arg(db)
        .args(["files", "--file-pattern", "tests/*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tests/current_session.rs"))
        .stdout(predicate::str::contains("tests/session.rs").not());
    let old_search = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .args(["search", "legacy_store", "--file-pattern", "*.rs"])
        .output()?;
    if !old_search.status.success() {
        return Err(io::Error::other("old-symbol search failed after refresh").into());
    }
    let old_search_json: Value = serde_json::from_slice(&old_search.stdout)?;
    require_json_usize(&old_search_json, &["returned"], 0)?;

    let fresh_messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_file_summary","arguments":{"file":"src/lib.rs"}}}"#.to_string(),
    ];
    let fresh_stdout = run_mcp_stdio(
        &executable,
        repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &fresh_messages,
    )?;
    let fresh_text = mcp_tool_text(&fresh_stdout, 2)?;
    if !fresh_text.contains("active_store")
        || fresh_text.contains("legacy_store")
        || fresh_text.contains("refresh_required")
    {
        return Err(io::Error::other(format!(
            "unchanged MCP read did not stay current after refresh: {fresh_text}"
        ))
        .into());
    }

    let mut interrupted_store = AtlasStore::open(db)?;
    let publication_before = interrupted_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("complete publication metadata was missing"))?;
    let contract_fingerprint = publication_before
        .contract_fingerprint
        .as_deref()
        .ok_or_else(|| io::Error::other("complete publication fingerprint was missing"))?;
    let node_before = interrupted_store
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("complete source node was missing"))?;
    let text_before = interrupted_store
        .load_file_text("src/lib.rs")?
        .ok_or_else(|| io::Error::other("complete indexed text was missing"))?;
    let symbols_before = interrupted_store.load_symbols(Some("src/lib.rs"), None, 100)?;
    let relations_before =
        interrupted_store.load_symbol_relations(Some("src/lib.rs"), None, 100)?;
    let parse_metadata_before = interrupted_store
        .load_source_parse_metadata("src/lib.rs")?
        .ok_or_else(|| io::Error::other("complete source parse metadata was missing"))?;
    let mut interrupted_publication =
        interrupted_store.begin_index_publication(contract_fingerprint)?;
    let mut staged_node = node_before.node.clone();
    staged_node.content_hash = Some("staged-hash".to_string());
    interrupted_publication.upsert_scan_nodes(&[staged_node])?;
    interrupted_publication.replace_file_texts_for_paths(
        &["src/lib.rs".to_string()],
        &[IndexedFileText {
            path: "src/lib.rs".to_string(),
            content_hash: Some("staged-hash".to_string()),
            byte_count: 15,
            line_count: 1,
            content: "staged content\n".to_string(),
        }],
    )?;
    interrupted_publication.replace_symbol_graph(&SymbolGraph {
        path: "src/lib.rs".to_string(),
        language: Some("rust".to_string()),
        parser: ParserKind::TreeSitter,
        symbols: vec![CodeSymbol {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            name: "staged_symbol".to_string(),
            kind: SymbolKind::Function,
            signature: "fn staged_symbol()".to_string(),
            exported: true,
            documentation: None,
            line_start: 1,
            line_end: 1,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }],
        relations: vec![SymbolRelation {
            path: "src/lib.rs".to_string(),
            source_name: "staged_symbol".to_string(),
            target_name: "staged_target".to_string(),
            kind: RelationKind::Calls,
            line: 1,
            context: "staged_target();".to_string(),
            parser: ParserKind::TreeSitter,
        }],
    })?;
    interrupted_publication.set_node_summary("src/lib.rs", "staged summary")?;
    let interrupted_read = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .args(["--format", "json"])
        .arg("--db")
        .arg(db)
        .args(["summary", "src/lib.rs"])
        .output()?;
    if !interrupted_read.status.success() {
        return Err(io::Error::other(format!(
            "reader could not use the prior complete generation during publication: {}",
            String::from_utf8_lossy(&interrupted_read.stderr)
        ))
        .into());
    }
    let interrupted_json: Value = serde_json::from_slice(&interrupted_read.stdout)?;
    let interrupted_text = serde_json::to_string(&interrupted_json)?;
    if !interrupted_text.contains("active_store") || interrupted_text.contains("legacy_store") {
        return Err(io::Error::other(format!(
            "reader observed a mixed or staged generation: {interrupted_text}"
        ))
        .into());
    }
    drop(interrupted_publication);
    if interrupted_store.index_publication()? != Some(publication_before)
        || interrupted_store.load_node_by_path("src/lib.rs")? != Some(node_before)
        || interrupted_store.load_file_text("src/lib.rs")? != Some(text_before)
        || interrupted_store.load_symbols(Some("src/lib.rs"), None, 100)? != symbols_before
        || interrupted_store.load_symbol_relations(Some("src/lib.rs"), None, 100)?
            != relations_before
        || interrupted_store.load_source_parse_metadata("src/lib.rs")?
            != Some(parse_metadata_before)
    {
        return Err(io::Error::other(
            "dropped publication did not roll back every staged mutation and generation change",
        )
        .into());
    }
    let generation_before_repeated_read = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing before repeated read"))?
        .generation;
    let repeated_summary = json_summary_command(repo, db, "src/lib.rs")?;
    if repeated_summary != current_summary {
        return Err(io::Error::other("unchanged repeated read drifted after refresh").into());
    }
    let generation_after_repeated_read = AtlasStore::open(db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("publication missing after repeated read"))?
        .generation;
    if generation_after_repeated_read != generation_before_repeated_read {
        return Err(io::Error::other(
            "unchanged repeated read advanced the publication generation",
        )
        .into());
    }
    Ok(())
}

fn git_command_for_root(root: &Path) -> StdCommand {
    let mut command = StdCommand::new("git");
    command.current_dir(root);
    for variable in GIT_REPOSITORY_ENVIRONMENT_VARIABLES {
        command.env_remove(variable);
    }
    command
}

#[test]
fn watch_once_preserves_unchanged_deep_summary_and_text_index() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "use std::fs;\npub fn helper() {}\npub fn main() { helper(); }\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("text_index:"))
        .stdout(predicate::str::contains("indexed: 1"));

    {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            ".",
            "Reviewed repository root for deep refresh preservation tests.",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            SRC_DIR_NAME,
            "Reviewed source folder for deep refresh preservation tests.",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/main.rs",
            "Reviewed Rust entrypoint for deep refresh preservation tests.",
            PurposeSource::Agent,
        )?;
    }

    let before = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/main.rs"])
        .output()?;
    if !before.status.success() {
        return Err(io::Error::other("summary before watch failed").into());
    }
    let before_json: Value = serde_json::from_slice(&before.stdout)?;
    let before_summary = json_at(&before_json, &["content_summary"])?
        .as_str()
        .ok_or_else(|| io::Error::other("content summary before watch was not a string"))?
        .to_string();
    if !before_summary.contains("helper") {
        return Err(io::Error::other("summary before watch did not include symbol facts").into());
    }
    require_json_string(
        &before_json,
        &["file_purpose"],
        "Reviewed Rust entrypoint for deep refresh preservation tests.",
    )?;
    require_json_string(&before_json, &["file_purpose_source"], "agent")?;
    require_json_bool(&before_json, &["file_purpose_agent_reviewed"], true)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parsed: 0"))
        .stdout(predicate::str::contains("unchanged: 0"));

    let after = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/main.rs"])
        .output()?;
    if !after.status.success() {
        return Err(io::Error::other("summary after watch failed").into());
    }
    let after_json: Value = serde_json::from_slice(&after.stdout)?;
    require_json_string(&after_json, &["content_summary"], &before_summary)?;
    require_json_string(
        &after_json,
        &["file_purpose"],
        "Reviewed Rust entrypoint for deep refresh preservation tests.",
    )?;
    require_json_string(&after_json, &["file_purpose_source"], "agent")?;
    require_json_bool(&after_json, &["file_purpose_agent_reviewed"], true)?;

    let search = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db)
        .args(["search", "helper", "--file-pattern", "*.rs"])
        .output()?;
    if !search.status.success() {
        return Err(io::Error::other("indexed search after watch failed").into());
    }
    let search_json: Value = serde_json::from_slice(&search.stdout)?;
    require_json_string(&search_json, &["source"], "sqlite-file-text")?;
    require_json_usize_at_least(&search_json, &["returned"], 1)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_purposes: 0"));

    let final_summary = json_summary_command(&repo, &db, "src/main.rs")?;
    require_json_string(
        &final_summary,
        &["file_purpose"],
        "Reviewed Rust entrypoint for deep refresh preservation tests.",
    )?;
    require_json_string(&final_summary, &["file_purpose_source"], "agent")?;
    require_json_bool(&final_summary, &["file_purpose_agent_reviewed"], true)?;
    let final_store = AtlasStore::open(&db)?;
    for (path, purpose) in [
        (
            ".",
            "Reviewed repository root for deep refresh preservation tests.",
        ),
        (
            SRC_DIR_NAME,
            "Reviewed source folder for deep refresh preservation tests.",
        ),
    ] {
        let node = final_store
            .load_node_by_path(path)?
            .ok_or_else(|| io::Error::other(format!("{path} missing after deep refresh")))?;
        if node.purpose.source != PurposeSource::Agent
            || node.purpose.purpose.as_deref() != Some(purpose)
            || !node.purpose.agent_reviewed()
        {
            return Err(io::Error::other(format!(
                "deep refresh did not preserve reviewed purpose for {path}: {:?}",
                node.purpose
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn watch_once_skips_unchanged_empty_native_parse() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("empty.rs"),
        "// comment only\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("parsed: 1"));

    let before = json_summary_command(&repo, &db, "src/empty.rs")?;
    require_json_string(&before, &["parser_kind"], "tree-sitter-symbol-graph")?;
    require_json_string(&before, &["summary_status"], "ok")?;
    require_json_string(
        &before,
        &["content_summary"],
        "rust source file with no declarations found.",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parsed: 0"))
        .stdout(predicate::str::contains("unchanged: 0"));

    let after = json_summary_command(&repo, &db, "src/empty.rs")?;
    require_json_string(&after, &["parser_kind"], "tree-sitter-symbol-graph")?;
    require_json_string(&after, &["summary_status"], "ok")?;
    Ok(())
}

#[test]
fn watch_once_preserves_manifest_symbol_summary() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"manifest-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let before = json_summary_command(&repo, &db, "Cargo.toml")?;
    require_json_string(&before, &["parser_kind"], "manifest-symbol-graph")?;
    require_json_string(&before, &["summary_status"], "ok")?;
    let before_summary = json_at(&before, &["content_summary"])?
        .as_str()
        .ok_or_else(|| io::Error::other("manifest summary before watch was not a string"))?
        .to_string();
    if !before_summary.contains("depending on serde") {
        return Err(io::Error::other(format!(
            "manifest summary did not include dependency facts before watch: {before_summary}"
        ))
        .into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parsed: 0"))
        .stdout(predicate::str::contains("unchanged: 0"));

    let after = json_summary_command(&repo, &db, "Cargo.toml")?;
    require_json_string(&after, &["parser_kind"], "manifest-symbol-graph")?;
    require_json_string(&after, &["summary_status"], "ok")?;
    require_json_string(&after, &["content_summary"], &before_summary)?;
    Ok(())
}

#[test]
fn watch_once_detects_new_files_folders_text_and_symbols() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn existing() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    fs::create_dir_all(repo.join(SRC_DIR_NAME).join("feature"))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("feature").join("new_file.rs"),
        "pub fn auto_detected_new_file() {}\n",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parsed: 1"))
        .stdout(predicate::str::contains("indexed: 2"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["folders", "feature"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/feature"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "new_file", "--folder", "src/feature"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/feature/new_file.rs"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/feature/new_file.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auto_detected_new_file"));
    Ok(())
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
fn conditional_purpose_review_cli_rejects_replayed_queue_work() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("main.rs"),
        "fn main() { run(); }\nfn run() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    let queue_output = Command::cargo_bin("projectatlas")?
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

    let apply_output = Command::cargo_bin("projectatlas")?
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
            "conditional purpose review failed: {}",
            String::from_utf8_lossy(&apply_output.stderr)
        ))
        .into());
    }
    let applied: Value = serde_json::from_slice(&apply_output.stdout)?;
    require_json_usize(&applied, &["changed"], 1)?;
    require_json_usize(&applied, &["conflicts"], 0)?;
    require_json_string(&applied, &["items", "0", "action"], "review")?;

    let repeat_output = Command::cargo_bin("projectatlas")?
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
        "Application entry point coordinating run.",
    )?;
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
    let fresh_strict_stderr = String::from_utf8(fresh_strict.stderr)?;
    if !fresh_strict_stderr.contains("[missing-purpose]")
        && !fresh_strict_stderr.contains("[suggested-purpose-review]")
    {
        return Err(io::Error::other(format!(
            "fresh strict purpose lint did not report missing or suggested purposes:\n{fresh_strict_stderr}"
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
    let low_stderr = String::from_utf8(low.stderr)?;
    for unexpected in [
        "purpose-agent-review-required",
        "src/detail.rs",
        "assets/logo.svg",
    ] {
        if low_stderr.contains(unexpected) {
            return Err(io::Error::other(format!(
                "low purpose lint should not block on advisory curation work `{unexpected}`:\n{low_stderr}"
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
    let medium_stderr = String::from_utf8(medium.stderr)?;
    if !medium_stderr.contains("[purpose-agent-review-required] src/detail.rs:") {
        return Err(io::Error::other(format!(
            "medium purpose lint missed source file:\n{medium_stderr}"
        ))
        .into());
    }
    if medium_stderr.contains("assets/logo.svg") {
        return Err(io::Error::other(format!(
            "medium purpose lint included asset file:\n{medium_stderr}"
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
    let strict_stderr = String::from_utf8(strict.stderr)?;
    if !strict_stderr.contains("[purpose-agent-review-required] assets/logo.svg:") {
        return Err(io::Error::other(format!(
            "strict purpose lint missed asset file:\n{strict_stderr}"
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
    let changed_low_stderr = String::from_utf8(changed_low.stderr)?;
    if changed_low_stderr.contains("[stale-purpose]") {
        return Err(io::Error::other(format!(
            "ordinary source changes produced stale-purpose findings:\n{changed_low_stderr}"
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

struct CliHelpSurface {
    subcommands: Vec<String>,
    defaults: Vec<String>,
    possible_values: Vec<String>,
    help_present: bool,
}

/// Read one packaged help route and retain its command/default contract.
fn cli_help_surface(executable: &Path, route: &str) -> Result<CliHelpSurface, Box<dyn Error>> {
    let output = StdCommand::new(executable)
        .args(route.split_whitespace())
        .arg("--help")
        .output()?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "packaged CLI help failed for {route:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let help = String::from_utf8(output.stdout)?.replace("\r\n", "\n");
    let mut subcommands = Vec::new();
    let mut help_present = false;
    let mut in_commands = false;
    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if in_commands && line.trim().is_empty() {
            break;
        }
        if in_commands {
            let name = line.split_whitespace().next().unwrap_or("");
            if name == "help" {
                help_present = true;
            } else if !name.is_empty() {
                subcommands.push(name.to_string());
            }
        }
    }
    let possible_values = help
        .lines()
        .skip_while(|line| line.trim() != "Possible values:")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("- ")
                .and_then(|value| value.split(':').next())
                .map(str::to_string)
        })
        .collect();
    let mut defaults = Vec::new();
    let mut default_owner = None;
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--")
            || trimmed.starts_with('<')
            || (trimmed.starts_with('[') && !trimmed.starts_with("[default:"))
        {
            default_owner = trimmed.split_whitespace().next().map(str::to_string);
        }
        if let Some((_, tail)) = trimmed.split_once("[default: ") {
            let (value, _) = tail.split_once(']').ok_or_else(|| {
                io::Error::other(format!("unterminated default in {route:?} help"))
            })?;
            let owner = default_owner.as_deref().ok_or_else(|| {
                io::Error::other(format!("unowned default {value:?} in {route:?} help"))
            })?;
            defaults.push(format!("{owner}={value}"));
        }
    }
    Ok(CliHelpSurface {
        subcommands,
        defaults,
        possible_values,
        help_present,
    })
}

/// Decode one ordered string array from the frozen CLI surface fixture.
fn cli_value_strings(value: &Value, label: &str) -> Result<Vec<String>, Box<dyn Error>> {
    value
        .as_array()
        .ok_or_else(|| io::Error::other(format!("{label:?} CLI fixture value was not an array")))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                io::Error::other(format!("{label:?} CLI fixture contained a non-string")).into()
            })
        })
        .collect()
}

/// Decode one ordered string array at a frozen CLI fixture path.
fn cli_surface_strings(value: &Value, path: &[&str]) -> Result<Vec<String>, Box<dyn Error>> {
    cli_value_strings(json_at(value, path)?, &path.join("."))
}

/// Return whether every older value survives in order among additive defaults.
fn ordered_subsequence(expected: &[String], actual: &[String]) -> bool {
    let mut actual = actual.iter();
    expected
        .iter()
        .all(|expected| actual.by_ref().any(|actual| actual == expected))
}

/// Return the explicitly selected packaged runtime or the local test binary.
fn mcp_contract_executable() -> PathBuf {
    std::env::var_os(MCP_CONTRACT_EXECUTABLE_ENV).map_or_else(
        || assert_cmd::cargo::cargo_bin("projectatlas"),
        PathBuf::from,
    )
}

/// Require the selected runtime and plugin skill to be the exact workspace release candidate.
fn assert_mcp_contract_runtime_and_skill(executable: &Path) -> Result<(), Box<dyn Error>> {
    let runtime = run_mcp_contract_json(
        executable,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[
            "--require-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "runtime-info".to_string(),
        ],
    )?;
    require_json_string(&runtime, &["version"], env!("CARGO_PKG_VERSION"))?;
    let plugin_root = if let Some(plugin_root) = std::env::var_os(MCP_CONTRACT_PLUGIN_ROOT_ENV) {
        PathBuf::from(plugin_root)
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .ok_or_else(|| io::Error::other("MCP contract workspace root was not found"))?
            .join("plugins")
            .join("projectatlas")
    };
    let manifest: Value = serde_json::from_slice(&fs::read(
        plugin_root.join(".codex-plugin").join("plugin.json"),
    )?)?;
    require_json_string(&manifest, &["version"], env!("CARGO_PKG_VERSION"))?;
    let skill = fs::read_to_string(
        plugin_root
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
    )?;
    for route in [
        "atlas_session_brief",
        "atlas_file_summary",
        "atlas_symbol_relations",
        "atlas_slice",
    ] {
        if !skill.contains(route) {
            return Err(io::Error::other(format!(
                "release-candidate ProjectAtlas skill omitted {route}"
            ))
            .into());
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

/// Run one packaged CLI command and enforce its stream and typed-output contract.
fn run_packaged_cli_contract_case(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    case: &CliContractCase,
) -> Result<Option<Value>, Box<dyn Error>> {
    if matches!(case.output, CliContractOutput::Mcp) {
        let stdout = run_mcp_contract_inventory(executable, cwd, database)?;
        let response = mcp_response(&stdout, 2)?;
        let tools = json_at(&response, &["result", "tools"])?
            .as_array()
            .ok_or_else(|| io::Error::other("packaged CLI mcp omitted its tool inventory"))?;
        if tools.is_empty() {
            return Err(io::Error::other("packaged CLI mcp advertised no tools").into());
        }
        assert_cli_contract_payload(case.name, &response)?;
        return Ok(Some(response));
    }

    let output = StdCommand::new(executable)
        .current_dir(cwd)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--require-version",
            env!("CARGO_PKG_VERSION"),
            "--format",
            "json",
            "--db",
        ])
        .arg(database)
        .args(&case.arguments)
        .output()?;
    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code != case.expected_exit_code {
        return Err(io::Error::other(format!(
            "{} exited {exit_code} instead of {}: stdout={} stderr={}",
            case.name,
            case.expected_exit_code,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    match case.output {
        CliContractOutput::JsonObject | CliContractOutput::JsonArray => {
            if !output.stderr.is_empty() {
                return Err(io::Error::other(format!(
                    "{} wrote typed success output to stderr: {}",
                    case.name,
                    String::from_utf8_lossy(&output.stderr)
                ))
                .into());
            }
            let decoded: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "{} emitted invalid JSON: {error}; stdout={}",
                    case.name,
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
            let expected_shape = match case.output {
                CliContractOutput::JsonObject => decoded.is_object(),
                CliContractOutput::JsonArray => decoded.is_array(),
                CliContractOutput::Empty | CliContractOutput::Mcp => false,
            };
            if !expected_shape {
                return Err(io::Error::other(format!(
                    "{} emitted the wrong JSON shape: {decoded}",
                    case.name
                ))
                .into());
            }
            assert_cli_contract_payload(case.name, &decoded)?;
            Ok(Some(decoded))
        }
        CliContractOutput::Empty => {
            if !output.stdout.is_empty() || !output.stderr.is_empty() {
                return Err(io::Error::other(format!(
                    "{} unexpectedly wrote output: stdout={} stderr={}",
                    case.name,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
                .into());
            }
            Ok(None)
        }
        CliContractOutput::Mcp => unreachable!("MCP output returned above"),
    }
}

/// Require one behavior-owned discriminator from every packaged CLI success payload.
fn assert_cli_contract_payload(name: &str, payload: &Value) -> Result<(), Box<dyn Error>> {
    match name {
        "init" => {
            require_json_bool(payload, &["ok"], true)?;
            require_json_array_len(payload, &["host_configs"], 3)?;
        }
        "map" | "lint" => {}
        "scan" => {
            require_json_usize_at_least(payload, &["overview", "files"], 1)?;
            require_json_usize_at_least(payload, &["symbols", "parsed"], 1)?;
        }
        "overview" => require_json_usize_at_least(payload, &["files"], 1)?,
        "folders" => require_json_string(payload, &["0", "path"], SRC_DIR_NAME)?,
        "files" => require_json_string(payload, &["0", "path"], "src/duplicate.rs")?,
        "next" => {
            require_json_string(payload, &["query"], "contract")?;
            require_json_string(
                payload,
                &["files", "0", "next_call", "capability"],
                "summary",
            )?;
        }
        "outline" => require_json_string(payload, &["path"], "src/lib.rs")?,
        "summary" => {
            require_json_string(payload, &["file_path"], "src/lib.rs")?;
            require_json_string(payload, &["source_status"], "live-source")?;
            require_json_string(payload, &["coverage", "trust"], "trusted")?;
        }
        "search" => {
            require_json_string(payload, &["query"], "contract")?;
            require_json_bool(payload, &["truncated"], true)?;
            require_json_string(payload, &["truncation_reason"], "result-limit")?;
        }
        "slice" => {
            require_json_string(payload, &["path"], "src/lib.rs")?;
            require_json_contains(payload, &["content"], "indexed")?;
        }
        "symbols" => {
            require_json_usize_at_least(payload, &["parsed"], 1)?;
            require_json_usize_at_least(payload, &["symbols"], 1)?;
        }
        "settings" => {
            require_json_bool(payload, &["root_verified"], true)?;
            require_json_string(payload, &["database", "schema", "compatibility"], "current")?;
            require_json_string(payload, &["database", "publication", "state"], "complete")?;
            require_json_usize_at_least(payload, &["database", "publication", "generation"], 1)?;
        }
        "snapshot" => {
            let digest = json_at(payload, &["snapshot_digest"])?
                .as_str()
                .ok_or_else(|| io::Error::other("snapshot digest was not a string"))?;
            if digest.len() != 64
                || !digest
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            {
                return Err(io::Error::other(format!(
                    "snapshot digest was not a SHA-256 hex digest: {digest:?}"
                ))
                .into());
            }
            require_json_string(payload, &["signature"], "unsigned_local")?;
        }
        "parser-pack" => {
            require_json_string(payload, &["operation"], "status")?;
            require_json_string(payload, &["pack_id"], "broad-parser")?;
        }
        "root" => {
            require_json_bool(payload, &["verified"], true)?;
            require_json_string(payload, &["runtime_version"], env!("CARGO_PKG_VERSION"))?;
        }
        "config" => {
            let extensions = json_at(payload, &["source_extensions"])?
                .as_array()
                .ok_or_else(|| io::Error::other("config source_extensions was not an array"))?;
            if extensions.is_empty() {
                return Err(io::Error::other("config advertised no source extensions").into());
            }
        }
        "ignore" => {
            require_json_bool(payload, &["gitignore_present"], true)?;
            require_json_string(payload, &["manual_layer_order"], "after-gitignore")?;
        }
        "watch-status" => require_json_contains(payload, &["recommendation"], "watch")?,
        "watch" => {
            require_json_string(payload, &["mode"], "single-refresh")?;
            require_json_usize(payload, &["cycles"], 1)?;
        }
        "health-check" => require_json_usize_at_least(payload, &["total"], 1)?,
        "health" => {
            require_json_string(payload, &["category"], "duplicate-purpose")?;
            require_json_string(payload, &["path"], "src/lib.rs")?;
        }
        "token" => require_json_string(payload, &["detail_availability"], "retained")?,
        "parity" => {
            require_json_string(payload, &["profile"], "repository-intelligence")?;
            require_json_bool(payload, &["ok"], true)?;
        }
        "strip-legacy-purpose" => require_json_bool(payload, &["applied"], false)?,
        "reset-index" => require_json_bool(payload, &["dry_run"], true)?,
        "mcp" => require_json_array_len(payload, &["result", "tools"], 40)?,
        "mcp-config" => {
            let arguments = json_at(payload, &["mcpServers", "projectatlas", "args"])?
                .as_array()
                .ok_or_else(|| io::Error::other("mcp-config args was not an array"))?;
            if !arguments.iter().any(|argument| argument == "mcp")
                || !arguments
                    .iter()
                    .any(|argument| argument == env!("CARGO_PKG_VERSION"))
            {
                return Err(io::Error::other(
                    "mcp-config omitted the command or exact runtime-version guard",
                )
                .into());
            }
        }
        "runtime-info" => {
            require_json_string(payload, &["version"], env!("CARGO_PKG_VERSION"))?;
            require_json_array_len(payload, &["mcp_tools"], 40)?;
        }
        "purpose" => {
            require_json_string(payload, &["purpose_set", "path"], "src/watched.rs")?;
            require_json_bool(payload, &["purpose_set", "agent_reviewed"], true)?;
        }
        unknown => {
            return Err(io::Error::other(format!(
                "packaged CLI contract has no payload discriminator for {unknown}"
            ))
            .into());
        }
    }
    Ok(())
}

/// Verify the small filesystem surface owned by packaged commands in the main table.
fn assert_cli_contract_filesystem_effect(name: &str, repo: &Path) -> Result<(), Box<dyn Error>> {
    let atlas = repo.join(ATLAS_DIR_NAME);
    match name {
        "init" => {
            for relative in [
                "config.toml",
                "projectatlas-nonsource-files.toon",
                "projectatlas.db",
                "projectatlas.mcp.json",
                "projectatlas.claude.mcp.json",
                "projectatlas.opencode.json",
            ] {
                let path = atlas.join(relative);
                if !path.is_file() || fs::metadata(&path)?.len() == 0 {
                    return Err(io::Error::other(format!(
                        "packaged init omitted owned artifact {}",
                        path.display()
                    ))
                    .into());
                }
            }
        }
        "map" => {
            let path = atlas.join("projectatlas.toon");
            let map = fs::read_to_string(&path).map_err(|error| {
                io::Error::other(format!(
                    "packaged map omitted readable artifact {}: {error}",
                    path.display()
                ))
            })?;
            if !map.contains("src/lib.rs") {
                return Err(io::Error::other(
                    "packaged map artifact omitted the indexed Rust source",
                )
                .into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Verify a packaged snapshot is a bounded readable archive with both owned entries.
fn assert_cli_snapshot_archive(path: &Path) -> Result<(), Box<dyn Error>> {
    let decoder = zstd::stream::read::Decoder::new(fs::File::open(path)?)?;
    let mut archive = tar::Archive::new(decoder);
    let mut entries = Vec::new();
    for entry in archive.entries()? {
        let entry = entry?;
        if !entry.header().entry_type().is_file() {
            return Err(io::Error::other("snapshot archive contained a non-file entry").into());
        }
        entries.push(entry.path()?.to_string_lossy().replace('\\', "/"));
    }
    entries.sort();
    let expected = vec![
        "projectatlas-derived-snapshot/graph.json".to_string(),
        "projectatlas-derived-snapshot/manifest.json".to_string(),
    ];
    if entries != expected {
        return Err(io::Error::other(format!(
            "snapshot archive entries drifted: actual={entries:?} expected={expected:?}"
        ))
        .into());
    }
    Ok(())
}

/// Prove first-run init owns exactly its declared non-database repository files.
fn assert_packaged_cli_first_init_filesystem(
    executable: &Path,
    temp: &Path,
) -> Result<(), Box<dyn Error>> {
    let repo = temp.join("cli-first-init-contract");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn init_contract() {}\n",
    )?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let before = repository_filesystem_snapshot(temp)?;
    let report = run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
    )?;
    require_json_bool(&report, &["ok"], true)?;
    let after = repository_filesystem_snapshot(temp)?;
    let expected_additions = BTreeSet::from([
        "cli-first-init-contract/.projectatlas".to_string(),
        "cli-first-init-contract/.projectatlas/config.toml".to_string(),
        "cli-first-init-contract/.projectatlas/projectatlas-nonsource-files.toon".to_string(),
        "cli-first-init-contract/.projectatlas/projectatlas.claude.mcp.json".to_string(),
        "cli-first-init-contract/.projectatlas/projectatlas.mcp.json".to_string(),
        "cli-first-init-contract/.projectatlas/projectatlas.opencode.json".to_string(),
    ]);
    let additions = after
        .keys()
        .filter(|path| !before.contains_key(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let removals = before
        .keys()
        .filter(|path| !after.contains_key(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let modifications = before
        .iter()
        .filter(|(path, value)| after.get(*path) != Some(*value))
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    if additions != expected_additions || !removals.is_empty() || !modifications.is_empty() {
        return Err(io::Error::other(format!(
            "first-run init filesystem ownership drifted: additions={additions:?} removals={removals:?} modifications={modifications:?}"
        ))
        .into());
    }
    if !database.is_file() || fs::metadata(&database)?.len() == 0 {
        return Err(io::Error::other("first-run init omitted its SQLite database").into());
    }
    Ok(())
}

/// Snapshot repository paths, types, and file bytes while `SQLite` owns its own checks.
fn repository_filesystem_snapshot(repo: &Path) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    fn visit(
        repo: &Path,
        directory: &Path,
        snapshot: &mut BTreeMap<String, String>,
    ) -> Result<(), Box<dyn Error>> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(repo)?
                .to_string_lossy()
                .replace('\\', "/");
            if relative == ".git" || relative.ends_with("/.git") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                snapshot.insert(
                    relative,
                    format!("symlink:{}", fs::read_link(&path)?.to_string_lossy()),
                );
            } else if file_type.is_dir() {
                snapshot.insert(relative, "directory".to_string());
                visit(repo, &path, snapshot)?;
            } else if file_type.is_file() {
                if matches!(
                    relative.as_str(),
                    ".projectatlas/projectatlas.db"
                        | ".projectatlas/projectatlas.db-wal"
                        | ".projectatlas/projectatlas.db-shm"
                ) || relative.ends_with("/.projectatlas/projectatlas.db")
                    || relative.ends_with("/.projectatlas/projectatlas.db-wal")
                    || relative.ends_with("/.projectatlas/projectatlas.db-shm")
                {
                    continue;
                }
                let bytes = fs::read(&path)?;
                snapshot.insert(
                    relative,
                    format!("file:{}:{}", bytes.len(), sha256_hex(&bytes)),
                );
            } else {
                snapshot.insert(relative, "other".to_string());
            }
        }
        Ok(())
    }

    let mut snapshot = BTreeMap::new();
    visit(repo, repo, &mut snapshot)?;
    Ok(snapshot)
}

/// Require exact effects across the enclosing contract fixture, including explicit output paths.
fn assert_cli_contract_outer_filesystem_delta(
    name: &str,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let allowed_path = match name {
        "map" => Some("cli-contract/.projectatlas/projectatlas.toon"),
        "snapshot" => Some("cli-contract-snapshot.tar.zst"),
        _ => None,
    };
    let mut expected = before.clone();
    if let Some(path) = allowed_path {
        let value = after.get(path).ok_or_else(|| {
            io::Error::other(format!(
                "{name} omitted its declared outer filesystem artifact"
            ))
        })?;
        expected.insert(path.to_string(), value.clone());
    }
    if after != &expected {
        return Err(io::Error::other(format!(
            "{name} changed an undeclared path in the enclosing fixture: before={before:?} after={after:?}"
        ))
        .into());
    }
    Ok(())
}

/// Require one exact repository-filesystem delta for a packaged command family.
fn assert_cli_contract_filesystem_delta(
    name: &str,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    if name == "map" {
        let path = ".projectatlas/projectatlas.toon";
        let mut expected = before.clone();
        let value = after
            .get(path)
            .ok_or_else(|| io::Error::other("packaged map omitted its repository artifact"))?;
        expected.insert(path.to_string(), value.clone());
        if after == &expected {
            return Ok(());
        }
    } else if before == after {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{name} changed an undeclared repository filesystem path: before={before:?} after={after:?}"
    ))
    .into())
}

/// Run every frozen v0.3.26 nested leaf through the real packaged executable.
fn assert_packaged_cli_legacy_leaf_contracts(
    executable: &Path,
    repo: &Path,
    database: &Path,
    temp: &Path,
) -> Result<(), Box<dyn Error>> {
    let before = mcp_database_snapshot(database)?;
    let filesystem_before = repository_filesystem_snapshot(repo)?;

    let symbols = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["symbols", "list", "--file", "src/lib.rs", "--limit", "2"],
    )?;
    require_json_string(&symbols, &["0", "path"], "src/lib.rs")?;
    let relations = run_packaged_cli_json(
        executable,
        repo,
        database,
        &[
            "symbols",
            "relations",
            "--file",
            "src/lib.rs",
            "--limit",
            "2",
        ],
    )?;
    require_json_string(&relations, &["0", "path"], "src/lib.rs")?;
    let symbol_slice = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["symbols", "slice", "src/lib.rs", "indexed"],
    )?;
    require_json_string(&symbol_slice, &["path"], "src/lib.rs")?;
    require_json_contains(&symbol_slice, &["content"], "indexed")?;

    let root = repo
        .canonicalize()?
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .replace('\\', "/");
    let root_set = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["root", "set", &root, "--transition", "bind"],
    )?;
    require_json_string(&root_set, &["transition"], "bind")?;
    require_json_bool(&root_set, &["verified"], true)?;
    for arguments in [&["root", "show"][..], &["root", "verify"][..]] {
        let report = run_packaged_cli_json(executable, repo, database, arguments)?;
        require_json_bool(&report, &["verified"], true)?;
    }
    let filesystem_after_root = repository_filesystem_snapshot(repo)?;
    assert_root_bind_filesystem_delta(&filesystem_before, &filesystem_after_root)?;

    let gitignore = repo.join(".gitignore");
    let gitignore_before = fs::read(&gitignore)?;
    let gitignore_report =
        run_packaged_cli_json(executable, repo, database, &["ignore", "init-gitignore"])?;
    require_json_bool(&gitignore_report, &["existed"], true)?;
    require_json_bool(&gitignore_report, &["created"], false)?;
    if fs::read(&gitignore)? != gitignore_before {
        return Err(io::Error::other("ignore init-gitignore rewrote an existing file").into());
    }

    let added = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["ignore", "add", "--kind", "dir-name", "cli-contract-temp"],
    )?;
    require_json_string(&added, &["action"], "add")?;
    require_json_bool(&added, &["changed"], true)?;
    let listed = run_packaged_cli_json(executable, repo, database, &["ignore", "list"])?;
    let names = json_at(&listed, &["exclude_dir_names"])?
        .as_array()
        .ok_or_else(|| io::Error::other("ignore list directory names was not an array"))?;
    if !names.iter().any(|name| name == "cli-contract-temp") {
        return Err(io::Error::other("ignore add was absent from the packaged list route").into());
    }
    let removed = run_packaged_cli_json(
        executable,
        repo,
        database,
        &[
            "ignore",
            "remove",
            "--kind",
            "dir-name",
            "cli-contract-temp",
        ],
    )?;
    require_json_string(&removed, &["action"], "remove")?;
    require_json_bool(&removed, &["changed"], true)?;
    let listed = run_packaged_cli_json(executable, repo, database, &["ignore", "list"])?;
    let names = json_at(&listed, &["exclude_dir_names"])?
        .as_array()
        .ok_or_else(|| io::Error::other("ignore list directory names was not an array"))?;
    if names.iter().any(|name| name == "cli-contract-temp") {
        return Err(io::Error::other("ignore remove left its packaged list entry behind").into());
    }
    let filesystem_after_ignore = repository_filesystem_snapshot(repo)?;
    if filesystem_after_ignore != filesystem_after_root {
        return Err(io::Error::other(format!(
            "ignore init/add/remove did not preserve the post-root-binding repository filesystem bytes exactly: before={filesystem_after_root:?} after={filesystem_after_ignore:?}"
        ))
        .into());
    }

    let review_file = temp.join("cli-contract-purpose-review.json");
    fs::write(
        &review_file,
        serde_json::to_vec(&serde_json::json!({
            "items": [{"path": "src/lib.rs", "confirm_existing": true}]
        }))?,
    )?;
    let review_path = review_file.to_string_lossy().into_owned();
    let review = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["purpose", "review", "--from-file", &review_path],
    )?;
    require_json_bool(&review, &["applied"], false)?;
    require_json_usize(&review, &["failed"], 0)?;
    let queue = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["purpose", "queue", "--task", "cli-contract", "--limit", "2"],
    )?;
    require_json_string(&queue, &["task"], "cli-contract")?;
    require_json_usize(&queue, &["limit"], 2)?;

    let parity = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["parity", "report", "--profile", "repository-intelligence"],
    )?;
    require_json_bool(&parity, &["ok"], true)?;

    let after = mcp_database_snapshot(database)?;
    if after != before {
        return Err(
            io::Error::other("a frozen v0.3.26 nested CLI leaf changed SQLite state").into(),
        );
    }
    if repository_filesystem_snapshot(repo)? != filesystem_after_root {
        return Err(io::Error::other(
            "a frozen v0.3.26 nested CLI leaf changed the post-root repository filesystem state",
        )
        .into());
    }
    Ok(())
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

/// Prove help, invalid input, bounded failure, cancellation, and TOON behavior.
fn assert_packaged_cli_edge_contracts(
    executable: &Path,
    repo: &Path,
    database: &Path,
) -> Result<(), Box<dyn Error>> {
    let before_toon = mcp_database_snapshot(database)?;
    let toon = StdCommand::new(executable)
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--require-version", env!("CARGO_PKG_VERSION"), "--db"])
        .arg(database)
        .arg("overview")
        .output()?;
    if !toon.status.success() || !toon.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "packaged CLI TOON overview failed: {}",
            String::from_utf8_lossy(&toon.stderr)
        ))
        .into());
    }
    let toon_payload: Value = toon_format::decode_default(&String::from_utf8(toon.stdout)?)?;
    if !toon_payload.is_object() || mcp_database_snapshot(database)? != before_toon {
        return Err(
            io::Error::other("packaged CLI TOON overview was untyped or changed SQLite").into(),
        );
    }

    for arguments in [
        vec!["unknown-contract-command"],
        vec!["folders"],
        vec!["help", "unknown-contract-command"],
    ] {
        let output = StdCommand::new(executable)
            .current_dir(repo)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .args(&arguments)
            .output()?;
        if output.status.code() != Some(2) || !output.stdout.is_empty() || output.stderr.is_empty()
        {
            return Err(io::Error::other(format!(
                "packaged CLI parse failure contract drifted for {arguments:?}: status={:?} stdout={} stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    let before_invalid = mcp_database_snapshot(database)?;
    let invalid = StdCommand::new(executable)
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "--db"])
        .arg(database)
        .args([
            "slice",
            "src/lib.rs",
            "--start-line",
            "1",
            "--output-bytes",
            "0",
        ])
        .output()?;
    let invalid_error = String::from_utf8(invalid.stderr)?;
    if invalid.status.code() != Some(1)
        || !invalid.stdout.is_empty()
        || !invalid_error.contains("output byte")
        || mcp_database_snapshot(database)? != before_invalid
    {
        return Err(io::Error::other(format!(
            "packaged CLI invalid-input contract drifted: status={:?} stderr={invalid_error}",
            invalid.status.code()
        ))
        .into());
    }

    let pending = repo.join("src/deadline.rs");
    fs::write(&pending, "pub fn deadline_contract() {}\n")?;
    let before_deadline = mcp_database_snapshot(database)?;
    let deadline = StdCommand::new(executable)
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["--format", "json", "--db"])
        .arg(database)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .output()?;
    let deadline_message = String::from_utf8(deadline.stderr)?.to_ascii_lowercase();
    if deadline.status.code() != Some(1)
        || !deadline.stdout.is_empty()
        || (!deadline_message.contains("deadline") && !deadline_message.contains("canceled"))
        || mcp_database_snapshot(database)? != before_deadline
    {
        return Err(io::Error::other(format!(
            "packaged CLI deadline preserved partial state: status={:?} stderr={deadline_message}",
            deadline.status.code()
        ))
        .into());
    }
    fs::remove_file(pending)?;
    assert_packaged_cli_restart_cleanup_interruption(executable, repo, database)?;
    Ok(())
}

/// Interrupt packaged restart cleanup after one owned abandoned stage is removed.
fn assert_packaged_cli_restart_cleanup_interruption(
    executable: &Path,
    repo: &Path,
    database: &Path,
) -> Result<(), Box<dyn Error>> {
    let pending = repo.join("src/restart-cleanup.rs");
    fs::write(&pending, "pub fn restart_cleanup_contract() {}\n")?;
    let project = AtlasStore::open(database)?
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("CLI interruption fixture omitted project identity"))?;
    let canonical_repo = fs::canonicalize(repo)?;
    let atlas = repo.join(ATLAS_DIR_NAME);
    for index in 0..64 {
        let stage = atlas.join(format!("graph-stage-interruption-{index:02}"));
        fs::create_dir(&stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &stage.join("projectatlas.db"),
            &canonical_repo,
            project,
        )?);
    }
    let initial_stages = graph_stage_directories(repo)?.len();
    let before = mcp_database_snapshot(database)?;
    let mut child = StdCommand::new(executable)
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args([
            "--require-version",
            env!("CARGO_PKG_VERSION"),
            "--format",
            "json",
            "--db",
        ])
        .arg(database)
        .args(["watch", ".", "--once", "--max-workers", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let observation_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if graph_stage_directories(repo)?.len() < initial_stages {
            break;
        }
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            return Err(io::Error::other(format!(
                "packaged CLI completed before restart cleanup could be interrupted: status={} stdout={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if Instant::now() >= observation_deadline {
            child.kill()?;
            let _status = child.wait()?;
            return Err(io::Error::other(
                "packaged CLI exposed no restart-cleanup progress within 30 seconds",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(1));
    }

    let interrupted_at = Instant::now();
    child.kill()?;
    let output = child.wait_with_output()?;
    if output.status.success()
        || !output.stdout.is_empty()
        || interrupted_at.elapsed() > Duration::from_secs(5)
    {
        return Err(io::Error::other(format!(
            "packaged CLI restart-cleanup interruption was not prompt and stream-safe: elapsed={:?} status={} stdout={} stderr={}",
            interrupted_at.elapsed(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let after = mcp_database_snapshot(database)?;
    if after != before {
        return Err(io::Error::other(
            "packaged CLI restart-cleanup interruption changed the primary SQLite state",
        )
        .into());
    }

    fs::remove_file(pending)?;
    let recovered = run_packaged_cli_json(
        executable,
        repo,
        database,
        &["watch", ".", "--once", "--max-workers", "1"],
    )?;
    require_json_string(&recovered, &["mode"], "single-refresh")?;
    require_json_usize(&recovered, &["cycles"], 1)?;
    if !graph_stage_directories(repo)?.is_empty() {
        return Err(io::Error::other(
            "packaged CLI restart left an abandoned graph staging directory",
        )
        .into());
    }
    Ok(())
}

/// List only ProjectAtlas-owned disposable graph stages in a test repository.
fn graph_stage_directories(repo: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let atlas = repo.join(ATLAS_DIR_NAME);
    let mut stages = Vec::new();
    for entry in fs::read_dir(atlas)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("graph-stage-")
        {
            stages.push(entry.path());
        }
    }
    Ok(stages)
}

/// Prove packaged CLI freshness and CLI/MCP reopen behavior without Git metadata.
fn assert_cli_non_git_freshness(executable: &Path) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("cli-non-git-contract");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn baseline() {}\n",
    )?;
    if repo.join(GIT_DIR_NAME).exists() {
        return Err(io::Error::other("non-Git CLI fixture unexpectedly contained .git").into());
    }
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    for arguments in [
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
        vec![
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    ] {
        run_mcp_contract_json(executable, &repo, &arguments)?;
    }
    Connection::open(&database)?.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
        (MCP_CONTRACT_METADATA_CANARY, "preserve"),
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn baseline() {}\n\npub fn non_git_cli_contract() {}\n",
    )?;
    let case = CliContractCase {
        name: "summary",
        arguments: vec![
            "summary".to_string(),
            "src/lib.rs".to_string(),
            "--limit".to_string(),
            "5".to_string(),
        ],
        output: CliContractOutput::JsonObject,
        effect: McpSqliteEffect::DerivedSourceAdvance,
        expected_exit_code: 0,
    };
    let before = mcp_database_snapshot(&database)?;
    let summary = run_packaged_cli_contract_case(executable, &repo, &database, &case)?
        .ok_or_else(|| io::Error::other("non-Git CLI summary omitted output"))?;
    require_json_contains(&summary, &["content_summary"], "non_git_cli_contract")?;
    let after = mcp_database_snapshot(&database)?;
    assert_contract_sqlite_effect(case.name, case.effect, &before, &after)?;
    assert_mcp_matches_clean_packaged_scan(
        executable,
        &repo,
        &database,
        temp.path(),
        "cli-non-git-freshness",
    )?;

    let stable_before = after;
    let stable = run_packaged_cli_contract_case(
        executable,
        &repo,
        &database,
        &CliContractCase {
            effect: McpSqliteEffect::None,
            ..case
        },
    )?
    .ok_or_else(|| io::Error::other("stable non-Git CLI summary omitted output"))?;
    require_json_contains(&stable, &["content_summary"], "non_git_cli_contract")?;
    if mcp_database_snapshot(&database)? != stable_before {
        return Err(io::Error::other("unchanged non-Git CLI summary republished state").into());
    }

    let reopen_case = McpToolContractCase {
        name: "atlas_file_summary",
        arguments: serde_json::json!({
            "project_path": repo,
            "file": "src/lib.rs",
            "compact": true
        }),
        expected_marker: "file_summary:",
        payload_key: Some("file_summary"),
        effect: McpSqliteEffect::None,
        telemetry_enabled: false,
    };
    let reopened = run_mcp_contract_call(executable, &repo, &database, &reopen_case)?;
    if !reopened.contains("non_git_cli_contract") {
        return Err(io::Error::other("MCP reopen lost packaged CLI non-Git freshness").into());
    }
    Ok(())
}

/// Return the exact advertised inventory from a real release-candidate stdio process.
fn run_mcp_contract_inventory(
    executable: &Path,
    cwd: &Path,
    database: &Path,
) -> Result<String, Box<dyn Error>> {
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-mcp-contract","version":"0.4.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    ];
    run_mcp_stdio_with_env(
        executable,
        cwd,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "mcp".to_string(),
        ],
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
    )
}

/// Execute one advertised tool through its own real stdio process.
fn run_mcp_contract_call(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    case: &McpToolContractCase,
) -> Result<String, Box<dyn Error>> {
    let (response, stdout) = run_mcp_contract_raw_call(
        executable,
        cwd,
        database,
        case.name,
        &case.arguments,
        case.telemetry_enabled,
    )?;
    if response
        .get("result")
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
        || response.get("error").is_some()
    {
        return Err(io::Error::other(format!(
            "MCP contract call {} failed: {response}",
            case.name
        ))
        .into());
    }
    mcp_tool_text(&stdout, 2)
}

/// Execute one tool call and retain its complete success or error envelope.
fn run_mcp_contract_raw_call(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    name: &str,
    arguments: &Value,
    telemetry_enabled: bool,
) -> Result<(Value, String), Box<dyn Error>> {
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-mcp-contract","version":"0.4.0"}}}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        })
        .to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        })
        .to_string(),
    ];
    let telemetry = if telemetry_enabled { None } else { Some("1") };
    let stdout = run_mcp_stdio_with_env(
        executable,
        cwd,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "mcp".to_string(),
        ],
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", telemetry)],
    )?;
    let response = mcp_response(&stdout, 2)?;
    Ok((response, stdout))
}

/// Require an invalid real MCP call to fail without changing logical `SQLite` state.
fn assert_mcp_contract_failure_no_mutation(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    name: &str,
    arguments: &Value,
    expected_error: &str,
) -> Result<(), Box<dyn Error>> {
    let before = mcp_database_snapshot(database)?;
    let (response, stdout) =
        run_mcp_contract_raw_call(executable, cwd, database, name, arguments, false)?;
    let error_text = if let Some(error) = response.get("error") {
        error.get("code").and_then(Value::as_i64).ok_or_else(|| {
            io::Error::other(format!("{name} protocol error omitted integer code"))
        })?;
        error
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other(format!("{name} protocol error omitted message")))?;
        error.to_string()
    } else if response
        .get("result")
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        require_json_string(&response, &["result", "content", "0", "type"], "text")?;
        let text = mcp_tool_text(&stdout, 2)?;
        match toon_format::decode_default::<Value>(&text) {
            Ok(error) if error.is_object() => error.to_string(),
            Ok(error) => {
                return Err(io::Error::other(format!(
                    "{name} typed error payload was not an object: {error}"
                ))
                .into());
            }
            Err(_) if text.starts_with("failed to deserialize parameters:") => text,
            Err(decode_error) => {
                return Err(io::Error::other(format!(
                    "{name} returned invalid typed error TOON: {decode_error}; payload={text:?}"
                ))
                .into());
            }
        }
    } else {
        let text = mcp_tool_text(&stdout, 2)?;
        let error: Value = toon_format::decode_default(&text).map_err(|decode_error| {
            io::Error::other(format!(
                "{name} invalid contract call returned neither an MCP nor typed domain error: {decode_error}; response={response}"
            ))
        })?;
        if error.get("error").is_none() {
            return Err(io::Error::other(format!(
                "{name} invalid contract call unexpectedly succeeded: {response}"
            ))
            .into());
        }
        error.to_string()
    };
    if !error_text
        .to_ascii_lowercase()
        .contains(&expected_error.to_ascii_lowercase())
    {
        return Err(io::Error::other(format!(
            "{name} error omitted {expected_error:?}: {error_text}"
        ))
        .into());
    }
    let after = mcp_database_snapshot(database)?;
    if before != after {
        return Err(io::Error::other(format!(
            "{name} invalid contract call changed SQLite state: before={before:?} after={after:?}"
        ))
        .into());
    }
    Ok(())
}

/// Prove saved-source freshness and stable reopen behavior without Git metadata.
fn assert_mcp_non_git_freshness(executable: &Path) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("non-git-contract");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn baseline() {}\n",
    )?;
    if repo.join(GIT_DIR_NAME).exists() {
        return Err(io::Error::other("non-Git MCP fixture unexpectedly contained .git").into());
    }
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
    )?;
    run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    )?;

    let before = mcp_database_snapshot(&database)?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn baseline() {}\n\npub fn non_git_contract() {}\n",
    )?;
    let case = McpToolContractCase {
        name: "atlas_file_summary",
        arguments: serde_json::json!({"project_path": repo, "file": "src/lib.rs", "compact": true}),
        expected_marker: "file_summary:",
        payload_key: Some("file_summary"),
        effect: McpSqliteEffect::DerivedSourceAdvance,
        telemetry_enabled: false,
    };
    let text = run_mcp_contract_call(executable, &repo, &database, &case)?;
    let decoded: Value = toon_format::decode_default(&text)?;
    require_json_contains(
        &decoded,
        &["file_summary", "content_summary"],
        "non_git_contract",
    )?;
    require_json_string(
        &decoded,
        &["file_summary", "parser_kind"],
        "tree-sitter-symbol-graph",
    )?;
    require_json_string(&decoded, &["file_summary", "summary_status"], "ok")?;

    let after = mcp_database_snapshot(&database)?;
    let changed = changed_snapshot_keys(&before.authoritative, &after.authoritative);
    if changed.is_empty()
        || changed
            .iter()
            .any(|table| !mcp_source_publication_table(table))
        || before.usage != after.usage
        || before.authored_purposes != after.authored_purposes
        || before.purpose_revision != after.purpose_revision
        || after.generation != before.generation.saturating_add(1)
        || after.publication_state != "complete"
    {
        return Err(io::Error::other(format!(
            "non-Git MCP freshness escaped atomic source ownership: changed={changed:?} before={before:?} after={after:?}"
        ))
        .into());
    }
    assert_mcp_matches_clean_packaged_scan(
        executable,
        &repo,
        &database,
        temp.path(),
        "non-git-freshness",
    )?;

    let stable_before = after;
    let stable_text = run_mcp_contract_call(executable, &repo, &database, &case)?;
    if !stable_text.contains("non_git_contract")
        || mcp_database_snapshot(&database)? != stable_before
    {
        return Err(io::Error::other(
            "unchanged non-Git MCP reopen repeated publication or lost fresh source",
        )
        .into());
    }
    let reopened = run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "summary".to_string(),
            "src/lib.rs".to_string(),
            "--limit".to_string(),
            "5".to_string(),
        ],
    )?;
    require_json_contains(&reopened, &["content_summary"], "non_git_contract")?;
    Ok(())
}

/// Require one MCP publication to equal a clean packaged scan of the same source.
fn assert_mcp_matches_clean_packaged_scan(
    executable: &Path,
    repo: &Path,
    database: &Path,
    scratch: &Path,
    checkpoint: &str,
) -> Result<(), Box<dyn Error>> {
    let clean_database = scratch.join(format!("mcp-clean-{checkpoint}.db"));
    run_mcp_contract_json(
        executable,
        repo,
        &[
            "--db".to_string(),
            clean_database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    )?;
    let actual = derived_result_snapshot(database)?;
    let mut clean = derived_result_snapshot(&clean_database)?;
    for path in authored_purpose_paths(database)? {
        clean.unreviewed_purposes.remove(&path);
    }
    if actual != clean {
        return Err(io::Error::other(format!(
            "{checkpoint} MCP publication diverged from an exact clean packaged scan:\nactual={actual:#?}\nclean={clean:#?}"
        ))
        .into());
    }
    Ok(())
}

/// Return whether one table belongs to a derived source publication.
fn mcp_source_publication_table(table: &str) -> bool {
    matches!(
        table,
        "metadata"
            | "nodes"
            | "summaries"
            | "symbols"
            | "source_parse_metadata"
            | "symbol_relations"
            | "file_texts"
            | "graph_entities"
            | "graph_relations"
            | "graph_relation_occurrences"
            | "graph_coverage"
            | "graph_resolution_keys"
            | "graph_entity_exports"
            | "graph_relation_dependencies"
            | "project_identity"
            | "purposes"
    ) || table.starts_with("file_text_fts")
}

/// Persistent real MCP session used for ordered task cancellation.
struct McpContractSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    responses: Receiver<io::Result<String>>,
    stdout_reader: Option<thread::JoinHandle<()>>,
    stderr_reader: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    next_request_id: u64,
}

impl McpContractSession {
    /// Spawn and initialize one telemetry-disabled release-candidate MCP process.
    fn spawn(executable: &Path, repo: &Path, database: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = StdCommand::new(executable)
            .current_dir(repo)
            .arg("--db")
            .arg(database)
            .arg("mcp")
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
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
        Ok(session)
    }

    /// Call one real MCP tool and return its text payload.
    fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
        let response = self.request(
            "tools/call",
            &serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        if response.get("error").is_some()
            || response
                .get("result")
                .and_then(|result| result.get("isError"))
                .and_then(Value::as_bool)
                == Some(true)
        {
            return Err(
                io::Error::other(format!("MCP contract tool {name} failed: {response}")).into(),
            );
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

/// Prove a real active background task accepts cancellation without partial publication.
fn assert_mcp_active_cancellation_preserves_generation(
    executable: &Path,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("cancellation-contract");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join("src/baseline.rs"), "pub fn baseline() {}\n")?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
    )?;
    run_mcp_contract_json(
        executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    )?;
    let before = mcp_database_snapshot(&database)?;

    let pending = repo.join("pending");
    fs::create_dir(&pending)?;
    let source = "pub fn pending_contract() { let value = 1_u64; let _ = value; }\n".repeat(128);
    for index in 0..512 {
        fs::write(pending.join(format!("work-{index:04}.rs")), &source)?;
    }
    let mut session = McpContractSession::spawn(executable, &repo, &database)?;
    let started: Value = toon_format::decode_default(&session.call_tool(
        "atlas_scan",
        &serde_json::json!({
            "project_path": repo,
            "path": repo,
            "background": true,
            "max_workers": 1
        }),
    )?)?;
    let task_id = json_string_at(&started, &["task_start", "task_id"])?.to_owned();
    require_json_string(&started, &["task_start", "operation"], "scan")?;
    require_json_string(
        &started,
        &["task_start", "status_tool"],
        "atlas_task_status",
    )?;
    require_json_string(
        &started,
        &["task_start", "cancel_tool"],
        "atlas_task_cancel",
    )?;
    let status: Value = toon_format::decode_default(&session.call_tool(
        "atlas_task_status",
        &serde_json::json!({"task_id": task_id}),
    )?)?;
    require_json_string(&status, &["task_status", "lookup"], "found")?;
    let canceled: Value = toon_format::decode_default(&session.call_tool(
        "atlas_task_cancel",
        &serde_json::json!({"task_id": task_id}),
    )?)?;
    require_json_string(
        &canceled,
        &["task_cancel", "result"],
        "cancellation_requested",
    )?;
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .ok_or_else(|| io::Error::other("MCP cancellation deadline overflowed"))?;
    loop {
        let status: Value = toon_format::decode_default(&session.call_tool(
            "atlas_task_status",
            &serde_json::json!({"task_id": task_id}),
        )?)?;
        match json_string_at(&status, &["task_status", "task", "state"])? {
            "canceled" => break,
            "pending" | "running" if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            state => {
                return Err(io::Error::other(format!(
                    "MCP task did not quiesce as canceled: state={state} status={status}"
                ))
                .into());
            }
        }
    }
    session.shutdown()?;

    let after = mcp_database_snapshot(&database)?;
    if before != after {
        return Err(io::Error::other(format!(
            "active MCP cancellation exposed partial publication: before={before:?} after={after:?}"
        ))
        .into());
    }
    Ok(())
}

/// Launch a real MCP stdio child and return stdout after stdin closes.
fn run_mcp_stdio(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env(executable, cwd, args, messages, &[])
}

/// Launch a real MCP stdio child with explicit per-process environment controls.
fn run_mcp_stdio_with_env(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
) -> Result<String, Box<dyn Error>> {
    let input = format!(
        "{}\n",
        messages
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut command = StdCommand::new(executable);
    command
        .current_dir(cwd)
        .args(args)
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

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("mcp stdin was not piped"))?;
    stdin.write_all(input.as_bytes())?;
    drop(stdin);

    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("mcp stdout was not piped"))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("mcp stderr was not piped"))?;
    let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        stdout_pipe.read_to_end(&mut output)?;
        Ok(output)
    });
    let stderr_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        stderr_pipe.read_to_end(&mut output)?;
        Ok(output)
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            match child.wait() {
                Ok(_status) => {}
                Err(error) => return Err(error.into()),
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "projectatlas mcp did not exit after stdin closed",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(100));
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stderr reader panicked"))??;
    if !status.success() {
        return Err(io::Error::other(format!(
            "projectatlas mcp failed: {}",
            String::from_utf8_lossy(&stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(stdout)?)
}

/// Return the text payload for one MCP tool-call response id.
fn mcp_tool_text(stdout: &str, id: i64) -> Result<String, Box<dyn Error>> {
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let response: Value = serde_json::from_str(line)?;
        if response.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        return response
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| io::Error::other(format!("MCP tool response {id} has no text")).into());
    }
    Err(io::Error::other(format!("MCP tool response {id} is missing")).into())
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

/// Prove that every frozen v0.3.26 tool and request-schema contract remains intact.
fn assert_legacy_mcp_surface_compatible(stdout: &str) -> Result<(), Box<dyn Error>> {
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
    if baseline_by_name.keys().collect::<Vec<_>>() != current_by_name.keys().collect::<Vec<_>>() {
        return Err(io::Error::other(format!(
            "MCP inventory drifted from v0.3.26: baseline={:?}, current={:?}",
            baseline_by_name.keys().collect::<Vec<_>>(),
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
        assert_no_local_schema_references(&format!("{name}.inputSchema"), schema)?;
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

/// Reject bridge-sensitive local definitions or references anywhere in an input schema.
fn assert_no_local_schema_references(path: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "$defs"
                    || key == "$ref"
                        && child
                            .as_str()
                            .is_some_and(|reference| reference.starts_with("#/$defs/"))
                {
                    return Err(io::Error::other(format!(
                        "Codex-facing schema retained local reference member {path}.{key}"
                    ))
                    .into());
                }
                assert_no_local_schema_references(&format!("{path}.{key}"), child)?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                assert_no_local_schema_references(&format!("{path}[{index}]"), child)?;
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

/// Capture bounded logical rows so WAL/page-layout changes do not masquerade as product state.
fn mcp_database_snapshot(database: &Path) -> Result<McpDatabaseSnapshot, Box<dyn Error>> {
    const MAX_TABLE_ROWS: usize = 16_384;
    const MAX_TABLE_BYTES: usize = 8 * 1024 * 1024;

    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let table_names = {
        let mut statement = connection.prepare(
            "SELECT name
             FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut authoritative = BTreeMap::new();
    let mut usage = BTreeMap::new();
    for table_name in table_names {
        let quoted_name = format!("\"{}\"", table_name.replace('"', "\"\""));
        let column_count = {
            let statement = connection.prepare(&format!("SELECT * FROM {quoted_name} LIMIT 0"))?;
            statement.column_count()
        };
        if column_count == 0 {
            return Err(io::Error::other(format!(
                "MCP contract table {table_name} has no columns"
            ))
            .into());
        }
        let ordering = (1..=column_count)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let mut statement =
            connection.prepare(&format!("SELECT * FROM {quoted_name} ORDER BY {ordering}"))?;
        let mut rows = statement.query([])?;
        let mut encoded = Vec::new();
        let mut row_count = 0usize;
        while let Some(row) = rows.next()? {
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| io::Error::other("MCP contract row count overflowed"))?;
            if row_count > MAX_TABLE_ROWS {
                return Err(io::Error::other(format!(
                    "MCP contract table {table_name} exceeded {MAX_TABLE_ROWS} rows"
                ))
                .into());
            }
            for index in 0..column_count {
                match row.get_ref(index)? {
                    ValueRef::Null => encoded.push(0),
                    ValueRef::Integer(value) => {
                        encoded.push(1);
                        encoded.extend_from_slice(&value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        encoded.push(2);
                        encoded.extend_from_slice(&value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) => {
                        encoded.push(3);
                        encoded.extend_from_slice(&u64::try_from(value.len())?.to_le_bytes());
                        encoded.extend_from_slice(value);
                    }
                    ValueRef::Blob(value) => {
                        encoded.push(4);
                        encoded.extend_from_slice(&u64::try_from(value.len())?.to_le_bytes());
                        encoded.extend_from_slice(value);
                    }
                }
            }
            encoded.push(0xff);
            if encoded.len() > MAX_TABLE_BYTES {
                return Err(io::Error::other(format!(
                    "MCP contract table {table_name} exceeded {MAX_TABLE_BYTES} encoded bytes"
                ))
                .into());
            }
        }
        let digest = format!("{row_count}:{}", sha256_hex(&encoded));
        if table_name.starts_with("usage_") {
            usage.insert(table_name, digest);
        } else {
            authoritative.insert(table_name, digest);
        }
    }
    drop(connection);

    let store = AtlasStore::open_read_only(database)?;
    let publication = store
        .index_publication()?
        .ok_or_else(|| io::Error::other("MCP contract database has no publication"))?;
    let authored_purposes = {
        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut statement = connection.prepare(
            "SELECT n.path, COALESCE(p.purpose, ''), p.source, p.status
             FROM purposes AS p
             JOIN nodes AS n ON n.id = p.node_id
             WHERE p.source IN ('imported', 'agent')
             ORDER BY n.path",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    format!(
                        "{}\0{}\0{}",
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?
                    ),
                ))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?
    };
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let metadata_canary = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [MCP_CONTRACT_METADATA_CANARY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let sealed_mcp_instances = usize::try_from(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM usage_instances WHERE owner = 'mcp_process' AND state = 'sealed'",
        [],
        |row| row.get(0),
    )?)?;
    let usage_events = store
        .usage_events(None)?
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let retention = store.telemetry_retention_state()?;
    Ok(McpDatabaseSnapshot {
        authoritative,
        usage,
        authored_purposes,
        metadata_canary,
        project_instance_id: store
            .project_instance_id()?
            .map(projectatlas_core::graph::ProjectInstanceId::as_hex),
        usage_calls: store.token_overview(None)?.calls,
        usage_events,
        active_usage_instances: retention.active_instance_rows,
        sealed_mcp_instances,
        generation: publication.generation.get(),
        purpose_revision: store.authored_purpose_revision()?,
        publication_state: format!("{:?}", publication.state).to_ascii_lowercase(),
    })
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

/// Require root binding to update only its three generated host configurations.
fn assert_root_bind_filesystem_delta(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let allowed = BTreeSet::from([
        ".projectatlas/projectatlas.claude.mcp.json".to_string(),
        ".projectatlas/projectatlas.mcp.json".to_string(),
        ".projectatlas/projectatlas.opencode.json".to_string(),
    ]);
    let missing = allowed
        .iter()
        .filter(|path| !after.contains_key(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let changed = changed_snapshot_keys(before, after);
    if !missing.is_empty() || !changed.is_subset(&allowed) {
        return Err(io::Error::other(format!(
            "root set/show/verify escaped generated host-config ownership: changed={changed:?} missing={missing:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn packaged_contract_accepts_only_owned_state_dependent_updates() -> Result<(), Box<dyn Error>> {
    let filesystem_before = BTreeMap::from([
        (
            ".projectatlas/projectatlas.claude.mcp.json".to_string(),
            "file:1:before".to_string(),
        ),
        (
            ".projectatlas/projectatlas.mcp.json".to_string(),
            "file:1:before".to_string(),
        ),
        (
            ".projectatlas/projectatlas.opencode.json".to_string(),
            "file:1:before".to_string(),
        ),
        ("src/lib.rs".to_string(), "file:1:source".to_string()),
    ]);
    let mut filesystem_after = filesystem_before.clone();
    for path in [
        ".projectatlas/projectatlas.claude.mcp.json",
        ".projectatlas/projectatlas.mcp.json",
        ".projectatlas/projectatlas.opencode.json",
    ] {
        filesystem_after.insert(path.to_string(), "file:1:after".to_string());
    }
    assert_root_bind_filesystem_delta(&filesystem_before, &filesystem_after)?;
    filesystem_after.insert("src/lib.rs".to_string(), "file:1:changed".to_string());
    if assert_root_bind_filesystem_delta(&filesystem_before, &filesystem_after).is_ok() {
        return Err(io::Error::other("root binding accepted an unrelated source change").into());
    }

    let authoritative = BTreeMap::from([
        ("graph_coverage".to_string(), "before".to_string()),
        ("metadata".to_string(), "before".to_string()),
        ("project_identity".to_string(), "before".to_string()),
        ("source_parse_metadata".to_string(), "stable".to_string()),
        ("summaries".to_string(), "stable".to_string()),
        ("symbols".to_string(), "before".to_string()),
    ]);
    let before = McpDatabaseSnapshot {
        authoritative,
        usage: BTreeMap::new(),
        authored_purposes: BTreeMap::new(),
        metadata_canary: Some("canary".to_string()),
        project_instance_id: Some("project".to_string()),
        usage_calls: 0,
        usage_events: Vec::new(),
        active_usage_instances: 0,
        sealed_mcp_instances: 0,
        generation: 3,
        purpose_revision: 0,
        publication_state: "complete".to_string(),
    };
    let mut after = before.clone();
    for table in ["graph_coverage", "metadata", "project_identity", "symbols"] {
        after
            .authoritative
            .insert(table.to_string(), "after".to_string());
    }
    after.generation = 4;
    assert_contract_sqlite_effect(
        "symbols",
        McpSqliteEffect::DerivedGraphAdvance,
        &before,
        &after,
    )?;
    let mut incomplete = after.clone();
    incomplete
        .authoritative
        .insert("project_identity".to_string(), "before".to_string());
    if assert_contract_sqlite_effect(
        "symbols",
        McpSqliteEffect::DerivedGraphAdvance,
        &before,
        &incomplete,
    )
    .is_ok()
    {
        return Err(io::Error::other(
            "graph publication accepted stale project generation identity",
        )
        .into());
    }
    after
        .authoritative
        .insert("health_resolutions".to_string(), "unexpected".to_string());
    if assert_contract_sqlite_effect(
        "symbols",
        McpSqliteEffect::DerivedGraphAdvance,
        &before,
        &after,
    )
    .is_ok()
    {
        return Err(
            io::Error::other("graph publication accepted an unrelated table change").into(),
        );
    }
    Ok(())
}

/// Require one packaged adapter call to stay inside its declared `SQLite` owner.
fn assert_contract_sqlite_effect(
    name: &str,
    effect: McpSqliteEffect,
    before: &McpDatabaseSnapshot,
    after: &McpDatabaseSnapshot,
) -> Result<(), Box<dyn Error>> {
    if after.publication_state != "complete" {
        return Err(io::Error::other(format!(
            "{} left publication trust at {}",
            name, after.publication_state
        ))
        .into());
    }
    let authoritative = changed_snapshot_keys(&before.authoritative, &after.authoritative);
    let usage = changed_snapshot_keys(&before.usage, &after.usage);
    if before.metadata_canary != after.metadata_canary
        || before.project_instance_id != after.project_instance_id
    {
        return Err(io::Error::other(format!(
            "{} changed unrelated metadata or project identity: canary={:?}->{:?} project={:?}->{:?}",
            name,
            before.metadata_canary,
            after.metadata_canary,
            before.project_instance_id,
            after.project_instance_id
        ))
        .into());
    }
    match effect {
        McpSqliteEffect::None => {
            if !authoritative.is_empty()
                || !usage.is_empty()
                || before.authored_purposes != after.authored_purposes
                || before.generation != after.generation
                || before.purpose_revision != after.purpose_revision
            {
                return Err(io::Error::other(format!(
                    "{} changed read-only SQLite state: authoritative={authoritative:?} usage={usage:?} generation={}->{} purpose_revision={}->{}",
                    name,
                    before.generation,
                    after.generation,
                    before.purpose_revision,
                    after.purpose_revision
                ))
                .into());
            }
        }
        McpSqliteEffect::Telemetry => {
            let new_event = after
                .usage_events
                .last()
                .map(|event| serde_json::from_str::<Value>(event))
                .transpose()?
                .ok_or_else(|| io::Error::other("telemetry call did not retain its usage event"))?;
            if !authoritative.is_empty()
                || usage.is_empty()
                || usage.iter().any(|table| !table.starts_with("usage_"))
                || before.authored_purposes != after.authored_purposes
                || before.generation != after.generation
                || before.purpose_revision != after.purpose_revision
                || after.usage_calls != before.usage_calls.saturating_add(1)
                || after.usage_events.len() != before.usage_events.len().saturating_add(1)
                || !after.usage_events.starts_with(&before.usage_events)
                || new_event.get("command").and_then(Value::as_str) != Some("mcp.atlas_overview")
                || after.active_usage_instances != 0
                || before.active_usage_instances != 0
                || after.sealed_mcp_instances != before.sealed_mcp_instances.saturating_add(1)
            {
                return Err(io::Error::other(format!(
                    "{} escaped one-call telemetry ownership: authoritative={authoritative:?} usage={usage:?} calls={}->{} events={}->{} active={}->{} sealed={}->{} new_event={new_event}",
                    name,
                    before.usage_calls,
                    after.usage_calls,
                    before.usage_events.len(),
                    after.usage_events.len(),
                    before.active_usage_instances,
                    after.active_usage_instances,
                    before.sealed_mcp_instances,
                    after.sealed_mcp_instances
                ))
                .into());
            }
        }
        McpSqliteEffect::DerivedSourceAdvance => {
            let required = BTreeSet::from([
                "file_texts".to_string(),
                "metadata".to_string(),
                "nodes".to_string(),
                "source_parse_metadata".to_string(),
                "summaries".to_string(),
                "symbols".to_string(),
            ]);
            if !usage.is_empty()
                || !required.is_subset(&authoritative)
                || authoritative
                    .iter()
                    .any(|table| !mcp_source_publication_table(table))
                || before.authored_purposes != after.authored_purposes
                || before.purpose_revision != after.purpose_revision
                || after.generation != before.generation.saturating_add(1)
            {
                return Err(io::Error::other(format!(
                    "{} escaped complete source-publication ownership: authoritative={authoritative:?} required={required:?} usage={usage:?} generation={}->{}",
                    name, before.generation, after.generation
                ))
                .into());
            }
        }
        McpSqliteEffect::DerivedGraphAdvance => {
            let required = BTreeSet::from([
                "graph_coverage".to_string(),
                "metadata".to_string(),
                "project_identity".to_string(),
                "symbols".to_string(),
            ]);
            if !usage.is_empty()
                || !required.is_subset(&authoritative)
                || authoritative
                    .iter()
                    .any(|table| !mcp_source_publication_table(table))
                || before.authored_purposes != after.authored_purposes
                || before.purpose_revision != after.purpose_revision
                || after.generation != before.generation.saturating_add(1)
            {
                return Err(io::Error::other(format!(
                    "{} escaped complete graph-publication ownership: authoritative={authoritative:?} required={required:?} usage={usage:?} generation={}->{}",
                    name, before.generation, after.generation
                ))
                .into());
            }
        }
        McpSqliteEffect::PurposeAdvance(expected_path) => {
            let allowed = BTreeSet::from(["metadata".to_string(), "purposes".to_string()]);
            let changed_authored =
                changed_snapshot_keys(&before.authored_purposes, &after.authored_purposes);
            if !usage.is_empty()
                || authoritative.is_empty()
                || !authoritative.is_subset(&allowed)
                || changed_authored != BTreeSet::from([expected_path.to_string()])
                || before.generation != after.generation
                || after.purpose_revision != before.purpose_revision.saturating_add(1)
            {
                return Err(io::Error::other(format!(
                    "{} escaped authored-purpose ownership: authoritative={authoritative:?} usage={usage:?} generation={}->{} purpose_revision={}->{}",
                    name,
                    before.generation,
                    after.generation,
                    before.purpose_revision,
                    after.purpose_revision
                ))
                .into());
            }
        }
        McpSqliteEffect::HealthResolution => {
            if authoritative != BTreeSet::from(["health_resolutions".to_string()])
                || !usage.is_empty()
                || before.authored_purposes != after.authored_purposes
                || before.generation != after.generation
                || before.purpose_revision != after.purpose_revision
            {
                return Err(io::Error::other(format!(
                    "{name} escaped health-resolution ownership: authoritative={authoritative:?} usage={usage:?}"
                ))
                .into());
            }
        }
    }
    Ok(())
}

/// Require durable typed fields for every advertised MCP tool payload.
fn assert_mcp_typed_payload(
    case: &McpToolContractCase,
    decoded: &Value,
    text: &str,
    after: &McpDatabaseSnapshot,
) -> Result<(), Box<dyn Error>> {
    let payload_key = case
        .payload_key
        .ok_or_else(|| io::Error::other(format!("{} omitted payload key", case.name)))?;
    let payload = json_at(decoded, &[payload_key])?;
    if matches!(case.name, "atlas_folders" | "atlas_files" | "atlas_symbols") {
        if !payload.is_array() {
            return Err(io::Error::other(format!(
                "{} payload {payload_key:?} was not an array",
                case.name
            ))
            .into());
        }
    } else if !payload.is_object() {
        return Err(io::Error::other(format!(
            "{} payload {payload_key:?} was not an object",
            case.name
        ))
        .into());
    }

    match case.name {
        "atlas_set_project_path" => {
            require_json_string(decoded, &["project", "status"], "active")?;
            json_string_at(decoded, &["project", "db"])?;
        }
        "atlas_init" => {
            require_json_bool(decoded, &["init", "ok"], true)?;
            require_json_string(decoded, &["init", "scan", "status"], "verified")?;
            require_json_usize_at_least(
                decoded,
                &["init", "scan", "report", "overview", "files"],
                4,
            )?;
            require_json_bool(
                decoded,
                &["init", "purpose_handoff", "server_started_curator"],
                false,
            )?;
        }
        "atlas_map" => {
            require_json_bool(decoded, &["map", "written"], true)?;
            json_string_at(decoded, &["map", "map_path"])?;
        }
        "atlas_root" | "atlas_root_set" => {
            require_json_bool(decoded, &["root", "verified"], true)?;
            require_json_string(
                decoded,
                &["root", "runtime_version"],
                env!("CARGO_PKG_VERSION"),
            )?;
            json_string_at(decoded, &["root", "project_instance_id"])?;
        }
        "atlas_config" => {
            json_at(decoded, &["config", "source_extensions"])?
                .as_array()
                .ok_or_else(|| {
                    io::Error::other("atlas_config source_extensions was not an array")
                })?;
            require_json_usize(decoded, &["config", "text_index_max_bytes"], 2_000_000)?;
        }
        "atlas_ignore_list" => {
            require_json_bool(decoded, &["ignore", "gitignore_present"], true)?;
            require_json_string(
                decoded,
                &["ignore", "manual_layer_order"],
                "after-gitignore",
            )?;
        }
        "atlas_ignore_init_gitignore" => {
            require_json_bool(decoded, &["gitignore", "existed"], true)?;
            require_json_bool(decoded, &["gitignore", "created"], false)?;
            require_json_bool(decoded, &["gitignore", "gitignore_inherited"], true)?;
        }
        "atlas_ignore_add" | "atlas_ignore_remove" => {
            let action = case.name.strip_prefix("atlas_ignore_").ok_or_else(|| {
                io::Error::other(format!("{} had no ignore action suffix", case.name))
            })?;
            require_json_string(decoded, &["ignore", "action"], action)?;
            require_json_bool(decoded, &["ignore", "changed"], true)?;
            require_json_string(decoded, &["ignore", "value"], "generated")?;
        }
        "atlas_scan" => {
            require_json_usize_at_least(decoded, &["scan", "overview", "files"], 4)?;
            require_json_usize(decoded, &["scan", "symbols", "parsed"], 2)?;
            require_json_usize(decoded, &["scan", "symbols", "max_workers"], 1)?;
        }
        "atlas_overview" => {
            require_json_usize_at_least(decoded, &["overview", "files"], 4)?;
            require_json_usize_at_least(decoded, &["overview", "folders"], 2)?;
        }
        "atlas_folders" => {
            require_json_array_len(decoded, &["folders"], 1)?;
            require_json_bool(decoded, &["folders", "0", "purpose_agent_reviewed"], true)?;
            require_json_string(
                decoded,
                &["folders", "0", "next_call", "capability"],
                "files",
            )?;
        }
        "atlas_files" => {
            require_json_array_len(decoded, &["files"], 1)?;
            require_json_string(decoded, &["files", "0", "path"], "src/lib.rs")?;
            require_json_bool(decoded, &["files", "0", "purpose_agent_reviewed"], true)?;
            require_json_string(
                decoded,
                &["files", "0", "next_call", "capability"],
                "summary",
            )?;
        }
        "atlas_next" => {
            require_json_array_len(decoded, &["next", "files"], 1)?;
            json_at(decoded, &["next", "suggestions"])?
                .as_array()
                .ok_or_else(|| io::Error::other("atlas_next suggestions was not an array"))?;
        }
        "atlas_outline" => {
            require_json_string(decoded, &["outline", "path"], "src/lib.rs")?;
            require_json_usize(decoded, &["outline", "line_count"], 5)?;
            require_json_array_len(decoded, &["outline", "preview_lines"], 4)?;
        }
        "atlas_file_summary" => {
            require_json_string(decoded, &["file_summary", "file_path"], "src/lib.rs")?;
            require_json_string(
                decoded,
                &["file_summary", "parser_kind"],
                "tree-sitter-symbol-graph",
            )?;
            require_json_string(decoded, &["file_summary", "summary_status"], "ok")?;
            require_json_bool(
                decoded,
                &["file_summary", "file_purpose_agent_reviewed"],
                true,
            )?;
            require_json_contains(
                decoded,
                &["file_summary", "content_summary"],
                "dirty_contract",
            )?;
        }
        "atlas_search" => {
            require_json_string(decoded, &["search", "retrieval_mode"], "lexical")?;
            require_json_bool(decoded, &["search", "truncated"], true)?;
            require_json_string(decoded, &["search", "truncation_reason"], "result-limit")?;
            require_json_usize(decoded, &["search", "returned"], 1)?;
            require_json_usize_greater_than(decoded, &["search", "searched_bytes"], 0)?;
            require_json_array_len(decoded, &["search", "results"], 1)?;
        }
        "atlas_slice" => {
            require_json_string(decoded, &["slice", "path"], "src/lib.rs")?;
            require_json_usize(decoded, &["slice", "start_line"], 1)?;
            require_json_usize(decoded, &["slice", "end_line"], 2)?;
            json_string_at(decoded, &["slice", "content"])?;
            if text.len() > 4_096 {
                return Err(io::Error::other(format!(
                    "atlas_slice exceeded its 4096-byte response contract: {}",
                    text.len()
                ))
                .into());
            }
        }
        "atlas_symbols_build" => {
            require_json_usize(decoded, &["symbols_build", "parsed"], 2)?;
            require_json_usize(decoded, &["symbols_build", "symbols"], 4)?;
            require_json_usize(decoded, &["symbols_build", "max_workers"], 1)?;
        }
        "atlas_symbols" => {
            require_json_array_len(decoded, &["symbols"], 1)?;
            require_json_string(decoded, &["symbols", "0", "path"], "src/lib.rs")?;
            require_json_string(decoded, &["symbols", "0", "name"], "indexed")?;
        }
        "atlas_symbol_relations" => {
            require_json_usize(
                decoded,
                &["symbol_relations", "generation"],
                usize::try_from(after.generation)?,
            )?;
            require_json_usize(
                decoded,
                &["symbol_relations", "authored_purpose_revision"],
                usize::try_from(after.purpose_revision)?,
            )?;
            require_json_string(decoded, &["symbol_relations", "direction"], "outbound")?;
            require_json_string(
                decoded,
                &[
                    "symbol_relations",
                    "rows",
                    "0",
                    "relation",
                    "resolution",
                    "status",
                ],
                "resolved",
            )?;
            require_json_string(
                decoded,
                &[
                    "symbol_relations",
                    "rows",
                    "0",
                    "source",
                    "coverage",
                    "0",
                    "state",
                ],
                "complete",
            )?;
            require_json_string(
                decoded,
                &["symbol_relations", "rows", "0", "next_call", "capability"],
                "symbol_slice",
            )?;
            require_json_usize_greater_than(
                decoded,
                &["symbol_relations", "work", "database_decoded_bytes"],
                0,
            )?;
        }
        "atlas_health" => {
            require_json_bool(decoded, &["health", "truncated"], true)?;
            require_json_usize(decoded, &["health", "returned"], 2)?;
            require_json_array_len(decoded, &["health_findings"], 2)?;
        }
        "atlas_health_resolve" => {
            require_json_string(decoded, &["health_resolution", "path"], "src/scanned.rs")?;
            require_json_string(
                decoded,
                &["health_resolution", "rationale"],
                "Contract-owned resolution.",
            )?;
        }
        "atlas_lint" => {
            require_json_bool(decoded, &["lint", "ok"], true)?;
            require_json_usize(decoded, &["lint", "exit_code"], 0)?;
        }
        "atlas_token_report" => {
            require_json_string(decoded, &["token_savings", "estimate_kind"], "heuristic")?;
            require_json_usize_at_least(decoded, &["token_savings", "calls"], 1)?;
            json_at(decoded, &["token_savings", "tokens_avoided"])?
                .as_i64()
                .ok_or_else(|| {
                    io::Error::other("token_savings.tokens_avoided was not an integer")
                })?;
        }
        "atlas_parity_report" => {
            require_json_string(decoded, &["parity", "profile"], "repository-intelligence")?;
            require_json_bool(decoded, &["parity", "ok"], true)?;
            json_at(decoded, &["parity", "checks"])?
                .as_array()
                .ok_or_else(|| io::Error::other("parity.checks was not an array"))?;
        }
        "atlas_settings" => {
            require_json_bool(decoded, &["settings", "root_verified"], true)?;
            require_json_string(
                decoded,
                &["settings", "database", "schema", "compatibility"],
                "current",
            )?;
            require_json_string(
                decoded,
                &["settings", "database", "publication", "state"],
                "complete",
            )?;
            require_json_usize(
                decoded,
                &["settings", "database", "publication", "generation"],
                usize::try_from(after.generation)?,
            )?;
            require_json_string(decoded, &["mcp_session", "telemetry", "mode"], "disabled")?;
        }
        "atlas_watch_status" => {
            require_json_bool(decoded, &["watch_status", "available"], true)?;
            require_json_bool(decoded, &["watch_status", "active"], false)?;
            require_json_string(decoded, &["watch_status", "mode"], "notify")?;
        }
        "atlas_watch_once" => {
            require_json_bool(decoded, &["watch", "once"], true)?;
            require_json_usize(decoded, &["watch", "cycles"], 1)?;
            require_json_usize(decoded, &["watch", "last_symbols", "parsed"], 1)?;
        }
        "atlas_strip_legacy_purpose" => {
            require_json_bool(decoded, &["legacy_purpose_migration", "applied"], false)?;
            require_json_usize(
                decoded,
                &["legacy_purpose_migration", "purpose_files_found"],
                0,
            )?;
        }
        "atlas_reset_index" => {
            require_json_bool(decoded, &["reset_index", "applied"], false)?;
            require_json_bool(decoded, &["reset_index", "dry_run"], true)?;
            require_json_usize(decoded, &["reset_index", "removed"], 0)?;
        }
        "atlas_mcp_config" => {
            json_string_at(
                decoded,
                &["mcp_config", "mcpServers", "projectatlas", "command"],
            )?;
            json_at(
                decoded,
                &["mcp_config", "mcpServers", "projectatlas", "args"],
            )?
            .as_array()
            .ok_or_else(|| io::Error::other("mcp_config args was not an array"))?;
        }
        "atlas_runtime_info" => {
            require_json_string(decoded, &["runtime", "version"], env!("CARGO_PKG_VERSION"))?;
            require_json_usize(decoded, &["runtime", "major_version"], 3)?;
            require_json_array_len(decoded, &["runtime", "mcp_tools"], 40)?;
        }
        "atlas_session_brief" => {
            require_json_string(
                decoded,
                &["session_brief", "project", "index_status"],
                "available",
            )?;
            require_json_string(
                decoded,
                &["session_brief", "recommendations", "0", "target"],
                "atlas_file_summary",
            )?;
            require_json_string(
                decoded,
                &["session_brief", "files", "0", "path"],
                "src/lib.rs",
            )?;
        }
        "atlas_task_status" => {
            require_json_string(decoded, &["task_status", "lookup"], "found")?;
            require_json_string(decoded, &["task_status", "task", "state"], "complete")?;
            require_json_bool(decoded, &["task_status", "task", "cancelable"], false)?;
        }
        "atlas_task_cancel" => {
            require_json_string(decoded, &["task_cancel", "result"], "already_finished")?;
            require_json_string(decoded, &["task_cancel", "task", "state"], "complete")?;
        }
        "atlas_purpose_queue" => {
            require_json_usize(
                decoded,
                &["purpose_curation", "active_generation"],
                usize::try_from(after.generation)?,
            )?;
            require_json_bool(decoded, &["purpose_curation", "actionable"], false)?;
            require_json_usize(decoded, &["purpose_curation", "returned"], 0)?;
        }
        "atlas_purpose_set" => {
            require_json_string(decoded, &["purpose_set", "status"], "approved")?;
            require_json_string(decoded, &["purpose_set", "source"], "agent")?;
            require_json_bool(decoded, &["purpose_set", "agent_reviewed"], true)?;
        }
        "atlas_purpose_review" => {
            require_json_bool(decoded, &["purpose_review", "applied"], true)?;
            require_json_usize(decoded, &["purpose_review", "changed"], 1)?;
            require_json_string(
                decoded,
                &["purpose_review_items", "0", "purpose"],
                "Reviewed MCP contract Rust library.",
            )?;
        }
        other => {
            return Err(io::Error::other(format!(
                "missing typed MCP payload contract for {other}"
            ))
            .into());
        }
    }
    Ok(())
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

/// Return the current token telemetry call count without mutating telemetry.
fn token_call_count(repo: &std::path::Path, db: &std::path::Path) -> Result<u64, Box<dyn Error>> {
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .arg("token")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "token call-count command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let token_json: Value = serde_json::from_slice(&output.stdout)?;
    token_json["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token call count missing").into())
}

/// Return the repository workspace root for fixture access.
fn workspace_root() -> Result<std::path::PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("workspace root not found").into())
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

/// Return the deterministic quarantine path for a fixture stale shim.
fn stale_shim_quarantine_path(path: &Path, version: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!(
        "{}.projectatlas-stale-{version}.bak",
        path.display()
    ))
}

/// Build a local Windows release archive containing the current test runtime.
#[cfg(windows)]
fn create_windows_release_archive(
    temp_root: &Path,
    runtime: &Path,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let asset_dir = temp_root.join("release-asset");
    fs::create_dir_all(&asset_dir)?;
    let release_runtime = asset_dir.join("projectatlas.exe");
    fs::copy(runtime, &release_runtime)?;
    let archive = temp_root.join(format!(
        "projectatlas-v{}-x86_64-pc-windows-msvc.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg("& { param($Source, $Destination) Compress-Archive -LiteralPath $Source -DestinationPath $Destination -Force }")
        .arg(&release_runtime)
        .arg(&archive)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "failed to create local release archive\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(archive)
}

/// Return the release suffix supported by the POSIX installer on this host.
#[cfg(unix)]
fn posix_release_suffix() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("x86_64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else {
        None
    }
}

/// Build a local POSIX release archive containing the current test runtime.
#[cfg(unix)]
fn create_posix_release_archive(
    temp_root: &Path,
    runtime: &Path,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let suffix = posix_release_suffix()
        .ok_or_else(|| io::Error::other("host is not supported by POSIX release installer"))?;
    let asset_root = temp_root.join("release-asset-posix");
    let asset_dir = asset_root.join("projectatlas");
    fs::create_dir_all(&asset_dir)?;
    let release_runtime = asset_dir.join("projectatlas");
    fs::copy(runtime, &release_runtime)?;
    fs::set_permissions(&release_runtime, fs::Permissions::from_mode(0o755))?;
    let archive = temp_root.join(format!(
        "projectatlas-v{}-{suffix}.tar.gz",
        env!("CARGO_PKG_VERSION")
    ));
    let output = StdCommand::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&asset_root)
        .arg("projectatlas")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "failed to create local POSIX release archive\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(archive)
}

/// Serve local release archive and checksum requests for installer smoke tests.
fn serve_release_assets(
    archive: &Path,
    override_hash: Option<&str>,
) -> Result<(String, thread::JoinHandle<Result<(), io::Error>>), Box<dyn Error>> {
    use std::io::Read as _;

    let asset_name = archive
        .file_name()
        .ok_or_else(|| io::Error::other("release archive file name missing"))?
        .to_string_lossy()
        .to_string();
    let asset = fs::read(archive)?;
    let checksum = override_hash.map_or_else(|| sha256_hex(&asset), ToString::to_string);
    let checksums = format!("{checksum}  {asset_name}\n").into_bytes();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_mins(1);
        let mut served_archive = false;
        let mut served_checksums = false;
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    let mut request = [0_u8; 1024];
                    let bytes_read = stream.read(&mut request)?;
                    if bytes_read == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "release asset request was empty",
                        ));
                    }
                    let request_text = String::from_utf8_lossy(&request[..bytes_read]);
                    let request_path = request_text
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("");
                    let (body, content_type) = if request_path.ends_with(&format!("/{asset_name}"))
                    {
                        served_archive = true;
                        let content_type = if Path::new(&asset_name)
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
                        {
                            "application/zip"
                        } else {
                            "application/gzip"
                        };
                        (asset.as_slice(), content_type)
                    } else if request_path.ends_with("/SHA256SUMS") {
                        served_checksums = true;
                        (checksums.as_slice(), "text/plain; charset=utf-8")
                    } else {
                        return Err(io::Error::other(format!(
                            "unexpected release asset request path {request_path:?}"
                        )));
                    };
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )?;
                    stream.write_all(body)?;
                    if served_archive && served_checksums {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "timed out waiting for release asset request",
                        ));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error),
            }
        }
    });
    Ok((base_url, handle))
}

fn sha256_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut rendered = String::with_capacity(digest.len() * 2);
    for byte in digest {
        rendered.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}

fn write_executable_script(path: &Path, script: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Run the bundled plugin installer with an explicit runtime path.
fn run_projectatlas_plugin_installer(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_projectatlas_plugin_installer_with_optional_path(workspace_root, repo, runtime, None)
}

/// Run the bundled plugin installer with a PATH shadow and isolated user-local dirs.
fn run_projectatlas_plugin_installer_with_path_shadow_and_home(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
    path_shadow: &Path,
    home: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_projectatlas_plugin_installer_with_optional_path_and_home(
        workspace_root,
        repo,
        runtime,
        Some(path_shadow),
        Some(home),
    )
}

/// Run the bundled plugin installer and return its process output.
fn run_projectatlas_plugin_installer_with_optional_path(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
    path_shadow: Option<&Path>,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_projectatlas_plugin_installer_with_optional_path_and_home(
        workspace_root,
        repo,
        runtime,
        path_shadow,
        None,
    )
}

/// Run the bundled plugin installer and return its process output.
fn run_projectatlas_plugin_installer_with_optional_path_and_home(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
    path_shadow: Option<&Path>,
    home: Option<&Path>,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut command = if cfg!(windows) {
        let mut command = StdCommand::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(
                workspace_root
                    .join("plugins")
                    .join("projectatlas")
                    .join("scripts")
                    .join("install-runtime.ps1"),
            )
            .arg("-ProjectRoot")
            .arg(repo)
            .arg("-RuntimePath")
            .arg(runtime);
        command
    } else {
        let mut command = StdCommand::new("bash");
        command
            .arg(
                workspace_root
                    .join("plugins")
                    .join("projectatlas")
                    .join("scripts")
                    .join("install-runtime.sh"),
            )
            .arg(repo);
        command
    };
    command
        .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
        .env("PROJECTATLAS_RUNTIME_PATH", runtime)
        .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1");
    if let Some(path_shadow) = path_shadow {
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let shadowed_path = std::env::join_paths(
            std::iter::once(path_shadow.to_path_buf()).chain(std::env::split_paths(&current_path)),
        )?;
        command.env("PATH", shadowed_path);
        let fake_codex = path_shadow.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
        if fake_codex.exists() {
            command
                .env("PROJECTATLAS_CODEX_COMMAND", fake_codex)
                .env_remove("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE")
                .env_remove("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE");
        }
    }
    if let Some(home) = home {
        let app_data = home.join("AppData").join("Roaming");
        let local_app_data = home.join("AppData").join("Local");
        fs::create_dir_all(&app_data)?;
        fs::create_dir_all(&local_app_data)?;
        command
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("APPDATA", app_data)
            .env("LOCALAPPDATA", local_app_data)
            .env(
                "PROJECTATLAS_FAKE_CODEX_LOG",
                home.join(FAKE_CODEX_LOG_FILE),
            )
            .env(
                FAKE_CODEX_PLUGIN_ADD_FAILURE_MARKER_ENV,
                home.join(FAKE_CODEX_PLUGIN_ADD_FAILURE_MARKER_FILE),
            )
            .env(
                "PROJECTATLAS_FAKE_PLUGIN_MANIFEST",
                home.join(FAKE_CODEX_PLUGIN_CACHE_DIR)
                    .join("projectatlas")
                    .join(CODEX_PLUGIN_MANIFEST_DIR)
                    .join("plugin.json"),
            );
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "plugin installer failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(output)
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

/// Run one successful normal CLI command and decode its JSON result.
#[cfg(feature = "optional-parser-supervisor")]
fn projectatlas_json(
    repo: &Path,
    host_state: &Path,
    arguments: &[&OsStr],
) -> Result<Value, Box<dyn Error>> {
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .env("HOME", host_state.join(PARSER_PACK_TEST_HOME_DIR))
        .env(
            "LOCALAPPDATA",
            host_state.join(PARSER_PACK_TEST_LOCAL_APP_DATA_DIR),
        )
        .env(
            "XDG_DATA_HOME",
            host_state.join(PARSER_PACK_TEST_XDG_DATA_DIR),
        )
        .arg("--format")
        .arg("json")
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "projectatlas command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

/// Repack a verified archive after adding one semantically inert manifest whitespace byte.
#[cfg(feature = "optional-parser-supervisor")]
fn derive_whitespace_distinct_parser_archive(
    source: &Path,
    destination: &Path,
) -> Result<String, Box<dyn Error>> {
    const ARCHIVE_ROOT: &str = "projectatlas-broad-parser";
    const ARTIFACT_MANIFEST: &str = "artifact-manifest.json";
    const TAR_FRAMING_ALLOWANCE_BYTES: u64 = 1024 * 1024;

    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.file_type().is_file()
        || source_metadata.len() == 0
        || source_metadata.len() > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES
    {
        return Err(
            io::Error::other("verified source archive is not a bounded regular file").into(),
        );
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "replacement parser archive already exists",
        )
        .into());
    }

    let decoder = zstd::Decoder::new(fs::File::open(source)?)?;
    let maximum_tar_bytes = OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES
        .checked_add(TAR_FRAMING_ALLOWANCE_BYTES)
        .ok_or_else(|| io::Error::other("replacement archive framing bound overflowed"))?;
    let mut input = decoder.take(maximum_tar_bytes);
    let mut source_archive = tar::Archive::new(&mut input);
    let mut encoder = zstd::Encoder::new(fs::File::create(destination)?, 9)?;
    encoder.include_checksum(true)?;
    let mut destination_archive = tar::Builder::new(encoder);
    let prefix = format!("{ARCHIVE_ROOT}/");
    let mut previous_path: Option<String> = None;
    let mut expanded_bytes = 0u64;
    let mut entries_seen = 0usize;
    let mut replacement_artifact = None;

    for entry in source_archive.entries()? {
        let mut entry = entry?;
        entries_seen = entries_seen
            .checked_add(1)
            .ok_or_else(|| io::Error::other("replacement archive entry count overflowed"))?;
        if entries_seen > OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES.saturating_add(1)
            || !entry.header().entry_type().is_file()
        {
            return Err(io::Error::other(
                "verified source archive has an invalid bounded regular-file inventory",
            )
            .into());
        }
        let archive_path = std::str::from_utf8(entry.path_bytes().as_ref())?.to_owned();
        let relative = PackRelativePath::new(
            archive_path
                .strip_prefix(&prefix)
                .ok_or_else(|| io::Error::other("verified archive entry is outside pack root"))?,
        )?;
        if previous_path
            .as_ref()
            .is_some_and(|previous| previous.as_str() >= relative.as_str())
        {
            return Err(io::Error::other(
                "verified source archive entries are not strictly path-sorted",
            )
            .into());
        }
        previous_path = Some(relative.as_str().to_owned());

        let source_bytes = entry.header().size()?;
        if source_bytes == 0 || source_bytes > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES {
            return Err(io::Error::other("verified archive entry exceeds its byte bound").into());
        }
        let mode = entry.header().mode()?;
        if entry.header().uid()? != 0 || entry.header().gid()? != 0 || entry.header().mtime()? != 0
        {
            return Err(
                io::Error::other("verified archive entry metadata is not canonical").into(),
            );
        }

        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        if relative.as_str() == ARTIFACT_MANIFEST {
            let maximum_manifest_bytes = u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?;
            if source_bytes >= maximum_manifest_bytes {
                return Err(io::Error::other(
                    "artifact manifest has no room for deterministic whitespace",
                )
                .into());
            }
            let mut manifest = Vec::with_capacity(usize::try_from(source_bytes)? + 1);
            entry
                .by_ref()
                .take(source_bytes.saturating_add(1))
                .read_to_end(&mut manifest)?;
            if u64::try_from(manifest.len())? != source_bytes {
                return Err(
                    io::Error::other("artifact manifest size differs from tar header").into(),
                );
            }
            let semantics: Value = serde_json::from_slice(&manifest)?;
            manifest.push(b' ');
            if serde_json::from_slice::<Value>(&manifest)? != semantics {
                return Err(io::Error::other("manifest whitespace changed JSON semantics").into());
            }
            header.set_size(u64::try_from(manifest.len())?);
            header.set_cksum();
            destination_archive.append_data(&mut header, archive_path, manifest.as_slice())?;
            expanded_bytes = expanded_bytes
                .checked_add(u64::try_from(manifest.len())?)
                .ok_or_else(|| io::Error::other("replacement archive byte count overflowed"))?;
            replacement_artifact = Some(blake3::hash(&manifest).to_hex().to_string());
        } else {
            header.set_size(source_bytes);
            header.set_cksum();
            destination_archive.append_data(&mut header, archive_path, &mut entry)?;
            expanded_bytes = expanded_bytes
                .checked_add(source_bytes)
                .ok_or_else(|| io::Error::other("replacement archive byte count overflowed"))?;
        }
        if expanded_bytes > OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES {
            return Err(
                io::Error::other("replacement archive exceeds its expanded-byte bound").into(),
            );
        }
    }
    destination_archive.finish()?;
    let encoder = destination_archive.into_inner()?;
    let output = encoder.finish()?;
    output.sync_all()?;
    let output_metadata = fs::symlink_metadata(destination)?;
    if !output_metadata.file_type().is_file()
        || output_metadata.len() == 0
        || output_metadata.len() > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES
    {
        return Err(io::Error::other("replacement archive is not a bounded regular file").into());
    }
    replacement_artifact
        .ok_or_else(|| io::Error::other("verified archive omitted artifact-manifest.json").into())
}

/// Read a `JSON` file from disk.
fn read_json_file(path: &Path) -> Result<Value, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(Into::into)
}

/// Return a nested `JSON` string.
fn json_string_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, Box<dyn Error>> {
    json_at(value, path)?
        .as_str()
        .ok_or_else(|| io::Error::other(format!("expected string at {path:?}")).into())
}

/// Require an emitted command to point at the expected runtime.
fn require_same_executable(
    actual: &str,
    expected: &Path,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual_path = Path::new(actual);
    if !actual_path.is_absolute() {
        return Err(io::Error::other(format!("{label} runtime path was not absolute")).into());
    }
    if actual_path.canonicalize()? == expected.canonicalize()? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{label} runtime path mismatch: expected {}, found {}",
            expected.display(),
            actual_path.display()
        ))
        .into())
    }
}

/// Require an emitted working directory to point at the expected project root.
fn require_same_directory(
    actual: &str,
    expected: &Path,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual_path = Path::new(actual);
    if !actual_path.is_absolute() {
        return Err(io::Error::other(format!("{label} path was not absolute")).into());
    }
    if actual_path.canonicalize()? == expected.canonicalize()? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{label} path mismatch: expected {}, found {}",
            expected.display(),
            actual_path.display()
        ))
        .into())
    }
}

/// Require one emitted database or config path to resolve to the expected file.
fn require_same_canonical_path(
    actual: &str,
    expected: &Path,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual_path = Path::new(actual);
    if !actual_path.is_absolute() {
        return Err(io::Error::other(format!("{label} path was not absolute")).into());
    }
    if actual_path.canonicalize()? == expected.canonicalize()? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{label} path mismatch: expected {}, found {}",
            expected.display(),
            actual_path.display()
        ))
        .into())
    }
}

/// Require a nested JSON string value.
fn require_json_string(value: &Value, path: &[&str], expected: &str) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_str() == Some(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected:?}, found {current:?}"
        ))
        .into())
    }
}

/// Require a nested JSON string to contain a substring.
fn require_json_contains(
    value: &Value,
    path: &[&str],
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let text = current
        .as_str()
        .ok_or_else(|| io::Error::other(format!("expected string at {path:?}")))?;
    if text.contains(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to contain {expected:?}, found {text:?}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value.
fn require_json_usize(value: &Value, path: &[&str], expected: usize) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_u64() == Some(u64::try_from(expected)?) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected}, found {current:?}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value to be at least a threshold.
fn require_json_usize_at_least(
    value: &Value,
    path: &[&str],
    expected_minimum: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_u64()
        .ok_or_else(|| io::Error::other(format!("expected integer at {path:?}")))?;
    if actual >= u64::try_from(expected_minimum)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to be at least {expected_minimum}, found {actual}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value to be greater than a threshold.
fn require_json_usize_greater_than(
    value: &Value,
    path: &[&str],
    threshold: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_u64()
        .ok_or_else(|| io::Error::other(format!("expected integer at {path:?}")))?;
    if actual > u64::try_from(threshold)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to be greater than {threshold}, found {actual}"
        ))
        .into())
    }
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

/// Require a nested JSON array length.
fn require_json_array_len(
    value: &Value,
    path: &[&str],
    expected: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let length = current
        .as_array()
        .ok_or_else(|| io::Error::other(format!("expected array at {path:?}")))?
        .len();
    if length == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} length {expected}, found {length}"
        ))
        .into())
    }
}

/// Require a nested JSON boolean value.
fn require_json_bool(value: &Value, path: &[&str], expected: bool) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_bool() == Some(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected}, found {current:?}"
        ))
        .into())
    }
}

/// Return symbol names from a structured summary section.
fn json_symbol_names(value: &Value, section: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let symbols = json_at(value, &[section])?
        .as_array()
        .ok_or_else(|| io::Error::other(format!("expected array section {section}")))?;
    symbols
        .iter()
        .map(|symbol| {
            symbol
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| io::Error::other(format!("missing symbol name in {section}")))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Run a JSON summary command for one indexed path.
fn json_summary_command(repo: &Path, db: &Path, file: &str) -> Result<Value, Box<dyn Error>> {
    let output = Command::cargo_bin("projectatlas")?
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .args(["summary", file, "--limit", "10"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "summary command failed for {file}: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

/// Navigate a JSON value by object keys and decimal array indexes.
fn json_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value, Box<dyn Error>> {
    let mut current = value;
    for segment in path {
        current = if let Some(array) = current.as_array() {
            let index = segment.parse::<usize>()?;
            array
                .get(index)
                .ok_or_else(|| io::Error::other(format!("missing json array index {segment}")))?
        } else {
            current
                .get(segment)
                .ok_or_else(|| io::Error::other(format!("missing json segment {segment}")))?
        };
    }
    Ok(current)
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
