//! Purpose: Validate worktree, watcher, freshness, and federation contracts.
#![allow(unused_imports)]

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
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";

const SRC_DIR_NAME: &str = "src";

const TESTS_DIR_NAME: &str = "tests";

const ALPHA_RS_FILE_NAME: &str = "alpha.rs";

const GUIDE_MD_PATH: &str = "docs/guide.md";

const CREATED_RS_FILE_NAME: &str = "created.rs";

const DUPLICATE_RS_FILE_NAME: &str = "duplicate.rs";

const HIDDEN_RS_FILE_NAME: &str = "hidden.rs";

const IGNORED_DIR_NAME: &str = "ignored";

const LIB_RS_FILE_NAME: &str = "lib.rs";

const GIT_DIR_NAME: &str = ".git";

const MAIN_CHECKOUT_DIR_NAME: &str = "main-checkout";

const LINKED_CHECKOUTS_DIR_NAME: &str = "branches";

const BARE_REPOSITORY_DIR_NAME: &str = "repository.git";

const FEATURE_ONLY_RS_FILE_NAME: &str = "feature_only.rs";

const ALTERNATE_ONLY_RS_FILE_NAME: &str = "alternate_only.rs";

const REVIEW_ONLY_RS_FILE_NAME: &str = "review_only.rs";

const ATLAS_DIR_NAME: &str = ".projectatlas";

const TS_CONFIG_FILE_NAME: &str = "tsconfig.json";

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

const MCP_CONTRACT_EXECUTABLE_ENV: &str = "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE";

const MCP_CONTRACT_METADATA_CANARY: &str = "mcp_contract_metadata_canary";

const SUBDIR_CONFIG_DIR: &str = "config";

const SESSION_TEST_FILE_NAME: &str = "session.rs";

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
#[ignore = "dedicated hosted cross-platform holistic worktree proof"]
fn holistic_agent_worktree_flow_keeps_local_atlases_isolated_across_cli_watch_and_mcp()
-> Result<(), Box<dyn Error>> {
    const CASE_RENAMED_RS_FILE_NAME: &str = "casetarget.rs";
    const CASE_SOURCE_RS_FILE_NAME: &str = "CaseTarget.rs";
    const DEEP_DIR_NAME: &str = "deep";
    const ELIGIBLE_DIR_NAME: &str = "eligible";
    const IGNORED_TREE_DIR_NAME: &str = "ignored-tree";
    const INDIRECT_SOURCE_DIR_NAME: &str = "indirect-source";
    const LINKED_WORKTREE_DIR_NAME: &str = "feature checkout 工作树";
    const REVIEW_WORKTREE_DIR_NAME: &str = "review checkout";
    const UNICODE_RS_FILE_NAME: &str = "unicode_ß.rs";
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(MAIN_CHECKOUT_DIR_NAME);
    fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    git_success(&repo, &["init"])?;
    git_success(&repo, &["config", "user.name", "ProjectAtlas Test"])?;
    git_success(
        &repo,
        &["config", "user.email", "projectatlas@example.invalid"],
    )?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::create_dir_all(
        repo.join(SRC_DIR_NAME)
            .join(DEEP_DIR_NAME)
            .join(ELIGIBLE_DIR_NAME),
    )?;
    fs::create_dir(repo.join("docs"))?;
    fs::create_dir_all(repo.join(IGNORED_TREE_DIR_NAME).join(DEEP_DIR_NAME))?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n/ignored-tree/\n")?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn main_checkout_marker() { main_checkout_leaf(); }\npub fn main_checkout_leaf() {}\n",
    )?;
    fs::write(
        repo.join(GUIDE_MD_PATH),
        "# Main Guide\n\nSee [the main source](../src/lib.rs).\n",
    )?;
    fs::write(
        repo.join(SRC_DIR_NAME)
            .join(DEEP_DIR_NAME)
            .join(ELIGIBLE_DIR_NAME)
            .join("nested.rs"),
        "pub fn deeply_nested_marker() {}\n",
    )?;
    fs::write(
        repo.join(IGNORED_TREE_DIR_NAME)
            .join(DEEP_DIR_NAME)
            .join("ignored.rs"),
        "pub fn ignored_nested_marker() {}\n",
    )?;
    git_success(&repo, &["add", "."])?;
    git_success(&repo, &["commit", "-m", "main fixture"])?;

    let submodule_source = temp.path().join("submodule-source");
    fs::create_dir(&submodule_source)?;
    git_success(&submodule_source, &["init"])?;
    git_success(
        &submodule_source,
        &["config", "user.name", "ProjectAtlas Test"],
    )?;
    git_success(
        &submodule_source,
        &["config", "user.email", "projectatlas@example.invalid"],
    )?;
    fs::create_dir(submodule_source.join(SRC_DIR_NAME))?;
    fs::write(
        submodule_source.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn nested_submodule_marker() {}\n",
    )?;
    git_success(&submodule_source, &["add", "."])?;
    git_success(&submodule_source, &["commit", "-m", "submodule fixture"])?;
    let submodule_output = git_command_for_root(&repo)
        .args(["-c", "protocol.file.allow=always", "submodule", "add"])
        .arg(&submodule_source)
        .arg("vendor/submodule")
        .output()?;
    if !submodule_output.status.success() {
        return Err(io::Error::other(format!(
            "git submodule add failed: {}{}",
            String::from_utf8_lossy(&submodule_output.stdout),
            String::from_utf8_lossy(&submodule_output.stderr)
        ))
        .into());
    }
    git_success(&repo, &["commit", "-m", "add submodule fixture"])?;

    let linked_checkout_path = temp
        .path()
        .join("unrelated feature root")
        .join(LINKED_WORKTREE_DIR_NAME);
    fs::create_dir_all(
        linked_checkout_path
            .parent()
            .ok_or_else(|| io::Error::other("linked worktree has no parent"))?,
    )?;
    let worktree_output = git_command_for_root(&repo)
        .args(["worktree", "add", "-b", "feature"])
        .arg(&linked_checkout_path)
        .output()?;
    if !worktree_output.status.success() {
        return Err(io::Error::other(format!(
            "git worktree add failed: {}{}",
            String::from_utf8_lossy(&worktree_output.stdout),
            String::from_utf8_lossy(&worktree_output.stderr)
        ))
        .into());
    }
    let linked = linked_checkout_path.canonicalize()?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn linked_feature_marker() { linked_feature_leaf(); }\npub fn linked_feature_second() { linked_feature_leaf(); }\npub fn linked_feature_leaf() {}\n",
    )?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(FEATURE_ONLY_RS_FILE_NAME),
        "pub fn feature_only_marker() {}\n",
    )?;
    git_success(&linked, &["add", "."])?;
    git_success(&linked, &["commit", "-m", "feature fixture"])?;
    let review = temp
        .path()
        .join("another independent root")
        .join(REVIEW_WORKTREE_DIR_NAME);
    fs::create_dir_all(
        review
            .parent()
            .ok_or_else(|| io::Error::other("review worktree has no parent"))?,
    )?;
    let review_worktree_output = git_command_for_root(&repo)
        .args(["worktree", "add", "-b", "review"])
        .arg(&review)
        .output()?;
    if !review_worktree_output.status.success() {
        return Err(io::Error::other(format!(
            "git review worktree add failed: {}{}",
            String::from_utf8_lossy(&review_worktree_output.stdout),
            String::from_utf8_lossy(&review_worktree_output.stderr)
        ))
        .into());
    }
    let review = review.canonicalize()?;
    fs::write(
        review.join(SRC_DIR_NAME).join(REVIEW_ONLY_RS_FILE_NAME),
        "pub fn review_only_marker() {}\n",
    )?;
    fs::write(
        review.join(GUIDE_MD_PATH),
        "# Review Guide\n\nSee [the review source](../src/review_only.rs).\n",
    )?;
    git_success(&review, &["add", "."])?;
    git_success(&review, &["commit", "-m", "review fixture"])?;

    let manager = repo.join(GIT_DIR_NAME);
    let status = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "root", "status"])
        .arg(&manager)
        .output()?;
    if !status.status.success() {
        return Err(io::Error::other(format!(
            "structural worktree status failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ))
        .into());
    }
    let status: Value = serde_json::from_slice(&status.stdout)?;
    require_json_string(&status, &["source_selection"], "explicit_worktree_required")?;
    require_json_bool(&status, &["worktree_required"], true)?;
    require_json_array_len(&status, &["worktrees"], 3)?;
    if status["worktrees"]
        .as_array()
        .is_none_or(|rows| rows.iter().any(|row| row["state"] != "active"))
    {
        return Err(io::Error::other(format!(
            "structural status did not retain both active worktrees: {status}"
        ))
        .into());
    }

    let missing_read = Command::cargo_bin("projectatlas")?
        .current_dir(&linked)
        .args(["--format", "json", "overview"])
        .output()?;
    if missing_read.status.success() {
        return Err(io::Error::other("uninitialized worktree overview succeeded").into());
    }
    let missing_error: Value = serde_json::from_slice(&missing_read.stderr)?;
    require_json_string(&missing_error, &["error", "kind"], "init_required")?;
    require_json_string(&missing_error, &["error", "next", "command"], "init")?;
    let linked_root = linked.canonicalize()?;
    let linked_root_text = projectatlas_core::normalize_native_path_display(&linked_root);
    require_json_string(
        &missing_error,
        &["error", "next", "project_path"],
        &linked_root_text,
    )?;
    if linked.join(ATLAS_DIR_NAME).exists() {
        return Err(io::Error::other("read-only init_required probe created project state").into());
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "scan", "."])
        .assert()
        .success();
    let main_db = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let main_store = AtlasStore::open_for_project(&main_db, &repo)?;
    if main_store
        .load_nodes()?
        .iter()
        .any(|node| node.node.path.starts_with("branches/feature"))
    {
        return Err(io::Error::other("main atlas admitted the registered linked worktree").into());
    }
    if main_store.load_node_by_path("src/lib.rs")?.is_none() {
        return Err(io::Error::other("main atlas omitted main source").into());
    }
    if main_store
        .load_node_by_path("src/deep/eligible/nested.rs")?
        .is_none()
        || main_store
            .load_node_by_path("vendor/submodule/src/lib.rs")?
            .is_none()
    {
        return Err(io::Error::other(
            "main atlas omitted deep eligible source or nested submodule source",
        )
        .into());
    }
    if main_store
        .load_nodes()?
        .iter()
        .any(|node| node.node.path.starts_with("ignored-tree"))
    {
        return Err(io::Error::other("main atlas descended into an ignored subtree").into());
    }
    let main_identity = main_store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("main project identity missing"))?;
    if main_store.repository_graph_generation()?.is_none()
        || main_store
            .load_symbols(Some("src/lib.rs"), None, 20)?
            .iter()
            .all(|symbol| symbol.name != "main_checkout_marker")
    {
        return Err(io::Error::other("main scan omitted symbol or graph publication").into());
    }
    main_store.set_purpose(
        "src/lib.rs",
        "Main checkout Rust library.",
        PurposeSource::Agent,
    )?;
    drop(main_store);

    let linked_atlas_dir = linked.join(ATLAS_DIR_NAME);
    fs::create_dir(&linked_atlas_dir)?;
    let linked_db = linked_atlas_dir.join("projectatlas.db");
    let linked_store = AtlasStore::open_for_project(&linked_db, &linked)?;
    drop(linked_store);
    let connection = Connection::open(&linked_db)?;
    connection.execute_batch("DELETE FROM project_root_identity;")?;
    connection.execute_batch(
        "INSERT INTO nodes(id, path, kind, extension, language, exists_now)
             VALUES(1, 'src/lib.rs', 'file', 'rs', 'rust', 1);
         INSERT INTO purposes(node_id, purpose, source, status, updated_by)
             VALUES(1, 'Preserved v0.4.4 worktree purpose.', 'agent', 'approved', 'agent');",
    )?;
    drop(connection);

    let control_mcp_config = mcp_config_for_harness(&repo, &main_db, "mcp-json")?;
    let (control_mcp_command, _control_mcp_args) = mcp_command_and_args(&control_mcp_config)?;
    let (mut worktree_session, _initialized) = McpContractSession::spawn_initialized(
        &control_mcp_command,
        &repo,
        &main_db,
        &[("PROJECTATLAS_NO_TELEMETRY", None)],
    )?;
    let inventory = worktree_session.call_tool(
        "atlas_worktree_list",
        &serde_json::json!({"include_retired": false}),
    )?;
    if !inventory.contains("control_alias: main")
        || !inventory.contains(&projectatlas_core::normalize_native_path_display(
            linked.canonicalize()?,
        ))
        || !inventory.contains(&projectatlas_core::normalize_native_path_display(
            review.canonicalize()?,
        ))
        || inventory.matches("wt-").count() < 2
    {
        return Err(io::Error::other(format!(
            "control worktree inventory omitted stable selectors or arbitrary roots: {inventory}"
        ))
        .into());
    }
    let selector_for = |root: &Path| -> Result<String, Box<dyn Error>> {
        let root = projectatlas_core::normalize_native_path_display(root.canonicalize()?);
        inventory
            .lines()
            .find(|line| line.contains(&root))
            .and_then(|line| line.split(',').next())
            .map(|selector| selector.trim().trim_matches('"').to_string())
            .filter(|selector| selector.starts_with("wt-"))
            .ok_or_else(|| {
                io::Error::other(format!(
                    "worktree inventory omitted the stable selector for {root}"
                ))
                .into()
            })
    };
    let feature_selector = selector_for(&linked)?;
    let review_selector = selector_for(&review)?;
    let ambiguous_registration = worktree_session.call_tool(
        "atlas_worktree_add",
        &serde_json::json!({"worktree": "checkout", "alias": "ambiguous"}),
    )?;
    if !(ambiguous_registration.contains("status: ambiguous")
        || ambiguous_registration.contains("status: not_found"))
        || ambiguous_registration.matches("wt-").count() < 2
    {
        return Err(io::Error::other(format!(
            "ambiguous short selector guessed instead of returning bounded candidates: {ambiguous_registration}"
        ))
        .into());
    }
    for (selector, alias) in [(&feature_selector, "feature"), (&review_selector, "review")] {
        let registration = worktree_session.call_tool(
            "atlas_worktree_add",
            &serde_json::json!({"worktree": selector, "alias": alias}),
        )?;
        if !registration.contains("status: registered")
            || !(registration.contains(&format!("alias: {alias}"))
                || registration.contains(&format!("alias: \"{alias}\"")))
            || !registration.contains("git_unchanged: true")
            || !registration.contains("files_unchanged: true")
        {
            return Err(io::Error::other(format!(
                "{alias} registration changed lifecycle state or lost its short alias: {registration}"
            ))
            .into());
        }
    }
    let feature_init =
        worktree_session.call_tool("atlas_init", &serde_json::json!({"worktree": "feature"}))?;
    if !feature_init.contains("status: existing") {
        return Err(io::Error::other(format!(
            "feature atlas was not repaired and preserved through targeted init: {feature_init}"
        ))
        .into());
    }
    let review_init =
        worktree_session.call_tool("atlas_init", &serde_json::json!({"worktree": "review"}))?;
    if !review_init.contains("status: hydrated")
        || !review_init.contains("source_project_instance_id:")
        || !review_init.contains("target_project_instance_id:")
        || !review_init.contains("reconciled_generation:")
    {
        return Err(io::Error::other(format!(
            "absent review atlas did not safely hydrate and reconcile from control: {review_init}"
        ))
        .into());
    }
    let routing_conflict = worktree_session.call_tool(
        "atlas_overview",
        &serde_json::json!({
            "worktree": "feature",
            "project_path": projectatlas_core::normalize_native_path_display(repo.canonicalize()?)
        }),
    )?;
    if !routing_conflict.contains("mutually exclusive") {
        return Err(io::Error::other(format!(
            "worktree and legacy project_path were not rejected before routing: {routing_conflict}"
        ))
        .into());
    }

    let repaired: String = Connection::open(&linked_db)?.query_row(
        "SELECT value FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )?;
    let current_schema: String = Connection::open(&main_db)?.query_row(
        "SELECT value FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )?;
    if repaired != current_schema {
        return Err(io::Error::other(format!(
            "linked first-write scan did not repair its current schema: expected {current_schema}, found {repaired}"
        ))
        .into());
    }
    let review_db = review.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let review_store = AtlasStore::open_for_project(&review_db, &review)?;
    let review_identity = review_store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("review project identity missing"))?;
    if review_identity == main_identity
        || review_store
            .load_node_by_path("src/review_only.rs")?
            .is_none()
        || review_store
            .load_node_by_path("src/feature_only.rs")?
            .is_some()
        || review_store
            .load_node_by_path("src/lib.rs")?
            .is_none_or(|node| {
                node.purpose.purpose.as_deref() != Some("Main checkout Rust library.")
            })
    {
        return Err(io::Error::other(
            "hydrated review atlas lost reusable purpose state or crossed a sibling source boundary",
        )
        .into());
    }
    drop(review_store);
    let hydration_source = AtlasStore::open_for_project(&main_db, &repo)?;
    let hydration_control =
        projectatlas_core::IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None);
    if !matches!(
        hydration_source.prepare_worktree_hydration(&review, &review_db, &hydration_control),
        Err(projectatlas_db::DbError::WorktreeHydrationDestinationExists { .. })
    ) {
        return Err(io::Error::other(
            "hydration fault path did not preserve an existing target database",
        )
        .into());
    }
    let canceled_database = review_db.with_file_name("canceled-hydration.db");
    let canceled_control =
        projectatlas_core::IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None);
    canceled_control.cancel();
    if !matches!(
        hydration_source
            .prepare_worktree_hydration(&review, &canceled_database, &canceled_control,),
        Err(projectatlas_db::DbError::IndexWork(_))
    ) || canceled_database.exists()
    {
        return Err(io::Error::other(
            "canceled hydration published or lost its typed cancellation failure",
        )
        .into());
    }
    drop(hydration_source);
    let main_before_linked_operations = mcp_database_snapshot(&main_db)?;
    let linked_after_first_write = AtlasStore::open_for_project(&linked_db, &linked)?;
    let repaired_library = linked_after_first_write
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("linked repair omitted its authored source"))?;
    if repaired_library.purpose.purpose.as_deref() != Some("Preserved v0.4.4 worktree purpose.")
        || linked_after_first_write
            .load_node_by_path("src/feature_only.rs")?
            .is_none()
        || linked_after_first_write
            .load_nodes()?
            .iter()
            .any(|node| node.node.path.contains("main-checkout"))
    {
        return Err(io::Error::other(
            "linked repair lost authored state or crossed its source boundary",
        )
        .into());
    }
    drop(linked_after_first_write);

    Command::cargo_bin("projectatlas")?
        .current_dir(&linked)
        .args(["--format", "json", "init"])
        .assert()
        .success();
    for file_name in [
        "config.toml",
        "projectatlas.mcp.json",
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
    ] {
        if !linked.join(ATLAS_DIR_NAME).join(file_name).is_file() {
            return Err(io::Error::other(format!(
                "linked-worktree init omitted local {file_name}"
            ))
            .into());
        }
    }
    let linked_store = AtlasStore::open_for_project(&linked_db, &linked)?;
    if linked_store
        .load_node_by_path("src/feature_only.rs")?
        .is_none()
        || linked_store
            .load_nodes()?
            .iter()
            .any(|node| node.node.path.contains("main-checkout"))
    {
        return Err(io::Error::other("linked atlas crossed its selected source root").into());
    }
    let linked_identity = linked_store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("linked project identity missing"))?;
    if main_identity == linked_identity {
        return Err(io::Error::other("linked worktrees shared one project identity").into());
    }
    if linked_store.repository_graph_generation()?.is_none()
        || linked_store
            .load_symbols(Some("src/lib.rs"), None, 20)?
            .iter()
            .all(|symbol| symbol.name != "linked_feature_marker")
    {
        return Err(io::Error::other(
            "linked init omitted worktree-local symbol or graph publication",
        )
        .into());
    }
    linked_store.set_purpose(
        "src/lib.rs",
        "Feature worktree Rust library.",
        PurposeSource::Agent,
    )?;
    drop(linked_store);
    for (selected, expected, sibling, label) in [
        (&repo, "2 nodes • 1 links", "3 nodes • 2 links", "main"),
        (&linked, "3 nodes • 2 links", "2 nodes • 1 links", "linked"),
    ] {
        let output = Command::cargo_bin("projectatlas")?
            .current_dir(selected)
            .env("COLUMNS", "200")
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .args(["token", "--view", "tui"])
            .output()?;
        let dashboard = String::from_utf8(output.stdout)?;
        if !output.status.success() || !dashboard.contains(expected) || dashboard.contains(sibling)
        {
            return Err(io::Error::other(format!(
                "{label} public token TUI crossed its selected worktree: status={:?} expected={expected:?} sibling={sibling:?} output={dashboard}",
                output.status.code(),
            ))
            .into());
        }
    }
    let main_purpose = AtlasStore::open_for_project(&main_db, &repo)?
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("main source missing after purpose write"))?;
    let linked_purpose = AtlasStore::open_for_project(&linked_db, &linked)?
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("linked source missing after purpose write"))?;
    if main_purpose.purpose.purpose.as_deref() != Some("Main checkout Rust library.")
        || linked_purpose.purpose.purpose.as_deref() != Some("Feature worktree Rust library.")
    {
        return Err(io::Error::other("worktree-local purposes crossed databases").into());
    }

    fs::write(
        linked.join(SRC_DIR_NAME).join(CASE_SOURCE_RS_FILE_NAME),
        "pub fn case_target_before_rename() {}\n",
    )?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(UNICODE_RS_FILE_NAME),
        "pub fn unicode_path_marker() {}\n",
    )?;
    let external_source = temp.path().join("outside indirect source");
    fs::create_dir(&external_source)?;
    fs::write(
        external_source.join("outside.rs"),
        "pub fn outside_indirect_marker() {}\n",
    )?;
    let indirect_source = linked.join(INDIRECT_SOURCE_DIR_NAME);
    create_test_directory_indirection(&external_source, &indirect_source)?;
    run_watch_once(&linked, &linked_db)?;
    let platform_paths = AtlasStore::open_for_project(&linked_db, &linked)?;
    if platform_paths
        .load_node_by_path(&format!("{SRC_DIR_NAME}/{CASE_SOURCE_RS_FILE_NAME}"))?
        .is_none()
        || platform_paths
            .load_node_by_path(&format!("{SRC_DIR_NAME}/{UNICODE_RS_FILE_NAME}"))?
            .is_none()
        || platform_paths
            .load_nodes()?
            .iter()
            .any(|node| node.node.path.starts_with(INDIRECT_SOURCE_DIR_NAME))
    {
        return Err(io::Error::other(
            "linked refresh lost Unicode/current paths or followed a symlink/junction",
        )
        .into());
    }
    drop(platform_paths);
    remove_test_directory_indirection(&indirect_source)?;
    git_success(&linked, &["add", "-A"])?;
    git_success(&linked, &["commit", "-m", "platform path fixture"])?;

    let mcp_cwd = json_string_at(&control_mcp_config, &["mcpServers", "projectatlas", "cwd"])?;
    if Path::new(mcp_cwd).canonicalize()? != repo.canonicalize()? {
        return Err(io::Error::other(format!(
            "control MCP config selected the wrong working directory: {mcp_cwd}"
        ))
        .into());
    }
    let linked_summary = json_summary_command(&linked, &linked_db, "src/lib.rs")?;
    require_json_contains(
        &linked_summary,
        &["content_summary"],
        "linked_feature_marker",
    )?;

    git_success(&linked, &["switch", "-c", "alternate"])?;
    let case_source = linked.join(SRC_DIR_NAME).join(CASE_SOURCE_RS_FILE_NAME);
    let case_step = linked.join(SRC_DIR_NAME).join("case-rename-step.rs");
    let case_renamed = linked.join(SRC_DIR_NAME).join(CASE_RENAMED_RS_FILE_NAME);
    fs::rename(&case_source, &case_step)?;
    fs::rename(&case_step, &case_renamed)?;
    fs::write(&case_renamed, "pub fn case_target_after_rename() {}\n")?;
    fs::remove_file(linked.join(SRC_DIR_NAME).join(FEATURE_ONLY_RS_FILE_NAME))?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(ALTERNATE_ONLY_RS_FILE_NAME),
        "pub fn alternate_only_marker() {}\n",
    )?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn linked_alternate_marker() {}\n",
    )?;
    git_success(&linked, &["add", "-A"])?;
    git_success(&linked, &["commit", "-m", "alternate fixture"])?;
    run_watch_once(&linked, &linked_db)?;
    let switched = AtlasStore::open_for_project(&linked_db, &linked)?;
    if switched.load_node_by_path("src/feature_only.rs")?.is_some()
        || switched
            .load_node_by_path("src/alternate_only.rs")?
            .is_none()
        || switched
            .load_node_by_path(&format!("{SRC_DIR_NAME}/{CASE_SOURCE_RS_FILE_NAME}"))?
            .is_some()
        || switched
            .load_node_by_path(&format!("{SRC_DIR_NAME}/{CASE_RENAMED_RS_FILE_NAME}"))?
            .is_none()
        || switched
            .load_symbols(
                Some(&format!("{SRC_DIR_NAME}/{CASE_RENAMED_RS_FILE_NAME}")),
                None,
                20,
            )?
            .iter()
            .all(|symbol| symbol.name != "case_target_after_rename")
    {
        return Err(io::Error::other(
            "branch-switch refresh published mixed branch or case-rename state",
        )
        .into());
    }
    drop(switched);

    fs::remove_file(linked.join(SRC_DIR_NAME).join(ALTERNATE_ONLY_RS_FILE_NAME))?;
    fs::remove_file(linked.join(SRC_DIR_NAME).join(UNICODE_RS_FILE_NAME))?;
    fs::write(
        linked.join(SRC_DIR_NAME).join("dirty_only.rs"),
        "pub fn dirty_only_marker() {}\n",
    )?;
    fs::write(
        linked.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn linked_dirty_marker() {}\n",
    )?;
    fs::write(
        linked.join(GUIDE_MD_PATH),
        "# Linked Guide\n\nSee [the dirty source](../src/dirty_only.rs).\n",
    )?;
    fs::write(
        review.join(SRC_DIR_NAME).join(REVIEW_ONLY_RS_FILE_NAME),
        "pub fn review_dirty_marker() {}\n",
    )?;
    fs::write(
        review.join(GUIDE_MD_PATH),
        "# Review Guide\n\nSee [the review source](../src/review_only.rs).\n",
    )?;
    run_watch_once(&linked, &linked_db)?;
    run_watch_once(&review, &review_db)?;
    let dirty = AtlasStore::open_for_project(&linked_db, &linked)?;
    let dirty_library = dirty
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("dirty refresh omitted linked library"))?;
    if dirty.project_instance_id()? != Some(linked_identity)
        || dirty.repository_graph_generation()?.is_none()
        || dirty_library.purpose.purpose.as_deref() != Some("Feature worktree Rust library.")
        || dirty.load_node_by_path("src/alternate_only.rs")?.is_some()
        || dirty
            .load_node_by_path(&format!("{SRC_DIR_NAME}/{UNICODE_RS_FILE_NAME}"))?
            .is_some()
        || dirty.load_node_by_path("src/dirty_only.rs")?.is_none()
        || dirty
            .load_symbols(Some("src/lib.rs"), None, 20)?
            .iter()
            .all(|symbol| symbol.name != "linked_dirty_marker")
    {
        return Err(io::Error::other("dirty refresh did not publish exact worktree state").into());
    }
    drop(dirty);
    let dirty_summary = json_summary_command(&linked, &linked_db, "src/lib.rs")?;
    require_json_contains(&dirty_summary, &["content_summary"], "linked_dirty_marker")?;
    let review_summary = json_summary_command(&review, &review_db, "src/review_only.rs")?;
    require_json_contains(&review_summary, &["content_summary"], "review_dirty_marker")?;

    let linked_local_calls_before = AtlasStore::open_for_project(&linked_db, &linked)?
        .token_overview(None)?
        .calls;
    let review_local_calls_before = AtlasStore::open_for_project(&review_db, &review)?
        .token_overview(None)?
        .calls;
    let linked_text = worktree_session.call_tool(
        "atlas_search",
        &serde_json::json!({"worktree": "feature", "pattern": "linked_dirty_marker"}),
    )?;
    let legacy_linked_text = worktree_session.call_tool(
        "atlas_search",
        &serde_json::json!({"project_path": linked_root_text, "pattern": "linked_dirty_marker"}),
    )?;
    for (route, text) in [("alias", linked_text), ("legacy path", legacy_linked_text)] {
        if !text.contains("linked_dirty_marker") || text.contains("main_checkout_marker") {
            return Err(io::Error::other(format!(
                "interleaved linked-worktree MCP {route} read crossed databases: {text}"
            ))
            .into());
        }
    }
    let main_text = worktree_session.call_tool(
        "atlas_search",
        &serde_json::json!({"worktree": "main", "pattern": "main_checkout_marker"}),
    )?;
    if !main_text.contains("main_checkout_marker") || main_text.contains("linked_dirty_marker") {
        return Err(io::Error::other(format!(
            "interleaved main-worktree MCP read crossed databases: {main_text}"
        ))
        .into());
    }
    let review_text = worktree_session.call_tool(
        "atlas_search",
        &serde_json::json!({"worktree": "review", "pattern": "review_dirty_marker"}),
    )?;
    if !review_text.contains("review_dirty_marker")
        || review_text.contains("linked_dirty_marker")
        || review_text.contains("main_checkout_marker")
    {
        return Err(io::Error::other(format!(
            "interleaved review-worktree MCP read crossed databases: {review_text}"
        ))
        .into());
    }
    let background_scan: Value = toon_format::decode_default(&worktree_session.call_tool(
        "atlas_scan",
        &serde_json::json!({"worktree":"review","background":true,"max_workers":1}),
    )?)?;
    let background_task = json_string_at(&background_scan, &["task_start", "task_id"])?;
    require_json_string(&background_scan, &["task_start", "operation"], "scan")?;
    let main_during_background = worktree_session.call_tool(
        "atlas_search",
        &serde_json::json!({"worktree": "main", "pattern": "main_checkout_marker"}),
    )?;
    if !main_during_background.contains("main_checkout_marker")
        || main_during_background.contains("review_dirty_marker")
    {
        return Err(io::Error::other(
            "an interleaved main call inherited the registered background target",
        )
        .into());
    }
    let background_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status: Value = toon_format::decode_default(&worktree_session.call_tool(
            "atlas_task_status",
            &serde_json::json!({"task_id": background_task}),
        )?)?;
        match json_string_at(&status, &["task_status", "task", "state"])? {
            "complete" => break,
            "pending" | "running" if Instant::now() < background_deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            state => {
                return Err(io::Error::other(format!(
                    "registered background scan did not complete against its captured target: state={state} status={status}"
                ))
                .into());
            }
        }
    }
    let linked_documents = worktree_session.call_tool(
        "atlas_symbol_relations",
        &serde_json::json!({"worktree":"feature","view":"detailed","file":"docs/guide.md","direction":"outbound","relation":"documents","content_selection":"documentation","limit":10}),
    )?;
    if !linked_documents.contains("documents")
        || !linked_documents.contains("src/dirty_only.rs")
        || linked_documents.contains("Main Guide")
    {
        return Err(io::Error::other(format!(
            "linked classified navigation crossed worktree state: {linked_documents}"
        ))
        .into());
    }
    let main_documented_by = worktree_session.call_tool(
        "atlas_symbol_relations",
        &serde_json::json!({"worktree":"main","view":"detailed","file":"src/lib.rs","direction":"inbound","relation":"documents","content_selection":"source","limit":10}),
    )?;
    if !main_documented_by.contains("documented_by")
        || !main_documented_by.contains("Main checkout Rust library")
        || main_documented_by.contains("Linked Guide")
    {
        return Err(io::Error::other(format!(
            "main classified navigation crossed worktree state: {main_documented_by}"
        ))
        .into());
    }
    let review_documents = worktree_session.call_tool(
        "atlas_symbol_relations",
        &serde_json::json!({"worktree":"review","view":"detailed","file":"docs/guide.md","direction":"outbound","relation":"documents","content_selection":"documentation","limit":10}),
    )?;
    if !review_documents.contains("documents")
        || !review_documents.contains("src/review_only.rs")
        || review_documents.contains("Linked Guide")
    {
        return Err(io::Error::other(format!(
            "review classified navigation crossed worktree state: {review_documents}"
        ))
        .into());
    }
    let federated = worktree_session.call_tool(
        "atlas_symbol_relations",
        &serde_json::json!({"view":"detailed","file":"src/lib.rs","worktrees":["main","feature","review"],"limit":10}),
    )?;
    if !federated.contains("primary_worktree: main")
        || !federated.contains("participants[3]{order,worktree")
        || !federated.contains("0,main,")
        || !federated.contains("1,feature,")
        || !federated.contains("2,review,")
    {
        return Err(io::Error::other(format!(
            "three-worktree federation omitted ordered labels or control authority: {federated}"
        ))
        .into());
    }
    let control_text = worktree_session.call_tool(
        "atlas_root",
        &serde_json::json!({"control_root": projectatlas_core::normalize_native_path_display(&manager)}),
    )?;
    if !control_text.contains("explicit_worktree_required")
        || !control_text.contains(&projectatlas_core::normalize_native_path_display(
            repo.canonicalize()?,
        ))
        || !control_text.contains(&projectatlas_core::normalize_native_path_display(
            linked.canonicalize()?,
        ))
    {
        return Err(io::Error::other(format!(
            "one MCP process lost its bounded structural worktree status: {control_text}"
        ))
        .into());
    }
    let main_tokens = worktree_session.call_tool(
        "atlas_token_report",
        &serde_json::json!({"worktree":"main","include_chart":false}),
    )?;
    let feature_tokens = worktree_session.call_tool(
        "atlas_token_report",
        &serde_json::json!({"worktree":"feature","include_chart":false}),
    )?;
    let review_tokens = worktree_session.call_tool(
        "atlas_token_report",
        &serde_json::json!({"worktree":"review","include_chart":false}),
    )?;
    if !main_tokens.contains("worktree: main")
        || !main_tokens.contains("calls:")
        || !feature_tokens.contains("worktree: feature")
        || !review_tokens.contains("worktree: review")
    {
        return Err(io::Error::other(format!(
            "aggregate or exact token scopes were not labelled: main={main_tokens} feature={feature_tokens} review={review_tokens}"
        ))
        .into());
    }
    let linked_local_calls_after = AtlasStore::open_for_project(&linked_db, &linked)?
        .token_overview(None)?
        .calls;
    let review_local_calls_after = AtlasStore::open_for_project(&review_db, &review)?
        .token_overview(None)?
        .calls;
    if linked_local_calls_after
        != linked_local_calls_before
            .checked_add(1)
            .ok_or_else(|| io::Error::other("linked local call counter overflowed"))?
        || review_local_calls_after != review_local_calls_before
    {
        return Err(io::Error::other(
            "alias-routed MCP telemetry bled locally or legacy project_path telemetry was lost",
        )
        .into());
    }
    let control_after_aggregate = AtlasStore::open_for_project(&main_db, &repo)?;
    let aggregate_calls_before_remove = control_after_aggregate.repository_token_overview()?.calls;
    if aggregate_calls_before_remove <= linked_local_calls_after + review_local_calls_after {
        return Err(io::Error::other(
            "control token aggregate omitted native or alias-routed MCP usage",
        )
        .into());
    }
    drop(control_after_aggregate);
    let aggregate_tui = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("COLUMNS", "200")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .args(["token", "--view", "tui"])
        .output()?;
    let aggregate_dashboard = String::from_utf8(aggregate_tui.stdout)?;
    let aggregate_dashboard_text = aggregate_dashboard
        .split('\u{1b}')
        .map(|segment| segment.split_once('m').map_or(segment, |(_, text)| text))
        .collect::<String>();
    let aggregate_calls_after_tui = AtlasStore::open_for_project(&main_db, &repo)?
        .repository_token_overview()?
        .calls;
    if !aggregate_tui.status.success()
        || !aggregate_dashboard_text.contains(&format!("Lookups: {aggregate_calls_after_tui}"))
        || aggregate_calls_after_tui < aggregate_calls_before_remove
    {
        return Err(io::Error::other(format!(
            "unchanged control token TUI omitted repository-wide worktree totals: {aggregate_dashboard}"
        ))
        .into());
    }
    let aggregate_calls_before_remove = aggregate_calls_after_tui;

    let git_before_remove = git_command_for_root(&repo)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    if !git_before_remove.status.success() {
        return Err(io::Error::other("git worktree list failed before unregister").into());
    }
    let linked_source_before_remove = fs::read(linked.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME))?;
    let review_source_before_remove =
        fs::read(review.join(SRC_DIR_NAME).join(REVIEW_ONLY_RS_FILE_NAME))?;
    for alias in ["feature", "review"] {
        let removed = worktree_session.call_tool(
            "atlas_worktree_remove",
            &serde_json::json!({"worktree": alias}),
        )?;
        if !removed.contains("status: retired")
            || !removed.contains("git_unchanged: true")
            || !removed.contains("files_unchanged: true")
        {
            return Err(io::Error::other(format!(
                "{alias} unregister changed Git/files or lost retirement state: {removed}"
            ))
            .into());
        }
    }
    let retired_inventory = worktree_session.call_tool(
        "atlas_worktree_list",
        &serde_json::json!({"include_retired": true}),
    )?;
    if !retired_inventory.contains("feature")
        || !retired_inventory.contains("review")
        || !retired_inventory.contains("retired[2]{")
    {
        return Err(io::Error::other(format!(
            "retired registrations were not retained and labelled: {retired_inventory}"
        ))
        .into());
    }
    let retired_read = worktree_session.call_tool(
        "atlas_overview",
        &serde_json::json!({"worktree": "feature"}),
    )?;
    if !retired_read.contains("active worktree registration")
        || !retired_read.contains("was not found")
    {
        return Err(
            io::Error::other(format!("retired alias remained routable: {retired_read}")).into(),
        );
    }
    let retained_tokens = worktree_session.call_tool(
        "atlas_token_report",
        &serde_json::json!({"worktree":"main","include_chart":false}),
    )?;
    if !retained_tokens.contains("worktree: main") {
        return Err(io::Error::other(format!(
            "control token report failed after unregister: {retained_tokens}"
        ))
        .into());
    }
    worktree_session.shutdown()?;

    let git_after_remove = git_command_for_root(&repo)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    if !git_after_remove.status.success()
        || git_after_remove.stdout != git_before_remove.stdout
        || fs::read(linked.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME))?
            != linked_source_before_remove
        || fs::read(review.join(SRC_DIR_NAME).join(REVIEW_ONLY_RS_FILE_NAME))?
            != review_source_before_remove
        || !linked_db.is_file()
        || !review_db.is_file()
    {
        return Err(io::Error::other(
            "ProjectAtlas unregister changed Git, source, or target atlas lifecycle state",
        )
        .into());
    }
    let control_after_remove = AtlasStore::open_for_project(&main_db, &repo)?;
    let registrations_after_remove = control_after_remove.worktree_registrations(true)?;
    let retired_feature = registrations_after_remove
        .iter()
        .find(|registration| {
            registration.alias.as_str() == "feature"
                && matches!(
                    registration.state,
                    projectatlas_db::WorktreeRegistrationState::Retired
                )
        })
        .ok_or_else(|| io::Error::other("retired feature registration was not retained"))?;
    let retired_feature_registration_id = retired_feature.registration_id;
    let retired_feature_administrative_identity =
        retired_feature.git_administrative_identity.clone();
    if control_after_remove.repository_token_overview()?.calls < aggregate_calls_before_remove
        || registrations_after_remove
            .iter()
            .filter(|registration| {
                matches!(
                    registration.state,
                    projectatlas_db::WorktreeRegistrationState::Retired
                )
            })
            .count()
            < 2
    {
        return Err(io::Error::other(
            "unregister discarded retained aggregate telemetry or registration identity",
        )
        .into());
    }
    drop(control_after_remove);
    let main_after_linked_operations = mcp_database_snapshot(&main_db)?;
    if main_after_linked_operations.authored_purposes
        != main_before_linked_operations.authored_purposes
        || main_after_linked_operations.project_instance_id
            != main_before_linked_operations.project_instance_id
        || main_after_linked_operations.generation != main_before_linked_operations.generation
        || main_after_linked_operations.purpose_revision
            != main_before_linked_operations.purpose_revision
        || main_after_linked_operations.publication_state
            != main_before_linked_operations.publication_state
    {
        return Err(io::Error::other(
            "linked init, scan, branch, dirty, watch, or MCP operations changed main authoritative state",
        )
        .into());
    }

    for (label, worktree) in [("feature", &linked), ("review", &review)] {
        let removed = git_command_for_root(&repo)
            .args(["worktree", "remove", "--force"])
            .arg(worktree)
            .output()?;
        if !removed.status.success() || worktree.exists() {
            return Err(io::Error::other(format!(
                "Git did not remove the retired {label} worktree: {}{}",
                String::from_utf8_lossy(&removed.stdout),
                String::from_utf8_lossy(&removed.stderr)
            ))
            .into());
        }
    }
    git_success(&repo, &["worktree", "prune"])?;
    let git_after_lifecycle = git_command_for_root(&repo)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    if !git_after_lifecycle.status.success()
        || String::from_utf8_lossy(&git_after_lifecycle.stdout)
            .contains("branch refs/heads/feature")
        || String::from_utf8_lossy(&git_after_lifecycle.stdout).contains("branch refs/heads/review")
    {
        return Err(io::Error::other(format!(
            "removed worktrees remained in Git structural discovery: {}{}",
            String::from_utf8_lossy(&git_after_lifecycle.stdout),
            String::from_utf8_lossy(&git_after_lifecycle.stderr)
        ))
        .into());
    }
    let status_after_lifecycle = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "root", "status"])
        .arg(&manager)
        .output()?;
    if !status_after_lifecycle.status.success() {
        return Err(io::Error::other(format!(
            "ProjectAtlas structural status failed after Git worktree removal: {}",
            String::from_utf8_lossy(&status_after_lifecycle.stderr)
        ))
        .into());
    }
    let status_after_lifecycle: Value = serde_json::from_slice(&status_after_lifecycle.stdout)?;
    require_json_array_len(&status_after_lifecycle, &["worktrees"], 1)?;
    require_json_string(
        &status_after_lifecycle,
        &["worktrees", "0", "state"],
        "active",
    )?;

    let recreated_worktree = git_command_for_root(&repo)
        .args(["worktree", "add"])
        .arg(&linked_checkout_path)
        .arg("feature")
        .output()?;
    if !recreated_worktree.status.success() {
        return Err(io::Error::other(format!(
            "Git did not recreate the feature worktree after prune: {}{}",
            String::from_utf8_lossy(&recreated_worktree.stdout),
            String::from_utf8_lossy(&recreated_worktree.stderr)
        ))
        .into());
    }
    let linked = linked_checkout_path.canonicalize()?;
    if linked.join(ATLAS_DIR_NAME).exists() {
        return Err(io::Error::other(
            "recreated worktree inherited the retired target atlas from removed files",
        )
        .into());
    }
    let (mut recreated_session, _initialized) = McpContractSession::spawn_initialized(
        &control_mcp_command,
        &repo,
        &main_db,
        &[("PROJECTATLAS_NO_TELEMETRY", None)],
    )?;
    let recreated_inventory = recreated_session.call_tool(
        "atlas_worktree_list",
        &serde_json::json!({"include_retired": true}),
    )?;
    let recreated_root_text =
        projectatlas_core::normalize_native_path_display(linked.canonicalize()?);
    let recreated_selector = recreated_inventory
        .lines()
        .find(|line| line.contains(&recreated_root_text) && !line.contains("retired"))
        .and_then(|line| line.split(',').next())
        .map(|selector| selector.trim().trim_matches('"').to_string())
        .filter(|selector| selector.starts_with("wt-"))
        .ok_or_else(|| {
            io::Error::other(format!(
                "recreated structural inventory omitted its stable selector: {recreated_inventory}"
            ))
        })?;
    let recreated_registration = recreated_session.call_tool(
        "atlas_worktree_add",
        &serde_json::json!({"worktree": recreated_selector, "alias": "feature"}),
    )?;
    if !recreated_registration.contains("status: registered")
        || !recreated_registration.contains("git_unchanged: true")
        || !recreated_registration.contains("files_unchanged: true")
    {
        return Err(io::Error::other(format!(
            "recreated worktree did not receive a new safe registration: {recreated_registration}"
        ))
        .into());
    }
    let control_after_recreate = AtlasStore::open_for_project(&main_db, &repo)?;
    let active_recreated = control_after_recreate
        .worktree_registrations(true)?
        .into_iter()
        .find(|registration| {
            registration.alias.as_str() == "feature"
                && matches!(
                    registration.state,
                    projectatlas_db::WorktreeRegistrationState::Active
                )
        })
        .ok_or_else(|| io::Error::other("recreated feature registration is not active"))?;
    if active_recreated.registration_id == retired_feature_registration_id
        || active_recreated.git_administrative_identity == retired_feature_administrative_identity
        || active_recreated.project_instance_id.is_some()
    {
        return Err(io::Error::other(
            "recreated worktree revived retired identity or bound before explicit init",
        )
        .into());
    }
    drop(control_after_recreate);

    let recreated_init =
        recreated_session.call_tool("atlas_init", &serde_json::json!({"worktree": "feature"}))?;
    if !recreated_init.contains("status: hydrated")
        || !recreated_init.contains("source_project_instance_id:")
        || !recreated_init.contains("target_project_instance_id:")
        || !recreated_init.contains("reconciled_generation:")
    {
        return Err(io::Error::other(format!(
            "recreated feature atlas did not hydrate and reconcile from control: {recreated_init}"
        ))
        .into());
    }
    let recreated_db = linked.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let recreated_store = AtlasStore::open_for_project(&recreated_db, &linked)?;
    let recreated_identity = recreated_store
        .project_instance_id()?
        .ok_or_else(|| io::Error::other("recreated feature project identity missing"))?;
    let recreated_library = recreated_store
        .load_node_by_path("src/lib.rs")?
        .ok_or_else(|| io::Error::other("recreated hydration omitted feature source"))?;
    if recreated_identity == main_identity
        || recreated_identity == linked_identity
        || recreated_library.purpose.purpose.as_deref() != Some("Main checkout Rust library.")
        || recreated_store
            .load_node_by_path("src/feature_only.rs")?
            .is_none()
        || recreated_store
            .load_node_by_path("src/dirty_only.rs")?
            .is_some()
        || recreated_store.repository_graph_generation()?.is_none()
        || recreated_store.token_overview(None)?.calls != 0
    {
        return Err(io::Error::other(
            "recreated hydration revived retired state, crossed source, lost reusable purpose/graph data, or copied telemetry",
        )
        .into());
    }
    drop(recreated_store);
    let recreated_scan = recreated_session.call_tool(
        "atlas_scan",
        &serde_json::json!({"worktree": "feature", "max_workers": 1}),
    )?;
    if !recreated_scan.contains("scan:") {
        return Err(io::Error::other(format!(
            "recreated feature scan returned no successful report: {recreated_scan}"
        ))
        .into());
    }
    run_watch_once(&linked, &recreated_db)?;
    let recreated_search = recreated_session.call_tool(
        "atlas_search",
        &serde_json::json!({"worktree": "feature", "pattern": "linked_feature_marker"}),
    )?;
    if !recreated_search.contains("linked_feature_marker")
        || recreated_search.contains("main_checkout_marker")
        || recreated_search.contains("linked_dirty_marker")
    {
        return Err(io::Error::other(format!(
            "recreated worktree MCP route crossed control or retired source: {recreated_search}"
        ))
        .into());
    }
    recreated_session.shutdown()?;
    Ok(())
}

#[test]
fn scan_refuses_unverified_registered_worktree_boundary_before_publication()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(MAIN_CHECKOUT_DIR_NAME);
    fs::create_dir(&repo)?;
    git_success(&repo, &["init"])?;
    git_success(&repo, &["config", "user.name", "ProjectAtlas Test"])?;
    git_success(
        &repo,
        &["config", "user.email", "projectatlas@example.invalid"],
    )?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(
        repo.join(SRC_DIR_NAME).join(LIB_RS_FILE_NAME),
        "pub fn main_checkout_marker() {}\n",
    )?;
    git_success(&repo, &["add", "."])?;
    git_success(&repo, &["commit", "-m", "main fixture"])?;

    let linked = repo.join(LINKED_CHECKOUTS_DIR_NAME).join("feature");
    let worktree_output = git_command_for_root(&repo)
        .args(["worktree", "add", "-b", "feature"])
        .arg(&linked)
        .output()?;
    if !worktree_output.status.success() {
        return Err(io::Error::other(format!(
            "git worktree add failed: {}{}",
            String::from_utf8_lossy(&worktree_output.stdout),
            String::from_utf8_lossy(&worktree_output.stderr)
        ))
        .into());
    }
    let registration = fs::read_dir(repo.join(GIT_DIR_NAME).join("worktrees"))?
        .next()
        .transpose()?
        .ok_or_else(|| io::Error::other("linked-worktree registration missing"))?;
    let registration_gitdir = registration.path().join("gitdir");
    let registered_git_control = fs::read_to_string(&registration_gitdir)?;
    if Path::new(registered_git_control.trim()).canonicalize()?
        != linked.join(GIT_DIR_NAME).canonicalize()?
    {
        return Err(io::Error::other("fixture selected the wrong worktree registration").into());
    }
    fs::write(&registration_gitdir, "unverified-worktree-root")?;
    if !matches!(
        scan_repo(&repo, &ScanOptions::default()),
        Err(FsError::RepositoryBoundary { .. })
    ) {
        return Err(io::Error::other(
            "filesystem scan did not classify the unverified worktree boundary",
        )
        .into());
    }

    let scan = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["--format", "json", "scan", "."])
        .output()?;
    if scan.status.success() {
        return Err(io::Error::other("scan accepted an unverified worktree boundary").into());
    }
    let error: Value = serde_json::from_slice(&scan.stderr)?;
    require_json_string(&error, &["error", "kind"], "verification_incomplete")?;
    require_json_string(
        &error,
        &["error", "verification_incomplete", "reason"],
        "policy_unavailable",
    )?;

    let database = repo.join(ATLAS_DIR_NAME).join("projectatlas.db");
    if database.is_file() {
        let store = AtlasStore::open_for_project(&database, &repo)?;
        if !store.load_nodes()?.is_empty() || store.repository_graph_generation()?.is_some() {
            return Err(io::Error::other(
                "unverified worktree boundary published a database generation",
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn git_control_roots_return_typed_worktree_guidance_without_state() -> Result<(), Box<dyn Error>> {
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
    let bare_dot_git_parent = temp.path().join("bare-dot-git-parent");
    fs::create_dir(&bare_dot_git_parent)?;
    let bare_dot_git = bare_dot_git_parent.join(GIT_DIR_NAME);
    let output = StdCommand::new("git")
        .args(["init", "--bare"])
        .arg(&bare_dot_git)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git init --bare .git failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    git_success(&bare, &["config", "--unset", "core.bare"])?;

    let manager = temp.path().join("repository-manager");
    fs::create_dir(&manager)?;
    git_success(&manager, &["init"])?;
    let common_git_dir = manager.join(".git");
    let common_dir_init = Command::cargo_bin("projectatlas")?
        .current_dir(&common_git_dir)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if !common_dir_init.status.success() {
        return Err(io::Error::other(format!(
            "single-worktree common manager did not select its source: {}",
            String::from_utf8_lossy(&common_dir_init.stderr)
        ))
        .into());
    }
    if common_git_dir.join(ATLAS_DIR_NAME).exists() {
        return Err(io::Error::other(
            "common-manager selection created ProjectAtlas state under .git",
        )
        .into());
    }
    if !manager
        .join(ATLAS_DIR_NAME)
        .join("projectatlas.db")
        .is_file()
    {
        return Err(io::Error::other(
            "common-manager selection did not initialize the selected worktree atlas",
        )
        .into());
    }
    let manager_db = manager.join(ATLAS_DIR_NAME).join("projectatlas.db");
    let manager_mcp_config = mcp_config_for_harness(&manager, &manager_db, "mcp-json")?;
    let (manager_mcp_command, manager_mcp_args) = mcp_command_and_args(&manager_mcp_config)?;
    let messages = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-bare-dot-git-e2e","version":"0.1.0"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_root","arguments":{"control_root":projectatlas_core::normalize_native_path_display(bare_dot_git.canonicalize()?)}}}).to_string(),
    ];
    let stdout = run_mcp_stdio(&manager_mcp_command, &manager, &manager_mcp_args, &messages)?;
    let bare_dot_git_status = mcp_tool_text(&stdout, 2)?;
    if !bare_dot_git_status.contains("worktree_required: true")
        || !bare_dot_git_status.contains("source_selection: worktree_unavailable")
    {
        return Err(io::Error::other(format!(
            "MCP bare .git status invented a source worktree: {bare_dot_git_status}"
        ))
        .into());
    }
    if bare_dot_git_parent.join(ATLAS_DIR_NAME).exists() {
        return Err(io::Error::other("MCP bare .git status created parent atlas state").into());
    }
    let common_atlas_dir = common_git_dir.join(ATLAS_DIR_NAME);
    fs::create_dir(&common_atlas_dir)?;
    let common_database = common_atlas_dir.join("projectatlas.db");
    {
        let connection = Connection::open(&common_database)?;
        connection.execute_batch(
            "
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO metadata(key, value) VALUES('schema_version', '18');
            CREATE TABLE authored_state(value TEXT NOT NULL);
            INSERT INTO authored_state(value) VALUES('preserve-common-dir');
            ",
        )?;
    }
    let common_database_before = fs::read(&common_database)?;
    let common_dir_settings = Command::cargo_bin("projectatlas")?
        .current_dir(&common_git_dir)
        .args(["--format", "json", "settings"])
        .output()?;
    if !common_dir_settings.status.success() {
        return Err(io::Error::other(format!(
            "common-manager settings did not use the selected worktree atlas: {}",
            String::from_utf8_lossy(&common_dir_settings.stderr)
        ))
        .into());
    }
    if fs::read(&common_database)? != common_database_before {
        return Err(io::Error::other("common Git directory refusal changed database bytes").into());
    }
    for sidecar in ["projectatlas.db-wal", "projectatlas.db-shm"] {
        if common_atlas_dir.join(sidecar).exists() {
            return Err(io::Error::other(format!(
                "common Git directory refusal created SQLite sidecar {sidecar}"
            ))
            .into());
        }
    }
    let included_config = temp.path().join("included-bare.config");
    fs::write(&included_config, "[core]\n\tbare = true\n")?;
    let included_config = included_config.to_string_lossy().to_string();
    git_success(&manager, &["config", "include.path", &included_config])?;

    let separate_worktree = temp.path().join("separate-worktree");
    let separate_control = temp.path().join("separate-control");
    fs::create_dir(&separate_worktree)?;
    let output = StdCommand::new("git")
        .args(["init", "--quiet", "--separate-git-dir"])
        .arg(&separate_control)
        .arg(&separate_worktree)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git init --separate-git-dir failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    for selected in [
        &bare,
        &separate_control,
        &bare_dot_git,
        &bare_dot_git_parent,
    ] {
        let init = Command::cargo_bin("projectatlas")?
            .current_dir(selected)
            .args(["--format", "json", "init"])
            .output()?;
        if init.status.success() {
            return Err(io::Error::other(format!(
                "bare Git control root initialized as source: {}",
                selected.display()
            ))
            .into());
        }
        let error: Value = serde_json::from_slice(&init.stderr)?;
        require_json_string(&error, &["error", "kind"], "worktree_required")?;
        require_json_contains(
            &error,
            &["error", "message"],
            "select a checked-out worktree",
        )?;
        if selected.join(ATLAS_DIR_NAME).exists() {
            return Err(io::Error::other(format!(
                "bare-root refusal created ProjectAtlas state: {}",
                selected.display()
            ))
            .into());
        }

        let root_set = Command::cargo_bin("projectatlas")?
            .args(["--format", "json", "root", "set"])
            .arg(selected)
            .output()?;
        if root_set.status.success() {
            return Err(io::Error::other(format!(
                "root set bound a bare Git control root: {}",
                selected.display()
            ))
            .into());
        }
        let root_set_error: Value = serde_json::from_slice(&root_set.stderr)?;
        require_json_string(&root_set_error, &["error", "kind"], "worktree_required")?;
        if selected.join(ATLAS_DIR_NAME).exists() {
            return Err(io::Error::other(format!(
                "bare root-set refusal created ProjectAtlas state: {}",
                selected.display()
            ))
            .into());
        }
    }

    let lookalike = temp.path().join("git-control-lookalike");
    fs::create_dir(&lookalike)?;
    fs::create_dir(lookalike.join("objects"))?;
    fs::create_dir_all(lookalike.join("refs").join("heads"))?;
    fs::write(lookalike.join("HEAD"), "ref: refs/heads/main\n")?;
    fs::write(
        lookalike.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
    )?;
    let lookalike_init = Command::cargo_bin("projectatlas")?
        .current_dir(&lookalike)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if lookalike_init.status.success() {
        return Err(io::Error::other(
            "structurally complete Git control lookalike initialized as source",
        )
        .into());
    }
    let lookalike_error: Value = serde_json::from_slice(&lookalike_init.stderr)?;
    require_json_string(&lookalike_error, &["error", "kind"], "worktree_required")?;
    if lookalike.join(ATLAS_DIR_NAME).exists() {
        return Err(
            io::Error::other("structural control-root refusal created ProjectAtlas state").into(),
        );
    }
    Ok(())
}

#[test]
fn explicit_config_rebases_implicit_database_from_descendant_and_git_manager()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    git_success(&repo, &["init"])?;
    let config = repo.join(ATLAS_DIR_NAME).join("custom-config.toml");
    let init = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .args(["--format", "json", "init", "--no-scan"])
        .output()?;
    if !init.status.success() {
        return Err(io::Error::other(format!(
            "fixture init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        ))
        .into());
    }

    let nested = repo.join(SRC_DIR_NAME).join("nested");
    fs::create_dir_all(&nested)?;
    for selected in [&nested, &repo.join(GIT_DIR_NAME)] {
        let token = Command::cargo_bin("projectatlas")?
            .current_dir(selected)
            .arg("--config")
            .arg(&config)
            .args(["--format", "json", "token"])
            .output()?;
        if !token.status.success() {
            return Err(io::Error::other(format!(
                "config-selected token report did not use the project database from '{}': {}",
                selected.display(),
                String::from_utf8_lossy(&token.stderr)
            ))
            .into());
        }
        if selected.join(ATLAS_DIR_NAME).exists() {
            return Err(io::Error::other(format!(
                "config-selected token report created invocation-local state under '{}'",
                selected.display()
            ))
            .into());
        }
    }
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
    let mut child = StdCommand::new(&executable)
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--poll-seconds", "1", "--max-cycles", "2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let readiness_started = Instant::now();
    let mut last_observation: String;
    loop {
        let initial_symbol_is_published = match AtlasStore::open_read_only(&db)
            .and_then(|store| store.load_symbols(Some("src/lib.rs"), None, 2))
        {
            Ok(symbols) => {
                let exact_initial = matches!(
                    symbols.as_slice(),
                    [symbol] if symbol.path == "src/lib.rs" && symbol.name == "initial"
                );
                last_observation = format!("symbols query returned {symbols:?}");
                exact_initial
            }
            Err(error) => {
                last_observation = format!("symbols query failed: {error}");
                false
            }
        };
        if let Some(status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            return Err(io::Error::other(format!(
                "projectatlas watch exited before initial symbol readiness: status={status}; {last_observation}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if initial_symbol_is_published {
            break;
        }
        if readiness_started.elapsed() > Duration::from_secs(15) {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            let output = child.wait_with_output()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "projectatlas watch did not publish the exact initial symbol within 15 seconds: {last_observation}; stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            )
            .into());
        }
        thread::sleep(Duration::from_millis(200));
    }
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
            let _status = child.wait()?;
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
fn configured_module_aliases_resolve_across_adapters_and_refresh_atomically()
-> Result<(), Box<dyn Error>> {
    const ALTERNATE_DIR_NAME: &str = "alternate";
    const CONTROLLER_TS_FILE: &str = "controller.ts";
    const JS_DIR_NAME: &str = "js";
    const TS_CONFIG: &str = r#"{
  // JSONC is the native compiler configuration format.
  "compilerOptions": {
    "baseUrl": "src",
    "paths": {
      "@/*": ["*"],
    },
  },
}
"#;
    const JS_CONFIG: &str = r#"{
  "compilerOptions": {
    "baseUrl": "../src",
    "paths": {
      "@/*": ["*"]
    }
  }
}
"#;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("configured-module-aliases");
    let source = repo.join(SRC_DIR_NAME);
    let js_source = repo.join(JS_DIR_NAME).join(SRC_DIR_NAME);
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&js_source)?;
    let db = temp.path().join("configured-module-aliases.db");
    let ts_config = repo.join(TS_CONFIG_FILE_NAME);
    let js_config = repo.join(JS_DIR_NAME).join("jsconfig.json");
    fs::write(&ts_config, TS_CONFIG)?;
    fs::write(&js_config, JS_CONFIG)?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"configured-shared\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        repo.join(JS_DIR_NAME).join("Cargo.toml"),
        "[package]\nname = \"configured-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        source.join(CONTROLLER_TS_FILE),
        "export function useController(): string { return \"ok\"; }\n",
    )?;
    fs::write(
        source.join("Page.vue"),
        "<script setup lang=\"ts\">\nimport { useController } from \"@/controller\";\nconst value = useController();\n</script>\n<template><div>{{ value }}</div></template>\n",
    )?;
    fs::write(
        source.join("js-controller.js"),
        "export function useJsController() { return \"ok\"; }\n",
    )?;
    fs::write(
        js_source.join("js-page.js"),
        "import { useJsController } from \"@/js-controller\";\nexport const value = useJsController();\n",
    )?;
    run_scan(&repo, &db)?;

    let ts_file = detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?;
    assert_detailed_resolution(&ts_file, 1, "resolved")?;
    let ts_symbol = detailed_relation_payload(
        &repo,
        &db,
        "src/controller.ts",
        Some("useController"),
        "inbound",
    )?;
    assert_detailed_resolution(&ts_symbol, 1, "resolved")?;
    let js_file = detailed_relation_payload(&repo, &db, "src/js-controller.js", None, "inbound")?;
    assert_detailed_resolution(&js_file, 1, "resolved")?;
    let js_symbol = detailed_relation_payload(
        &repo,
        &db,
        "src/js-controller.js",
        Some("useJsController"),
        "inbound",
    )?;
    assert_detailed_resolution(&js_symbol, 1, "resolved")?;
    let js_outbound =
        detailed_relation_payload(&repo, &db, "js/src/js-page.js", Some("value"), "outbound")?;
    assert_detailed_resolution(&js_outbound, 1, "resolved")?;

    let impact = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&db)
        .args([
            "--format",
            "json",
            "symbols",
            "relations",
            "--view",
            "analysis",
            "--analysis-mode",
            "impact",
            "--vcs",
            "working-tree",
            "--include-dead-code",
            "--file",
            "src/controller.ts",
            "--symbol",
            "useController",
            "--direction",
            "inbound",
            "--depth",
            "2",
            "--limit",
            "50",
        ])
        .output()?;
    if !impact.status.success() {
        return Err(io::Error::other(format!(
            "configured alias impact analysis failed: {}",
            String::from_utf8_lossy(&impact.stderr)
        ))
        .into());
    }
    let impact: Value = serde_json::from_slice(&impact.stdout)?;
    let dead_code_nodes = impact
        .pointer("/symbol_relations/findings")
        .and_then(Value::as_array)
        .and_then(|findings| {
            findings
                .iter()
                .find(|finding| finding.get("kind").and_then(Value::as_str) == Some("dead_code"))
        })
        .and_then(|finding| finding.get("nodes"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("impact analysis omitted typed dead-code findings"))?;
    if dead_code_nodes.iter().any(|node| {
        node.pointer("/node/entity/selector/symbol/name")
            .and_then(Value::as_str)
            == Some("useController")
    }) {
        return Err(io::Error::other(
            "configured alias target was presented as a dead-code candidate",
        )
        .into());
    }

    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"configured-module-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "atlas_symbol_relations",
                "arguments": {
                    "view": "detailed",
                    "file": "src/controller.ts",
                    "direction": "inbound",
                    "limit": 10
                }
            }
        })
        .to_string(),
    ];
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let stdout = run_mcp_stdio_with_env(
        &executable,
        &repo,
        &[
            "--db".to_string(),
            db.display().to_string(),
            "mcp".to_string(),
        ],
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
    )?;
    let mcp = mcp_tool_text(&stdout, 2)?;
    for expected in ["status: resolved", "value: 1", "src/controller.ts"] {
        if !mcp.contains(expected) {
            return Err(io::Error::other(format!(
                "configured alias MCP result omitted {expected:?}: {mcp}"
            ))
            .into());
        }
    }

    fs::create_dir_all(source.join(ALTERNATE_DIR_NAME))?;
    fs::write(
        source.join(ALTERNATE_DIR_NAME).join(CONTROLLER_TS_FILE),
        "export function useController(): string { return \"alternate\"; }\n",
    )?;
    fs::write(
        &ts_config,
        r#"{"compilerOptions":{"baseUrl":"src","paths":{"@/*":["alternate/*"]}}}"#,
    )?;
    run_watch_once(&repo, &db)?;
    assert_detailed_resolution(
        &detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?,
        0,
        "resolved",
    )?;
    assert_detailed_resolution(
        &detailed_relation_payload(&repo, &db, "src/alternate/controller.ts", None, "inbound")?,
        1,
        "resolved",
    )?;

    fs::remove_file(&ts_config)?;
    run_watch_once(&repo, &db)?;
    assert_detailed_resolution(
        &detailed_relation_payload(&repo, &db, "src/alternate/controller.ts", None, "inbound")?,
        0,
        "resolved",
    )?;
    let unresolved = detailed_relation_payload(&repo, &db, "src/Page.vue", None, "outbound")?;
    assert_detailed_resolution(&unresolved, 2, "unresolved")?;

    let pending_ts_config = repo.join("tsconfig.pending.json");
    fs::write(&pending_ts_config, TS_CONFIG)?;
    fs::rename(&pending_ts_config, &ts_config)?;
    run_watch_once(&repo, &db)?;
    assert_detailed_resolution(
        &detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?,
        1,
        "resolved",
    )?;
    fs::rename(&ts_config, &pending_ts_config)?;
    run_watch_once(&repo, &db)?;
    assert_detailed_resolution(
        &detailed_relation_payload(&repo, &db, "src/controller.ts", None, "inbound")?,
        0,
        "resolved",
    )?;
    fs::remove_file(&pending_ts_config)?;

    let generation_before_failure = AtlasStore::open(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("configured alias publication is missing"))?
        .generation;
    fs::write(
        &js_config,
        r#"{"compilerOptions":{"baseUrl":"src","paths":["invalid"]}}"#,
    )?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("field 'paths' must be an object"));
    fs::write(&js_config, JS_CONFIG)?;
    let generation_after_failure = AtlasStore::open(&db)?
        .index_publication()?
        .ok_or_else(|| io::Error::other("configured alias publication disappeared"))?
        .generation;
    if generation_after_failure != generation_before_failure {
        return Err(io::Error::other(
            "malformed compiler configuration advanced the published generation",
        )
        .into());
    }
    assert_detailed_resolution(
        &detailed_relation_payload(
            &repo,
            &db,
            "src/js-controller.js",
            Some("useJsController"),
            "inbound",
        )?,
        1,
        "resolved",
    )?;
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
    fs::create_dir_all(repo.join("docs"))?;
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
    let guide = repo.join(GUIDE_MD_PATH);
    let document_target = repo.join("docs/target.md");
    fs::write(&guide, "# Guide\n\n[target](target.md#target)\n")?;
    fs::write(&document_target, "# Target\n")?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\nignored/\n")?;
    let config = repo.join(ATLAS_DIR_NAME).join("config.toml");
    fs::write(&config, CONFIG)?;

    run_scan(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "initial")?;

    let created = repo.join(SRC_DIR_NAME).join(CREATED_RS_FILE_NAME);
    let created_document = repo.join("docs/created.md");
    fs::write(&created, "pub fn created() -> u32 { 2 }\n")?;
    fs::write(&created_document, "# Created\n")?;
    fs::write(&guide, "# Guide\n\n[created](created.md#created)\n")?;
    let _created_summary = json_summary_command(&repo, &db, "src/created.rs")?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "create")?;

    fs::write(
        &created,
        "pub fn created() -> u32 { helper() }\nfn helper() -> u32 { 3 }\n",
    )?;
    fs::write(&created_document, "# Revised\n")?;
    fs::write(&guide, "# Guide\n\n[revised](created.md#revised)\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "modify")?;

    let moved = repo.join(TESTS_DIR_NAME).join(CREATED_RS_FILE_NAME);
    let moved_document = repo.join("docs/moved.md");
    fs::rename(&created, &moved)?;
    fs::rename(&created_document, &moved_document)?;
    fs::write(&guide, "# Guide\n\n[moved](moved.md#revised)\n")?;
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
    let renamed_document = repo.join("docs/Renamed.md");
    fs::rename(&moved, &renamed)?;
    fs::rename(&moved_document, &renamed_document)?;
    fs::write(&guide, "# Guide\n\n[renamed](Renamed.md#revised)\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "rename")?;

    fs::remove_file(repo.join(SRC_DIR_NAME).join(ALPHA_RS_FILE_NAME))?;
    fs::remove_file(&renamed_document)?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn entry() {}\n",
    )?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "delete")?;

    fs::write(
        repo.join(".gitignore"),
        ".projectatlas/\nignored/\ntests/\ndocs/Renamed.md\n",
    )?;
    fs::write(&renamed_document, "# Revised\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "ignore")?;

    fs::write(repo.join(".gitignore"), ".projectatlas/\nignored/\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "unignore")?;

    let case_intermediate = repo.join("docs/case-rename.tmp");
    let case_renamed_document = repo.join("docs/renamed.md");
    fs::rename(&renamed_document, &case_intermediate)?;
    fs::rename(&case_intermediate, &case_renamed_document)?;
    fs::write(&guide, "# Guide\n\n[renamed](renamed.md#revised)\n")?;
    run_watch_once(&repo, &db)?;
    assert_clean_scan_convergence(&repo, &db, temp.path(), "case-rename")?;

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
    let before_failed_results = derived_result_snapshot(&db)?;
    fs::write(
        repo.join(SRC_DIR_NAME).join("lib.rs"),
        "pub fn entry() {}\npub fn after_retry() {}\n",
    )?;
    fs::write(&guide, "# Guide\n\n[after retry](missing-after-retry.md)\n")?;
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
    if derived_result_snapshot(&db)? != before_failed_results {
        return Err(io::Error::other(
            "failed watcher work changed classified document or graph results",
        )
        .into());
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
    let stale_reads = [
        (
            "atlas_file_summary",
            serde_json::json!({"file": "src/lib.rs"}),
        ),
        (
            "atlas_search",
            serde_json::json!({"pattern": "legacy_store", "file_pattern": "*.rs"}),
        ),
        (
            "atlas_symbol_relations",
            serde_json::json!({"file": "src/lib.rs"}),
        ),
        (
            "atlas_files",
            serde_json::json!({"file_pattern": "tests/*.rs"}),
        ),
        (
            "atlas_slice",
            serde_json::json!({"file": "src/lib.rs", "symbol": "legacy_store"}),
        ),
        (
            "atlas_file_summary",
            serde_json::json!({"file": deleted_absolute_selector}),
        ),
    ];
    let mut stale_session = McpContractSession::spawn(&executable, repo, db)?;
    let stale_result = (|| -> Result<(), Box<dyn Error>> {
        for (tool, arguments) in stale_reads {
            let tool_text = stale_session.call_tool(tool, &arguments)?;
            if !tool_text.contains("kind: refresh_required")
                || !tool_text.contains("status: refresh_required")
                || !tool_text.contains("tool: atlas_watch_once")
                || !tool_text.contains("changed:")
                || tool_text.contains("changed: 0")
                || tool_text.contains("legacy_store")
            {
                return Err(io::Error::other(format!(
                    "stale MCP read {tool} did not return the typed fail-closed state: {tool_text}"
                ))
                .into());
            }
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(stale_result, || stale_session.shutdown())?;

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
            source_selector: None,
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

#[cfg(unix)]
fn create_test_directory_indirection(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn create_test_directory_indirection(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            let output = StdCommand::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "test junction creation failed: {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
                .into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn remove_test_directory_indirection(link: &Path) -> Result<(), Box<dyn Error>> {
    fs::remove_file(link)?;
    Ok(())
}

#[cfg(windows)]
fn remove_test_directory_indirection(link: &Path) -> Result<(), Box<dyn Error>> {
    fs::remove_dir(link)?;
    Ok(())
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

/// Return the explicitly selected packaged runtime or the local test binary.
fn mcp_contract_executable() -> PathBuf {
    std::env::var_os(MCP_CONTRACT_EXECUTABLE_ENV).map_or_else(
        || assert_cmd::cargo::cargo_bin("projectatlas"),
        PathBuf::from,
    )
}

fn complete_mcp_test_after_shutdown<T>(
    operation_result: Result<T, Box<dyn Error>>,
    shutdown: impl FnOnce() -> Result<(), Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    let shutdown_result = shutdown();
    let value = operation_result?;
    shutdown_result?;
    Ok(value)
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

/// Launch a real MCP stdio child and return stdout after stdin closes.
fn run_mcp_stdio(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env(executable, cwd, args, messages, &[])
}

/// Launch a real MCP stdio child and close stdin only after every request has a response.
fn run_mcp_stdio_with_env(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
) -> Result<String, Box<dyn Error>> {
    let mut expected_responses = BTreeSet::new();
    for message in messages {
        let request: Value = serde_json::from_str(message.as_ref())?;
        if let Some(id) = request.get("id") {
            expected_responses.insert(id.to_string());
        }
    }
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
    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("mcp stdout was not piped"))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("mcp stderr was not piped"))?;
    let (response_sender, response_receiver) = mpsc::channel();
    let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut reader = BufReader::new(&mut stdout_pipe);
        let mut output = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            output.extend_from_slice(line.as_bytes());
            drop(response_sender.send(line));
        }
        Ok(output)
    });
    let stderr_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        stderr_pipe.read_to_end(&mut output)?;
        Ok(output)
    });

    let response_result = (|| -> Result<(), Box<dyn Error>> {
        stdin.write_all(input.as_bytes())?;
        stdin.flush()?;
        let mut response_deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| io::Error::other("MCP response deadline overflowed"))?;
        while !expected_responses.is_empty() {
            let remaining = response_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "projectatlas mcp did not answer request ids before shutdown: {expected_responses:?}"
                    ),
                )
                .into());
            }
            let line = response_receiver
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "projectatlas mcp response deadline elapsed with request ids pending: {expected_responses:?}"
                        ),
                    ),
                    mpsc::RecvTimeoutError::Disconnected => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "projectatlas mcp closed before answering every request",
                    ),
                })?;
            let response: Value = serde_json::from_str(line.trim())?;
            if response
                .get("id")
                .is_some_and(|id| expected_responses.remove(&id.to_string()))
            {
                response_deadline = Instant::now()
                    .checked_add(Duration::from_secs(10))
                    .ok_or_else(|| io::Error::other("MCP response deadline overflowed"))?;
            }
        }
        Ok(())
    })();
    drop(stdin);
    drop(response_receiver);

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            let _status = child.wait()?;
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
    response_result?;
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

/// Hash every bounded user-table row through one read-only `SQLite` connection.
fn sqlite_table_digests(
    connection: &Connection,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    const MAX_TABLE_ROWS: usize = 16_384;
    const MAX_TABLE_BYTES: usize = 8 * 1024 * 1024;

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
    let mut tables = BTreeMap::new();
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
        tables.insert(table_name, digest);
    }
    Ok(tables)
}

/// Capture bounded logical rows so WAL/page-layout changes do not masquerade as product state.
fn mcp_database_snapshot(database: &Path) -> Result<McpDatabaseSnapshot, Box<dyn Error>> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tables = sqlite_table_digests(&connection)?;
    let (usage, authoritative) = tables
        .into_iter()
        .partition(|(table_name, _)| table_name.starts_with("usage_"));
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

/// Return a nested `JSON` string.
fn json_string_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, Box<dyn Error>> {
    json_at(value, path)?
        .as_str()
        .ok_or_else(|| io::Error::other(format!("expected string at {path:?}")).into())
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

/// Run a JSON summary command for one indexed path.
fn json_summary_command(repo: &Path, db: &Path, file: &str) -> Result<Value, Box<dyn Error>> {
    let output = Command::new(mcp_contract_executable())
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
