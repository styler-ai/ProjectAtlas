//! Purpose: Validate lifecycle, database, startup, and parser-pack contracts.
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
    MCP_CONTRACT_EXECUTABLE_ENV, McpDatabaseSnapshot, complete_mcp_test_after_shutdown,
    git_command_for_root, json_at, json_summary_command, mcp_contract_executable,
    mcp_database_snapshot, mcp_tool_text, require_json_array_len, require_json_bool,
    require_json_contains, require_json_string, require_json_usize, require_json_usize_at_least,
    require_json_usize_greater_than, run_mcp_stdio, run_mcp_stdio_with_env, sha256_hex,
    sqlite_table_digests, workspace_root,
};
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";

const SRC_DIR_NAME: &str = "src";

const HIDDEN_RS_FILE_NAME: &str = "hidden.rs";

const LIB_RS_FILE_NAME: &str = "lib.rs";

const BARE_REPOSITORY_DIR_NAME: &str = "repository.git";

const ATLAS_DIR_NAME: &str = ".projectatlas";

#[cfg(feature = "optional-parser-supervisor")]
const VERSIONS_DIR_NAME: &str = "versions";

const PACKAGE_JSON_FILE_NAME: &str = "package.json";

const IGNORED_FIXTURE_DIR: &str = "ignored-dir";

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

#[cfg(target_os = "linux")]
const BTRFS_TEST_ROOT_ENV: &str = "PROJECTATLAS_BTRFS_TEST_ROOT";

const SUBDIR_CONFIG_DIR: &str = "config";

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

#[test]
fn installed_candidate_version_is_consistent_across_cli_runtime_and_token_tui()
-> Result<(), Box<dyn Error>> {
    let expected_version = env!("CARGO_PKG_VERSION");
    let executable = mcp_contract_executable();
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::write(repo.join("lib.rs"), "pub fn indexed() {}\n")?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");

    let version_output = StdCommand::new(&executable).arg("--version").output()?;
    if !version_output.status.success() {
        return Err(io::Error::other(format!(
            "installed candidate --version failed: {}",
            String::from_utf8_lossy(&version_output.stderr)
        ))
        .into());
    }
    let version_stdout = String::from_utf8(version_output.stdout)?;
    let cli_version = version_stdout
        .trim()
        .strip_prefix("projectatlas ")
        .ok_or_else(|| io::Error::other("installed candidate emitted an invalid --version line"))?;

    let runtime_info = run_mcp_contract_json(&executable, &repo, &["runtime-info".to_string()])?;
    let runtime_version = json_at(&runtime_info, &["version"])?
        .as_str()
        .ok_or_else(|| io::Error::other("runtime-info.version was not a string"))?;

    run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    )?;
    let token_output = StdCommand::new(&executable)
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .env("COLUMNS", "40")
        .env("LINES", "8")
        .arg("--db")
        .arg(&database)
        .args(["token", "--view", "tui"])
        .output()?;
    if !token_output.status.success() {
        return Err(io::Error::other(format!(
            "installed candidate token TUI failed: {}",
            String::from_utf8_lossy(&token_output.stderr)
        ))
        .into());
    }
    let token_stdout = String::from_utf8(token_output.stdout)?;
    require_tui_output_within_viewport(&token_stdout, 40, 8)?;
    let footer_prefix = "ProjectAtlas v";
    if token_stdout.matches(footer_prefix).count() != 1 {
        return Err(io::Error::other(
            "token TUI must render exactly one ProjectAtlas version footer",
        )
        .into());
    }
    let footer_version = token_stdout
        .split_once(footer_prefix)
        .and_then(|(_, tail)| {
            tail.split(|character: char| character.is_whitespace() || character == '\u{1b}')
                .next()
        })
        .ok_or_else(|| io::Error::other("token TUI version footer was not parseable"))?;

    if [cli_version, runtime_version, footer_version]
        .into_iter()
        .any(|version| version != expected_version)
    {
        return Err(io::Error::other(format!(
            "installed candidate version drift: workspace={expected_version} cli={cli_version} runtime-info={runtime_version} token-tui={footer_version}"
        ))
        .into());
    }
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

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a supplied native Btrfs subvolume and exact candidate runtime"]
fn linux_btrfs_subvolume_database_supports_cli_and_persistent_mcp_reopen()
-> Result<(), Box<dyn Error>> {
    let btrfs_root = std::env::var_os(BTRFS_TEST_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{BTRFS_TEST_ROOT_ENV} must be supplied")))?
        .canonicalize()?;
    let executable = std::env::var_os(MCP_CONTRACT_EXECUTABLE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::other(format!("{MCP_CONTRACT_EXECUTABLE_ENV} must be supplied"))
        })?;
    if !executable.is_file() {
        return Err(io::Error::other(format!(
            "candidate executable does not exist: {}",
            executable.display()
        ))
        .into());
    }

    let fixture = tempfile::Builder::new()
        .prefix("projectatlas-btrfs-")
        .tempdir_in(&btrfs_root)?;
    let repo = fixture.path().join(TEST_REPO_DIR);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn btrfs_contract() -> &'static str { \"ready\" }\n",
    )?;
    git_success(&repo, &["init", "--quiet"])?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");

    let runtime = run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--require-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "runtime-info".to_string(),
        ],
    )?;
    require_json_string(&runtime, &["version"], env!("CARGO_PKG_VERSION"))?;
    for arguments in [
        vec![
            "--require-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "--db".to_string(),
            database.display().to_string(),
            "init".to_string(),
            "--no-scan".to_string(),
        ],
        vec![
            "--require-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "--db".to_string(),
            database.display().to_string(),
            "scan".to_string(),
            ".".to_string(),
        ],
    ] {
        run_mcp_contract_json(&executable, &repo, &arguments)?;
    }
    let cli_summary = run_mcp_contract_json(
        &executable,
        &repo,
        &[
            "--require-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "--db".to_string(),
            database.display().to_string(),
            "summary".to_string(),
            "src/lib.rs".to_string(),
            "--limit".to_string(),
            "5".to_string(),
        ],
    )?;
    require_json_contains(&cli_summary, &["content_summary"], "btrfs_contract")?;

    let stable_database = mcp_database_snapshot(&database)?;
    let project_path = repo.to_string_lossy().to_string();
    for phase in ["initial", "reopened"] {
        let mut session = McpContractSession::spawn(&executable, &repo, &database)?;
        let operation_result = (|| -> Result<(), Box<dyn Error>> {
            let summary = session.call_tool(
                "atlas_file_summary",
                &json!({
                    "project_path": project_path,
                    "file": "src/lib.rs",
                    "compact": true
                }),
            )?;
            if !summary.contains("file_summary:") || !summary.contains("btrfs_contract") {
                return Err(io::Error::other(format!(
                    "{phase} persistent MCP summary lost Btrfs fixture evidence: {summary}"
                ))
                .into());
            }
            Ok(())
        })();
        complete_mcp_test_after_shutdown(operation_result, || session.shutdown())?;
    }
    if mcp_database_snapshot(&database)? != stable_database {
        return Err(io::Error::other(
            "stable persistent MCP summaries mutated the Btrfs-backed database",
        )
        .into());
    }
    Ok(())
}

#[test]
fn persistent_mcp_stdin_does_not_block_repository_startup_probes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("persistent-git-probe");
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn ready() {}\n",
    )?;
    git_success(&repo, &["init", "--quiet"])?;
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let executable = mcp_contract_executable();
    let init = StdCommand::new(&executable)
        .current_dir(&repo)
        .args(["--format", "json", "init"])
        .output()?;
    if !init.status.success() {
        return Err(io::Error::other(format!(
            "persistent-probe fixture init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        ))
        .into());
    }

    let project_path = repo.to_string_lossy().to_string();
    let mut session = McpContractSession::spawn(&executable, &repo, &database)?;
    for (tool, arguments, expected) in [
        (
            "atlas_session_brief",
            json!({"project_path": project_path, "compact": true}),
            "session_brief:",
        ),
        (
            "atlas_root",
            json!({"project_path": project_path, "verify": true}),
            "root:",
        ),
        (
            "atlas_init",
            json!({"project_path": project_path, "no_scan": true}),
            "init:",
        ),
        (
            "atlas_overview",
            json!({"project_path": project_path}),
            "overview:",
        ),
        (
            "atlas_folders",
            json!({"project_path": project_path, "query": SRC_DIR_NAME, "limit": 2}),
            "folders",
        ),
        (
            "atlas_files",
            json!({"project_path": project_path, "query": "ready", "folder": SRC_DIR_NAME, "limit": 2}),
            "files",
        ),
        (
            "atlas_file_summary",
            json!({"project_path": project_path, "file": "src/lib.rs", "compact": true}),
            "file_summary:",
        ),
        (
            "atlas_outline",
            json!({"project_path": project_path, "file": "src/lib.rs", "lines": 4}),
            "outline:",
        ),
        (
            "atlas_search",
            json!({"project_path": project_path, "pattern": "ready", "file_pattern": "src/*.rs", "limit": 1}),
            "search:",
        ),
        (
            "atlas_slice",
            json!({"project_path": project_path, "file": "src/lib.rs", "start_line": 1, "end_line": 1, "output_bytes": 4096}),
            "slice:",
        ),
    ] {
        let started = Instant::now();
        let text = session.call_tool(tool, &arguments)?;
        if started.elapsed() > Duration::from_secs(5) || !text.contains(expected) {
            return Err(io::Error::other(format!(
                "{tool} did not complete over persistent stdin: elapsed={:?} text={text}",
                started.elapsed()
            ))
            .into());
        }
    }
    let followup = session.call_tool(
        "atlas_root",
        &json!({"project_path": project_path, "verify": true}),
    )?;
    if !followup.contains("verified: true") {
        return Err(io::Error::other(format!(
            "immediate root follow-up failed after persistent probes: {followup}"
        ))
        .into());
    }
    session.shutdown()
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

    let release_root = temp.path().join("release-verifier");
    fs::create_dir(&release_root)?;
    for (label, archive, context, proof) in [
        (
            "missing archive and context",
            release_root.join("missing/archive.tar.zst"),
            release_root.join("missing/runner-context.json"),
            release_root.join("missing/output/platform-proof.json"),
        ),
        (
            "invalid archive and context",
            release_root.join("invalid/archive.tar.zst"),
            release_root.join("invalid/runner-context.json"),
            release_root.join("invalid/output/platform-proof.json"),
        ),
        (
            "unreadable archive and context",
            release_root.join("unreadable/archive.tar.zst"),
            release_root.join("unreadable/runner-context.json"),
            release_root.join("unreadable/output/platform-proof.json"),
        ),
    ] {
        let case_root = archive
            .parent()
            .ok_or_else(|| io::Error::other("release verifier case has no parent"))?;
        fs::create_dir_all(case_root)?;
        let temp_root = case_root.join("temp");
        let home_root = case_root.join("home");
        fs::create_dir(&temp_root)?;
        fs::create_dir(&home_root)?;
        match label {
            "invalid archive and context" => {
                fs::write(&archive, b"not a parser-pack archive")?;
                fs::write(&context, b"not runner context")?;
                fs::create_dir(
                    proof
                        .parent()
                        .ok_or_else(|| io::Error::other("invalid release proof has no parent"))?,
                )?;
            }
            "unreadable archive and context" => {
                fs::create_dir(&archive)?;
                fs::create_dir(&context)?;
            }
            _ => {}
        }
        let output = Command::cargo_bin("optional_parser_pack_release")?
            .current_dir(&release_root)
            .env("HOME", &home_root)
            .env("TMPDIR", &temp_root)
            .args([
                OsStr::new("verify"),
                archive.as_os_str(),
                context.as_os_str(),
                proof.as_os_str(),
            ])
            .output()?;
        if output.status.success()
            || !String::from_utf8_lossy(&output.stderr).contains("unsupported_containment")
        {
            return Err(io::Error::other(format!(
                "macOS release verifier {label} did not refuse typed unsupported containment: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if proof.exists()
            || fs::read_dir(&temp_root)?.next().is_some()
            || fs::read_dir(&home_root)?.next().is_some()
        {
            return Err(io::Error::other(format!(
                "macOS release verifier {label} touched proof, temporary, or payload state"
            ))
            .into());
        }
        if archive.is_file() && fs::read(&archive)?.as_slice() != b"not a parser-pack archive" {
            return Err(io::Error::other(format!(
                "macOS release verifier {label} changed the invalid archive"
            ))
            .into());
        }
        if context.is_file() && fs::read(&context)?.as_slice() != b"not runner context" {
            return Err(io::Error::other(format!(
                "macOS release verifier {label} changed runner context"
            ))
            .into());
        }
    }

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

    let host_state = temp.path().join("builtin-host-state");
    fs::create_dir_all(&host_state)?;
    let _init = projectatlas_json(&repo, &host_state, &[OsStr::new("init")])?;
    let files = projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("files"), OsStr::new("main")],
    )?;
    if !files.to_string().contains("src/main.rs") {
        return Err(
            io::Error::other("macOS built-in parser navigation omitted src/main.rs").into(),
        );
    }
    let summary = projectatlas_json(
        &repo,
        &host_state,
        &[OsStr::new("summary"), OsStr::new("src/main.rs")],
    )?;
    require_json_contains(&summary, &["content_summary"], "untouched")?;
    let settings = projectatlas_json(&repo, &host_state, &[OsStr::new("settings")])?;
    let lifecycle = settings
        .pointer("/optional_parser_pack/lifecycle")
        .ok_or_else(|| io::Error::other("macOS settings omitted parser-pack lifecycle"))?;
    if lifecycle
        .pointer("/capability/mode")
        .and_then(Value::as_str)
        != Some("built_in_only")
        || lifecycle.get("platform").is_some()
    {
        return Err(
            io::Error::other("macOS settings overstated optional parser-pack support").into(),
        );
    }
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let mut mcp = McpContractSession::spawn(&executable, &repo, &database)?;
    let mcp_result = (|| -> Result<(), Box<dyn Error>> {
        let mcp_settings_text = mcp.call_tool("atlas_settings", &json!({}))?;
        let mcp_settings: Value = toon_format::decode_default(&mcp_settings_text)?;
        let mcp_lifecycle = mcp_settings
            .pointer("/settings/optional_parser_pack/lifecycle")
            .ok_or_else(|| io::Error::other("macOS MCP settings omitted parser-pack lifecycle"))?;
        if mcp_lifecycle
            .pointer("/capability/mode")
            .and_then(Value::as_str)
            != Some("built_in_only")
            || mcp_lifecycle.pointer("/platform").is_some()
            || mcp_lifecycle.pointer("/supported").and_then(Value::as_bool) != Some(false)
        {
            return Err(io::Error::other(format!(
                "macOS MCP settings overstated optional parser-pack support: {mcp_settings}"
            ))
            .into());
        }
        let mcp_summary = mcp.call_tool(
            "atlas_file_summary",
            &json!({
                "project_path": repo.to_string_lossy(),
                "file": "src/main.rs",
                "compact": true
            }),
        )?;
        if !mcp_summary.contains("file_summary:") || !mcp_summary.contains("untouched") {
            return Err(io::Error::other(format!(
                "macOS MCP lost built-in parser summary evidence: {mcp_summary}"
            ))
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(mcp_result, || mcp.shutdown())?;
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
    for (label, schema) in [
        (
            "fresh-v0.3.26",
            include_str!("../../projectatlas-db/tests/fixtures/released-schema-8.sql"),
        ),
        (
            "evolved-v0.3.11-to-v0.3.26",
            include_str!("../../projectatlas-db/tests/fixtures/released-schema-8-evolved.sql"),
        ),
    ] {
        assert_settings_reports_supported_predecessor_without_migration(label, schema)?;
    }
    Ok(())
}

/// Verify settings reports one released predecessor layout without writing it.
fn assert_settings_reports_supported_predecessor_without_migration(
    label: &str,
    schema: &str,
) -> Result<(), Box<dyn Error>> {
    #[cfg(not(unix))]
    let supported_schema = projectatlas_db::CURRENT_SCHEMA_VERSION;
    #[cfg(not(unix))]
    let migration_steps = u64::try_from(supported_schema - 8)?;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let db_path = atlas_dir.join("projectatlas.db");
    let connection = Connection::open(&db_path)?;
    connection.execute_batch(schema)?;
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('schema_version', '8')",
        [],
    )?;
    let project_root = normalize_native_path_display(fs::canonicalize(&repo)?);
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('project_root', ?1)",
        [project_root],
    )?;
    drop(connection);
    let bytes_before = fs::read(&db_path)?;

    #[cfg(unix)]
    {
        let token = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", "--db"])
            .arg(&db_path)
            .arg("token")
            .output()?;
        let token_error = String::from_utf8_lossy(&token.stderr);
        if token.status.success() || !token_error.contains("canonical project-root identity") {
            return Err(io::Error::other(format!(
                "Unix predecessor {label} was not refused before root selection: {token_error}"
            ))
            .into());
        }
        if fs::read(&db_path)? != bytes_before {
            return Err(io::Error::other(format!(
                "Unix predecessor {label} refusal changed the database"
            ))
            .into());
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let token = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", "--db"])
            .arg(&db_path)
            .arg("token")
            .output()?;
        let token_error = String::from_utf8_lossy(&token.stderr);
        let token_error_json: Value = serde_json::from_slice(&token.stderr).map_err(|source| {
        io::Error::other(format!(
            "token predecessor response was not JSON: status={:?} stdout={} stderr={} error={source}",
            token.status.code(),
            String::from_utf8_lossy(&token.stdout),
            token_error,
        ))
    })?;
        if token.status.success()
            || token_error_json
                .pointer("/error/kind")
                .and_then(Value::as_str)
                != Some("schema_migration_required")
            || token_error_json
                .pointer("/error/schema_migration_required/found_schema_version")
                .and_then(Value::as_i64)
                != Some(8)
            || token_error_json
                .pointer("/error/schema_migration_required/supported_schema_version")
                .and_then(Value::as_i64)
                != Some(supported_schema)
            || token_error_json
                .pointer("/error/schema_migration_required/migration_steps_remaining")
                .and_then(Value::as_u64)
                != Some(migration_steps)
            || !token_error.contains("projectatlas init")
            || !token_error.contains("atlas_init")
            || !token_error.contains("same global `--db`/`--config` selection")
            || !token_error.contains("same MCP server/database binding")
            || token_error.contains("schema_version_mismatch")
            || token_error.contains("unsupported schema version")
        {
            return Err(io::Error::other(format!(
                "token misclassified the supported predecessor {label}: {token_error}"
            ))
            .into());
        }

        let executable = assert_cmd::cargo::cargo_bin("projectatlas");
        let mut mcp = McpContractSession::spawn(&executable, &repo, &db_path)?;
        let mcp_result = (|| -> Result<(), Box<dyn Error>> {
            let token_report = mcp.call_tool("atlas_token_report", &json!({}))?;
            if !token_report.contains("kind: schema_migration_required")
                || !token_report.contains("found_schema_version: 8")
                || !token_report.contains(&format!("supported_schema_version: {supported_schema}"))
                || !token_report.contains(&format!("migration_steps_remaining: {migration_steps}"))
                || !token_report.contains("projectatlas init")
                || !token_report.contains("atlas_init")
                || !token_report.contains("same global `--db`/`--config` selection")
                || !token_report.contains("same MCP server/database binding")
                || token_report.contains("schema_version_mismatch")
                || token_report.contains("unsupported schema version")
            {
                return Err(io::Error::other(format!(
                "MCP token report misclassified the supported predecessor {label}: {token_report}"
            ))
            .into());
            }
            let mcp_settings = mcp.call_tool("atlas_settings", &json!({}))?;
            for required in [
                "compatibility: supported_predecessor".to_string(),
                "migration_required: true".to_string(),
                "migration_supported: true".to_string(),
                format!("migration_steps_remaining: {migration_steps}"),
            ] {
                if !mcp_settings.contains(&required) {
                    return Err(io::Error::other(format!(
                    "MCP settings omitted predecessor field {required:?} for {label}: {mcp_settings}"
                ))
                .into());
                }
            }
            Ok(())
        })();
        complete_mcp_test_after_shutdown(mcp_result, || mcp.shutdown())?;

        let output = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", "settings"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "settings failed for {label}: {}",
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
                != Some(migration_steps)
            || !settings.get("index").is_some_and(Value::is_null)
            || !settings.get("telemetry").is_some_and(Value::is_null)
        {
            return Err(io::Error::other(format!(
                "settings misstated the supported predecessor {label}"
            ))
            .into());
        }
        if settings
            .get("search")
            .and_then(|value| value.get("lexical"))
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            != Some("unavailable")
        {
            return Err(
                io::Error::other("predecessor settings overstated lexical readiness").into(),
            );
        }
        if fs::read(&db_path)? != bytes_before {
            return Err(io::Error::other(format!(
                "settings migrated or mutated the predecessor database {label}"
            ))
            .into());
        }
        Ok(())
    }
}

#[test]
fn supported_predecessor_recovery_preserves_explicit_database_selection()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let source_dir = repo.join(SRC_DIR_NAME);
    let selected_dir = repo.join("selected-state");
    fs::create_dir_all(&source_dir)?;
    fs::create_dir_all(&selected_dir)?;
    fs::write(source_dir.join(LIB_RS_FILE_NAME), "pub fn selected() {}\n")?;

    let cli_db = selected_dir.join("cli.db");
    let mcp_db = selected_dir.join("mcp.db");
    let config = selected_dir.join("config.toml");
    let project_root = normalize_native_path_display(fs::canonicalize(&repo)?);
    fs::write(
        &config,
        format!(
            "[project]\nroot = \"{}\"\n",
            project_root.replace('\\', "/")
        ),
    )?;
    for database in [&cli_db, &mcp_db] {
        let connection = Connection::open(database)?;
        connection.execute_batch(include_str!(
            "../../projectatlas-db/tests/fixtures/released-schema-8.sql"
        ))?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES ('schema_version', '8')",
            [],
        )?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES ('project_root', ?1)",
            [&project_root],
        )?;
    }
    let default_db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let executable = mcp_contract_executable();

    #[cfg(unix)]
    {
        for database in [&cli_db, &mcp_db] {
            let before = fs::read(database)?;
            let output = Command::new(&executable)
                .current_dir(&repo)
                .args(["--format", "json", "--db"])
                .arg(database)
                .arg("--config")
                .arg(&config)
                .arg("token")
                .output()?;
            let error = String::from_utf8_lossy(&output.stderr);
            if output.status.success() || !error.contains("canonical project-root identity") {
                return Err(io::Error::other(format!(
                    "Unix explicit predecessor was not refused before selection: {error}"
                ))
                .into());
            }
            if fs::read(database)? != before {
                return Err(io::Error::other(
                    "Unix explicit predecessor refusal changed the selected database",
                )
                .into());
            }
        }
        if default_db.exists() {
            return Err(
                io::Error::other("Unix predecessor refusal created the default database").into(),
            );
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let cli_error = Command::new(&executable)
            .current_dir(&repo)
            .args(["--format", "json", "--db"])
            .arg(&cli_db)
            .arg("--config")
            .arg(&config)
            .arg("token")
            .output()?;
        let cli_error_text = String::from_utf8_lossy(&cli_error.stderr);
        if cli_error.status.success()
            || !cli_error_text.contains("schema_migration_required")
            || !cli_error_text.contains("same global `--db`/`--config` selection")
        {
            return Err(io::Error::other(format!(
            "CLI recovery did not preserve the explicit database selection: status={:?} stdout={} stderr={cli_error_text}",
            cli_error.status.code(),
            String::from_utf8_lossy(&cli_error.stdout),
        ))
        .into());
        }
        let cli_init = Command::new(&executable)
            .current_dir(&repo)
            .args(["--format", "json", "--db"])
            .arg(&cli_db)
            .arg("--config")
            .arg(&config)
            .args(["init", "--no-scan"])
            .output()?;
        if !cli_init.status.success() {
            return Err(io::Error::other(format!(
                "CLI recovery failed for the selected database: {}",
                String::from_utf8_lossy(&cli_init.stderr)
            ))
            .into());
        }
        if default_db.exists() {
            return Err(io::Error::other("CLI recovery created the default database").into());
        }

        let mut mcp = McpContractSession::spawn(&executable, &repo, &mcp_db)?;
        let mcp_result = (|| -> Result<(), Box<dyn Error>> {
            let migration = mcp.call_tool("atlas_token_report", &json!({}))?;
            if !migration.contains("schema_migration_required")
                || !migration.contains("same MCP server/database binding")
            {
                return Err(io::Error::other(format!(
                    "MCP recovery did not preserve the configured database binding: {migration}"
                ))
                .into());
            }
            mcp.call_tool("atlas_init", &json!({"no_scan": true}))?;
            let settings = mcp.call_tool("atlas_settings", &json!({}))?;
            if !settings.contains("compatibility: current") {
                return Err(io::Error::other(format!(
                    "MCP recovery did not migrate the configured database: {settings}"
                ))
                .into());
            }
            Ok(())
        })();
        complete_mcp_test_after_shutdown(mcp_result, || mcp.shutdown())?;

        for (adapter, database) in [("CLI", &cli_db), ("MCP", &mcp_db)] {
            let connection = Connection::open(database)?;
            let stored_version: String = connection.query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )?;
            if stored_version != projectatlas_db::CURRENT_SCHEMA_VERSION.to_string() {
                return Err(io::Error::other(format!(
                    "{adapter} recovery did not migrate the explicitly selected database"
                ))
                .into());
            }
        }
        if default_db.exists() {
            return Err(io::Error::other("MCP recovery created the default database").into());
        }
        Ok(())
    }
}

#[test]
fn init_and_scan_migrate_both_released_schema_layouts() -> Result<(), Box<dyn Error>> {
    for (label, schema) in [
        (
            "fresh-v0.3.26",
            include_str!("../../projectatlas-db/tests/fixtures/released-schema-8.sql"),
        ),
        (
            "evolved-v0.3.11-to-v0.3.26",
            include_str!("../../projectatlas-db/tests/fixtures/released-schema-8-evolved.sql"),
        ),
    ] {
        for command in ["init", "scan"] {
            assert_cli_migrates_released_schema_layout(label, schema, command)?;
        }
    }
    Ok(())
}

/// Verify one public writable command migrates one released predecessor layout.
fn assert_cli_migrates_released_schema_layout(
    label: &str,
    schema: &str,
    command: &str,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn indexed() {}\n",
    )?;
    let db_path = atlas_dir.join("projectatlas.db");
    let project_root = normalize_native_path_display(fs::canonicalize(&repo)?);
    let connection = Connection::open(&db_path)?;
    connection.execute_batch(schema)?;
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('schema_version', '8')",
        [],
    )?;
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES ('project_root', ?1)",
        [&project_root],
    )?;
    drop(connection);

    #[cfg(unix)]
    {
        let bytes_before = fs::read(&db_path)?;
        let sidecars_before = ["-wal", "-shm", "-journal"]
            .map(|suffix| fs::read(sqlite_sidecar_path(&db_path, suffix)).ok());
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", command])
            .output()?;
        let error = String::from_utf8_lossy(&output.stderr);
        if output.status.success() || !error.contains("canonical project-root identity") {
            return Err(io::Error::other(format!(
                "Unix predecessor {label} {command} was not refused before migration: {error}"
            ))
            .into());
        }
        let sidecars_after = ["-wal", "-shm", "-journal"]
            .map(|suffix| fs::read(sqlite_sidecar_path(&db_path, suffix)).ok());
        if fs::read(&db_path)? != bytes_before || sidecars_after != sidecars_before {
            return Err(io::Error::other(format!(
                "Unix predecessor {label} {command} refusal changed database state"
            ))
            .into());
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", command])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "{command} failed to migrate {label}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        let connection = Connection::open(&db_path)?;
        let (stored_version, stored_root): (String, String) = connection.query_row(
            "SELECT
             (SELECT value FROM metadata WHERE key = 'schema_version'),
             (SELECT value FROM metadata WHERE key = 'project_root')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored_version != projectatlas_db::CURRENT_SCHEMA_VERSION.to_string()
            || stored_root != project_root
        {
            return Err(io::Error::other(format!(
            "{command} did not preserve and migrate {label}: version={stored_version}, root={stored_root}"
        ))
        .into());
        }
        drop(connection);

        let verify = Command::cargo_bin("projectatlas")?
            .current_dir(&repo)
            .args(["--format", "json", "root", "verify"])
            .output()?;
        if !verify.status.success() {
            return Err(io::Error::other(format!(
                "root verify failed after {command} migrated {label}: {}",
                String::from_utf8_lossy(&verify.stderr)
            ))
            .into());
        }
        let report: Value = serde_json::from_slice(&verify.stdout)?;
        require_json_bool(&report, &["verified"], true)?;
        Ok(())
    }
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
    let sealed_instances: i64 = connection.query_row(
        "SELECT COUNT(*) FROM usage_instances WHERE owner = 'mcp_process' AND state = 'sealed'",
        [],
        |row| row.get(0),
    )?;
    if sealed_instances != i64::try_from(RESTART_COUNT)? {
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
        "lowest_reliable_host_supported",
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
fn implicit_bare_root_refuses_before_opening_a_future_schema_database() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let bare = temp.path().join(BARE_REPOSITORY_DIR_NAME);
    let output = StdCommand::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git init --bare failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    let atlas_dir = bare.join(ATLAS_DIR_NAME);
    fs::create_dir(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let supported_schema_version = usize::try_from(projectatlas_db::CURRENT_SCHEMA_VERSION)?;
    let future_schema_version = supported_schema_version.saturating_add(1);
    {
        let connection = Connection::open(&database)?;
        connection.execute_batch(
            "
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE authored_state(value TEXT NOT NULL);
            INSERT INTO authored_state(value) VALUES('preserve-me');
            ",
        )?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES('schema_version', ?1)",
            [future_schema_version.to_string()],
        )?;
    }
    let database_before = fs::read(&database)?;

    let implicit = Command::cargo_bin("projectatlas")?
        .current_dir(&bare)
        .args(["--format", "json", "token", "--view", "tui"])
        .output()?;
    if implicit.status.success() {
        return Err(io::Error::other("implicit bare-root token read succeeded").into());
    }
    let implicit_error: Value = serde_json::from_slice(&implicit.stderr)?;
    require_json_string(&implicit_error, &["error", "kind"], "worktree_required")?;
    if fs::read(&database)? != database_before {
        return Err(
            io::Error::other("bare-root refusal changed future-schema database bytes").into(),
        );
    }
    for sidecar in ["projectatlas.db-wal", "projectatlas.db-shm"] {
        if atlas_dir.join(sidecar).exists() {
            return Err(io::Error::other(format!(
                "bare-root refusal created SQLite sidecar {sidecar}"
            ))
            .into());
        }
    }

    let explicit = Command::cargo_bin("projectatlas")?
        .current_dir(&bare)
        .args(["--format", "json", "--db"])
        .arg(&database)
        .args(["token", "--view", "tui"])
        .output()?;
    if explicit.status.success() {
        return Err(io::Error::other("explicit future-schema token read succeeded").into());
    }
    let explicit_error: Value = serde_json::from_slice(&explicit.stderr)?;
    require_json_string(
        &explicit_error,
        &["error", "kind"],
        "schema_version_mismatch",
    )?;
    require_json_usize(
        &explicit_error,
        &["error", "schema_version_mismatch", "found_schema_version"],
        future_schema_version,
    )?;
    require_json_usize(
        &explicit_error,
        &[
            "error",
            "schema_version_mismatch",
            "supported_schema_version",
        ],
        supported_schema_version,
    )?;
    require_json_string(
        &explicit_error,
        &["error", "schema_version_mismatch", "runtime_version"],
        env!("CARGO_PKG_VERSION"),
    )?;
    let explicit_stderr = String::from_utf8_lossy(&explicit.stderr);
    for private in [database.display().to_string(), bare.display().to_string()] {
        if explicit_stderr.contains(&private) {
            return Err(io::Error::other(format!(
                "explicit database schema error exposed private path {private}"
            ))
            .into());
        }
    }
    if fs::read(&database)? != database_before {
        return Err(
            io::Error::other("explicit future-schema refusal changed database bytes").into(),
        );
    }
    Ok(())
}

#[test]
fn implicit_bare_root_refusal_is_database_state_agnostic() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    for state in ["absent", "compatible", "older-supported", "malformed"] {
        let bare = temp.path().join(format!("{state}.git"));
        let output = StdCommand::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "git init --bare failed for {state}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }

        let atlas_dir = bare.join(ATLAS_DIR_NAME);
        let database = atlas_dir.join("projectatlas.db");
        match state {
            "absent" => {}
            "compatible" => {
                fs::create_dir(&atlas_dir)?;
                drop(AtlasStore::open(&database)?);
            }
            "older-supported" => {
                fs::create_dir(&atlas_dir)?;
                let connection = Connection::open(&database)?;
                connection.execute_batch(
                    "
                    CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                    INSERT INTO metadata(key, value) VALUES('schema_version', '8');
                    CREATE TABLE authored_state(value TEXT NOT NULL);
                    INSERT INTO authored_state(value) VALUES('preserve-me');
                    ",
                )?;
            }
            "malformed" => {
                fs::create_dir(&atlas_dir)?;
                fs::write(&database, b"not a SQLite database")?;
            }
            _ => unreachable!(),
        }
        let database_before = database.exists().then(|| fs::read(&database)).transpose()?;

        let implicit = Command::cargo_bin("projectatlas")?
            .current_dir(&bare)
            .args(["--format", "json", "settings"])
            .output()?;
        if implicit.status.success() {
            return Err(io::Error::other(format!(
                "implicit bare-root settings succeeded for {state}"
            ))
            .into());
        }
        let error: Value = serde_json::from_slice(&implicit.stderr)?;
        require_json_string(&error, &["error", "kind"], "worktree_required")?;
        if database.exists().then(|| fs::read(&database)).transpose()? != database_before {
            return Err(io::Error::other(format!(
                "bare-root refusal changed {state} database state"
            ))
            .into());
        }
        for sidecar in ["projectatlas.db-wal", "projectatlas.db-shm"] {
            if atlas_dir.join(sidecar).exists() {
                return Err(io::Error::other(format!(
                    "bare-root refusal created {sidecar} for {state} database state"
                ))
                .into());
            }
        }
    }
    Ok(())
}

#[test]
fn implicit_bare_root_commands_preserve_live_wal_and_authored_state() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let bare = temp.path().join(BARE_REPOSITORY_DIR_NAME);
    let output = StdCommand::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git init --bare failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    let atlas_dir = bare.join(ATLAS_DIR_NAME);
    fs::create_dir(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    drop(AtlasStore::open(&database)?);
    let connection = Connection::open(&database)?;
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if journal_mode != "wal" {
        return Err(
            io::Error::other(format!("SQLite did not enter WAL mode: {journal_mode}")).into(),
        );
    }
    connection.execute_batch(
        "
        PRAGMA wal_autocheckpoint = 0;
        CREATE TABLE authored_state(value TEXT NOT NULL);
        INSERT INTO authored_state(value) VALUES('preserve-purpose');
        CREATE TABLE authored_telemetry(value TEXT NOT NULL);
        INSERT INTO authored_telemetry(value) VALUES('preserve-telemetry');
        ",
    )?;
    let config = atlas_dir.join("config.toml");
    fs::write(&config, b"[scan]\nmax_file_bytes = 1000000\n")?;
    let backup = atlas_dir.join("projectatlas.db.backup");
    fs::write(&backup, b"preserve-backup")?;
    let wal = atlas_dir.join("projectatlas.db-wal");
    let shm = atlas_dir.join("projectatlas.db-shm");
    if !wal.is_file() || !shm.is_file() {
        return Err(io::Error::other("active WAL fixture did not create WAL and SHM files").into());
    }
    let preserved_paths = [database, wal, shm, config, backup];
    let preserved_before = preserved_paths
        .iter()
        .map(fs::read)
        .collect::<Result<Vec<_>, _>>()?;

    let commands = [
        ("settings", vec!["settings"]),
        ("root show", vec!["root"]),
        ("root verify", vec!["root", "verify"]),
        ("map", vec!["map", "--force"]),
        ("config", vec!["config", "--print"]),
        ("lint", vec!["lint"]),
        ("reset-index", vec!["reset-index", "--apply"]),
        ("mcp", vec!["mcp"]),
        ("mcp-config", vec!["mcp-config"]),
    ];
    for (name, arguments) in commands {
        let implicit = Command::cargo_bin("projectatlas")?
            .current_dir(&bare)
            .args(["--format", "json"])
            .args(arguments)
            .output()?;
        if implicit.status.success() {
            return Err(
                io::Error::other(format!("implicit bare-root {name} command succeeded")).into(),
            );
        }
        let error: Value = serde_json::from_slice(&implicit.stderr)?;
        require_json_string(&error, &["error", "kind"], "worktree_required")?;
        let preserved_after = preserved_paths
            .iter()
            .map(fs::read)
            .collect::<Result<Vec<_>, _>>()?;
        if preserved_after != preserved_before {
            return Err(io::Error::other(format!(
                "implicit bare-root {name} command changed DB, WAL, SHM, config, or backup bytes"
            ))
            .into());
        }
    }

    let purpose: String =
        connection.query_row("SELECT value FROM authored_state", [], |row| row.get(0))?;
    let telemetry: String =
        connection.query_row("SELECT value FROM authored_telemetry", [], |row| row.get(0))?;
    if purpose != "preserve-purpose" || telemetry != "preserve-telemetry" {
        return Err(io::Error::other("bare-root refusal changed authored SQLite state").into());
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
fn packaged_skill_routes_startup_and_registered_worktrees() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let skill = fs::read_to_string(
        workspace_root
            .join("plugins")
            .join("projectatlas")
            .join(PROJECTATLAS_SKILL_DIR)
            .join(PROJECTATLAS_SKILL_NAME)
            .join(SKILL_FILE_NAME),
    )?;
    for required in [
        "For task-directed work in an existing indexed repository",
        "On first use in each distinct project root",
        "execute its exact `atlas_init` next call using the returned `worktree` alias or `project_path`",
        "Every project root owns its own `.projectatlas/projectatlas.db`, config, generated host configs, and exact index",
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
        "lowest reliable reasoning and cost tier the host supports",
        "Examples when available: Codex `gpt-5.6-luna` with `low` reasoning, or Claude Code `haiku`",
        "otherwise use the host's lowest reliable equivalent as model names and availability change",
        "becomes `approved`, `source: agent`, and `agent_reviewed: true` immediately",
        "add one durable pointer to the nearest harness instruction file",
        "a runtime `version` matching the selected plugin release",
        "resolve the installer from the installed, version-matched ProjectAtlas plugin root",
        "-ProjectRoot \"<target-project-root>\"",
        "Do not assume an unrelated target repository contains `plugins/projectatlas/scripts`",
        "## Worktree MCP Workflow",
        "The control checkout may itself be linked, live under `.worktrees`, or live anywhere else on the filesystem",
        "atlas_worktree_list(include_retired: false)",
        r#"atlas_worktree_add(worktree: "<selector>", alias: "issue-430")"#,
        r#"atlas_init(worktree: "issue-430")"#,
        r#"atlas_session_brief(worktree: "issue-430", compact: true)"#,
        r#"worktrees: ["main", "issue-430"]"#,
        r#"atlas_token_report(worktree: "main")"#,
        r#"atlas_worktree_remove(worktree: "issue-430")"#,
        "leaves the checkout, Git registration, branch, files, `.projectatlas`, and SQLite database untouched",
        "`project_path` remains the compatibility route for unregistered and older workflows",
        "without adding a worktree selector UI",
    ] {
        if !skill.contains(required) {
            return Err(io::Error::other(format!(
                "packaged skill is missing task-oriented session-brief guidance {required:?}"
            ))
            .into());
        }
    }
    for path in [
        "templates/AGENTS.md",
        "plugins/projectatlas/skills/projectatlas/SKILL.md",
        "docs/agent-integration.md",
        "docs/agent-navigation.md",
        "openspec/changes/advance-rust-repository-intelligence/proposal.md",
        "openspec/changes/advance-rust-repository-intelligence/design.md",
        "openspec/changes/advance-rust-repository-intelligence/specs/graph-retrieval-and-analysis/spec.md",
        "openspec/changes/advance-rust-repository-intelligence/tasks.md",
        "openspec/changes/enhance-projectatlas-init-first-run/proposal.md",
        "openspec/changes/enhance-projectatlas-init-first-run/design.md",
        "openspec/changes/enhance-projectatlas-init-first-run/specs/projectatlas-first-run-init/spec.md",
        "openspec/changes/enhance-projectatlas-init-first-run/tasks.md",
    ] {
        let guidance = fs::read_to_string(workspace_root.join(path))?;
        for required in [
            "bounded isolated subagent",
            "lowest reliable reasoning and cost tier",
        ] {
            if !guidance.contains(required) {
                return Err(io::Error::other(format!(
                    "{path} must preserve fixed-tier-compatible purpose delegation; missing {required:?}"
                ))
                .into());
            }
        }
        for stale in [
            "Reserve low-reasoning subagents",
            "to a low-reasoning subagent",
            "Low-reasoning purpose curator",
            "low-reasoning purpose curator",
            "low-reasoning curator",
            "low-reasoning subagent",
            "low-reasoning subagents",
            "subagent with low reasoning",
            "lowest reasoning tier the host can enforce",
            "cannot enforce subagent execution or reasoning selection",
            "cannot enforce isolated subagents or reasoning selection",
        ] {
            if guidance.contains(stale) {
                return Err(io::Error::other(format!(
                    "{path} still contains stale purpose-delegation guidance {stale:?}"
                ))
                .into());
            }
        }
    }
    for path in [
        "openspec/changes/advance-rust-repository-intelligence/specs/graph-retrieval-and-analysis/spec.md",
        "openspec/changes/advance-rust-repository-intelligence/tasks.md",
        "openspec/changes/enhance-projectatlas-init-first-run/specs/projectatlas-first-run-init/spec.md",
        "openspec/changes/enhance-projectatlas-init-first-run/tasks.md",
    ] {
        let contract = fs::read_to_string(workspace_root.join(path))?;
        for required in [
            "fixed reliable subagent tier",
            "reasoning selection is optional",
            "only absence of bounded isolated subagent execution",
            "main-agent fallback",
        ] {
            if !contract.contains(required) {
                return Err(io::Error::other(format!(
                    "{path} must keep fixed-tier purpose delegation independent from selector availability; missing {required:?}"
                ))
                .into());
            }
        }
    }
    for path in ["docs/agent-integration.md", "docs/agent-navigation.md"] {
        let guidance = fs::read_to_string(workspace_root.join(path))?;
        if !guidance.contains(
            "fixed reliable subagent tier still delegates at that tier; reasoning selection is optional",
        ) {
            return Err(io::Error::other(format!(
                "{path} must keep fixed-tier hosts on the bounded purpose-delegation path"
            ))
            .into());
        }
    }
    let adoption = fs::read_to_string(workspace_root.join("docs/adoption.md"))?;
    if !adoption.contains("Add the startup snippet from `templates/AGENTS.md` to your `AGENTS.md`")
    {
        return Err(io::Error::other(
            "adoption guidance must distribute the current ProjectAtlas AGENTS template",
        )
        .into());
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
        let guidance = fs::read_to_string(workspace_root.join(path))?;
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
fn generated_mcp_config_preserves_explicit_conventional_database_authority()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    let selected = temp.path().join("selected");
    fs::create_dir_all(source.join(SRC_DIR_NAME))?;
    fs::create_dir_all(selected.join(ATLAS_DIR_NAME))?;
    fs::write(
        source.join(SRC_DIR_NAME).join("main.rs"),
        "pub fn stored_project_root_marker() {}\n",
    )?;
    let source_database = source.join(ATLAS_DIR_NAME).join("projectatlas.db");
    Command::cargo_bin("projectatlas")?
        .current_dir(&source)
        .arg("--db")
        .arg(&source_database)
        .args(["scan", "."])
        .assert()
        .success();

    let selected_database = selected.join(ATLAS_DIR_NAME).join("projectatlas.db");
    fs::copy(&source_database, &selected_database)?;
    let mcp_config = mcp_config_for_harness(&source, &selected_database, "mcp-json")?;
    let (mcp_command, mcp_args) = mcp_command_and_args(&mcp_config)?;
    let mcp_output = run_mcp_stdio(
        &mcp_command,
        &source,
        &mcp_args,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_root","arguments":{}}}"#,
        ],
    )?;
    let root: Value = toon_format::decode_default(&mcp_tool_text(&mcp_output, 2)?)?;
    let canonical_source = normalize_native_path_display(source.canonicalize()?);
    require_json_string(&root, &["root", "root"], &canonical_source)?;
    require_json_string(&root, &["root", "db_project_root"], &canonical_source)?;
    require_json_bool(&root, &["root", "verified"], true)?;
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
        repo.join(PACKAGE_JSON_FILE_NAME),
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
        .arg("--db")
        .arg(&db)
        .args(["config", "--print"])
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
    require_json_string(&readme_summary, &["parser_kind"], "structural-symbol-graph")?;
    require_json_string(&readme_summary, &["summary_status"], "ok")?;
    require_json_string(&readme_summary, &["file_purpose_status"], "suggested")?;

    let package_summary = json_summary_command(&repo, &db, PACKAGE_JSON_FILE_NAME)?;
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

#[test]
fn mcp_test_shutdown_runs_after_primary_failure_without_hiding_it() -> Result<(), Box<dyn Error>> {
    let mut shutdown_attempted = false;
    let result = complete_mcp_test_after_shutdown(
        Err::<(), Box<dyn Error>>(io::Error::other("primary MCP test failure").into()),
        || {
            shutdown_attempted = true;
            Err(io::Error::other("secondary MCP shutdown failure").into())
        },
    );
    if !shutdown_attempted {
        return Err(io::Error::other("MCP test shutdown was not attempted").into());
    }
    match result {
        Err(error) if error.to_string() == "primary MCP test failure" => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "MCP shutdown hid the primary test failure: {error}"
        ))
        .into()),
        Ok(()) => Err(io::Error::other("MCP test failure was discarded").into()),
    }
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
