//! Purpose: Validate installer, release, packaged, and plugin delivery contracts.
#![allow(unused_imports)]

#[path = "support/source_contract.rs"]
mod frozen_source_contract;
mod support;
use assert_cmd::Command;
use frozen_source_contract::CLI_E2E_SOURCE_SHA256;
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
use serde::Deserialize;
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
    MCP_CONTRACT_METADATA_CANARY, McpDatabaseSnapshot, McpStdioCleanupPacket,
    assert_planted_community_values, complete_mcp_test_after_shutdown, git_command_for_root,
    json_at, json_community_values, json_summary_command, mcp_contract_executable,
    mcp_database_snapshot, mcp_tool_text, reap_mcp_stdio_packet, require_json_array_len,
    require_json_bool, require_json_contains, require_json_string, require_json_usize,
    require_json_usize_at_least, require_json_usize_greater_than, run_mcp_stdio,
    run_mcp_stdio_with_env, run_mcp_stdio_with_env_and_test_delay,
    run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff, sha256_hex, sqlite_table_digests,
    synchronize_prompt_exit_before_delayed_observation, workspace_root,
};
use yaml_rust2::{Yaml, YamlLoader};

const TEST_REPO_DIR: &str = "repo";

const SRC_DIR_NAME: &str = "src";

const DUPLICATE_RS_FILE_NAME: &str = "duplicate.rs";

const LIB_RS_FILE_NAME: &str = "lib.rs";

const SCANNED_RS_FILE_NAME: &str = "scanned.rs";

const GIT_DIR_NAME: &str = ".git";

const OUTSIDE_CANARY_FILE_NAME: &str = "outside-canary.txt";

const PARENT_CANARY_FILE_NAME: &str = "parent-canary.txt";

const ATLAS_DIR_NAME: &str = ".projectatlas";

const MISSING_INDEX_DIR_NAME: &str = "missing-index";

const GITHOOKS_DIR_NAME: &str = ".githooks";

const ISSUE_TEMPLATE_DIR_NAME: &str = "ISSUE_TEMPLATE";

const PRE_PUSH_HOOK_FILE_NAME: &str = "pre-push";

const PACKAGE_JSON_FILE_NAME: &str = "package.json";

const OPENSPEC_DIR_NAME: &str = "openspec";

const ISSUE_CHECKLISTS_SCRIPT_FILE_NAME: &str = "issue-checklists.py";

const ISSUE_MAP_FILE_NAME: &str = "issue-map.json";

const CHANGE_DIR_NAME: &str = "changes";

const ISSUEOPS_CHANGE_NAME: &str = "scope-local-issueops-branch-validation";

const TASKS_FILE_NAME: &str = "tasks.md";

const ISSUEOPS_TASKS_RELATIVE_PATH: &str =
    "openspec/changes/scope-local-issueops-branch-validation/tasks.md";

const CANDIDATE_FILE_NAME: &str = "candidate.txt";

const DISPATCH_LOG_FILE_NAME: &str = "dispatch.log";

const AGENT_INTEGRATION_DOC_FILE_NAME: &str = "agent-integration.md";

const WORKFLOW_DOC_FILE_NAME: &str = "workflow.md";

const DOCS_WORKFLOW_FILE_NAME: &str = "04-docs.yml";

const FILTERED_CUSTOM_HARNESS_COMMAND: &str = "cargo test --locked -p projectatlas-cli --all-features task_errors_classify_only_typed_cancellation_as_canceled";

const CODEX_CONFIG_DIR: &str = ".codex";

const CODEX_PLUGIN_MANIFEST_DIR: &str = ".codex-plugin";

const CODEX_MARKETPLACE_METADATA_DIR: &str = ".agents";

const CODEX_MARKETPLACE_INSTALL_RECORD_FILE_NAME: &str = ".codex-marketplace-install.json";

const CODEX_MARKETPLACE_MANIFEST_FILE_NAME: &str = "marketplace.json";

#[cfg(windows)]
const CODEX_MARKETPLACE_SNAPSHOT_DIR_NAME: &str = "marketplace-root";

const CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME: &str = "runtime-integration.json";

const CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME: &str = ".projectatlas-plugin-update.lock";

#[cfg(windows)]
const CODEX_FIXTURE_EXECUTABLE_FILE_NAME: &str = "codex.exe";

#[cfg(unix)]
const POSIX_CODEX_EXECUTABLE_FILE_NAME: &str = "codex";

#[cfg(unix)]
const POSIX_FIND_EXECUTABLE_FILE_NAME: &str = "find";

#[cfg(unix)]
const POSIX_FLOCK_EXECUTABLE_FILE_NAME: &str = "flock";

const FAKE_CODEX_LOG_FILE: &str = "fake-codex.log";

#[cfg(windows)]
const FAKE_CODEX_PLUGIN_CACHE_DIR: &str = "plugin-cache";

const FAKE_CODEX_JUNCTION_TARGET_DIR: &str = "junction-target";

const FAKE_CODEX_CLEANUP_SNAPSHOT_TARGET_DIR: &str = "cleanup-snapshot-target";

#[cfg(any(windows, unix))]
const INSTALLER_CANARY_FILE_NAME: &str = "canary.txt";

#[cfg(unix)]
const INSTALLER_OUTSIDE_SENTINEL_FILE_NAME: &str = "sentinel";

#[cfg(windows)]
const FAKE_CODEX_PLUGIN_LIST_FILE_NAME: &str = "codex-plugin-list.json";

const FAKE_CODEX_REGISTRY_CURRENT_FILE_NAME: &str = "codex-registry-current.json";

const FAKE_CODEX_REGISTRY_STALE_FILE_NAME: &str = "codex-registry-stale.json";

#[cfg(windows)]
const FAKE_CODEX_REGISTRY_STATE_FILE_NAME: &str = "codex-registry-state.txt";

#[cfg(windows)]
const CODEX_OWNER_READINESS_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(windows)]
// The failure path gives the owner this bounded window to observe the stop marker and exit.
const CODEX_OWNER_FAILURE_CLEANUP_BUDGET: Duration = Duration::from_secs(5);

#[cfg(windows)]
// The exact child-stop helper allows its owned process up to this wait budget to exit.
const CODEX_OWNER_CHILD_STOP_BUDGET: Duration = Duration::from_secs(5);

#[cfg(windows)]
const CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET: Duration = Duration::from_secs(5);

#[cfg(windows)]
const CODEX_OWNER_CHILD_STOP_FINAL_BUDGET: Duration = Duration::from_secs(5);

#[cfg(windows)]
const CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE: Duration = Duration::from_secs(2);

#[cfg(windows)]
// Allow normal scheduling variance without allowing a late readiness retry to hide.
const CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE: Duration = Duration::from_secs(2);

#[cfg(windows)]
const CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(windows)]
const CODEX_OWNER_IDENTITY_CAPTURE_TEST_DELAY: Duration = Duration::from_secs(6);

#[cfg(windows)]
const CODEX_OWNER_READINESS_BOUNDARY_PUBLICATION_DELAY: Duration = Duration::from_secs(24);

#[cfg(windows)]
const CODEX_OWNER_READINESS_BOUNDARY_CAPTURE_DELAY: Duration = Duration::from_secs(8);

#[cfg(windows)]
// Delay the polling caller, not the child operation, so a completed helper is observed late.
const CODEX_OWNER_LATE_COMPLETION_TEST_DELAY: Duration = Duration::from_secs(2);

#[cfg(windows)]
// Delay the outer owner observer past its deadline after the fixture has received stop.
const CODEX_OWNER_LATE_OWNER_OBSERVATION_TEST_DELAY: Duration = Duration::from_secs(6);

#[cfg(windows)]
// Delay the readiness poll past the deadline after the fixture has published, without
// changing process-global state, proving the admission guard through the real helper.
const CODEX_OWNER_OBSERVATION_TEST_DELAY: Duration = Duration::from_secs(31);

#[cfg(windows)]
const CODEX_OWNER_STOP_HELPER_TEST_DELAY: Duration = Duration::from_secs(6);

#[cfg(windows)]
// Early owner exit must be observed before readiness expires; this bound allows only
// the same scheduler margin as the bounded publication contract.
fn codex_owner_early_exit_max_elapsed() -> Duration {
    CODEX_OWNER_READINESS_TIMEOUT + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE
}

#[cfg(windows)]
const CODEX_OWNER_DELAYED_PUBLICATION: Duration = Duration::from_secs(6);

#[cfg(windows)]
const CODEX_OWNER_PUBLICATION_DELAY_ENV: &str =
    "PROJECTATLAS_TEST_CODEX_OWNER_PUBLICATION_DELAY_MS";

#[cfg(windows)]
const CODEX_OWNER_PUBLICATION_MODE_ENV: &str = "PROJECTATLAS_TEST_CODEX_OWNER_PUBLICATION_MODE";

#[cfg(windows)]
const CODEX_OWNER_IDENTITY_CAPTURE_DELAY_ENV: &str =
    "PROJECTATLAS_TEST_CODEX_OWNER_IDENTITY_CAPTURE_DELAY_MS";

#[cfg(windows)]
const CODEX_OWNER_STOP_DELAY_ENV: &str = "PROJECTATLAS_TEST_CODEX_OWNER_STOP_DELAY_MS";

#[cfg(windows)]
const OBSOLETE_PROJECTATLAS_FIXTURE_SOURCE_FILE_NAME: &str = "obsolete-projectatlas.cs";

#[cfg(windows)]
const OBSOLETE_PROJECTATLAS_FIXTURE_EXECUTABLE_FILE_NAME: &str = "obsolete-projectatlas.exe";

const FAKE_CODEX_SKILL_CONTENT: &str =
    include_str!("../../../plugins/projectatlas/skills/projectatlas/SKILL.md");

const FAKE_PATH_DIR: &str = "fake-path";

const ISOLATED_HOME_DIR: &str = "isolated-home";

const NPM_SHIM_DIR: &str = "npm";

#[cfg(windows)]
const WINDOWS_SYSTEM32_DIR: &str = "System32";

#[cfg(windows)]
const WINDOWS_POWERSHELL_DIR: &str = "WindowsPowerShell";

#[cfg(windows)]
const WINDOWS_POWERSHELL_VERSION_DIR: &str = "v1.0";

#[cfg(windows)]
const WINDOWS_POWERSHELL_EXECUTABLE: &str = "powershell.exe";

#[cfg(windows)]
static WINDOWS_RELEASE_ASSET_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
const CODEX_MCP_OWNER_FIXTURE_SOURCE: &str = r#"using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

public static class Program
{
    private static string Quote(string value)
    {
        return "\"" + value.Replace("\"", "\\\"") + "\"";
    }

    public static int Main(string[] arguments)
    {
        if (arguments.Length < 3)
            return 2;
        string childPath = Path.GetFullPath(arguments[1]);
        string childArguments =
            "--require-version 0.3.26 --db " + Quote(arguments[2]);
        if (arguments.Length >= 4)
            childArguments += " --config " + Quote(arguments[3]);
        childArguments += " mcp";
        using (Process child = Process.Start(new ProcessStartInfo
        {
            FileName = childPath,
            Arguments = childArguments,
            UseShellExecute = false,
            CreateNoWindow = true
        }))
        {
            try
            {
                string identityPath = arguments[0];
                string temporaryIdentityPath = identityPath + ".tmp";
                string retainedIdentityPath = identityPath + ".owner";
                string retainedIdentityTemporaryPath = retainedIdentityPath + ".tmp";
                // Retain exact fixture ownership even when normal publication is withheld.
                string retainedIdentityDelay = Environment.GetEnvironmentVariable(
                    "PROJECTATLAS_TEST_CODEX_OWNER_RETAINED_IDENTITY_DELAY_MS"
                );
                int retainedIdentityDelayMilliseconds;
                if (!String.IsNullOrWhiteSpace(retainedIdentityDelay)
                    && Int32.TryParse(retainedIdentityDelay, out retainedIdentityDelayMilliseconds)
                    && retainedIdentityDelayMilliseconds > 0)
                {
                    Thread.Sleep(retainedIdentityDelayMilliseconds);
                }
                string creationTime = child.StartTime.ToUniversalTime()
                    .ToFileTimeUtc()
                    .ToString();
                File.WriteAllLines(retainedIdentityTemporaryPath, new[]
                {
                    child.Id.ToString(),
                    creationTime,
                    childPath
                });
                File.Move(retainedIdentityTemporaryPath, retainedIdentityPath);
                string publicationMode = Environment.GetEnvironmentVariable(
                    "PROJECTATLAS_TEST_CODEX_OWNER_PUBLICATION_MODE"
                );
                if (publicationMode == "early-exit")
                    return 3;
                string publicationDelay = Environment.GetEnvironmentVariable(
                    "PROJECTATLAS_TEST_CODEX_OWNER_PUBLICATION_DELAY_MS"
                );
                int delayMilliseconds;
                if (!String.IsNullOrWhiteSpace(publicationDelay)
                    && Int32.TryParse(publicationDelay, out delayMilliseconds)
                    && delayMilliseconds > 0)
                {
                    Thread.Sleep(delayMilliseconds);
                }
                if (publicationMode != "timeout"
                    && publicationMode != "timeout-ignore-stop")
                {
                    if (publicationMode == "malformed")
                    {
                        File.WriteAllText(temporaryIdentityPath, "not-an-identity");
                    }
                    else
                    {
                        if (publicationMode == "mismatched")
                            creationTime = (Int64.Parse(creationTime) + 1).ToString();
                        File.WriteAllLines(temporaryIdentityPath, new[]
                        {
                            child.Id.ToString(),
                            creationTime,
                            childPath
                        });
                    }
                    File.Move(temporaryIdentityPath, identityPath);
                }
                while (!child.WaitForExit(25))
                {
                    if (publicationMode != "timeout-ignore-stop"
                        && publicationMode != "ignore-stop"
                        && File.Exists(identityPath + ".stop"))
                    {
                        child.Kill();
                        child.WaitForExit();
                        return 0;
                    }
                }
                Thread.Sleep(Timeout.Infinite);
                return 0;
            }
            finally
            {
                if (!child.HasExited)
                {
                    child.Kill();
                    child.WaitForExit();
                }
            }
        }
    }
}
"#;
const PROJECTATLAS_SKILL_DIR: &str = "skills";

const PROJECTATLAS_SKILL_NAME: &str = "projectatlas";

const SKILL_FILE_NAME: &str = "SKILL.md";

const MCP_CONTRACT_PLUGIN_ROOT_ENV: &str = "PROJECTATLAS_MCP_CONTRACT_PLUGIN_ROOT";

const MCP_TOOLS_SHA256: &str = "c364a97710088181c61ebf3ba57573fae5cf26b0eb21fe12f49d956a18ad6fcd";

const WRONG_PROJECT_OWNER_DIR_NAME: &str = "wrong-owner";

#[cfg(windows)]
const PROJECTATLAS_LOCAL_APPDATA_DIR: &str = "ProjectAtlas";

const CLI_E2E_INVENTORY_FILE: &str = "docs/v050-cli-e2e-inventory.json";

const CLI_E2E_INVENTORY_NORMALIZATION: &str = "UTF-8 source with CRLF and CR normalized to LF; relative binary keys only; no absolute paths or line metadata";

const CLI_E2E_SUPPORT_PATH: &str = "crates/projectatlas-cli/tests/support/mod.rs";

const RELEASE_WORKFLOW_PATH: &str = ".github/workflows/release.yml";

const CLI_E2E_BASELINE_COMMIT: &str = "b8f368c0f1e2299b7d0cbb0c3646bb4c238dbceb";

const CLI_E2E_BASELINE_SOURCE: &str = "crates/projectatlas-cli/tests/e2e.rs";

const CLI_E2E_SOURCE_SHA256_BASELINE: &str =
    "e26c7b9d450b105e09c2259b243f95a1fddb26cd8b64e176379149ca8050b43c";

const CLI_E2E_SOURCE_SHA256_BEFORE_DELETION: &str =
    "942b802ab4c215f1742d2c41f35eb29654946da8e8372218e0d1a787cc3c4757";

const CLI_E2E_SYMBOL_COUNT: usize = 431;

const CLI_E2E_FIXTURE_COUNT: usize = 91;

const CLI_E2E_SELECTOR_BEFORE_MOVE_COUNT: usize = 52;

const CLI_E2E_ALL_OWNERS: &[&str] = &[
    "e2e_delivery",
    "e2e_lifecycle",
    "e2e_maintenance",
    "e2e_navigation",
    "e2e_worktrees",
];

const CLI_E2E_SUPPORT_ALL_OWNERS: &[&str] = CLI_E2E_ALL_OWNERS;

const CLI_E2E_SUPPORT_WORKSPACE_ROOT_OWNERS: &[&str] =
    &["e2e_delivery", "e2e_lifecycle", "e2e_maintenance"];

const CLI_E2E_SUPPORT_USIZE_AT_LEAST_OWNERS: &[&str] = &[
    "e2e_lifecycle",
    "e2e_delivery",
    "e2e_navigation",
    "e2e_worktrees",
];

const CLI_E2E_SUPPORT_USIZE_GREATER_THAN_OWNERS: &[&str] = &["e2e_delivery", "e2e_navigation"];

const CLI_E2E_SUPPORT_COMMUNITY_OWNERS: &[&str] = &["e2e_delivery", "e2e_navigation"];

const CLI_E2E_SYMBOLS_DIGEST: &str =
    "70d2e0e9d05f0044304d8e2a198650cee0cb25c399642fdbb41fb8d613cdb661";

const CLI_E2E_FIXTURES_DIGEST: &str =
    "0dd300d503e6f82b6824bff69ac8ae954eac90a6bfb4e52d5ecbb6b3fd9ab61e";

const CLI_E2E_ENVIRONMENT_FACETS_DIGEST: &str =
    "addd2e61ae70c2a11c1acb53abfb49d8dff160d0eda87b1e48164270a0c651a4";

const CLI_E2E_TIMEOUT_FACETS_DIGEST: &str =
    "e218dd8e0d97b946adf22f6ad9a3e702039a3858d8cb7d7bda59eb8d35e003a4";

const CLI_E2E_CLEANUP_FACETS_DIGEST: &str =
    "d84ba7d169f1c4ea581aa986edf4466993e81b28a873269a4d559dea6612849e";

const CLI_E2E_ISOLATION_FACETS_DIGEST: &str =
    "a34f40d2d7249dafba32af229a0a068c7ab0ab6c2ffd5128c90c8d65a8c834bf";

const CLI_E2E_PACKAGED_FACETS_DIGEST: &str =
    "0a2d634dad641fa3dded1fcb23fc74abc8ff27bc9027f1bf0e3659f542709a4c";

const CLI_E2E_ATTRIBUTES_FACETS_DIGEST: &str =
    "cdd9b72f5c1ec65285a955a0383a33b7fcac509e38d1889022cb108f41f5423d";

const CLI_E2E_SELECTORS_BEFORE_MOVE_DIGEST: &str =
    "cc3a43c320d863ce3f42e42488959b8e2195b28504835289174c7544f1869689";

const CLI_E2E_SUPPORT_SHA256: &str =
    "fd0333474bc67c4af22f023c4d78cc6478421d15e99223d72ed3a871c4f41fa0";
const CLI_E2E_INVENTORY_LIST_SEPARATOR: &str = "\u{1d}";

const CLI_E2E_SOURCE_PATHS: &[&str] = &[
    "crates/projectatlas-cli/tests/e2e_delivery.rs",
    "crates/projectatlas-cli/tests/e2e_lifecycle.rs",
    "crates/projectatlas-cli/tests/e2e_maintenance.rs",
    "crates/projectatlas-cli/tests/e2e_navigation.rs",
    "crates/projectatlas-cli/tests/e2e_worktrees.rs",
];

const CLI_E2E_WORKFLOW_PATHS: &[&str] = &[
    ".github/workflows/ci.yml",
    RELEASE_WORKFLOW_PATH,
    ".github/workflows/optional-parser-pack.yml",
];

const CLI_E2E_SELECTOR_PREFIXES: &[&str] = &["--test e2e", "--test=e2e"];

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
    WorktreeRegistryAdvance,
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
struct SqliteCompatibilitySnapshot {
    database_bytes: Vec<u8>,
    wal_bytes: Option<Vec<u8>>,
    sidecars: BTreeSet<String>,
    schema_objects: Vec<String>,
    tables: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventory {
    schema_version: u64,
    baseline_commit: String,
    source: String,
    source_sha256_baseline: String,
    source_sha256_before_deletion: String,
    test_count: usize,
    symbol_count: usize,
    binaries: BTreeMap<String, String>,
    enforcement_tests: Vec<String>,
    tests: Vec<CliE2eInventoryTest>,
    symbols: Vec<CliE2eInventorySymbol>,
    fixtures: Vec<CliE2eInventoryFixture>,
    contract_facets: CliE2eContractFacets,
    selectors_before_move: Vec<CliE2eInventorySelectorBeforeMove>,
    selectors_after_move: Vec<CliE2eInventorySelector>,
    source_contract: CliE2eSourceContract,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventoryTest {
    name: String,
    owner: String,
    attributes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventorySymbol {
    kind: String,
    name: String,
    owners: Vec<String>,
    attributes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventoryFixture {
    name: String,
    owners: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventoryFacetLine {
    line: usize,
    text: String,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventoryFacetAttributes {
    test: String,
    line: usize,
    attributes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CliE2eContractFacets {
    environment_mutations: Vec<CliE2eInventoryFacetLine>,
    attributes_and_platform_gates: Vec<CliE2eInventoryFacetAttributes>,
    timeouts_and_deadlines: Vec<CliE2eInventoryFacetLine>,
    cleanup_and_process_ownership: Vec<CliE2eInventoryFacetLine>,
    process_isolation_and_fixtures: Vec<CliE2eInventoryFacetLine>,
    packaged_product_routes: Vec<CliE2eInventoryFacetLine>,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventorySelector {
    workflow: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct CliE2eInventorySelectorBeforeMove {
    workflow: String,
    line: usize,
    text: String,
}

#[derive(Debug, Deserialize)]
struct CliE2eSourceContract {
    normalization: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ObservedCliE2eTest {
    owner: String,
    attributes: Vec<String>,
}

#[derive(Debug)]
struct ObservedCliE2eSymbol {
    kind: String,
    name: String,
    attributes: Vec<String>,
}

fn scan_cli_e2e_symbols(source: &str) -> Vec<ObservedCliE2eSymbol> {
    let mut pending_attributes = Vec::new();
    let mut symbols = Vec::new();
    for line in normalize_cli_e2e_text(source).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") || trimmed.starts_with("///") {
            pending_attributes.push(trimmed.to_owned());
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let declaration = loop {
            let Some(token) = tokens.next() else {
                break None;
            };
            if matches!(
                token,
                "const" | "static" | "fn" | "struct" | "enum" | "type" | "trait"
            ) {
                break Some(token);
            }
        };
        if let Some(kind) = declaration
            && let Some(name) = tokens.next().and_then(|token| {
                token
                    .trim_start_matches("r#")
                    .split(['(', '<', ':', '=', '{', ';'])
                    .next()
                    .filter(|name| !name.is_empty())
            })
        {
            symbols.push(ObservedCliE2eSymbol {
                kind: if kind == "const" || kind == "static" {
                    "value".to_owned()
                } else if kind == "fn" {
                    "function".to_owned()
                } else {
                    kind.to_owned()
                },
                name: name.to_owned(),
                attributes: pending_attributes.clone(),
            });
        }
        if !trimmed.is_empty() {
            pending_attributes.clear();
        }
    }
    symbols
}

fn inventory_symbol_digest(symbols: &[CliE2eInventorySymbol]) -> String {
    let mut canonical = String::new();
    for symbol in symbols {
        canonical.push_str(&symbol.kind);
        canonical.push('\u{1f}');
        canonical.push_str(&symbol.name);
        canonical.push('\u{1f}');
        canonical.push_str(&symbol.owners.join(CLI_E2E_INVENTORY_LIST_SEPARATOR));
        canonical.push('\u{1f}');
        canonical.push_str(&symbol.attributes.join(CLI_E2E_INVENTORY_LIST_SEPARATOR));
        canonical.push('\u{1e}');
    }
    sha256_text(&canonical)
}

fn inventory_fixture_digest(fixtures: &[CliE2eInventoryFixture]) -> String {
    let mut canonical = String::new();
    for fixture in fixtures {
        canonical.push_str(&fixture.name);
        canonical.push('\u{1f}');
        canonical.push_str(&fixture.owners.join(CLI_E2E_INVENTORY_LIST_SEPARATOR));
        canonical.push('\u{1e}');
    }
    sha256_text(&canonical)
}

fn inventory_facet_lines_digest(lines: &[CliE2eInventoryFacetLine]) -> String {
    let mut canonical = String::new();
    for line in lines {
        canonical.push_str(&line.line.to_string());
        canonical.push('\u{1f}');
        canonical.push_str(&line.text);
        canonical.push('\u{1e}');
    }
    sha256_text(&canonical)
}

fn inventory_attribute_facets_digest(facets: &[CliE2eInventoryFacetAttributes]) -> String {
    let mut canonical = String::new();
    for facet in facets {
        canonical.push_str(&facet.test);
        canonical.push('\u{1f}');
        canonical.push_str(&facet.line.to_string());
        canonical.push('\u{1f}');
        canonical.push_str(&facet.attributes.join(CLI_E2E_INVENTORY_LIST_SEPARATOR));
        canonical.push('\u{1e}');
    }
    sha256_text(&canonical)
}

fn inventory_selector_before_move_digest(
    selectors: &[CliE2eInventorySelectorBeforeMove],
) -> String {
    let mut canonical = String::new();
    for selector in selectors {
        canonical.push_str(&selector.workflow);
        canonical.push('\u{1f}');
        canonical.push_str(&selector.line.to_string());
        canonical.push('\u{1f}');
        canonical.push_str(&selector.text);
        canonical.push('\u{1e}');
    }
    sha256_text(&canonical)
}

fn cli_e2e_support_owners(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "workspace_root" => Some(CLI_E2E_SUPPORT_WORKSPACE_ROOT_OWNERS),
        "require_json_usize_at_least" => Some(CLI_E2E_SUPPORT_USIZE_AT_LEAST_OWNERS),
        "require_json_usize_greater_than" => Some(CLI_E2E_SUPPORT_USIZE_GREATER_THAN_OWNERS),
        "json_community_values" | "assert_planted_community_values" => {
            Some(CLI_E2E_SUPPORT_COMMUNITY_OWNERS)
        }
        "McpDatabaseSnapshot"
        | "MCP_CONTRACT_EXECUTABLE_ENV"
        | "MCP_CONTRACT_METADATA_CANARY"
        | "GIT_REPOSITORY_ENVIRONMENT_VARIABLES"
        | "run_mcp_stdio"
        | "run_mcp_stdio_with_env"
        | "mcp_tool_text"
        | "sqlite_table_digests"
        | "mcp_database_snapshot"
        | "require_json_string"
        | "require_json_contains"
        | "require_json_usize"
        | "require_json_array_len"
        | "require_json_bool"
        | "sha256_hex"
        | "json_at"
        | "mcp_contract_executable"
        | "json_summary_command"
        | "git_command_for_root"
        | "complete_mcp_test_after_shutdown" => Some(CLI_E2E_SUPPORT_ALL_OWNERS),
        _ => None,
    }
}

/// Enforce the frozen CLI E2E ownership contract against current split sources.
fn assert_cli_e2e_inventory_contract(workspace_root: &Path) -> Result<(), Box<dyn Error>> {
    let inventory_path = workspace_root.join(CLI_E2E_INVENTORY_FILE);
    let inventory: CliE2eInventory = serde_json::from_str(&fs::read_to_string(&inventory_path)?)?;
    if inventory.schema_version != 2 {
        return Err(io::Error::other(format!(
            "CLI E2E inventory schema must be 2, found {}",
            inventory.schema_version
        ))
        .into());
    }
    if inventory.baseline_commit != CLI_E2E_BASELINE_COMMIT {
        return Err(io::Error::other("CLI E2E baseline commit identity drifted").into());
    }
    if inventory.source != CLI_E2E_BASELINE_SOURCE {
        return Err(io::Error::other("CLI E2E baseline source identity drifted").into());
    }
    if inventory.source_sha256_baseline != CLI_E2E_SOURCE_SHA256_BASELINE {
        return Err(io::Error::other("CLI E2E baseline source digest drifted").into());
    }
    if inventory.source_sha256_before_deletion != CLI_E2E_SOURCE_SHA256_BEFORE_DELETION {
        return Err(io::Error::other("CLI E2E pre-deletion source digest drifted").into());
    }
    if inventory.source_contract.normalization != CLI_E2E_INVENTORY_NORMALIZATION {
        return Err(io::Error::other("CLI E2E inventory normalization contract drifted").into());
    }
    if inventory.symbol_count != CLI_E2E_SYMBOL_COUNT
        || inventory.symbols.len() != CLI_E2E_SYMBOL_COUNT
    {
        return Err(io::Error::other(format!(
            "CLI E2E inventory symbol coverage drifted: expected {CLI_E2E_SYMBOL_COUNT}, found {}",
            inventory.symbols.len()
        ))
        .into());
    }
    if inventory_fixture_digest(&inventory.fixtures) != CLI_E2E_FIXTURES_DIGEST {
        return Err(io::Error::other("CLI E2E inventory fixture identity drifted").into());
    }
    if inventory_symbol_digest(&inventory.symbols) != CLI_E2E_SYMBOLS_DIGEST {
        return Err(io::Error::other("CLI E2E inventory symbol identity drifted").into());
    }
    if inventory.fixtures.len() != CLI_E2E_FIXTURE_COUNT {
        return Err(io::Error::other(format!(
            "CLI E2E inventory fixture coverage drifted: expected {CLI_E2E_FIXTURE_COUNT}, found {}",
            inventory.fixtures.len()
        ))
        .into());
    }
    if inventory.selectors_before_move.len() != CLI_E2E_SELECTOR_BEFORE_MOVE_COUNT {
        return Err(io::Error::other(format!(
            "CLI E2E pre-move selector coverage drifted: expected {CLI_E2E_SELECTOR_BEFORE_MOVE_COUNT}, found {}",
            inventory.selectors_before_move.len()
        ))
        .into());
    }
    if inventory_selector_before_move_digest(&inventory.selectors_before_move)
        != CLI_E2E_SELECTORS_BEFORE_MOVE_DIGEST
    {
        return Err(io::Error::other("CLI E2E pre-move selector identity drifted").into());
    }
    if inventory.symbol_count == 0 {
        return Err(
            io::Error::other("CLI E2E inventory must retain top-level support coverage").into(),
        );
    }
    let enforcement_tests = inventory
        .enforcement_tests
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if enforcement_tests.len() != inventory.enforcement_tests.len() {
        return Err(io::Error::other(
            "CLI E2E inventory contains duplicate enforcement test names",
        )
        .into());
    }

    let expected_files = CLI_E2E_SOURCE_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    let discovered_files = discover_cli_e2e_source_paths(workspace_root)?;
    if discovered_files != expected_files {
        return Err(io::Error::other(format!(
            "CLI E2E integration binaries drifted: expected {expected_files:?}, found {discovered_files:?}"
        ))
        .into());
    }
    let binary_files = inventory
        .binaries
        .keys()
        .map(|owner| format!("crates/projectatlas-cli/tests/{owner}.rs"))
        .collect::<BTreeSet<_>>();
    if binary_files != expected_files {
        return Err(io::Error::other(format!(
            "CLI E2E binary ownership map drifted: expected {expected_files:?}, found {binary_files:?}"
        ))
        .into());
    }
    let recorded_files = inventory
        .source_contract
        .files
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if recorded_files != expected_files {
        return Err(io::Error::other(format!(
            "CLI E2E source contract files drifted: expected {expected_files:?}, found {recorded_files:?}"
        ))
        .into());
    }
    let frozen_source_contract = CLI_E2E_SOURCE_SHA256
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    if frozen_source_contract
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != CLI_E2E_SOURCE_PATHS.iter().copied().collect()
    {
        return Err(io::Error::other("frozen CLI E2E source files drifted").into());
    }
    let facet_counts = [
        (
            "environment_mutations",
            inventory.contract_facets.environment_mutations.len(),
            416,
        ),
        (
            "attributes_and_platform_gates",
            inventory
                .contract_facets
                .attributes_and_platform_gates
                .len(),
            158,
        ),
        (
            "timeouts_and_deadlines",
            inventory.contract_facets.timeouts_and_deadlines.len(),
            184,
        ),
        (
            "cleanup_and_process_ownership",
            inventory
                .contract_facets
                .cleanup_and_process_ownership
                .len(),
            313,
        ),
        (
            "process_isolation_and_fixtures",
            inventory
                .contract_facets
                .process_isolation_and_fixtures
                .len(),
            1_029,
        ),
        (
            "packaged_product_routes",
            inventory.contract_facets.packaged_product_routes.len(),
            651,
        ),
    ];
    for (name, observed, expected) in facet_counts {
        if observed != expected {
            return Err(io::Error::other(format!(
                "CLI E2E {name} facet coverage drifted: expected {expected}, found {observed}"
            ))
            .into());
        }
    }
    let facet_digests = [
        (
            "environment_mutations",
            inventory_facet_lines_digest(&inventory.contract_facets.environment_mutations),
            CLI_E2E_ENVIRONMENT_FACETS_DIGEST,
        ),
        (
            "timeouts_and_deadlines",
            inventory_facet_lines_digest(&inventory.contract_facets.timeouts_and_deadlines),
            CLI_E2E_TIMEOUT_FACETS_DIGEST,
        ),
        (
            "cleanup_and_process_ownership",
            inventory_facet_lines_digest(&inventory.contract_facets.cleanup_and_process_ownership),
            CLI_E2E_CLEANUP_FACETS_DIGEST,
        ),
        (
            "process_isolation_and_fixtures",
            inventory_facet_lines_digest(&inventory.contract_facets.process_isolation_and_fixtures),
            CLI_E2E_ISOLATION_FACETS_DIGEST,
        ),
        (
            "packaged_product_routes",
            inventory_facet_lines_digest(&inventory.contract_facets.packaged_product_routes),
            CLI_E2E_PACKAGED_FACETS_DIGEST,
        ),
    ];
    for (name, observed, expected) in facet_digests {
        if observed != expected {
            return Err(io::Error::other(format!("CLI E2E {name} facet identity drifted")).into());
        }
    }
    if inventory_attribute_facets_digest(&inventory.contract_facets.attributes_and_platform_gates)
        != CLI_E2E_ATTRIBUTES_FACETS_DIGEST
    {
        return Err(io::Error::other("CLI E2E attribute/platform facet identity drifted").into());
    }

    let mut expected_tests = BTreeMap::new();
    for test in &inventory.tests {
        if expected_tests
            .insert(
                test.name.clone(),
                (test.owner.clone(), test.attributes.clone()),
            )
            .is_some()
        {
            return Err(io::Error::other(format!(
                "CLI E2E inventory contains duplicate test name {:?}",
                test.name
            ))
            .into());
        }
    }
    if expected_tests.len() != inventory.test_count {
        return Err(io::Error::other(format!(
            "CLI E2E inventory test_count is {}, but it records {} names",
            inventory.test_count,
            expected_tests.len()
        ))
        .into());
    }

    let mut observed_tests = BTreeMap::new();
    let mut observed_enforcement_tests = BTreeSet::new();
    for relative_path in CLI_E2E_SOURCE_PATHS {
        let source = fs::read_to_string(workspace_root.join(relative_path))?;
        let expected_digest = inventory
            .source_contract
            .files
            .get(*relative_path)
            .ok_or_else(|| {
                io::Error::other(format!("missing source digest for {relative_path}"))
            })?;
        let frozen_digest = frozen_source_contract.get(relative_path).ok_or_else(|| {
            io::Error::other(format!("missing frozen source digest for {relative_path}"))
        })?;
        if expected_digest != frozen_digest {
            return Err(io::Error::other(format!(
                "CLI E2E frozen source digest drifted for {relative_path}"
            ))
            .into());
        }
        let observed_digest = sha256_text(&normalize_cli_e2e_text(&source));
        if observed_digest != *expected_digest {
            return Err(io::Error::other(format!(
                "CLI E2E source-content/assertion/facet digest drift in {relative_path}: expected {expected_digest}, found {observed_digest}"
            ))
            .into());
        }
        let owner = relative_path
            .strip_prefix("crates/projectatlas-cli/tests/")
            .and_then(|path| path.strip_suffix(".rs"))
            .ok_or_else(|| {
                io::Error::other(format!("invalid CLI E2E source path {relative_path}"))
            })?;
        for test in scan_cli_e2e_tests(&source) {
            if enforcement_tests.contains(&test.name) {
                if !observed_enforcement_tests.insert(test.name.clone()) {
                    return Err(io::Error::other(format!(
                        "CLI E2E enforcement test is duplicated: {}",
                        test.name
                    ))
                    .into());
                }
                continue;
            }
            if observed_tests
                .insert(
                    test.name.clone(),
                    ObservedCliE2eTest {
                        owner: owner.to_owned(),
                        attributes: test.attributes,
                    },
                )
                .is_some()
            {
                return Err(io::Error::other(format!(
                    "CLI E2E split sources contain duplicate test name {:?}",
                    test.name
                ))
                .into());
            }
        }
    }
    if observed_enforcement_tests != enforcement_tests {
        return Err(io::Error::other(format!(
            "CLI E2E enforcement test selection drifted: expected {enforcement_tests:?}, found {observed_enforcement_tests:?}"
        ))
        .into());
    }

    if observed_tests.len() != expected_tests.len() {
        return Err(io::Error::other(format!(
            "CLI E2E test count drifted: expected {}, found {}",
            expected_tests.len(),
            observed_tests.len()
        ))
        .into());
    }
    for name in expected_tests.keys() {
        if !observed_tests.contains_key(name) {
            return Err(io::Error::other(format!(
                "CLI E2E test is missing from split sources: {name}"
            ))
            .into());
        }
    }
    for name in observed_tests.keys() {
        if !expected_tests.contains_key(name) {
            return Err(io::Error::other(format!(
                "CLI E2E split source contains unrecorded test: {name}"
            ))
            .into());
        }
    }
    for (name, (expected_owner, expected_attributes)) in expected_tests {
        let observed = observed_tests
            .get(&name)
            .ok_or_else(|| io::Error::other(format!("missing observed test {name}")))?;
        if observed.owner != expected_owner {
            return Err(io::Error::other(format!(
                "CLI E2E test owner drift for {name}: expected {expected_owner}, found {}",
                observed.owner
            ))
            .into());
        }
        if observed.attributes != expected_attributes {
            return Err(io::Error::other(format!(
                "CLI E2E attribute/platform facet drift for {name}: expected {expected_attributes:?}, found {:?}",
                observed.attributes
            ))
            .into());
        }
    }

    let mut observed_symbols_by_owner = BTreeMap::<String, Vec<ObservedCliE2eSymbol>>::new();
    let mut observed_source_lines = BTreeSet::new();
    for relative_path in CLI_E2E_SOURCE_PATHS
        .iter()
        .copied()
        .chain(std::iter::once(CLI_E2E_SUPPORT_PATH))
    {
        let source =
            normalize_cli_e2e_text(&fs::read_to_string(workspace_root.join(relative_path))?);
        for line in source.lines() {
            observed_source_lines.insert(line.to_owned());
            if let Some(unqualified) = line.strip_prefix("pub(super) ") {
                observed_source_lines.insert(unqualified.to_owned());
            }
        }
        let owner = relative_path
            .strip_prefix("crates/projectatlas-cli/tests/")
            .and_then(|path| path.strip_suffix(".rs"))
            .unwrap_or(relative_path)
            .to_owned();
        observed_symbols_by_owner.insert(owner, scan_cli_e2e_symbols(&source));
    }
    let observed_support_symbols = observed_symbols_by_owner
        .get("support/mod")
        .ok_or_else(|| io::Error::other("CLI E2E shared support source is missing"))?;
    let support_source = normalize_cli_e2e_text(&fs::read_to_string(
        workspace_root.join(CLI_E2E_SUPPORT_PATH),
    )?);
    if sha256_text(&support_source) != CLI_E2E_SUPPORT_SHA256 {
        return Err(io::Error::other("CLI E2E shared support source digest drifted").into());
    }
    let observed_binary_symbols = observed_symbols_by_owner
        .iter()
        .filter(|(owner, _)| owner.as_str() != "support/mod");
    for symbol in &inventory.symbols {
        let matching_support = observed_support_symbols.iter().any(|observed| {
            observed.kind == symbol.kind
                && observed.name == symbol.name
                && observed.attributes == symbol.attributes
        });
        let mut observed_owners = BTreeSet::<String>::new();
        for (owner, symbols) in observed_binary_symbols.clone() {
            if symbols
                .iter()
                .any(|observed| observed.kind == symbol.kind && observed.name == symbol.name)
            {
                observed_owners.insert(owner.clone());
            }
        }
        let expected_owners = cli_e2e_support_owners(&symbol.name)
            .map(|owners| {
                owners
                    .iter()
                    .map(|owner| (*owner).to_owned())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or(observed_owners.clone());
        if !matching_support
            && cli_e2e_support_owners(&symbol.name).is_some()
            && observed_owners.is_empty()
        {
            return Err(io::Error::other(format!(
                "CLI E2E shared support symbol is missing: {}",
                symbol.name
            ))
            .into());
        }
        if observed_owners.is_empty() && !matching_support {
            return Err(io::Error::other(format!(
                "CLI E2E inventory symbol is missing from current sources: {}",
                symbol.name
            ))
            .into());
        }
        if symbol.owners.iter().cloned().collect::<BTreeSet<_>>() != expected_owners {
            return Err(io::Error::other(format!(
                "CLI E2E symbol ownership drift for {}",
                symbol.name
            ))
            .into());
        }
    }
    let mut observed_fixture_owners = BTreeMap::<String, BTreeSet<String>>::new();
    for (owner, symbols) in observed_symbols_by_owner
        .iter()
        .filter(|(owner, _)| owner.as_str() != "support/mod")
    {
        for symbol in symbols.iter().filter(|symbol| symbol.kind == "value") {
            observed_fixture_owners
                .entry(symbol.name.clone())
                .or_default()
                .insert(owner.clone());
        }
    }
    let mut recorded_fixture_names = BTreeSet::new();
    for fixture in &inventory.fixtures {
        if !recorded_fixture_names.insert(&fixture.name) {
            return Err(io::Error::other(format!(
                "CLI E2E inventory contains duplicate fixture {}",
                fixture.name
            ))
            .into());
        }
        let support_owners = cli_e2e_support_owners(&fixture.name);
        let expected_owners = if let Some(owners) = support_owners {
            owners
                .iter()
                .map(|owner| (*owner).to_owned())
                .collect::<BTreeSet<_>>()
        } else {
            observed_fixture_owners
                .get(&fixture.name)
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "CLI E2E inventory fixture is missing from current sources: {}",
                        fixture.name
                    ))
                })?
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        if fixture.owners.iter().cloned().collect::<BTreeSet<_>>() != expected_owners {
            return Err(io::Error::other(format!(
                "CLI E2E fixture ownership drift for {}",
                fixture.name
            ))
            .into());
        }
    }

    for (name, facets) in [
        (
            "environment_mutations",
            &inventory.contract_facets.environment_mutations,
        ),
        (
            "timeouts_and_deadlines",
            &inventory.contract_facets.timeouts_and_deadlines,
        ),
        (
            "cleanup_and_process_ownership",
            &inventory.contract_facets.cleanup_and_process_ownership,
        ),
        (
            "process_isolation_and_fixtures",
            &inventory.contract_facets.process_isolation_and_fixtures,
        ),
        (
            "packaged_product_routes",
            &inventory.contract_facets.packaged_product_routes,
        ),
    ] {
        for facet in facets {
            if !observed_source_lines.contains(&facet.text) {
                return Err(io::Error::other(format!(
                    "CLI E2E {name} facet line disappeared: {}",
                    facet.text.trim()
                ))
                .into());
            }
        }
    }
    let observed_attributes = inventory
        .tests
        .iter()
        .map(|test| (test.name.as_str(), test.attributes.as_slice()))
        .collect::<BTreeMap<_, _>>();
    for facet in &inventory.contract_facets.attributes_and_platform_gates {
        if observed_attributes.get(facet.test.as_str()) != Some(&facet.attributes.as_slice()) {
            return Err(io::Error::other(format!(
                "CLI E2E attribute/platform facet drift for {}",
                facet.test
            ))
            .into());
        }
    }

    let expected_selectors = inventory
        .selectors_after_move
        .iter()
        .map(|selector| {
            (
                selector.workflow.replace('\\', "/"),
                selector.text.trim().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let mut workflow_paths = Vec::new();
    for (workflow, _) in &expected_selectors {
        if !workflow_paths.iter().any(|path| path == workflow) {
            workflow_paths.push(workflow.clone());
        }
    }
    let mut observed_selectors = Vec::new();
    for workflow in workflow_paths {
        let workflow_path = workspace_root.join(&workflow);
        let workflow_text = fs::read_to_string(&workflow_path)?;
        for line in normalize_cli_e2e_text(&workflow_text).lines() {
            if is_cli_e2e_selector(line) {
                observed_selectors.push((workflow.clone(), line.trim().to_owned()));
            }
        }
    }
    if observed_selectors != expected_selectors {
        return Err(io::Error::other(format!(
            "CLI E2E workflow selector drift: expected {} selectors, found {}",
            expected_selectors.len(),
            observed_selectors.len()
        ))
        .into());
    }
    let release_workflow = fs::read_to_string(workspace_root.join(RELEASE_WORKFLOW_PATH))?;
    if workflow_job_block(&release_workflow, "package-unix")?.contains(r"\${") {
        return Err(io::Error::other(
            "Unix packaged contract runner must not escape Bash parameter expansion",
        )
        .into());
    }
    Ok(())
}

#[derive(Debug)]
struct ObservedCliE2eTestDeclaration {
    name: String,
    attributes: Vec<String>,
}

/// Scan only test attributes and declarations in the five known integration files.
fn scan_cli_e2e_tests(source: &str) -> Vec<ObservedCliE2eTestDeclaration> {
    let mut pending_attributes = Vec::new();
    let mut tests = Vec::new();
    for line in normalize_cli_e2e_text(source).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") && trimmed.ends_with(']') {
            pending_attributes.push(trimmed.to_owned());
            continue;
        }
        if let Some(name) = trimmed
            .strip_prefix("fn ")
            .and_then(|declaration| declaration.split(['(', '<', ' ', '\t']).next())
            .filter(|name| !name.is_empty())
            && pending_attributes
                .iter()
                .any(|attribute| attribute == "#[test]")
        {
            tests.push(ObservedCliE2eTestDeclaration {
                name: name.to_owned(),
                attributes: pending_attributes.clone(),
            });
        }
        pending_attributes.clear();
    }
    tests
}

fn discover_cli_e2e_source_paths(
    workspace_root: &Path,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let tests_dir = workspace_root.join("crates/projectatlas-cli/tests");
    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(tests_dir)? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            && (file_name == "e2e.rs" || file_name.starts_with("e2e_"))
        {
            paths.insert(format!("crates/projectatlas-cli/tests/{file_name}"));
        }
    }
    Ok(paths)
}

fn is_cli_e2e_selector(line: &str) -> bool {
    CLI_E2E_SELECTOR_PREFIXES.iter().any(|prefix| {
        line.match_indices(prefix).any(|(index, _)| {
            let suffix = &line[index + prefix.len()..];
            suffix
                .chars()
                .next()
                .is_none_or(|character| character == '_' || character.is_whitespace())
        })
    })
}

fn normalize_cli_e2e_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn sha256_text(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn copy_cli_e2e_contract_fixture(
    workspace_root: &Path,
    fixture_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut paths = CLI_E2E_SOURCE_PATHS.to_vec();
    paths.push(CLI_E2E_SUPPORT_PATH);
    paths.extend_from_slice(CLI_E2E_WORKFLOW_PATHS);
    paths.push(CLI_E2E_INVENTORY_FILE);
    for relative_path in paths {
        let destination = fixture_root.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(workspace_root.join(relative_path), destination)?;
    }
    Ok(())
}

fn tampered_cli_e2e_inventory_fixture(
    workspace_root: &Path,
    mutate: impl FnOnce(&mut Value),
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let fixture = tempfile::tempdir()?;
    copy_cli_e2e_contract_fixture(workspace_root, fixture.path())?;
    let inventory_path = fixture.path().join(CLI_E2E_INVENTORY_FILE);
    let mut inventory: Value = serde_json::from_str(&fs::read_to_string(&inventory_path)?)?;
    mutate(&mut inventory);
    fs::write(inventory_path, serde_json::to_string_pretty(&inventory)?)?;
    Ok(fixture)
}

fn require_cli_e2e_contract_rejection(
    result: Result<(), Box<dyn Error>>,
    expected_fragment: &str,
) -> Result<(), Box<dyn Error>> {
    let error = result
        .err()
        .ok_or_else(|| io::Error::other("CLI E2E drift fixture unexpectedly passed"))?;
    if error.to_string().contains(expected_fragment) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "CLI E2E drift fixture returned an unexpected error: {error}"
        ))
        .into())
    }
}

#[test]
fn cli_e2e_inventory_contract_matches_split_sources_and_workflows() -> Result<(), Box<dyn Error>> {
    assert_cli_e2e_inventory_contract(&workspace_root()?)
}

#[test]
fn cli_e2e_inventory_contract_rejects_source_and_selector_drift() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let fixture = tempfile::tempdir()?;
    copy_cli_e2e_contract_fixture(&workspace_root, fixture.path())?;
    assert_cli_e2e_inventory_contract(fixture.path())?;

    let shell_fixture = tempfile::tempdir()?;
    copy_cli_e2e_contract_fixture(&workspace_root, shell_fixture.path())?;
    let release_path = shell_fixture.path().join(RELEASE_WORKFLOW_PATH);
    let release_source = fs::read_to_string(&release_path)?;
    let escaped_source = release_source.replacen("${binary_name}", r"\${binary_name}", 1);
    if escaped_source == release_source {
        return Err(io::Error::other("Unix wrapper tamper fixture did not change workflow").into());
    }
    fs::write(release_path, escaped_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(shell_fixture.path()),
        "must not escape Bash parameter expansion",
    )?;

    let legacy_path = fixture.path().join("crates/projectatlas-cli/tests/e2e.rs");
    fs::write(&legacy_path, "#[test]\nfn legacy_monolith() {}\n")?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "integration binaries drifted",
    )?;
    fs::remove_file(&legacy_path)?;

    let delivery_path = fixture.path().join(CLI_E2E_SOURCE_PATHS[0]);
    let delivery_source = fs::read_to_string(&delivery_path)?;
    let weakened_source = delivery_source.replacen("assert!(", "assert!(false && ", 1);
    if weakened_source == delivery_source {
        return Err(io::Error::other("assertion tamper fixture did not change source").into());
    }
    fs::write(&delivery_path, &weakened_source)?;
    let inventory_path = fixture.path().join(CLI_E2E_INVENTORY_FILE);
    let mut inventory: Value = serde_json::from_str(&fs::read_to_string(&inventory_path)?)?;
    inventory["source_contract"]["files"][CLI_E2E_SOURCE_PATHS[0]] =
        json!(sha256_text(&normalize_cli_e2e_text(&weakened_source)));
    fs::write(&inventory_path, serde_json::to_string_pretty(&inventory)?)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "frozen source digest drift",
    )?;
    fs::write(&delivery_path, &delivery_source)?;
    fs::copy(workspace_root.join(CLI_E2E_INVENTORY_FILE), &inventory_path)?;

    let facet_source = delivery_source.replacen(
        ".env(\"PROJECTATLAS_NO_TELEMETRY\", \"1\")",
        ".env(\"PROJECTATLAS_DRIFTED_FACET\", \"1\")",
        1,
    );
    if facet_source == delivery_source {
        return Err(io::Error::other("facet tamper fixture did not change source").into());
    }
    fs::write(&delivery_path, facet_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "source-content/assertion/facet digest drift",
    )?;
    fs::write(&delivery_path, &delivery_source)?;

    let ci_path = fixture.path().join(CLI_E2E_WORKFLOW_PATHS[0]);
    let ci_source = fs::read_to_string(&ci_path)?;
    let selector_source = ci_source.replacen("--test e2e_worktrees", "--test e2e_lifecycle", 1);
    if selector_source == ci_source {
        return Err(io::Error::other("selector tamper fixture did not change workflow").into());
    }
    fs::write(&ci_path, selector_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "workflow selector drift",
    )?;
    fs::write(&ci_path, &ci_source)?;

    let legacy_spaced_selector_source = format!("{ci_source}\n    cargo test --test e2e\n");
    fs::write(&ci_path, legacy_spaced_selector_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "workflow selector drift",
    )?;
    fs::write(&ci_path, &ci_source)?;

    let legacy_selector_source = format!("{ci_source}\n    cargo test --test=e2e\n");
    fs::write(&ci_path, legacy_selector_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "workflow selector drift",
    )?;
    fs::write(&ci_path, &ci_source)?;

    let unknown_selector_source = format!("{ci_source}\n    cargo test --test=e2e_extra\n");
    fs::write(&ci_path, unknown_selector_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "workflow selector drift",
    )?;
    Ok(())
}

#[test]
fn cli_e2e_inventory_contract_rejects_frozen_metadata_and_support_drift()
-> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;

    for (field, expected_fragment) in [
        ("symbols", "missing field `symbols`"),
        ("fixtures", "missing field `fixtures`"),
        ("contract_facets", "missing field `contract_facets`"),
        (
            "selectors_before_move",
            "missing field `selectors_before_move`",
        ),
    ] {
        let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
            if let Some(object) = inventory.as_object_mut() {
                object.remove(field);
            }
        })?;
        require_cli_e2e_contract_rejection(
            assert_cli_e2e_inventory_contract(fixture.path()),
            expected_fragment,
        )?;
    }

    let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
        inventory["symbols"][0]["name"] = json!("drifted_shared_helper");
    })?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "inventory symbol identity drifted",
    )?;

    let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
        inventory["fixtures"][0]["name"] = json!("drifted_fixture");
    })?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "inventory fixture identity drifted",
    )?;

    let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
        inventory["contract_facets"]["environment_mutations"][0]["text"] =
            json!("drifted environment mutation");
    })?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "environment_mutations facet identity drifted",
    )?;

    let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
        inventory["baseline_commit"] = json!("drifted_baseline");
    })?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "baseline commit identity drifted",
    )?;

    let fixture = tampered_cli_e2e_inventory_fixture(&workspace_root, |inventory| {
        if let Some(selectors) = inventory["selectors_before_move"].as_array_mut() {
            selectors.pop();
        }
    })?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "pre-move selector coverage drifted",
    )?;

    let fixture = tempfile::tempdir()?;
    copy_cli_e2e_contract_fixture(&workspace_root, fixture.path())?;
    let support_path = fixture.path().join(CLI_E2E_SUPPORT_PATH);
    let support_source = normalize_cli_e2e_text(&fs::read_to_string(&support_path)?);
    let drifted_source = support_source.replacen("    \"GIT_CONFIG\",\n", "", 1);
    if drifted_source == support_source {
        return Err(io::Error::other(
            "Git scrub-list tamper fixture did not change support source",
        )
        .into());
    }
    fs::write(support_path, drifted_source)?;
    let inventory_path = fixture.path().join(CLI_E2E_INVENTORY_FILE);
    let mut inventory: Value = serde_json::from_str(&fs::read_to_string(&inventory_path)?)?;
    let delivery_path = fixture.path().join(CLI_E2E_SOURCE_PATHS[0]);
    let delivery_source = fs::read_to_string(delivery_path)?;
    inventory["source_contract"]["files"][CLI_E2E_SOURCE_PATHS[0]] =
        json!(sha256_text(&normalize_cli_e2e_text(&delivery_source)));
    fs::write(inventory_path, serde_json::to_string_pretty(&inventory)?)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "shared support source digest drifted",
    )?;

    let fixture = tempfile::tempdir()?;
    copy_cli_e2e_contract_fixture(&workspace_root, fixture.path())?;
    let support_path = fixture.path().join(CLI_E2E_SUPPORT_PATH);
    let support_source = fs::read_to_string(&support_path)?;
    let missing_helper_source =
        support_source.replacen("pub(super) fn mcp_tool_text", "fn mcp_tool_text", 1);
    if missing_helper_source == support_source {
        return Err(io::Error::other("support helper tamper fixture did not change source").into());
    }
    fs::write(support_path, missing_helper_source)?;
    require_cli_e2e_contract_rejection(
        assert_cli_e2e_inventory_contract(fixture.path()),
        "shared support source digest drifted",
    )?;
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
    let agent_integration = fs::read_to_string(
        workspace_root
            .join("docs")
            .join(AGENT_INTEGRATION_DOC_FILE_NAME),
    )?;
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
        "Initialize-ProjectAtlasRuntimeProbe",
        "CreateSuspended",
        "ExtendedStartupInfoPresent",
        "ProcThreadAttributeHandleList",
        "JobObjectLimitKillOnJobClose",
        "InitializeProcThreadAttributeList",
        "UpdateProcThreadAttribute",
        "DeleteProcThreadAttributeList",
        "CreateJobObject",
        "SetInformationJobObject",
        "AssignProcessToJobObject",
        "ResumeThread",
        "TerminateJobObject",
        "RuntimeProbeProcess]::Start",
        r#"String.Equals(extension, ".ps1""#,
        r#"String.Equals(extension, ".cmd""#,
        r"Environment.SpecialFolder.System",
        "$runtime.version -eq $expectedRuntimeVersion",
        "Sync-ProjectAtlasRuntimeToLocalAppData",
        "ObsoleteMcpProcess",
        "CommandLineToArgvW",
        "Process.GetProcessById(processId)",
        "expectedCreationFileTimeUtc",
        "candidate.MainModule.FileName",
        "TerminateProcess(handle, 0)",
        "Find-ProjectAtlasObsoleteStableMcpProcess",
        "Get-CimInstance -ClassName Win32_Process",
        "ProjectAtlas Codex parent was created after the MCP child",
        "Invoke-ProjectAtlasObsoleteStableMcpHandoff",
        "ProjectAtlas convergence: update_state=",
        "obsolete_mcp_handoff=",
        "Get-ReleaseRuntimeInstallPath",
        r"ProjectAtlas\runtimes\$safeVersion\x86_64-pc-windows-msvc",
        "ProjectAtlas LocalAppData mirror is locked",
        "PROJECTATLAS_SKIP_USER_PATH_UPDATE",
        "Set-ProjectAtlasProcessPathPrecedence",
        "Test-ProjectAtlasBareCommandResolutionOnPath",
        "Test-ProjectAtlasPersistedBareCommandResolution",
        "Get-ProjectAtlasTokenLaunchArguments",
        r#"[Environment]::SetEnvironmentVariable("Path""#,
        r#"[Environment]::GetEnvironmentVariable("Path", "Machine")"#,
        "Test-ProjectAtlasPersistedBareCommandResolution $FilePath",
        "$futureProcessPathReady = Test-ProjectAtlasPersistedBareCommandResolution $projectAtlas",
        "Confirm-ProjectAtlasBareCommandResolution",
        "Active process resolves bare projectatlas to verified runtime",
        "Restart Codex or the shell",
        "$inheritedProjectAtlasCommand = Get-Command projectatlas",
        "$stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData",
        "$inheritedSynchronizedMirrorReady = $stableMirrorReady",
        "$futureProcessPathReady = Set-ProjectAtlasPathPrecedence",
        "$parentCliReady = $inheritedCommandReady -or $inheritedSynchronizedMirrorReady",
        "$hostRestartRequired = $verifiedRuntimeReady -and -not $parentCliReady -and $futureProcessPathReady",
        "ProjectAtlas readiness: runtime_ready=",
        "generated_mcp_configs_ready=",
        "runtime_mcp_configs_ready=",
        "installer_cli_ready=",
        "parent_cli_ready=",
        "host_restart_required=",
        "Existing host restart required:",
        "restart alone will not repair it",
        "first bare command for a fresh process",
        "Resolve-ProjectAtlasCodexCommand",
        "Update-ProjectAtlasCodexPlugin",
        "PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE",
        "Codex ProjectAtlas plugin marketplace updated",
        "Confirm-ProjectAtlasCodexSkillArtifact",
        "Codex ProjectAtlas plugin skill verified",
        "Codex does not expose the active in-process ProjectAtlas skill path",
        "plugin marketplace add styler-ai/ProjectAtlas --ref",
        "refs/tags/${releaseTag}:refs/tags/${releaseTag}",
        "Update-ProjectAtlasCodexMcpRegistry",
        "Test-ProjectAtlasCodexPluginReady",
        "Test-ProjectAtlasCodexMcpRegistryReady",
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
        "Test-ProjectAtlasGeneratedMcpConfigReadiness",
        "$verifiedRuntimeReady = Test-ProjectAtlasRuntime $projectAtlas $ProjectAtlasVersion",
        "$generatedMcpConfigsReady = Test-ProjectAtlasGeneratedMcpConfigReadiness",
        "$runtimeMcpConfigsReady = $verifiedRuntimeReady -and $generatedMcpConfigsReady",
        "$probeCleanupSucceeded = -not $probeCleanupFailure",
        "if (-not $probeCleanupSucceeded)",
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
    for forbidden in [
        "Stop-Process",
        "Suspend-Process",
        "PROCESS_TERMINATE",
        r"System32\taskkill.exe",
        "runtime_mcp_configs_ready=true",
    ] {
        if powershell_installer.contains(forbidden) {
            return Err(io::Error::other(format!(
                "PowerShell installer must not use broad process control, found {forbidden:?}"
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
        "refs/tags/$release_tag:refs/tags/$release_tag",
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
    let posix_tag_fetch_failure = posix_installer
        .split("could not fetch release tag %s.")
        .nth(1)
        .and_then(|tail| tail.split("return 0").next())
        .ok_or_else(|| io::Error::other("POSIX release-tag fetch failure branch missing"))?;
    if !posix_tag_fetch_failure.contains("restore_codex_projectatlas_snapshot") {
        return Err(
            io::Error::other("POSIX release-tag fetch failure must restore its snapshot").into(),
        );
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
    if env!("CARGO_PKG_VERSION").contains("-rc") {
        for required in [
            format!("releases/tag/{release_tag}"),
            format!("docs/{release_tag}-release-notes.md"),
            "published as a prerelease".to_string(),
            "does not replace the preceding stable GitHub Latest release".to_string(),
        ] {
            if !readme.contains(&required) {
                return Err(io::Error::other(format!(
                    "README release docs are missing candidate reference {required:?}"
                ))
                .into());
            }
        }
        for forbidden in [
            format!("badge/release-{release_tag}-blue"),
            format!("--ref {release_tag}"),
            format!("--tag {release_tag}"),
            format!("`{release_tag}` ships through the full release matrix"),
        ] {
            if readme.contains(&forbidden) {
                return Err(io::Error::other(format!(
                    "README must not promote release candidate {forbidden:?} as the stable install default"
                ))
                .into());
            }
        }
    } else {
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
    let unix_installer_smoke = workflow_job_block(&release_workflow, "installer-smoke-unix")?;
    for required in [
        "Holistic release-candidate E2E",
        "contains(env.RELEASE_VERSION, '-rc')",
        "atlas_session_brief",
        "atlas_slice",
        "index_status: available",
        "release_e2e_marker",
    ] {
        if !unix_installer_smoke.contains(required) {
            return Err(io::Error::other(format!(
                "hosted release-candidate E2E is missing contract {required:?}"
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
        "installer_workflow_pin_reports_preserve_exact_rc_identity",
        "plugin_update_skips_non_official_codex_marketplace",
        "plugin_update_leaves_current_codex_marketplace_untouched",
        "plugin_update_repairs_current_codex_plugin_with_stale_source_manifest",
        "plugin_update_restores_current_ref_marketplace_when_plugin_reinstall_fails",
        "plugin_update_preserves_prior_integration_when_all_replacement_adds_fail",
        "plugin_update_refuses_unavailable_or_ambiguous_inventory",
        "plugin_update_serializes_restore_before_the_next_installer_reads_state",
        "windows_plugin_update_fails_closed_when_lock_root_cannot_be_canonicalized",
        "plugin_update_refuses_retained_recovery_state_before_mutation",
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
    for required in [
        "windows_installer_fresh_path_probe_respects_machine_precedence",
        "PROJECTATLAS_TEST_DISPOSABLE_RUNNER: ${{ runner.environment }}",
        "PROJECTATLAS_TEST_PERSIST_USER_PATH: \"1\"",
    ] {
        if !e2e_smoke.contains(required) {
            return Err(io::Error::other(format!(
                "Windows CI smoke must exercise persisted User PATH through a fresh process; missing {required:?}"
            ))
            .into());
        }
    }
    if !e2e_smoke
        .contains("windows_release_binary_installer_repairs_stale_mirror_without_registering_it")
    {
        return Err(io::Error::other(
            "Windows CI smoke must run the stale Codex MCP registry repair regression",
        )
        .into());
    }
    for required in [
        "posix_plugin_inventory_without_jq_rejects_split_object_fields",
        "posix_plugin_restore_rejects_hostile_paths_and_retains_recovery_state",
        "windows_plugin_restore_rejects_cache_junction_and_retains_recovery_snapshot",
        "windows_plugin_restore_rejects_config_directory_and_retains_recovery_snapshot",
        "windows_plugin_snapshot_rejects_reparse_above_codex_home_before_mutation",
        "windows_plugin_snapshot_cleanup_refuses_path_swap_without_outside_deletion",
        "windows_plugin_snapshot_cleanup_failure_retains_usable_direct_snapshot",
    ] {
        if !e2e_smoke.contains(required) {
            return Err(io::Error::other(format!(
                "multi-OS CI smoke omitted installer trust-boundary regression {required}"
            ))
            .into());
        }
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
            "the current ProjectAtlas release uses generated OpenCode MCP config and must not ship an unverified plugin directory",
        )
        .into());
    }
    if !agent_integration
        .contains("An installable `opencode plugin` package is separate distribution work.")
        || !architecture.contains("`opencode plugin` package is separate distribution work")
    {
        return Err(io::Error::other(
            "docs must distinguish current generated OpenCode MCP config support from future plugin packaging",
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
    for required in [
        "codex plugin add projectatlas --marketplace projectatlas",
        "docs/agent-integration.md",
    ] {
        if !readme.contains(required) {
            return Err(io::Error::other(format!(
                "README must keep concise install guidance and link its detailed owner; missing {required:?}"
            ))
            .into());
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

#[cfg(windows)]
#[test]
fn windows_installer_fresh_path_probe_respects_machine_precedence() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let machine_bin = temp.path().join("machine-bin");
    let user_bin = temp.path().join("user-bin");
    let empty_bin = temp.path().join("empty-bin");
    fs::create_dir_all(&machine_bin)?;
    fs::create_dir_all(&user_bin)?;
    fs::create_dir_all(&empty_bin)?;
    let system_powershell = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?,
    )
    .join(WINDOWS_SYSTEM32_DIR)
    .join(WINDOWS_POWERSHELL_DIR)
    .join(WINDOWS_POWERSHELL_VERSION_DIR)
    .join(WINDOWS_POWERSHELL_EXECUTABLE);
    fs::write(machine_bin.join("projectatlas.cmd"), "@exit /b 0\r\n")?;
    let verified_runtime = user_bin.join("projectatlas.cmd");
    fs::write(&verified_runtime, "@exit /b 0\r\n")?;
    let flooding_runtime = temp.path().join("flooding-projectatlas.cmd");
    let flood_child_pid = temp.path().join("flood-child.pid");
    let timeout_runtime = temp.path().join("timeout-projectatlas.cmd");
    let timeout_child_pid = temp.path().join("timeout-child.pid");
    let probe_temp = temp.path().join("runtime-probe-temp");
    fs::create_dir_all(&probe_temp)?;
    fs::write(
        &flooding_runtime,
        "@echo off\r\npowershell -NoProfile -Command \"$PID | Set-Content -NoNewline -LiteralPath $env:PROJECTATLAS_TEST_FLOOD_CHILD_PID; $chunk = -join ('x' * 131072); while ($true) { [Console]::Out.Write($chunk); [Console]::Error.Write($chunk) }\"\r\n",
    )?;
    fs::write(
        &timeout_runtime,
        "@echo off\r\npowershell -NoProfile -Command \"$PID | Set-Content -NoNewline -LiteralPath $env:PROJECTATLAS_TEST_TIMEOUT_CHILD_PID; Start-Sleep -Seconds 60; [Console]::Out.WriteLine('{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}')\"\r\n",
    )?;
    let async_runtime = temp.path().join("async-projectatlas.cmd");
    let async_child_pid = temp.path().join("async-child.pid");
    fs::write(
        &async_runtime,
        "@echo off\r\nstart \"\" /b powershell -NoLogo -NoProfile -NonInteractive -Command \"$PID | Set-Content -NoNewline -LiteralPath $env:PROJECTATLAS_TEST_ASYNC_CHILD_PID; while ($true) { Start-Sleep -Seconds 60 }\"\r\nfor /L %%i in (1,1,200) do (\r\n  if exist \"%PROJECTATLAS_TEST_ASYNC_CHILD_PID%\" goto child_started\r\n  powershell -NoLogo -NoProfile -NonInteractive -Command \"Start-Sleep -Milliseconds 10\"\r\n)\r\nexit /b 2\r\n:child_started\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\nexit /b 0\r\n",
    )?;
    let powershell_runtime = temp.path().join("projectatlas.ps1");
    fs::write(
        &powershell_runtime,
        "[Console]::Out.WriteLine('{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}')\r\n",
    )?;
    let unicode_json = temp.path().join("unicode-runtime-info.json");
    fs::write(
        &unicode_json,
        serde_json::to_vec(&json!({
            "project": "ProjectAtlas",
            "major_version": 3,
            "version": "0.4.1",
            "capabilities": ["mcp"],
            "text_format": "TOON",
            "unicode_path": "M\u{fc}nchen\\\u{8def}\u{5f84}"
        }))?,
    )?;
    let unicode_json_runtime = temp.path().join("unicode-runtime-info.cmd");
    fs::write(
        &unicode_json_runtime,
        "@echo off\r\ntype \"%PROJECTATLAS_TEST_UNICODE_JSON%\"\r\n",
    )?;
    let singleton_runtime_root = temp.path().join("singleton-runtime-root.ps1");
    fs::write(
        &singleton_runtime_root,
        "[Console]::Out.WriteLine('[{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}]')\r\n",
    )?;
    let singleton_nested_runtime = temp.path().join("singleton-nested-runtime.ps1");
    fs::write(
        &singleton_nested_runtime,
        "[Console]::Out.WriteLine('{\"runtime\":[{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}]}')\r\n",
    )?;
    let command_host_dir = temp.path().join("command & (safe) !^");
    fs::create_dir(&command_host_dir)?;
    let command_host_runtime = command_host_dir.join("projectatlas.cmd");
    fs::write(
        &command_host_runtime,
        "@echo off\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n",
    )?;
    let nonnumeric_runtime = temp.path().join("nonnumeric-projectatlas.cmd");
    fs::write(
        &nonnumeric_runtime,
        "@echo off\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":\"invalid\",\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n",
    )?;
    let out_of_range_runtime = temp.path().join("out-of-range-projectatlas.cmd");
    fs::write(
        &out_of_range_runtime,
        "@echo off\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":\"999999999999999999999\",\"version\":\"0.4.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n",
    )?;
    let unpinned_runtime = temp.path().join("unpinned-runtime-version.txt");
    fs::write(&unpinned_runtime, "0.4.1")?;
    let local_app_data = temp.path().join("local-app-data");
    let stable_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("bin")
        .join("projectatlas.exe");
    fs::create_dir_all(
        stable_runtime
            .parent()
            .ok_or_else(|| io::Error::other("stable runtime parent missing"))?,
    )?;
    fs::write(&stable_runtime, "0.4.0")?;

    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("pwsh")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            r#"
$ErrorActionPreference = "Stop"
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $env:PROJECTATLAS_TEST_INSTALLER,
    [ref]$tokens,
    [ref]$errors
)
if ($errors.Count -ne 0) {
    throw ($errors | Out-String)
}
$names = @(
    "Convert-ProjectAtlasVersionTag",
    "Get-NormalizedPathEntry",
    "Initialize-ProjectAtlasRuntimeProbe",
    "Invoke-ProjectAtlasBoundedJsonCommand",
    "Invoke-ProjectAtlasRuntimeInfo",
    "Set-ProjectAtlasPathPrecedence",
    "Set-ProjectAtlasProcessPathPrecedence",
    "Split-PathList",
    "Sync-ProjectAtlasRuntimeToLocalAppData",
    "Test-ProjectAtlasBareCommandResolutionOnPath",
    "Test-ProjectAtlasPersistedBareCommandResolution",
    "Test-ProjectAtlasJsonObject",
    "Test-ProjectAtlasRuntime",
    "Test-Truthy"
)
$functions = $ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] `
        -and $names -contains $node.Name
}, $true)
foreach ($name in $names) {
    $definition = $functions | Where-Object Name -eq $name | Select-Object -First 1
    if (-not $definition) {
        throw "Installer function not found: $name"
    }
    Invoke-Expression $definition.Extent.Text
}
$boundedJsonDefinition = ($functions |
    Where-Object Name -eq "Invoke-ProjectAtlasBoundedJsonCommand" |
    Select-Object -First 1).Extent.Text
$machineFirst = "$env:PROJECTATLAS_TEST_MACHINE_BIN;$env:PROJECTATLAS_TEST_USER_BIN"
$userFirst = "$env:PROJECTATLAS_TEST_USER_BIN;$env:PROJECTATLAS_TEST_MACHINE_BIN"
if (Test-ProjectAtlasBareCommandResolutionOnPath $machineFirst $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME) {
    throw "Machine PATH shadow was incorrectly classified as restart-recoverable"
}
if (-not (Test-ProjectAtlasBareCommandResolutionOnPath $userFirst $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME)) {
    throw "Verified runtime first on PATH was not classified as restart-recoverable"
}
if (Test-ProjectAtlasBareCommandResolutionOnPath $env:PROJECTATLAS_TEST_EMPTY_BIN $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME) {
    throw "PATH without ProjectAtlas was incorrectly classified as restart-recoverable"
}
$unicodePayload = Invoke-ProjectAtlasBoundedJsonCommand `
    $env:PROJECTATLAS_TEST_UNICODE_JSON_RUNTIME `
    ([string[]]@("runtime-info"))
$expectedUnicodePath = "M$([char]0x00FC)nchen\$([char]0x8DEF)$([char]0x5F84)"
if ($unicodePayload.unicode_path -ne $expectedUnicodePath) {
    throw "Bounded JSON command did not strictly decode BOM-less UTF-8 output: expected='$expectedUnicodePath' actual='$($unicodePayload.unicode_path)'"
}
if (-not (Test-ProjectAtlasRuntime $env:PROJECTATLAS_TEST_UNICODE_JSON_RUNTIME "0.4.1")) {
    throw "Valid structured runtime probe was rejected"
}
$shortBoundedJsonDefinition = $boundedJsonDefinition.Replace(
    '$probeTimeoutMs = 5000',
    '$probeTimeoutMs = 100'
)
Invoke-Expression $shortBoundedJsonDefinition
function Remove-Item {
    [CmdletBinding()]
    param(
        [string[]]$LiteralPath,
        [switch]$Force
    )
}
$cleanupFailureProbe = [Diagnostics.Stopwatch]::StartNew()
try {
    $cleanupFailurePayload = Invoke-ProjectAtlasBoundedJsonCommand `
        $env:PROJECTATLAS_TEST_UNICODE_JSON_RUNTIME `
        ([string[]]@("runtime-info"))
}
finally {
    $cleanupFailureProbe.Stop()
    Microsoft.PowerShell.Management\Remove-Item -LiteralPath Function:\Remove-Item -Force
    Invoke-Expression $boundedJsonDefinition
}
if ($null -ne $cleanupFailurePayload) {
    throw "Bounded JSON command emitted a payload after cleanup could not be verified."
}
if ($cleanupFailureProbe.Elapsed -gt [TimeSpan]::FromSeconds(2)) {
    throw "Cleanup failure probe exceeded its bounded tolerance: $($cleanupFailureProbe.Elapsed)"
}
$leftoverCommandProbeFiles = @(
    Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
        -Filter "projectatlas-command-probe-*" `
        -File
)
if ($leftoverCommandProbeFiles.Count -ne 2) {
    throw "Suppressed cleanup did not preserve both bounded-probe files: $($leftoverCommandProbeFiles.FullName -join ', ')"
}
Microsoft.PowerShell.Management\Remove-Item `
    -LiteralPath $leftoverCommandProbeFiles.FullName `
    -Force
if (Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
    -Filter "projectatlas-command-probe-*" `
    -File) {
    throw "Bounded JSON cleanup regression left temporary files."
}
Invoke-Expression $shortBoundedJsonDefinition
function Remove-Item {
    [CmdletBinding()]
    param(
        [string[]]$LiteralPath,
        [switch]$Force
    )
    throw "Injected bounded-probe cleanup failure."
}
$cleanupExceptionProbe = [Diagnostics.Stopwatch]::StartNew()
$cleanupException = $null
$cleanupExceptionPayload = $null
try {
    $cleanupExceptionPayload = Invoke-ProjectAtlasBoundedJsonCommand `
        $env:PROJECTATLAS_TEST_UNICODE_JSON_RUNTIME `
        ([string[]]@("runtime-info"))
}
catch {
    $cleanupException = $_
}
finally {
    $cleanupExceptionProbe.Stop()
    Microsoft.PowerShell.Management\Remove-Item -LiteralPath Function:\Remove-Item -Force
    Invoke-Expression $boundedJsonDefinition
    $cleanupExceptionFiles = @(
        Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
            -Filter "projectatlas-command-probe-*" `
            -File
    )
    if ($cleanupExceptionFiles.Count -ne 0) {
        Microsoft.PowerShell.Management\Remove-Item `
            -LiteralPath $cleanupExceptionFiles.FullName `
            -Force
    }
}
if ($cleanupException) {
    throw "Bounded JSON cleanup exception escaped: $cleanupException"
}
if ($null -ne $cleanupExceptionPayload) {
    throw "Bounded JSON command emitted a payload after cleanup threw."
}
if ($cleanupExceptionFiles.Count -ne 2) {
    throw "Injected cleanup exception did not preserve both bounded-probe files: $($cleanupExceptionFiles.FullName -join ', ')"
}
if ($cleanupExceptionProbe.Elapsed -gt [TimeSpan]::FromSeconds(2)) {
    throw "Cleanup exception probe exceeded its bounded tolerance: $($cleanupExceptionProbe.Elapsed)"
}
foreach ($invalidRuntimeShape in @(
    $env:PROJECTATLAS_TEST_SINGLETON_RUNTIME_ROOT,
    $env:PROJECTATLAS_TEST_SINGLETON_NESTED_RUNTIME
)) {
    if (Test-ProjectAtlasRuntime $invalidRuntimeShape $null) {
        throw "Runtime probe accepted a singleton-array object shape: $invalidRuntimeShape"
    }
}
if ($env:PROJECTATLAS_TEST_PERSIST_USER_PATH -eq "1") {
    if ($env:PROJECTATLAS_TEST_DISPOSABLE_RUNNER -ne "github-hosted") {
        throw "Persisted User PATH test requires a disposable GitHub-hosted runner"
    }
    $originalUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $originalProcessPath = $env:Path
    try {
        if (-not (Set-ProjectAtlasPathPrecedence $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME)) {
            throw "Persisted User PATH was not classified as fresh-host ready"
        }
        if (-not (Test-ProjectAtlasPersistedBareCommandResolution $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME)) {
            throw "Supplied-runtime mode did not recognize the existing persisted User PATH"
        }
        $persistedUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
        $env:Path = @($machinePath, $persistedUserPath) -join ";"
        $freshChildPath = & $env:PROJECTATLAS_TEST_SYSTEM_POWERSHELL -NoProfile -Command `
            "(Get-Command projectatlas -ErrorAction Stop).Source"
        if ($LASTEXITCODE -ne 0) {
            throw "Fresh child could not resolve projectatlas from persisted User PATH"
        }
        if ((Get-NormalizedPathEntry $freshChildPath) -ne `
            (Get-NormalizedPathEntry $env:PROJECTATLAS_TEST_VERIFIED_RUNTIME)) {
            throw "Fresh child resolved '$freshChildPath' instead of the persisted verified runtime"
        }
    }
    finally {
        $env:Path = $originalProcessPath
        [Environment]::SetEnvironmentVariable("Path", $originalUserPath, "User")
    }
}
$probe = [Diagnostics.Stopwatch]::StartNew()
if (Test-ProjectAtlasRuntime $env:PROJECTATLAS_TEST_FLOODING_RUNTIME $null) {
    throw "Output-flooding runtime was accepted"
}
$probe.Stop()
if ($probe.Elapsed -gt [TimeSpan]::FromSeconds(4)) {
    throw "Output-flooding runtime reached the timeout instead of the live byte limit: $($probe.Elapsed)"
}
$floodChildPid = $null
try {
    if (-not (Test-Path -LiteralPath $env:PROJECTATLAS_TEST_FLOOD_CHILD_PID)) {
        throw "Output-flooding runtime did not report its child process"
    }
    $floodChildPid = [int](Get-Content -Raw -LiteralPath $env:PROJECTATLAS_TEST_FLOOD_CHILD_PID)
    $childDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $floodChild = Get-Process -Id $floodChildPid -ErrorAction SilentlyContinue
        if (-not $floodChild) {
            break
        }
        Start-Sleep -Milliseconds 25
    }
    while ([DateTime]::UtcNow -lt $childDeadline)
    if ($floodChild) {
        throw "Bounded runtime probe left its owned child process alive: $floodChildPid"
    }
    $leftoverProbeFiles = @(
        Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
            -Filter "projectatlas-command-probe-*" `
            -File |
        Select-Object -ExpandProperty FullName
    )
    if ($leftoverProbeFiles.Count -ne 0) {
        throw "Bounded runtime probe left temporary files: $($leftoverProbeFiles -join ', ')"
    }
}
finally {
    if ($floodChildPid -and (Get-Process -Id $floodChildPid -ErrorAction SilentlyContinue)) {
        & (Join-Path $env:SystemRoot "System32\taskkill.exe") /PID $floodChildPid /T /F | Out-Null
    }
}
$timeoutDisposition = $null
$timeoutPayload = Invoke-ProjectAtlasBoundedJsonCommand `
    $env:PROJECTATLAS_TEST_TIMEOUT_RUNTIME `
    ([string[]]@("runtime-info")) `
    ([ref]$timeoutDisposition)
if ($null -ne $timeoutPayload -or $timeoutDisposition -ne "timeout") {
    throw "Below-limit delayed runtime was not causally stopped by its timeout: payload='$timeoutPayload' disposition='$timeoutDisposition'"
}
$timeoutChildPid = $null
try {
    if (-not (Test-Path -LiteralPath $env:PROJECTATLAS_TEST_TIMEOUT_CHILD_PID)) {
        throw "Below-limit delayed runtime did not report its child process"
    }
    $timeoutChildPid = [int](Get-Content -Raw -LiteralPath $env:PROJECTATLAS_TEST_TIMEOUT_CHILD_PID)
    $childDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $timeoutChild = Get-Process -Id $timeoutChildPid -ErrorAction SilentlyContinue
        if (-not $timeoutChild) {
            break
        }
        Start-Sleep -Milliseconds 25
    }
    while ([DateTime]::UtcNow -lt $childDeadline)
    if ($timeoutChild) {
        throw "Timed-out runtime probe left its owned child process alive: $timeoutChildPid"
    }
    $leftoverProbeFiles = @(
        Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
            -Filter "projectatlas-command-probe-*" `
            -File |
        Select-Object -ExpandProperty FullName
    )
    if ($leftoverProbeFiles.Count -ne 0) {
        throw "Timed-out runtime probe left temporary files: $($leftoverProbeFiles -join ', ')"
    }
}
finally {
    if ($timeoutChildPid -and (Get-Process -Id $timeoutChildPid -ErrorAction SilentlyContinue)) {
        & (Join-Path $env:SystemRoot "System32\taskkill.exe") /PID $timeoutChildPid /T /F | Out-Null
    }
}
$canary = Start-Process `
    -FilePath (Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe") `
    -ArgumentList @("-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "while (`$true) { Start-Sleep -Seconds 60 }") `
    -WindowStyle Hidden `
    -PassThru
$asyncChildPid = $null
try {
    if (-not (Test-ProjectAtlasRuntime $env:PROJECTATLAS_TEST_ASYNC_RUNTIME $null)) {
        throw "Runtime with an asynchronously exiting launcher was rejected"
    }
    if (-not (Test-Path -LiteralPath $env:PROJECTATLAS_TEST_ASYNC_CHILD_PID)) {
        throw "Asynchronous launcher did not report its child process"
    }
    $asyncChildPid = [int](Get-Content -Raw -LiteralPath $env:PROJECTATLAS_TEST_ASYNC_CHILD_PID)
    $childDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $asyncChild = Get-Process -Id $asyncChildPid -ErrorAction SilentlyContinue
        if (-not $asyncChild) {
            break
        }
        Start-Sleep -Milliseconds 25
    }
    while ([DateTime]::UtcNow -lt $childDeadline)
    if ($asyncChild) {
        throw "Contained job left its asynchronously spawned child alive: $asyncChildPid"
    }
    if ($canary.HasExited) {
        throw "Contained job terminated the unrelated canary process"
    }
    if (-not (Test-ProjectAtlasRuntime $env:PROJECTATLAS_TEST_POWERSHELL_RUNTIME $null)) {
        throw "PowerShell ProjectAtlas shim was not dispatched through PowerShell"
    }
    if (-not (Test-ProjectAtlasRuntime $env:PROJECTATLAS_TEST_COMMAND_HOST_RUNTIME $null)) {
        throw "Command shim path was not safely quoted for cmd.exe"
    }
    $leftoverProbeFiles = @(
        Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
            -Filter "projectatlas-command-probe-*" `
            -File |
        Select-Object -ExpandProperty FullName
    )
    if ($leftoverProbeFiles.Count -ne 0) {
        throw "Contained probe left temporary files: $($leftoverProbeFiles -join ', ')"
    }
}
finally {
    if ($asyncChildPid) {
        $asyncChild = Get-Process -Id $asyncChildPid -ErrorAction SilentlyContinue
        if ($asyncChild) {
            $asyncChild.Kill()
            [void]$asyncChild.WaitForExit(2000)
        }
    }
    if (-not $canary.HasExited) {
        $canary.Kill()
        [void]$canary.WaitForExit(2000)
    }
    $canary.Dispose()
}
foreach ($invalidRuntime in @(
    $env:PROJECTATLAS_TEST_NONNUMERIC_RUNTIME,
    $env:PROJECTATLAS_TEST_OUT_OF_RANGE_RUNTIME
)) {
    if (Test-ProjectAtlasRuntime $invalidRuntime $null) {
        throw "Runtime with invalid major_version was accepted: $invalidRuntime"
    }
}
function Get-ProjectAtlasRuntimeVersion {
    param([string]$FilePath)
    if (-not (Test-Path -LiteralPath $FilePath)) {
        return $null
    }
    return (Get-Content -Raw -LiteralPath $FilePath).Trim()
}
function Test-ProjectAtlasRuntime {
    param([string]$FilePath, [string]$ExpectedVersion)
    if (-not (Test-Path -LiteralPath $FilePath)) {
        return $false
    }
    if ($script:ProjectAtlasTestForceStaleTarget `
        -and (Get-NormalizedPathEntry $FilePath) -eq (Get-NormalizedPathEntry $env:PROJECTATLAS_TEST_STABLE_RUNTIME)) {
        return $false
    }
    $actualVersion = Get-ProjectAtlasRuntimeVersion $FilePath
    $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    return -not $expectedRuntimeVersion -or $actualVersion -eq $expectedRuntimeVersion
}
if (-not (Sync-ProjectAtlasRuntimeToLocalAppData $env:PROJECTATLAS_TEST_UNPINNED_RUNTIME $null)) {
    throw "Unpinned runtime did not synchronize its exact version"
}
if ((Get-ProjectAtlasRuntimeVersion $env:PROJECTATLAS_TEST_STABLE_RUNTIME) -ne "0.4.1") {
    throw "Unpinned synchronization accepted an older stable mirror"
}
Set-Content -NoNewline -LiteralPath $env:PROJECTATLAS_TEST_STABLE_RUNTIME -Value "0.4.0"
$script:ProjectAtlasTestForceStaleTarget = $true
$stableLock = [IO.File]::Open(
    $env:PROJECTATLAS_TEST_STABLE_RUNTIME,
    [IO.FileMode]::Open,
    [IO.FileAccess]::Read,
    [IO.FileShare]::None
)
try {
    if (Sync-ProjectAtlasRuntimeToLocalAppData $env:PROJECTATLAS_TEST_UNPINNED_RUNTIME $null) {
        throw "Locked older stable mirror was reported synchronized"
    }
}
finally {
    $stableLock.Dispose()
}
"#,
        ])
        .env("PROJECTATLAS_TEST_INSTALLER", installer)
        .env("PROJECTATLAS_TEST_MACHINE_BIN", &machine_bin)
        .env("PROJECTATLAS_TEST_USER_BIN", &user_bin)
        .env("PROJECTATLAS_TEST_EMPTY_BIN", &empty_bin)
        .env("PROJECTATLAS_TEST_SYSTEM_POWERSHELL", &system_powershell)
        .env("PROJECTATLAS_TEST_VERIFIED_RUNTIME", &verified_runtime)
        .env("PROJECTATLAS_TEST_FLOODING_RUNTIME", &flooding_runtime)
        .env("PROJECTATLAS_TEST_FLOOD_CHILD_PID", &flood_child_pid)
        .env("PROJECTATLAS_TEST_TIMEOUT_RUNTIME", &timeout_runtime)
        .env("PROJECTATLAS_TEST_TIMEOUT_CHILD_PID", &timeout_child_pid)
        .env("PROJECTATLAS_TEST_ASYNC_RUNTIME", &async_runtime)
        .env("PROJECTATLAS_TEST_ASYNC_CHILD_PID", &async_child_pid)
        .env(
            "PROJECTATLAS_TEST_POWERSHELL_RUNTIME",
            &powershell_runtime,
        )
        .env("PROJECTATLAS_TEST_UNICODE_JSON", &unicode_json)
        .env(
            "PROJECTATLAS_TEST_UNICODE_JSON_RUNTIME",
            &unicode_json_runtime,
        )
        .env(
            "PROJECTATLAS_TEST_SINGLETON_RUNTIME_ROOT",
            &singleton_runtime_root,
        )
        .env(
            "PROJECTATLAS_TEST_SINGLETON_NESTED_RUNTIME",
            &singleton_nested_runtime,
        )
        .env(
            "PROJECTATLAS_TEST_COMMAND_HOST_RUNTIME",
            &command_host_runtime,
        )
        .env("TEMP", &probe_temp)
        .env("TMP", &probe_temp)
        .env("PROJECTATLAS_TEST_NONNUMERIC_RUNTIME", &nonnumeric_runtime)
        .env(
            "PROJECTATLAS_TEST_OUT_OF_RANGE_RUNTIME",
            &out_of_range_runtime,
        )
        .env("PROJECTATLAS_TEST_UNPINNED_RUNTIME", &unpinned_runtime)
        .env("PROJECTATLAS_TEST_STABLE_RUNTIME", &stable_runtime)
        .env("LOCALAPPDATA", &local_app_data)
        .spawn()?;
    let output = wait_for_plugin_installer_output(
        output,
        "fresh Windows PATH probe",
        Duration::from_secs(30),
    )?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "fresh Windows PATH probe failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

fn assert_filtered_custom_harness_step(release: &str) -> Result<(), Box<dyn Error>> {
    const STEP_NAME: &str = "      - name: Filtered custom harness compatibility";
    if release.lines().filter(|line| *line == STEP_NAME).count() != 1 {
        return Err(io::Error::other(
            "release must contain exactly one filtered custom harness step",
        )
        .into());
    }
    let step = release
        .lines()
        .skip_while(|line| *line != STEP_NAME)
        .skip(1)
        .take_while(|line| !line.starts_with("      -"))
        .collect::<Vec<_>>()
        .join("\n");
    let timeout_line = step
        .lines()
        .find(|line| line.trim_start().starts_with("timeout-minutes:"));
    let run_line = step
        .lines()
        .find(|line| line.trim_start().starts_with("run:"));
    let expected_run_line = format!("        run: {FILTERED_CUSTOM_HARNESS_COMMAND}");
    if step.matches("timeout-minutes:").count() != 1
        || timeout_line != Some("        timeout-minutes: 10")
        || step.matches("        run:").count() != 1
        || run_line != Some(expected_run_line.as_str())
    {
        return Err(io::Error::other(
            "filtered custom harness must keep one step-level 10-minute timeout and exact Cargo command",
        )
        .into());
    }
    Ok(())
}

#[test]
fn filtered_custom_harness_contract_rejects_timeout_drift() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility\n        timeout-minutes: 5\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
}

#[test]
fn filtered_custom_harness_contract_rejects_fractional_timeout() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility\n        timeout-minutes: 10.5\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
}

#[test]
fn filtered_custom_harness_contract_rejects_suffixed_command() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility\n        timeout-minutes: 10\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND} && another-command\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
}

#[test]
fn filtered_custom_harness_contract_rejects_suffixed_step_name() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility (release)\n        timeout-minutes: 10\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
}

#[test]
fn filtered_custom_harness_contract_rejects_drift_after_suffixed_step() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility (release)\n        timeout-minutes: 10\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n      - name: Filtered custom harness compatibility\n        timeout-minutes: 5\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
}

#[test]
fn filtered_custom_harness_contract_rejects_timeout_borrowed_from_unnamed_step() {
    let drifted = format!(
        "      - name: Filtered custom harness compatibility\n        run: {FILTERED_CUSTOM_HARNESS_COMMAND}\n      - uses: actions/checkout@v4\n        timeout-minutes: 10\n"
    );
    assert!(assert_filtered_custom_harness_step(&drifted).is_err());
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
fn issueops_and_workflows_use_behavior_focused_quality_gates() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let github = workspace_root.join(".github");
    let workflows = github.join("workflows");
    let issueops = fs::read_to_string(
        github
            .join("scripts")
            .join(ISSUE_CHECKLISTS_SCRIPT_FILE_NAME),
    )?;
    let mermaid_parser = github.join("mermaid-parser");
    let mermaid_package = fs::read_to_string(mermaid_parser.join(PACKAGE_JSON_FILE_NAME))?;
    let mermaid_lock = fs::read_to_string(mermaid_parser.join("package-lock.json"))?;
    let ci = fs::read_to_string(workflows.join("ci.yml"))?;
    let pr_state = fs::read_to_string(workflows.join("pr-state.yml"))?;
    let planner = fs::read_to_string(github.join("scripts").join("affected-ci-proof.py"))?;
    let docs_workflow = fs::read_to_string(workflows.join(DOCS_WORKFLOW_FILE_NAME))?;
    let auto_release_workflow = fs::read_to_string(workflows.join("03-auto-release.yml"))?;
    let optional_parser_workflow = fs::read_to_string(workflows.join("optional-parser-pack.yml"))?;
    let release = fs::read_to_string(workflows.join("release.yml"))?;
    let issueops_workflow = fs::read_to_string(workflows.join("issueops.yml"))?;
    if github
        .join("scripts")
        .join("codex-pr-review-gate.py")
        .exists()
    {
        return Err(io::Error::other("superseded Codex-only review poller still exists").into());
    }
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
    let workflow_docs =
        fs::read_to_string(workspace_root.join("docs").join(WORKFLOW_DOC_FILE_NAME))?;
    let toolchain = fs::read_to_string(workspace_root.join("rust-toolchain.toml"))?;
    let rust_toolchain_preflight =
        fs::read_to_string(github.join("scripts").join("verify-rust-toolchain.py"))?;
    let issue_map = fs::read_to_string(
        workspace_root
            .join(OPENSPEC_DIR_NAME)
            .join(ISSUE_MAP_FILE_NAME),
    )?;
    let tasks = fs::read_to_string(
        workspace_root
            .join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join("enforce-rust-test-quality-gates")
            .join(TASKS_FILE_NAME),
    )?;

    let issueops_self_test_command = "python3 .github/scripts/issue-checklists.py --self-test";
    for (name, owner) in [
        ("pre-push", hook.as_str()),
        ("CI", ci.as_str()),
        ("IssueOps", issueops_workflow.as_str()),
        ("release", release.as_str()),
    ] {
        if !owner.contains(issueops_self_test_command) {
            return Err(io::Error::other(format!(
                "{name} omitted the explicit IssueOps self-test owner"
            ))
            .into());
        }
    }

    if !mermaid_package.contains(r#""jsdom": "27.4.0""#)
        || !mermaid_package.contains(r#""mermaid": "11.16.1""#)
        || !mermaid_lock.contains(r#""node_modules/jsdom""#)
        || !mermaid_lock.contains(r#""node_modules/mermaid""#)
        || !mermaid_lock.contains(r#""version": "11.16.1""#)
    {
        return Err(io::Error::other(
            "IssueOps Mermaid syntax validation must use one exact lockfile-owned parser",
        )
        .into());
    }
    for (name, workflow) in [
        ("release", release.as_str()),
        ("IssueOps", issueops_workflow.as_str()),
    ] {
        for command in [
            "npm ci --ignore-scripts --prefix .github/mermaid-parser",
            "npm audit --omit=dev --audit-level=moderate --prefix .github/mermaid-parser",
        ] {
            if !workflow.contains(command) {
                return Err(io::Error::other(format!(
                    "{name} workflow omitted locked Mermaid parser gate {command:?}"
                ))
                .into());
            }
        }
    }
    for (name, workflow) in [("release", release.as_str())] {
        let first_cargo_test = workflow
            .find("cargo test")
            .ok_or_else(|| io::Error::other(format!("{name} omitted cargo tests")))?;
        for gate in [
            "npm ci --ignore-scripts --prefix .github/mermaid-parser",
            "npm audit --omit=dev --audit-level=moderate --prefix .github/mermaid-parser",
        ] {
            let gate_position = workflow
                .find(gate)
                .ok_or_else(|| io::Error::other(format!("{name} omitted Mermaid gate {gate:?}")))?;
            if gate_position > first_cargo_test {
                return Err(io::Error::other(format!(
                    "{name} Mermaid gate {gate:?} must precede its first cargo test"
                ))
                .into());
            }
        }
    }
    for (name, owner) in [("CI", ci.as_str()), ("pre-push", hook.as_str())] {
        for command in [
            "npm ci --ignore-scripts --no-audit --prefix .github/mermaid-parser",
            "npm audit --omit=dev --audit-level=moderate --prefix .github/mermaid-parser",
        ] {
            if !owner.contains(command) {
                return Err(io::Error::other(format!(
                    "{name} omitted affected Mermaid dependency gate {command:?}"
                ))
                .into());
            }
        }
    }

    for required in [
        "validate_unique_issue_ownership",
        "owner_slices",
        "visible_markdown",
        "remote != expected",
        "milestone_issue_failures",
        "REQUIRED_OPEN_ISSUE_HEADINGS",
        "architecture_diagram_link_failures",
        "contains_mermaid_diagram",
        "mermaid_syntax_is_valid",
        "mermaid-parser",
        "len(meaningful) > 1",
        "ACCEPTANCE_REVIEW_TASKS",
        "planned_issue_failures",
        "openspec_readiness_failures",
        "required_markdown_section_failures",
        "planned_issue=args.planned_issue",
        "MITIGATION_RE",
        "issue_contract_failures",
        "IMPLEMENTATION_TASK_HEADING",
        "acceptance_task_failures",
        "acceptance_state_failures",
        "complexity_label_failures",
        "check_open_issue_complexity",
        "ISSUE_STATE_QUERY",
        "issue_state_payloads",
        "\"graphql\"",
        "\"--paginate\"",
        "\"--slurp\"",
        "issues(first: 100, after: $endCursor, states: [OPEN, CLOSED])",
        "pageInfo",
        "totalCount",
        "total_count != len(label_nodes)",
        "isinstance(total_count, int)",
        "isinstance(total_count, bool)",
        "GitHub GraphQL issue labels were incomplete",
        "\"body\" not in ISSUE_STATE_QUERY.lower()",
        "closed issue body is inert",
        "must be OPEN",
        "ISSUE_REFERENCE_RE",
        "pull_request_owner_issue",
        "COMMIT_ISSUE_REFERENCE_RE",
        "candidate_owner_issue_from_subjects",
        "configured_issue_map_path",
        "base_issue_map(",
        "base_local_tasks",
        "check_pull_request_tasks",
        "check_candidate_tasks",
        "issue_map_path=args.issue_map",
    ] {
        if !issueops.contains(required) {
            return Err(io::Error::other(format!(
                "IssueOps is missing lean checklist behavior {required:?}"
            ))
            .into());
        }
    }
    let rust_preflight_command = "python3 .github/scripts/verify-rust-toolchain.py --install";
    for (name, workflow) in [
        ("CI", ci.as_str()),
        ("Docs", docs_workflow.as_str()),
        ("optional-parser-pack", optional_parser_workflow.as_str()),
        ("release", release.as_str()),
    ] {
        if !workflow.contains(rust_preflight_command) {
            return Err(io::Error::other(format!(
                "{name} workflow omitted the shared Rust toolchain preflight"
            ))
            .into());
        }
        let preflight = workflow
            .find("- name: Rust toolchain preflight")
            .ok_or_else(|| io::Error::other(format!("{name} omitted Rust preflight step")))?;
        let first_cargo = workflow
            .find("cargo ")
            .ok_or_else(|| io::Error::other(format!("{name} omitted Rust command")))?;
        if preflight > first_cargo {
            return Err(
                io::Error::other(format!("{name} Rust preflight must precede Rust work")).into(),
            );
        }
        if workflow.contains("RUSTUP_TOOLCHAIN")
            || workflow.contains("rustup default")
            || workflow.contains("rustup toolchain install stable")
        {
            return Err(io::Error::other(format!(
                "{name} retains a duplicated or floating Rust toolchain selection"
            ))
            .into());
        }
    }
    if !hook.contains("python3 .github/scripts/verify-rust-toolchain.py") {
        return Err(io::Error::other(
            "local pre-push validation omitted the shared Rust toolchain preflight",
        )
        .into());
    }
    let normalized_hook = hook.replace("\r\n", "\n");
    for required in [
        "if ! push_record=\"$(",
        "NF != 4",
        "if ($2 ~ /^0+$/ || $3 !~",
        "if (!seen || invalid || updates != 1) exit 1",
        "updates += 1",
        "local_oid = $2",
        "print remote_ref \"|\" local_oid \"|\" remote_oid",
        "IFS='|' read -r remote_ref candidate_local_oid remote_oid",
        "refs/heads/main",
        "git rev-parse --verify 'HEAD^{commit}'",
        "pushed local object must match the checked-out HEAD",
        "git status --porcelain=v1 --untracked-files=all",
        "git ls-files -v",
        "git merge-base origin/main HEAD",
        "affected-ci-proof.py --self-test",
        "affected-ci-proof.py plan",
        "--base \"$accepted_base\"",
        "--head \"$current_head\"",
        "--event pre_push",
        "npm ls --depth=0 --prefix .github/mermaid-parser",
        "has_repository_contract dependency-audit",
        "git log --format=%s",
        "--owner-from-commits",
        "--candidate-issue \"$owner_issue\"",
        "--candidate-local-oid \"$candidate_local_oid\"",
        "--base \"$accepted_base\"",
    ] {
        if !normalized_hook.contains(required) {
            return Err(io::Error::other(format!(
                "local pre-push hook is missing candidate/main IssueOps routing {required:?}"
            ))
            .into());
        }
    }
    let main_route = normalized_hook
        .find("if [ \"$remote_ref\" = \"refs/heads/main\" ]")
        .ok_or_else(|| io::Error::other("pre-push hook omitted main route"))?;
    let global_route = normalized_hook
        .find("--repo \"$repo\" --root . --issue-map openspec/issue-map.json")
        .ok_or_else(|| io::Error::other("pre-push hook omitted global IssueOps route"))?;
    let candidate_route = normalized_hook
        .find("--candidate-issue \"$owner_issue\"")
        .ok_or_else(|| io::Error::other("pre-push hook omitted candidate IssueOps route"))?;
    if !(main_route < global_route && global_route < candidate_route) {
        return Err(io::Error::other(
            "pre-push hook must keep global and candidate IssueOps routes in their target scopes",
        )
        .into());
    }
    let planner_position = normalized_hook
        .find("affected-ci-proof.py plan")
        .ok_or_else(|| io::Error::other("pre-push hook omitted affected planner"))?;
    let first_expensive_position = normalized_hook
        .find("npm ls --depth=0")
        .ok_or_else(|| io::Error::other("pre-push hook omitted dependency reuse check"))?;
    if planner_position > first_expensive_position {
        return Err(io::Error::other(
            "pre-push hook must bind its affected plan before dependency or build work",
        )
        .into());
    }
    let lockfile_install = "if has_repository_contract dependency-audit; then\n  npm ci --ignore-scripts --no-audit --prefix .github/mermaid-parser\nelif has_repository_contract issueops || has_repository_contract mermaid; then";
    if !normalized_hook.contains(lockfile_install) {
        return Err(io::Error::other(
            "pre-push must install the submitted Mermaid lockfile before its audit",
        )
        .into());
    }
    if normalized_hook.contains("git branch --show-current") {
        return Err(io::Error::other(
            "pre-push hook must select validation from pushed remote refs, not checkout state",
        )
        .into());
    }
    for required in [
        "read_declared_channel",
        "RUSTUP_TOOLCHAIN override",
        "rustup is missing from PATH",
        "Rust toolchain preflight passed",
        "Rust toolchain preflight self-test passed",
    ] {
        if !rust_toolchain_preflight.contains(required) {
            return Err(io::Error::other(format!(
                "Rust toolchain preflight is missing boundary {required:?}"
            ))
            .into());
        }
    }
    if issueops.contains("MERMAID_DECLARATION_RE") {
        return Err(io::Error::other(
            "IssueOps must let the locked Mermaid parser own diagram-family admission",
        )
        .into());
    }
    if !issue_map.contains(r#""schema_version": 2"#)
        || issue_map.contains(r#""legacy_closed_issues": ["#)
        || !issue_map.contains(r#""enforce-rust-test-quality-gates": 309"#)
    {
        return Err(io::Error::other("#309 must be mapped by the schema-2 issue map").into());
    }
    for required in [
        "label: Why",
        "label: What Changes",
        "label: Capabilities",
        "label: Architecture Diagrams",
        "blob/main/docs/",
        "label: Release Scope",
        "label: Acceptance criteria",
        "label: Non-Goals",
        "label: Pre-Mortem",
        "Implementation tasks:",
        "label: OpenSpec plan and task checklist",
        "Maintainers add the mapped OpenSpec change",
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
    for (name, content) in [
        ("bug", bug_issue_template.as_str()),
        ("chore", chore_issue_template.as_str()),
        ("improvement", improvement_issue_template.as_str()),
    ] {
        for forbidden in [
            "id: acceptance_review_tasks",
            "label: Acceptance and Review Tasks",
            "## Implementation Tasks",
        ] {
            if content.contains(forbidden) {
                return Err(io::Error::other(format!(
                    "{name} issue form must not fabricate authoritative task field {forbidden:?}"
                ))
                .into());
            }
        }
    }
    for required in [
        "Pre-Mortem",
        "Architecture Diagrams",
        "docs/*.md#user-content-heading` view on `main",
        "Implementation tasks:",
        "Acceptance and Review Tasks",
        "Already closed mapped issues",
        "commit/SHA permalink evidence",
    ] {
        if !workflow_docs.contains(required) {
            return Err(io::Error::other(format!(
                "workflow guidance is missing lean issue contract {required:?}"
            ))
            .into());
        }
    }

    for required in [
        "affected-ci-proof.py plan",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo check \"$@\" --lib --bins --examples --all-features --locked",
        "cargo test \"$@\" --lib --bins --all-features --locked",
        "cargo test --locked -p projectatlas-cli --all-features --test \"$target\"",
        "cargo test --locked -p projectatlas-cli --all-features --test parser_supervisor_adversarial task_errors_classify_only_typed_cancellation_as_canceled",
        "npm ls --depth=0 --prefix .github/mermaid-parser",
        "has_repository_contract cargo-dependency",
        "has_repository_contract source-policy",
    ] {
        if !hook.contains(required) {
            return Err(io::Error::other(format!(
                "pre-push hook omitted affected proof gate {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "exact clean candidate",
        "affected-ci-proof.py",
        "only the repository, Rust package",
        "Unknown paths and changes to shared proof authorities",
        "Run the complete local suite only",
        "retarget replans against the new base",
        "body edit runs no source job",
    ] {
        if !workflow_docs.contains(required) {
            return Err(io::Error::other(format!(
                "workflow guidance omitted affected proof boundary {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "types: [opened, reopened, synchronize, edited]",
        "merge_group:",
        "schedule:",
        "group: projectatlas-ci-${{ github.event_name }}-${{ github.event_name == 'pull_request' && (github.event.action != 'edited' || github.event.changes.base != null) && github.event.pull_request.number || github.run_id }}",
        "cancel-in-progress: ${{ github.event_name == 'pull_request' && (github.event.action != 'edited' || github.event.changes.base != null) }}",
        "force_full: ${{ steps.inputs.outputs.force_full }}",
        "EVENT_BASE: ${{ github.event.pull_request.base.sha || github.event.merge_group.base_sha || github.event.before }}",
        "elif [[ -z \"$base\" || \"$base\" =~ ^0+$ ]]; then\n            base=\"$head\"\n            force_full=true",
        "if [[ \"$INPUT_FORCE_FULL\" == \"true\"",
        "python3 .github/scripts/affected-ci-proof.py --self-test",
        "python3 .github/scripts/affected-ci-proof.py plan",
        "if: needs.plan.outputs.repository == 'true'",
        "if: needs.plan.outputs.rust == 'true'",
        "matrix: ${{ fromJSON(needs.plan.outputs.platform_matrix) }}",
        "cargo fmt --all --check",
        "cargo check --locked -p projectatlas-cli --all-features --test \"$target\"",
        "cargo test --locked -p projectatlas-cli --all-features --test \"$target\"",
        "cargo test --locked -p projectatlas-cli --all-features --test parser_supervisor_adversarial task_errors_classify_only_typed_cancellation_as_canceled",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo check \"${package_args[@]}\" --lib --bins --examples --all-features --locked",
        "cargo test \"${package_args[@]}\" --lib --bins --all-features --locked",
        "cargo test --workspace --all-features --locked",
        "cargo deny --locked --all-features check -D warnings",
        "test-optional-parser-proof-inputs.py",
        "--issue-map openspec/issue-map.json",
        "if: always()",
        "needs: [plan, repository, rust, e2e-smoke]",
        "affected-ci-proof.py aggregate",
        "--event \"$EVENT_NAME\"",
    ] {
        if !ci.contains(required) {
            return Err(io::Error::other(format!(
                "ordinary CI is missing blocking gate {required:?}"
            ))
            .into());
        }
    }
    let source_event = "github.event_name != 'pull_request' || github.event.action != 'edited' || github.event.changes.base != null";
    let plan_job = workflow_job_block(&ci, "plan")?;
    if !plan_job.contains(&format!("if: {source_event}")) {
        return Err(io::Error::other(
            "source planning must rerun for a base retarget and skip metadata-only edits",
        )
        .into());
    }
    let verify_job = workflow_job_block(&ci, "verify")?;
    for required in [
        "name: ${{ github.event_name == 'pull_request' && github.event.action == 'edited' && github.event.changes.base == null && 'metadata-edit' || 'verify' }}",
        &format!("if: always() && ({source_event})"),
    ] {
        if !verify_job.contains(required) {
            return Err(io::Error::other(format!(
                "metadata-only edits must not emit or satisfy source aggregate {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "pull request must reference exactly one owning issue",
        "pull request owner must be an open issue",
        "Pull request milestone must match owning issue",
    ] {
        if !pr_state.contains(required) {
            return Err(io::Error::other(format!(
                "pr-state omitted ownership boundary {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "MAX_DIFF_BYTES",
        "MAX_PATHS",
        "GIT_NO_REPLACE_OBJECTS",
        "[\"cargo\", \"metadata\"",
        "Cargo workspace package inventory drifted",
        "shared core changed",
        "shared CLI test support changed",
        "unknown or incompletely owned path",
        "dependency-audit",
        "cargo-dependency",
        "selected {name} job concluded",
        "omitted {name} job concluded",
        "proof plan binding is stale",
        "binding.get(\"event\")",
        "ordinary change requires mapped issue-state consistency",
        "affected CI proof self-test passed",
    ] {
        if !planner.contains(required) {
            return Err(io::Error::other(format!(
                "affected CI planner omitted fail-closed contract {required:?}"
            ))
            .into());
        }
    }
    for (binary, test, label) in [
        (
            "e2e_navigation",
            "csharp_symbol_identity_boundary_preserves_full_and_incremental_publication",
            "C# symbol-identity publication",
        ),
        (
            "e2e_navigation",
            "deep_qualified_symbol_parents_preserve_full_and_incremental_publication",
            "deep qualified-symbol publication",
        ),
        (
            "e2e_navigation",
            "partial_markdown_limit_persists_without_losing_complete_publication",
            "partial Markdown publication",
        ),
        (
            "e2e_maintenance",
            "lint_formats_share_typed_cli_and_mcp_report",
            "typed lint serialization",
        ),
    ] {
        let contract = format!(
            "cargo test --locked -p projectatlas-cli --test {binary} {test} -- --exact --include-ignored --nocapture"
        );
        for (name, workflow) in [("ordinary CI", ci.as_str()), ("release", release.as_str())] {
            if !workflow.contains(&contract) {
                return Err(
                    io::Error::other(format!("{name} omitted the {label} contract")).into(),
                );
            }
        }
    }
    let checklist_self_test_step = ci
        .split("- name: Issue checklist self-test")
        .nth(1)
        .and_then(|tail| tail.split("- name:").next())
        .ok_or_else(|| io::Error::other("ordinary IssueOps self-test step is missing"))?;
    if ci.matches("- name: Issue checklist self-test").count() != 1 {
        return Err(io::Error::other("CI must run the IssueOps self-test exactly once").into());
    }
    if !checklist_self_test_step.contains(
        "if: contains(fromJSON(needs.plan.outputs.plan).repository_contracts, 'issueops')",
    ) {
        return Err(io::Error::other("CI IssueOps self-test must follow the affected plan").into());
    }
    for required in [issueops_self_test_command] {
        if !checklist_self_test_step.contains(required) {
            return Err(io::Error::other(format!(
                "CI IssueOps self-test omitted gate {required:?}"
            ))
            .into());
        }
    }
    let checklist_step = ci
        .split("- name: Issue checklist check")
        .nth(1)
        .and_then(|tail| tail.split("- name:").next())
        .ok_or_else(|| io::Error::other("ordinary IssueOps step is missing"))?;
    let mermaid_setup_step = ci
        .split("- name: Install Mermaid parser")
        .nth(1)
        .and_then(|tail| tail.split("- name:").next())
        .ok_or_else(|| io::Error::other("CI Mermaid setup step is missing"))?;
    if ci.matches("- name: Install Mermaid parser").count() != 1 {
        return Err(io::Error::other("CI must install the Mermaid parser exactly once").into());
    }
    let first_cargo_test = ci
        .find("cargo test")
        .ok_or_else(|| io::Error::other("CI omitted cargo tests"))?;
    let mermaid_setup_position = ci
        .find("- name: Install Mermaid parser")
        .ok_or_else(|| io::Error::other("CI Mermaid setup step is missing"))?;
    if mermaid_setup_position > first_cargo_test {
        return Err(io::Error::other("CI Mermaid setup must precede its first cargo test").into());
    }
    if !mermaid_setup_step
        .contains("if: contains(fromJSON(needs.plan.outputs.plan).repository_contracts, 'mermaid')")
    {
        return Err(io::Error::other("CI Mermaid setup must follow the affected plan").into());
    }
    for event in ["pull_request_review:", "pull_request_review_comment:"] {
        if ci.contains(event) || pr_state.contains(event) {
            return Err(io::Error::other(format!(
                "review activity must not launch source or metadata workflow {event:?}"
            ))
            .into());
        }
    }
    if !mermaid_setup_step
        .contains("npm ci --ignore-scripts --no-audit --prefix .github/mermaid-parser")
    {
        return Err(io::Error::other("CI Mermaid setup must avoid an implicit audit").into());
    }
    let dependency_audit_step = ci
        .split("- name: Audit Mermaid dependencies")
        .nth(1)
        .and_then(|tail| tail.split("- name:").next())
        .ok_or_else(|| io::Error::other("CI dependency-audit step is missing"))?;
    for required in [
        "contains(fromJSON(needs.plan.outputs.plan).repository_contracts, 'dependency-audit')",
        "npm audit --omit=dev --audit-level=moderate --prefix .github/mermaid-parser",
    ] {
        if !dependency_audit_step.contains(required) {
            return Err(io::Error::other(format!(
                "CI dependency audit omitted affected gate {required:?}"
            ))
            .into());
        }
    }
    if checklist_step.contains(issueops_self_test_command)
        || checklist_step.contains("test-optional-parser-proof-inputs.py")
    {
        return Err(
            io::Error::other("CI mutable IssueOps check must not relaunch the self-test").into(),
        );
    }
    if checklist_step.contains("--milestone") {
        return Err(io::Error::other(
            "ordinary pull requests must not require full milestone completion",
        )
        .into());
    }
    for required in [
        "PR_BASE_SHA: ${{ needs.plan.outputs.base }}",
        "if [ \"$GITHUB_EVENT_NAME\" = \"pull_request\" ]; then",
        "git fetch --no-tags --depth=1 origin \"$PR_BASE_SHA\"",
        "--pull-request \"$PR_NUMBER\"",
        "--base \"$PR_BASE_SHA\"",
        "else",
    ] {
        if !checklist_step.contains(required) {
            return Err(io::Error::other(format!(
                "pull-request IssueOps step is missing branch-aware gate {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "types: [opened, reopened, synchronize, edited, milestoned, demilestoned]",
        "types: [closed, reopened, milestoned, demilestoned]",
        "group: projectatlas-pr-state-${{ github.event_name }}-${{ github.event.pull_request.number || github.event.issue.number }}-${{ github.run_id }}",
        "cancel-in-progress: false",
        "name: pr-state",
        "if: github.event_name == 'pull_request'",
        "name: refresh-pr-state",
        "if: github.event_name == 'issues'",
        "timeout-minutes: 2",
        "actions: write",
        "--refresh-pr-state-for-issue",
        "Validate issue reference and milestone",
        "gh api \"repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER\"",
        "name: Revalidate IssueOps for owner edits",
        "if: github.event.action == 'edited'",
        "ref: ${{ github.event.pull_request.head.sha }}",
        "--pull-request \"$PR_NUMBER\"",
        "--base \"$PR_BASE_SHA\"",
    ] {
        if !pr_state.contains(required) {
            return Err(io::Error::other(format!(
                "PR-state workflow omitted live metadata contract {required:?}"
            ))
            .into());
        }
    }
    let direct_pr_state_job = workflow_job_block(&pr_state, "pr-state")?;
    let refresh_pr_state_job = workflow_job_block(&pr_state, "refresh-pr-state")?;
    for forbidden in ["cargo ", "codex-pr-review-gate.py"] {
        if direct_pr_state_job.contains(forbidden) {
            return Err(io::Error::other(format!(
                "direct PR-state validation retained unrelated source work {forbidden:?}"
            ))
            .into());
        }
    }
    for forbidden in [
        "cargo ",
        "github.event.pull_request.head",
        "continue-on-error: true",
    ] {
        if refresh_pr_state_job.contains(forbidden) {
            return Err(io::Error::other(format!(
                "issue-event PR-state refresh retained unsafe behavior {forbidden:?}"
            ))
            .into());
        }
    }
    for forbidden in ["checks: write", "--refresh-pr-state-for-issue"] {
        if issueops_workflow.contains(forbidden) {
            return Err(io::Error::other(format!(
                "IssueOps retained duplicate PR-state refresh ownership {forbidden:?}"
            ))
            .into());
        }
    }
    for required in [
        "def pr_state_refreshes(",
        "open pull-request inventory reached the refresh bound",
        "actions/workflows/pr-state.yml/runs",
        "actions/runs/{workflow_run['id']}/rerun",
        "no PR-state workflow run found",
    ] {
        if !issueops.contains(required) {
            return Err(io::Error::other(format!(
                "PR-state refresh omitted fail-closed behavior {required:?}"
            ))
            .into());
        }
    }
    for (name, workflow, group) in [
        (
            "IssueOps",
            issueops_workflow.as_str(),
            "projectatlas-issueops-${{ github.run_id }}",
        ),
        (
            "publish",
            auto_release_workflow.as_str(),
            "projectatlas-publish-${{ github.run_id }}",
        ),
        (
            "deploy",
            docs_workflow.as_str(),
            "projectatlas-deploy-${{ github.run_id }}",
        ),
        (
            "release",
            release.as_str(),
            "projectatlas-release-${{ inputs.version }}",
        ),
    ] {
        if !workflow.contains(group) || !workflow.contains("cancel-in-progress: false") {
            return Err(io::Error::other(format!(
                "{name} workflow omitted its non-cancelling namespace"
            ))
            .into());
        }
    }
    if !release.contains("--milestone \"${{ steps.release_version.outputs.milestone }}\"")
        || !release.contains("cargo fmt --all --check")
        || !release.contains(FILTERED_CUSTOM_HARNESS_COMMAND)
        || !release.contains("test-optional-parser-proof-inputs.py")
    {
        return Err(io::Error::other(
            "release must retain milestone completion, ordinary gates, and a non-publishing package-proof mode",
        )
        .into());
    }
    assert_filtered_custom_harness_step(&release)?;
    for required in [
        "def tool_text(name, response):",
        "if not isinstance(response, dict):",
        "if \"error\" in response:",
        "is_error = result.get(\"isError\")",
        "if not isinstance(is_error, bool) or is_error:",
        "return tool_text(name, responses.get(2))",
        "tool_text(\"fault-probe\", invalid_response)",
        "scan = call_tool(\"atlas_scan\"",
        "if \"scan:\" not in scan:",
    ] {
        if !release.contains(required) {
            return Err(io::Error::other(format!(
                "release MCP proof omitted fail-closed response or scan contract {required:?}"
            ))
            .into());
        }
    }
    for required in [
        "types: [opened, edited, reopened, labeled, unlabeled, milestoned, demilestoned, closed]",
        "--planned-issue \"$ISSUE_NUMBER\"",
        "timeout-minutes: 5",
        "contents: read",
        "issues: read",
    ] {
        if !issueops_workflow.contains(required) {
            return Err(io::Error::other(format!(
                "issue-event IssueOps workflow is missing readiness guard {required:?}"
            ))
            .into());
        }
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
    let rc_first_gate = release
        .split("      - name: Enforce RC-first promotion")
        .nth(1)
        .and_then(|tail| tail.split("\n      - name:").next())
        .ok_or_else(|| io::Error::other("release omitted the RC-first promotion step"))?;
    if !rc_first_gate.contains(prepublish_guard) {
        return Err(io::Error::other(
            "hosted RC-first admission must not block non-publishing package proof",
        )
        .into());
    }
    let exact_main_gate = release
        .split("      - name: Require exact main head for publication")
        .nth(1)
        .and_then(|tail| tail.split("\n      - name:").next())
        .ok_or_else(|| io::Error::other("release omitted the exact-main publication step"))?;
    if !exact_main_gate.contains(prepublish_guard)
        || !exact_main_gate.contains("git fetch --force origin main:refs/remotes/origin/main")
        || !exact_main_gate.contains("refs/remotes/origin/main^{commit}")
    {
        return Err(io::Error::other(
            "exact-main publication admission must preserve branch prepublication proof",
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
    if !publish_header.contains(prepublish_guard) || release.matches(prepublish_guard).count() != 4
    {
        return Err(io::Error::other(
            "prepublish-only guard must own exactly the RC-first, exact-main, checklist, and publish boundaries",
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
        "      - verify\n      - prepublish-installer-smoke-unix\n      - prepublish-installer-smoke-windows\n      - parser-pack-assets",
        "pattern: projectatlas-*",
    ] {
        if !release.contains(required) {
            return Err(io::Error::other(format!(
                "release omitted packaged CLI contract wiring {required:?}"
            ))
            .into());
        }
    }
    let exact_contracts = release.matches("--exact").count();
    if exact_contracts == 0
        || release
            .matches("--exact --include-ignored --nocapture")
            .count()
            != exact_contracts
    {
        return Err(io::Error::other(
            "every exact release contract must execute even when marked ignored",
        )
        .into());
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
    let hosted_tui_step = ci
        .split("      - name: Capture hosted PTY token dashboards")
        .nth(1)
        .and_then(|tail| tail.split("\n      - name:").next())
        .ok_or_else(|| io::Error::other("ordinary CI omitted hosted PTY TUI evidence"))?;
    for required in [
        "if: matrix.label != 'windows'",
        "timeout-minutes: 5",
        "script -q",
        "stty rows \"$PROJECTATLAS_TUI_ROWS\" cols \"$PROJECTATLAS_TUI_COLUMNS\"",
        "capture compact-overview-40x8 8 40 compact-overview",
        "capture full-overview-80x50 50 80 full-overview",
        "capture compact-trend-79x29 29 79 compact-trend",
        "capture full-trend-80x30 30 80 full-trend",
        "--session \"界\" --view tui",
        "--view tui --theme light",
        "--view tui --trend month",
        "test -s \"$output\"",
        "grep -a -q \"Token\" \"$output\"",
    ] {
        if !hosted_tui_step.contains(required) {
            return Err(
                io::Error::other(format!("hosted PTY TUI evidence omitted {required:?}")).into(),
            );
        }
    }
    let hosted_tui_upload = ci
        .split("      - name: Upload hosted PTY token dashboards")
        .nth(1)
        .and_then(|tail| tail.split("\n      - name:").next())
        .ok_or_else(|| io::Error::other("ordinary CI omitted hosted PTY TUI upload"))?;
    for required in [
        "if: matrix.label != 'windows'",
        "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        "name: token-tui-pty-${{ matrix.label }}",
        "if-no-files-found: error",
    ] {
        if !hosted_tui_upload.contains(required) {
            return Err(
                io::Error::other(format!("hosted PTY TUI upload omitted {required:?}")).into(),
            );
        }
    }
    let btrfs_contract = "linux_btrfs_subvolume_database_supports_cli_and_persistent_mcp_reopen";
    let ci_btrfs_step = ci
        .split("      - name: Native Btrfs database placement contract")
        .nth(1)
        .and_then(|tail| tail.split("\n      - name:").next())
        .ok_or_else(|| io::Error::other("ordinary CI omitted the native Btrfs contract step"))?;
    let release_btrfs_step = unix_prepublish
        .split("      - name: Installed-candidate native Btrfs database placement contract")
        .nth(1)
        .ok_or_else(|| {
            io::Error::other(
                "Unix prepublish omitted the installed-candidate native Btrfs contract step",
            )
        })?;
    for (scope, step, platform_guard) in [
        ("ordinary CI", ci_btrfs_step, "if: matrix.label == 'linux'"),
        (
            "installed candidate",
            release_btrfs_step,
            "if: matrix.label == 'linux-x64-posix'",
        ),
    ] {
        for required in [
            platform_guard,
            "timeout-minutes: 10",
            "sudo apt-get install --yes btrfs-progs",
            "truncate -s 256M \"$image\"",
            "sudo mkfs.btrfs -f \"$image\"",
            "trap cleanup EXIT",
            "sudo mount -o loop \"$image\" \"$mount_root\"",
            "sudo btrfs subvolume create \"$subvolume\"",
            "sudo umount \"$mount_root\"",
            "PROJECTATLAS_BTRFS_TEST_ROOT=\"$subvolume\"",
            "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE=",
            btrfs_contract,
            "--exact --include-ignored --nocapture",
        ] {
            if !step.contains(required) {
                return Err(io::Error::other(format!(
                    "{scope} native Btrfs contract omitted {required:?}"
                ))
                .into());
            }
        }
    }
    if windows_prepublish.contains(btrfs_contract) {
        return Err(io::Error::other(
            "Windows prepublish must not execute the Linux-only native Btrfs contract",
        )
        .into());
    }
    for (job, body) in [("Unix", unix_prepublish), ("Windows", windows_prepublish)] {
        let packaged_step_name = "- name: Install packaged runtime through plugin";
        let installed_candidate_step_name =
            "- name: Installed-candidate regression and upgrade contracts";
        for (step_name, label) in [
            (packaged_step_name, "packaged contract"),
            (
                installed_candidate_step_name,
                "installed-candidate contract",
            ),
        ] {
            if body.matches(step_name).count() != 1 {
                return Err(io::Error::other(format!(
                    "{job} prepublish must own exactly one {label} step"
                ))
                .into());
            }
        }
        let packaged_step = body
            .split(packaged_step_name)
            .nth(1)
            .and_then(|tail| tail.split(installed_candidate_step_name).next())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "{job} prepublish omitted the packaged contract step"
                ))
            })?;
        for contract in [
            "mcp_advertised_tools_own_their_real_sqlite_effects",
            "mcp_tools_list_preserves_frozen_contracts_without_index_state",
            "packaged_cli_surface_preserves_frozen_routes_and_defaults",
            "packaged_cli_commands_own_their_real_sqlite_effects",
        ] {
            if !packaged_step.contains(contract) {
                return Err(io::Error::other(format!(
                    "{job} prepublish omitted packaged contract {contract:?}"
                ))
                .into());
            }
        }
        let installed_candidate_step = body
            .split(installed_candidate_step_name)
            .nth(1)
            .and_then(|tail| tail.split("\n      - name:").next())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "{job} prepublish omitted the installed-candidate regression and upgrade step"
                ))
            })?;
        for contract in [
            "installed_candidate_version_is_consistent_across_cli_runtime_and_token_tui",
            "csharp_symbol_identity_boundary_preserves_full_and_incremental_publication",
            "deep_qualified_symbol_parents_preserve_full_and_incremental_publication",
            "partial_markdown_limit_persists_without_losing_complete_publication",
            "lint_formats_share_typed_cli_and_mcp_report",
            "token_tui_cli_respects_selected_terminal_viewport",
            "classified_document_navigation_agrees_across_cli_and_mcp",
            "conditional_purpose_review_cli_reconciles_source_before_apply",
            "persistent_mcp_purpose_review_reconciles_source_before_apply",
            "persistent_mcp_stdin_does_not_block_repository_startup_probes",
            "installed_candidate_without_git_keeps_navigation_and_typed_vcs_unavailability",
            "compiler_config_utf8_bom_refreshes_through_cli_and_mcp",
            "supported_predecessor_recovery_preserves_explicit_database_selection",
            "plugin_update_replaces_stale_runtime_configs_and_launches_new_mcp",
            "plugin_update_preserves_prior_integration_when_all_replacement_adds_fail",
            "plugin_update_refuses_unavailable_or_ambiguous_inventory",
            "plugin_update_serializes_restore_before_the_next_installer_reads_state",
            "plugin_update_refuses_retained_recovery_state_before_mutation",
        ] {
            if !installed_candidate_step.contains(contract) {
                return Err(io::Error::other(format!(
                    "{job} installed-candidate step omitted regression or upgrade contract {contract:?}"
                ))
                .into());
            }
        }
        let platform_contracts: &[&str] = if job == "Unix" {
            &[
                "posix_plugin_inventory_without_jq_rejects_split_object_fields",
                "posix_plugin_restore_rejects_hostile_paths_and_retains_recovery_state",
            ]
        } else {
            &[
                "windows_plugin_restore_rejects_cache_junction_and_retains_recovery_snapshot",
                "windows_plugin_snapshot_rejects_reparse_above_codex_home_before_mutation",
                "windows_plugin_snapshot_cleanup_refuses_path_swap_without_outside_deletion",
                "windows_plugin_snapshot_cleanup_failure_retains_usable_direct_snapshot",
                "windows_plugin_update_fails_closed_when_lock_root_cannot_be_canonicalized",
                "windows_plugin_restore_rejects_config_directory_and_retains_recovery_snapshot",
            ]
        };
        for contract in platform_contracts {
            if !installed_candidate_step.contains(contract) {
                return Err(io::Error::other(format!(
                    "{job} installed-candidate step omitted platform contract {contract:?}"
                ))
                .into());
            }
        }
        for required in [
            "timeout-minutes: 5",
            "--exact --include-ignored --nocapture",
        ] {
            if !packaged_step.contains(required) {
                return Err(io::Error::other(format!(
                    "{job} packaged contract step omitted fail-closed contract {required:?}"
                ))
                .into());
            }
        }
        for required in [
            "timeout-minutes: 10",
            "--exact --include-ignored --nocapture",
        ] {
            if !installed_candidate_step.contains(required) {
                return Err(io::Error::other(format!(
                    "{job} installed-candidate step omitted fail-closed contract {required:?}"
                ))
                .into());
            }
        }
        for (scope, step) in [
            ("Packaged", packaged_step),
            ("Installed-candidate", installed_candidate_step),
        ] {
            let inventory_contracts: &[&str] = if job == "Unix" {
                &[
                    "if ! inventory=\"$(\"$contract\" --list)\"; then",
                    "awk -v expected=\"${test}: test\" '$0 == expected { matches += 1 } END { print matches + 0 }'",
                    "if [ \"$matches\" -ne 1 ]; then",
                    "exit 1",
                ]
            } else {
                &[
                    "$inventory = @(& pwsh -NoProfile -File $contractRunner --list)",
                    "$LASTEXITCODE -ne 0",
                    "Where-Object { $_ -ceq \"${test}: test\" }",
                    "if ($matches -ne 1) {",
                ]
            };
            for required in inventory_contracts {
                if !step.contains(required) {
                    return Err(io::Error::other(format!(
                        "{job} {scope} contract step omitted exact inventory guard {required:?}"
                    ))
                    .into());
                }
            }
            for required in [
                format!("{scope} contract inventory failed"),
                format!("{scope} contract inventory must contain exactly one"),
            ] {
                if !step.contains(&required) {
                    return Err(io::Error::other(format!(
                        "{job} {scope} contract step omitted inventory failure contract {required:?}"
                    ))
                    .into());
                }
            }
        }
        let platform_contracts: &[&str] = if job == "Unix" {
            &[
                "set -euo pipefail",
                "runtime=\"$RUNNER_TEMP/projectatlas-prepublish/projectatlas/projectatlas\"",
                "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE=\"$runtime\"",
                "PROJECTATLAS_MCP_CONTRACT_PLUGIN_ROOT=\"$GITHUB_WORKSPACE/plugins/projectatlas\"",
            ]
        } else {
            &[
                "$env:PROJECTATLAS_MCP_CONTRACT_EXECUTABLE = Join-Path $env:RUNNER_TEMP \"projectatlas-prepublish\\projectatlas.exe\"",
                "$env:PROJECTATLAS_MCP_CONTRACT_PLUGIN_ROOT = Join-Path $env:GITHUB_WORKSPACE \"plugins\\projectatlas\"",
                "foreach ($test in @(",
                "$LASTEXITCODE -ne 0",
                "throw \"Installed-candidate contract '$test' failed with exit code $LASTEXITCODE.\"",
            ]
        };
        for required in platform_contracts {
            if !installed_candidate_step.contains(required) {
                return Err(io::Error::other(format!(
                    "{job} installed-candidate step omitted platform contract {required:?}"
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
    let declared_channel = toolchain
        .lines()
        .find(|line| line.trim_start().starts_with("channel ="))
        .and_then(|line| line.split('"').nth(1));
    let declared_parts = declared_channel.map(|channel| channel.split('.').collect::<Vec<_>>());
    if toolchain.matches("channel =").count() != 1
        || declared_parts.as_ref().is_none_or(|parts| {
            parts.len() != 3
                || parts
                    .iter()
                    .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        })
    {
        return Err(io::Error::other(
            "Rust toolchain must be repository-owned and pinned exactly once",
        )
        .into());
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
fn pre_push_dispatch_follows_pushed_remote_targets() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let temp = tempfile::tempdir()?;
    let fixture_repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir_all(fixture_repo.join(GITHOOKS_DIR_NAME))?;
    fs::create_dir_all(fixture_repo.join(".github").join("scripts"))?;
    fs::create_dir_all(
        fixture_repo
            .join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join(ISSUEOPS_CHANGE_NAME),
    )?;
    fs::copy(
        workspace_root
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
        fixture_repo
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
    )?;
    fs::write(
        fixture_repo
            .join(".github")
            .join("scripts")
            .join(ISSUE_CHECKLISTS_SCRIPT_FILE_NAME),
        "",
    )?;
    fs::write(
        fixture_repo
            .join(OPENSPEC_DIR_NAME)
            .join(ISSUE_MAP_FILE_NAME),
        "{\"schema_version\": 2, \"changes\": {}}\n",
    )?;
    let issueops_tasks = fixture_repo
        .join(OPENSPEC_DIR_NAME)
        .join(CHANGE_DIR_NAME)
        .join(ISSUEOPS_CHANGE_NAME)
        .join(TASKS_FILE_NAME);
    fs::write(&issueops_tasks, "- [x] 1.1 baseline\n")?;
    fs::write(fixture_repo.join(CANDIDATE_FILE_NAME), "candidate\n")?;
    git_success(&fixture_repo, &["init", "--initial-branch=main"])?;
    git_success(
        &fixture_repo,
        &["config", "user.email", "test@example.invalid"],
    )?;
    git_success(&fixture_repo, &["config", "user.name", "ProjectAtlas test"])?;
    git_success(&fixture_repo, &["add", "."])?;
    git_success(&fixture_repo, &["commit", "-m", "baseline (#549)"])?;
    let base_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !base_output.status.success() {
        return Err(io::Error::other("pre-push fixture base commit lookup failed").into());
    }
    let base = String::from_utf8(base_output.stdout)?.trim().to_owned();
    git_success(&fixture_repo, &["checkout", "-b", "feature"])?;
    git_success(
        &fixture_repo,
        &["commit", "--allow-empty", "-m", "candidate (#549)"],
    )?;
    git_success(
        &fixture_repo,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "follow-up candidate (#549)",
        ],
    )?;
    git_success(
        &fixture_repo,
        &["update-ref", "refs/remotes/origin/main", &base],
    )?;
    fs::create_dir(&fake_path)?;
    let dispatch_log = temp.path().join(DISPATCH_LOG_FILE_NAME);
    let python_stub = r#"#!/bin/sh
printf 'python3 %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
case " $* " in
  *"affected-ci-proof.py plan "*)
    printf '%s\n' '{"mode":"narrow","repository_contracts":["issueops","mermaid"],"rust_packages":[],"test_targets":[],"test_only":false,"jobs":{"rust":false}}'
    exit 0
    ;;
  *" -c "*) exec python "$@" ;;
  *" --owner-from-commits "*)
    awk '
      {
        matches = 0
        remainder = $0
        while (match(remainder, /\(#[1-9][0-9]*\)/)) {
          matches++
          owner = substr(remainder, RSTART + 2, RLENGTH - 3)
          remainder = substr(remainder, RSTART + RLENGTH)
        }
        if (matches != 1) {
          invalid = 1
          next
        }
        if (resolved == "") resolved = owner
        else if (resolved != owner) invalid = 1
      }
      END {
        if (invalid || NR == 0 || resolved == "") exit 1
        print resolved
      }
    '
    exit $?
    ;;
esac
exit 0
"#;
    let cargo_stub = r#"#!/bin/sh
printf 'cargo %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let npm_stub = r#"#!/bin/sh
printf 'npm %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let gh_stub = r#"#!/bin/sh
printf 'gh %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
if [ "${1:-}" = repo ] && [ "${2:-}" = view ]; then
  printf '%s\n' styler-ai/ProjectAtlas
fi
exit 0
"#;
    for (name, script) in [
        ("python3", python_stub),
        ("cargo", cargo_stub),
        ("npm", npm_stub),
        ("gh", gh_stub),
    ] {
        write_executable_script(&fake_path.join(name), script)?;
    }
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(fake_path).chain(std::env::split_paths(&current_path)),
    )?;
    let hook = fixture_repo
        .join(GITHOOKS_DIR_NAME)
        .join(PRE_PUSH_HOOK_FILE_NAME);
    let shell = if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")
    } else {
        PathBuf::from("sh")
    };
    let head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()?;
    if !head_output.status.success() {
        return Err(io::Error::other(format!(
            "git rev-parse --verify HEAD^{{commit}} failed: {}{}",
            String::from_utf8_lossy(&head_output.stdout),
            String::from_utf8_lossy(&head_output.stderr)
        ))
        .into());
    }
    let current_head = String::from_utf8(head_output.stdout)?.trim().to_owned();
    if current_head.is_empty() {
        return Err(io::Error::other("git rev-parse returned an empty HEAD").into());
    }
    let run_hook = |records: &str, fail_ls_files: bool| -> Result<(bool, String), Box<dyn Error>> {
        fs::write(&dispatch_log, "")?;
        let mut command = StdCommand::new(&shell);
        command.current_dir(&fixture_repo);
        if fail_ls_files {
            command
                .arg("-c")
                .arg(
                    r#"git() {
  printf 'git %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
  if [ "${1:-}" = ls-files ] && [ "${2:-}" = -v ]; then
    printf 'git ls-files forced failure\n' >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
    return 1
  fi
  command git "$@"
}
. "$1"
"#,
                )
                .arg("projectatlas-pre-push")
                .arg(&hook);
        } else {
            command.arg(&hook);
        }
        command
            .env("PATH", &test_path)
            .env("PROJECTATLAS_HOOK_DISPATCH_LOG", &dispatch_log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("pre-push hook stdin was not piped"))?
            .write_all(records.as_bytes())?;
        let output = child.wait_with_output()?;
        Ok((output.status.success(), fs::read_to_string(&dispatch_log)?))
    };
    let final_issueops = |log: &str| {
        log.lines()
            .filter(|line| line.contains("issue-checklists.py --repo"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (empty_status, empty_log) = run_hook("", false)?;
    if empty_status
        || !final_issueops(&empty_log).is_empty()
        || empty_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "empty pre-push input did not fail closed before validation:\n{empty_log}"
        ))
        .into());
    }
    let (short_record_status, short_record_log) = run_hook(
        "refs/heads/fix/549 1111111111111111111111111111111111111111 refs/heads/fix/549\n",
        false,
    )?;
    if short_record_status
        || !final_issueops(&short_record_log).is_empty()
        || short_record_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "malformed pre-push input did not fail closed before validation:\n{short_record_log}"
        ))
        .into());
    }
    let (main_status, main_target) = run_hook(
        &format!(
            "refs/heads/fix/549 {current_head} refs/heads/main 2222222222222222222222222222222222222222\n"
        ),
        false,
    )?;
    let main_calls = final_issueops(&main_target);
    if !main_status || main_calls.len() != 1 || main_calls[0].contains("--candidate-issue") {
        return Err(io::Error::other(format!(
            "feature-checkout push to refs/heads/main did not use global IssueOps dispatch:\n{main_target}"
        ))
        .into());
    }
    for zero_remote_oid in ["0".repeat(40), "0".repeat(64)] {
        let (new_main_status, new_main_target) = run_hook(
            &format!("refs/heads/fix/549 {current_head} refs/heads/main {zero_remote_oid}\n"),
            false,
        )?;
        let new_main_calls = final_issueops(&new_main_target);
        if !new_main_status
            || new_main_calls.len() != 1
            || new_main_calls[0].contains("--candidate-issue")
            || !new_main_target.contains(&format!("--base {current_head}"))
            || !new_main_target.contains("--force-full")
        {
            return Err(io::Error::other(format!(
                "new main push did not use complete global proof:\n{new_main_target}"
            ))
            .into());
        }
    }
    let (multiple_candidate_status, multiple_candidate_log) = run_hook(
        "refs/heads/fix/549 1111111111111111111111111111111111111111 refs/heads/fix/549 2222222222222222222222222222222222222222\nrefs/heads/fix/547 3333333333333333333333333333333333333333 refs/heads/fix/547 4444444444444444444444444444444444444444\n",
        false,
    )?;
    if multiple_candidate_status || !final_issueops(&multiple_candidate_log).is_empty() {
        return Err(io::Error::other(format!(
            "multiple candidate refs did not fail closed before IssueOps dispatch:\n{multiple_candidate_log}"
        ))
        .into());
    }
    let mismatched_candidate_records = format!(
        "refs/heads/fix/549 {} refs/heads/fix/549 2222222222222222222222222222222222222222\n",
        if current_head == "1111111111111111111111111111111111111111" {
            "2222222222222222222222222222222222222222"
        } else {
            "1111111111111111111111111111111111111111"
        }
    );
    let (mismatched_candidate_status, mismatched_candidate_log) =
        run_hook(&mismatched_candidate_records, false)?;
    if mismatched_candidate_status
        || !final_issueops(&mismatched_candidate_log).is_empty()
        || mismatched_candidate_log.contains("--candidate-issue")
        || mismatched_candidate_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "candidate local object mismatch did not fail closed before scoped validation:\n{mismatched_candidate_log}"
        ))
        .into());
    }
    let zero_candidate_records = "refs/heads/fix/549 0000000000000000000000000000000000000000 refs/heads/fix/549 2222222222222222222222222222222222222222\n";
    let (zero_candidate_status, zero_candidate_log) = run_hook(zero_candidate_records, false)?;
    if zero_candidate_status
        || !final_issueops(&zero_candidate_log).is_empty()
        || zero_candidate_log.contains("--candidate-issue")
        || zero_candidate_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "candidate deletion did not fail closed before scoped validation:\n{zero_candidate_log}"
        ))
        .into());
    }
    let (zero_main_status, zero_main_log) = run_hook(
        "refs/heads/main 0000000000000000000000000000000000000000 refs/heads/main 2222222222222222222222222222222222222222\n",
        false,
    )?;
    if zero_main_status
        || !final_issueops(&zero_main_log).is_empty()
        || zero_main_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "main deletion did not fail before IssueOps dispatch:\n{zero_main_log}"
        ))
        .into());
    }
    let (candidate_status, candidate_target) = run_hook(
        &format!(
            "refs/heads/fix/549 {current_head} refs/heads/fix/549 2222222222222222222222222222222222222222\n"
        ),
        false,
    )?;
    let candidate_calls = final_issueops(&candidate_target);
    if !candidate_status
        || candidate_calls.len() != 1
        || !candidate_calls[0].contains("--candidate-issue 549")
    {
        return Err(io::Error::other(format!(
            "ordinary candidate push did not use candidate IssueOps dispatch:\n{candidate_target}"
        ))
        .into());
    }
    git_success(&fixture_repo, &["checkout", "-b", "unowned-candidate"])?;
    git_success(
        &fixture_repo,
        &["commit", "--allow-empty", "-m", "unowned follow-up"],
    )?;
    let unowned_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()?;
    if !unowned_head_output.status.success() {
        return Err(io::Error::other("fixture unowned candidate commit lookup failed").into());
    }
    let unowned_head = String::from_utf8(unowned_head_output.stdout)?
        .trim()
        .to_owned();
    let (unowned_status, unowned_log) = run_hook(
        &format!(
            "refs/heads/unowned-candidate {unowned_head} refs/heads/unowned-candidate 2222222222222222222222222222222222222222\n"
        ),
        false,
    )?;
    if unowned_status
        || !unowned_log.contains("--owner-from-commits")
        || !final_issueops(&unowned_log).is_empty()
    {
        return Err(io::Error::other(format!(
            "referenced and unreferenced candidate commits did not fail before scoped IssueOps dispatch:\n{unowned_log}"
        ))
        .into());
    }
    git_success(&fixture_repo, &["checkout", "feature"])?;
    let (index_failure_status, index_failure_log) = run_hook(
        &format!(
            "refs/heads/fix/549 {current_head} refs/heads/fix/549 2222222222222222222222222222222222222222\n"
        ),
        true,
    )?;
    if index_failure_status
        || !index_failure_log.contains("git ls-files forced failure")
        || !final_issueops(&index_failure_log).is_empty()
        || index_failure_log.contains("--owner-from-commits")
        || index_failure_log.contains("git merge-base")
        || index_failure_log.contains("git log")
    {
        return Err(io::Error::other(format!(
            "tracked index inspection failure did not fail before scoped validation:\n{index_failure_log}"
        ))
        .into());
    }
    for index_flag in ["--assume-unchanged", "--skip-worktree"] {
        git_success(
            &fixture_repo,
            &[
                "update-index",
                "--no-assume-unchanged",
                ISSUEOPS_TASKS_RELATIVE_PATH,
            ],
        )?;
        git_success(
            &fixture_repo,
            &[
                "update-index",
                "--no-skip-worktree",
                ISSUEOPS_TASKS_RELATIVE_PATH,
            ],
        )?;
        git_success(
            &fixture_repo,
            &["update-index", index_flag, ISSUEOPS_TASKS_RELATIVE_PATH],
        )?;
        fs::write(&issueops_tasks, "- [ ] 1.1 hidden index state\n")?;
        let status_output = git_command_for_root(&fixture_repo)
            .args(["status", "--porcelain=v1", "--untracked-files=all"])
            .output()?;
        if !status_output.status.success() || !status_output.stdout.is_empty() {
            return Err(io::Error::other(format!(
                "{index_flag} did not hide the tracked fixture edit from porcelain"
            ))
            .into());
        }
        let (hidden_status, hidden_log) = run_hook(
            &format!(
                "refs/heads/fix/549 {current_head} refs/heads/fix/549 2222222222222222222222222222222222222222\n"
            ),
            false,
        )?;
        if hidden_status
            || !final_issueops(&hidden_log).is_empty()
            || hidden_log.contains("--owner-from-commits")
        {
            return Err(io::Error::other(format!(
                "{index_flag} tracked fixture edit did not fail before scoped IssueOps dispatch:\n{hidden_log}"
            ))
            .into());
        }
        fs::write(&issueops_tasks, "- [x] 1.1 baseline\n")?;
        git_success(
            &fixture_repo,
            &[
                "update-index",
                "--no-assume-unchanged",
                ISSUEOPS_TASKS_RELATIVE_PATH,
            ],
        )?;
        git_success(
            &fixture_repo,
            &[
                "update-index",
                "--no-skip-worktree",
                ISSUEOPS_TASKS_RELATIVE_PATH,
            ],
        )?;
    }
    let (mixed_status, mixed_log) = run_hook(
        "refs/heads/fix/549 1111111111111111111111111111111111111111 refs/heads/fix/549 2222222222222222222222222222222222222222\nrefs/heads/fix/549 1111111111111111111111111111111111111111 refs/heads/main 2222222222222222222222222222222222222222\n",
        false,
    )?;
    if mixed_status
        || !final_issueops(&mixed_log).is_empty()
        || mixed_log.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
            "mixed candidate/main push did not fail before IssueOps dispatch:\n{mixed_log}"
        ))
        .into());
    }
    let (malformed_status, malformed_log) = run_hook(
        "refs/heads/fix/549 1111111111111111111111111111111111111111 refs/tags/v0.5.0 2222222222222222222222222222222222222222\n",
        false,
    )?;
    if malformed_status || !final_issueops(&malformed_log).is_empty() {
        return Err(io::Error::other(format!(
            "unsupported pre-push target did not fail closed before IssueOps dispatch:\n{malformed_log}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn pre_push_candidate_rejects_ignored_linked_document_outside_tree() -> Result<(), Box<dyn Error>> {
    const LINKED_DOCUMENT_DIR_NAME: &str = "docs";
    const LINKED_DOCUMENT_FILE_NAME: &str = "ignored.md";
    const LINKED_DOCUMENT_RELATIVE_PATH: &str = "docs/ignored.md";

    let workspace_root = workspace_root()?;
    let temp = tempfile::tempdir()?;
    let fixture_repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    let issue_payload = temp.path().join("issue.json");
    let dispatch_log = temp.path().join(DISPATCH_LOG_FILE_NAME);
    let hook = fixture_repo
        .join(GITHOOKS_DIR_NAME)
        .join(PRE_PUSH_HOOK_FILE_NAME);
    let issueops_tasks = fixture_repo
        .join(OPENSPEC_DIR_NAME)
        .join(CHANGE_DIR_NAME)
        .join(ISSUEOPS_CHANGE_NAME)
        .join(TASKS_FILE_NAME);
    fs::create_dir_all(fixture_repo.join(GITHOOKS_DIR_NAME))?;
    fs::create_dir_all(fixture_repo.join(".github").join("scripts"))?;
    fs::create_dir_all(
        issueops_tasks
            .parent()
            .ok_or_else(|| io::Error::other("issueops tasks fixture has no parent"))?,
    )?;
    fs::create_dir_all(&fake_path)?;
    fs::copy(
        workspace_root
            .join(GITHOOKS_DIR_NAME)
            .join(PRE_PUSH_HOOK_FILE_NAME),
        &hook,
    )?;
    fs::copy(
        workspace_root
            .join(".github")
            .join("scripts")
            .join(ISSUE_CHECKLISTS_SCRIPT_FILE_NAME),
        fixture_repo
            .join(".github")
            .join("scripts")
            .join(ISSUE_CHECKLISTS_SCRIPT_FILE_NAME),
    )?;
    fs::write(
        fixture_repo
            .join(OPENSPEC_DIR_NAME)
            .join(ISSUE_MAP_FILE_NAME),
        format!("{{\"schema_version\": 2, \"changes\": {{\"{ISSUEOPS_CHANGE_NAME}\": 549}}}}\n"),
    )?;
    fs::write(&issueops_tasks, "- [x] 1.1 baseline\n")?;
    let issue_body = r"## Why

Candidate validation needs the linked architecture document from its committed tree.

## What Changes

Reject candidate validation when linked documentation is absent from the candidate tree.

## Capabilities

- `release-issueops`: validates candidate documentation from committed inputs.

## Architecture Diagrams

- [Issue task authority](https://github.com/styler-ai/ProjectAtlas/blob/main/docs/ignored.md#user-content-issue-task-authority)

## Release Scope

This is a release-tooling correctness fix for v0.5.0-00.

## Non-Goals

- Changing product behavior.

## Pre-Mortem

Likely failure modes:
- A candidate reads an ignored document that is absent from its tree.

Mitigations:
- [x] Require committed linked documentation. (Implementation tasks: 1.1)

## Implementation Tasks

- [x] 1.1 baseline

## Acceptance and Review Tasks

- [ ] Intent and outcome review: Confirm the delivered behavior solves the complete issue `Why` and `What Changes`, provides the declared capabilities and release scope, and respects the non-goals at the real user or agent boundary.
- [ ] Implementation review: Review the complete implementation for correctness, architecture and ownership, applicable Rust and database pattern fit, security, resource bounds, compatibility, and unnecessary complexity; resolve every material finding.
- [ ] Specification and architecture review: Reconcile the issue, OpenSpec requirements and tasks, source, documentation, and every required architecture diagram; add missing specifications or diagrams or record a reasoned N/A when no view is needed.
- [ ] Test and proof review: Confirm the owning unit, integration, E2E, fault, concurrency, performance, and platform tests required by the issue are sound, causally exercise real behavior, and cover positive, negative, failure, and compatibility outcomes.
- [ ] Final readiness review: Confirm every implementation task is complete, all human and automated review feedback is resolved or dispositioned, required local and hosted gates pass, and no behavior or proof boundary remains partial.
";
    fs::write(
        &issue_payload,
        serde_json::to_vec(&json!({
            "state": "OPEN",
            "number": 549,
            "labels": [{"name": "complexity:medium"}],
            "milestone": {"title": "v0.5.0-00"},
            "body": issue_body,
        }))?,
    )?;
    git_success(
        &fixture_repo,
        &["init", "--object-format=sha256", "--initial-branch=main"],
    )?;
    git_success(
        &fixture_repo,
        &["config", "user.email", "test@example.invalid"],
    )?;
    git_success(&fixture_repo, &["config", "user.name", "ProjectAtlas test"])?;
    git_success(&fixture_repo, &["add", "."])?;
    git_success(&fixture_repo, &["commit", "-m", "baseline (#549)"])?;
    let base_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !base_output.status.success() {
        return Err(io::Error::other("ignored-document fixture base lookup failed").into());
    }
    let base = String::from_utf8(base_output.stdout)?.trim().to_owned();
    git_success(&fixture_repo, &["checkout", "-b", "feature"])?;
    git_success(
        &fixture_repo,
        &["commit", "--allow-empty", "-m", "candidate (#549)"],
    )?;
    let head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !head_output.status.success() {
        return Err(io::Error::other("ignored-document fixture HEAD lookup failed").into());
    }
    let head = String::from_utf8(head_output.stdout)?.trim().to_owned();
    git_success(
        &fixture_repo,
        &["update-ref", "refs/remotes/origin/main", &base],
    )?;
    fs::create_dir_all(fixture_repo.join("docs"))?;
    fs::write(
        fixture_repo.join(GIT_DIR_NAME).join("info").join("exclude"),
        "docs/ignored.md\n",
    )?;
    fs::write(
        fixture_repo
            .join(LINKED_DOCUMENT_DIR_NAME)
            .join(LINKED_DOCUMENT_FILE_NAME),
        "## Issue task authority\n\n```mermaid\nflowchart LR\nA --> B\n```\n",
    )?;
    let status_output = git_command_for_root(&fixture_repo)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()?;
    if !status_output.status.success() || !status_output.stdout.is_empty() {
        return Err(io::Error::other(
            "ignored linked document was not hidden from ordinary Git status",
        )
        .into());
    }
    let python_stub = r#"#!/bin/sh
printf 'python3 %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
case " $* " in
  *"affected-ci-proof.py plan "*)
    printf '%s\n' '{"mode":"narrow","repository_contracts":["issueops","mermaid"],"rust_packages":[],"test_targets":[],"test_only":false,"jobs":{"rust":false}}'
    exit 0
    ;;
  *" -c "*) exec python "$@" ;;
  *" --owner-from-commits "*) exec python "$@" ;;
  *" --candidate-issue "*) exec python -c '
import os
import json
import pathlib
import sys
import types

script = sys.argv[1]
sys.argv = sys.argv[1:]
module = types.ModuleType("issue_checklists_fixture")
module.__file__ = script
sys.modules[module.__name__] = module
exec(compile(pathlib.Path(script).read_bytes(), script, "exec"), module.__dict__, module.__dict__)
module.__dict__["gh_json"] = lambda _args: json.loads(
    pathlib.Path(os.environ["PROJECTATLAS_ISSUE_PAYLOAD"]).read_text(encoding="utf-8")
)
module.__dict__["gh_api_json"] = lambda _args: json.loads(
    "[{\"data\":{\"repository\":{\"issues\":{\"nodes\":[{\"number\":549,\"state\":\"OPEN\",\"labels\":{\"totalCount\":1,\"nodes\":[{\"name\":\"complexity:medium\"}]},\"milestone\":{\"title\":\"v0.5.0-00\"}}],\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null}}}}}]"
)
def fake_mermaid(diagram):
    outcome = module.__dict__["MermaidValidationOutcome"]
    return outcome.INVALID if "not-mermaid" in diagram else outcome.VALID

module.__dict__["_run_mermaid_parser"] = fake_mermaid
module.__dict__["mermaid_syntax_is_valid"].cache_clear()
module.__dict__["main"]()
' "$@" ;;
esac
exit 0
"#;
    let cargo_stub = r#"#!/bin/sh
printf 'cargo %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let npm_stub = r#"#!/bin/sh
printf 'npm %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let gh_stub = r#"#!/bin/sh
printf 'gh %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
if [ "${1:-}" = repo ] && [ "${2:-}" = view ]; then
  printf '%s\n' styler-ai/ProjectAtlas
elif [ "${1:-}" = issue ] && [ "${2:-}" = view ]; then
  cat "$PROJECTATLAS_ISSUE_PAYLOAD"
elif [ "${1:-}" = api ] && [ "${2:-}" = graphql ]; then
  printf '%s\n' '[{"data":{"repository":{"issues":{"nodes":[{"number":549,"state":"OPEN","labels":{"totalCount":1,"nodes":[{"name":"complexity:medium"}]},"milestone":{"title":"v0.5.0-00"}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}]'
fi
exit 0
"#;
    for (name, script) in [
        ("python3", python_stub),
        ("cargo", cargo_stub),
        ("npm", npm_stub),
        ("gh", gh_stub),
    ] {
        write_executable_script(&fake_path.join(name), script)?;
    }
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(fake_path.clone()).chain(std::env::split_paths(&current_path)),
    )?;
    fs::write(&dispatch_log, "")?;
    let shell = if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")
    } else {
        PathBuf::from("sh")
    };
    let mut command = StdCommand::new(&shell);
    command
        .current_dir(&fixture_repo)
        .arg(&hook)
        .env("PATH", &test_path)
        .env("PROJECTATLAS_HOOK_DISPATCH_LOG", &dispatch_log)
        .env("PROJECTATLAS_ISSUE_PAYLOAD", &issue_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("ignored-document hook stdin was not piped"))?
        .write_all(format!(
            "refs/heads/feature {head} refs/heads/feature 2222222222222222222222222222222222222222\n"
        ).as_bytes())?;
    let output = child.wait_with_output()?;
    let dispatch = fs::read_to_string(&dispatch_log)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success()
        || !stderr.contains("has no tracked regular Markdown file in candidate tree")
        || !dispatch.contains("--candidate-issue 549")
    {
        return Err(io::Error::other(format!(
            "ignored linked document was not rejected from the candidate tree:\nstdout={}\nstderr={stderr}\ndispatch={dispatch}",
            String::from_utf8_lossy(&output.stdout),
        ))
        .into());
    }

    let linked_document = fixture_repo
        .join(LINKED_DOCUMENT_DIR_NAME)
        .join(LINKED_DOCUMENT_FILE_NAME);
    fs::write(
        &linked_document,
        "## Issue task authority\n\n```mermaid\nflowchart LR\nA --> B\n```\n",
    )?;
    let blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", LINKED_DOCUMENT_RELATIVE_PATH])
        .output()?;
    if !blob_output.status.success() {
        return Err(io::Error::other("symlink-tree fixture blob lookup failed").into());
    }
    let blob = String::from_utf8(blob_output.stdout)?.trim().to_owned();
    let cacheinfo = format!("120000,{blob},{LINKED_DOCUMENT_RELATIVE_PATH}");
    git_success(
        &fixture_repo,
        &["update-index", "--add", "--cacheinfo", &cacheinfo],
    )?;
    git_success(
        &fixture_repo,
        &["commit", "-m", "candidate linked document (#549)"],
    )?;
    let symlink_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !symlink_head_output.status.success() {
        return Err(io::Error::other("symlink-tree fixture HEAD lookup failed").into());
    }
    let symlink_head = String::from_utf8(symlink_head_output.stdout)?
        .trim()
        .to_owned();
    let tree_output = git_command_for_root(&fixture_repo)
        .args([
            "ls-tree",
            &symlink_head,
            "--",
            LINKED_DOCUMENT_RELATIVE_PATH,
        ])
        .output()?;
    if !tree_output.status.success()
        || !String::from_utf8_lossy(&tree_output.stdout).starts_with("120000 blob ")
    {
        return Err(io::Error::other(format!(
            "symlink-tree fixture did not create a mode-120000 entry:\n{}{}",
            String::from_utf8_lossy(&tree_output.stdout),
            String::from_utf8_lossy(&tree_output.stderr),
        ))
        .into());
    }
    let inherited_path = std::env::var_os("PATH")
        .ok_or_else(|| io::Error::other("symlink-tree fixture requires PATH"))?;
    let real_git = std::env::split_paths(&inherited_path)
        .map(|directory| {
            if cfg!(windows) {
                directory.join("git.exe")
            } else {
                directory.join("git")
            }
        })
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| io::Error::other("symlink-tree fixture requires git"))?;
    let real_git = real_git.to_string_lossy();
    let git_stub_name = if cfg!(windows) { "git.cmd" } else { "git" };
    let git_stub = if cfg!(windows) {
        format!(
            "@echo off\r\nif /I \"%~1\"==\"status\" exit /b 0\r\n\"{real_git}\" %*\r\nexit /b %ERRORLEVEL%\r\n"
        )
    } else {
        format!(
            "#!/bin/sh\nif [ \"${{1:-}}\" = status ]; then exit 0; fi\nexec '{real_git}' \"$@\"\n"
        )
    };
    write_executable_script(&fake_path.join(git_stub_name), &git_stub)?;
    let run_candidate_hook = |head: &str| -> Result<(bool, String, String), Box<dyn Error>> {
        fs::write(&dispatch_log, "")?;
        let mut command = StdCommand::new(&shell);
        command
            .current_dir(&fixture_repo)
            .arg(&hook)
            .env("PATH", &test_path)
            .env("PROJECTATLAS_HOOK_DISPATCH_LOG", &dispatch_log)
            .env("PROJECTATLAS_ISSUE_PAYLOAD", &issue_payload)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("candidate hook stdin was not piped"))?
            .write_all(
                format!(
                    "refs/heads/feature {head} refs/heads/feature 2222222222222222222222222222222222222222\n"
                )
                .as_bytes(),
            )?;
        let output = child.wait_with_output()?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            fs::read_to_string(&dispatch_log)?,
        ))
    };
    let (symlink_status, symlink_stderr, symlink_dispatch) = run_candidate_hook(&symlink_head)?;
    if symlink_status
        || !symlink_stderr.contains("has no tracked regular Markdown file")
        || !symlink_dispatch.contains("--candidate-issue 549")
    {
        return Err(io::Error::other(format!(
            "mode-120000 linked document was not rejected before worktree content validation:\nstderr={symlink_stderr}\ndispatch={symlink_dispatch}",
        ))
        .into());
    }

    let issue_map_relative_path = format!("{OPENSPEC_DIR_NAME}/{ISSUE_MAP_FILE_NAME}");
    let issue_map_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", &issue_map_relative_path])
        .output()?;
    if !issue_map_blob_output.status.success() {
        return Err(io::Error::other("issue-map tree fixture blob lookup failed").into());
    }
    let issue_map_blob = String::from_utf8(issue_map_blob_output.stdout)?
        .trim()
        .to_owned();
    let issue_map_cacheinfo = format!("120000,{issue_map_blob},{issue_map_relative_path}");
    git_success(
        &fixture_repo,
        &["update-index", "--add", "--cacheinfo", &issue_map_cacheinfo],
    )?;
    git_success(
        &fixture_repo,
        &["commit", "-m", "candidate issue map (#549)"],
    )?;
    let issue_map_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !issue_map_head_output.status.success() {
        return Err(io::Error::other("issue-map tree fixture HEAD lookup failed").into());
    }
    let issue_map_head = String::from_utf8(issue_map_head_output.stdout)?
        .trim()
        .to_owned();
    let (issue_map_status, issue_map_stderr, issue_map_dispatch) =
        run_candidate_hook(&issue_map_head)?;
    if issue_map_status
        || !issue_map_stderr.contains("candidate branch issue-map")
        || !issue_map_stderr.contains("tracked regular file")
        || issue_map_dispatch.contains("gh issue")
        || issue_map_dispatch.contains("gh api")
    {
        return Err(io::Error::other(format!(
            "mode-120000 issue map was not rejected before scoped IssueOps:\nstderr={issue_map_stderr}\ndispatch={issue_map_dispatch}"
        ))
        .into());
    }

    let issue_map_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", &issue_map_relative_path])
        .output()?;
    if !issue_map_blob_output.status.success() {
        return Err(io::Error::other("issue-map restore blob lookup failed").into());
    }
    let issue_map_blob = String::from_utf8(issue_map_blob_output.stdout)?
        .trim()
        .to_owned();
    let issue_map_cacheinfo = format!("100644,{issue_map_blob},{issue_map_relative_path}");
    git_success(
        &fixture_repo,
        &["update-index", "--add", "--cacheinfo", &issue_map_cacheinfo],
    )?;
    git_success(&fixture_repo, &["commit", "-m", "restore issue map (#549)"])?;

    let task_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", ISSUEOPS_TASKS_RELATIVE_PATH])
        .output()?;
    if !task_blob_output.status.success() {
        return Err(io::Error::other("mapped-task tree fixture blob lookup failed").into());
    }
    let task_blob = String::from_utf8(task_blob_output.stdout)?
        .trim()
        .to_owned();
    let task_cacheinfo = format!("120000,{task_blob},{ISSUEOPS_TASKS_RELATIVE_PATH}");
    git_success(
        &fixture_repo,
        &["update-index", "--add", "--cacheinfo", &task_cacheinfo],
    )?;
    git_success(
        &fixture_repo,
        &["commit", "-m", "candidate mapped tasks (#549)"],
    )?;
    let task_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !task_head_output.status.success() {
        return Err(io::Error::other("mapped-task tree fixture HEAD lookup failed").into());
    }
    let task_head = String::from_utf8(task_head_output.stdout)?
        .trim()
        .to_owned();
    let (task_status, task_stderr, task_dispatch) = run_candidate_hook(&task_head)?;
    if task_status
        || !task_stderr.contains("candidate branch mapped task file")
        || !task_stderr.contains("tracked regular file")
        || task_dispatch.contains("gh issue")
        || task_dispatch.contains("gh api")
    {
        return Err(io::Error::other(format!(
            "mode-120000 mapped task file was not rejected before scoped IssueOps:\nstderr={task_stderr}\ndispatch={task_dispatch}"
        ))
        .into());
    }

    fs::write(
        &linked_document,
        "## Issue task authority\n\n```mermaid\nflowchart LR\nCOMMITTED --> B\n```\n",
    )?;
    let linked_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", LINKED_DOCUMENT_RELATIVE_PATH])
        .output()?;
    if !linked_blob_output.status.success() {
        return Err(io::Error::other("clean/smudge fixture document blob lookup failed").into());
    }
    let linked_blob = String::from_utf8(linked_blob_output.stdout)?
        .trim()
        .to_owned();
    git_success(
        &fixture_repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{linked_blob},{LINKED_DOCUMENT_RELATIVE_PATH}"),
        ],
    )?;
    let task_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w", ISSUEOPS_TASKS_RELATIVE_PATH])
        .output()?;
    if !task_blob_output.status.success() {
        return Err(io::Error::other("clean/smudge fixture task blob lookup failed").into());
    }
    let task_blob = String::from_utf8(task_blob_output.stdout)?
        .trim()
        .to_owned();
    git_success(
        &fixture_repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{task_blob},{ISSUEOPS_TASKS_RELATIVE_PATH}"),
        ],
    )?;
    git_success(
        &fixture_repo,
        &[
            "config",
            "filter.issueops-clean-smudge.clean",
            "sed 's/WORKTREE/COMMITTED/g; s/not-mermaid/flowchart LR/'",
        ],
    )?;
    git_success(
        &fixture_repo,
        &[
            "config",
            "filter.issueops-clean-smudge.smudge",
            "sed 's/COMMITTED/WORKTREE/g; s/flowchart LR/not-mermaid/'",
        ],
    )?;
    fs::write(
        fixture_repo.join(".gitattributes"),
        "docs/ignored.md filter=issueops-clean-smudge\n",
    )?;
    git_success(&fixture_repo, &["add", ".gitattributes"])?;
    git_success(
        &fixture_repo,
        &["commit", "-m", "candidate clean smudge inputs (#549)"],
    )?;
    fs::remove_file(&linked_document)?;
    git_success(
        &fixture_repo,
        &["checkout", "--", LINKED_DOCUMENT_RELATIVE_PATH],
    )?;
    let filtered_document = fs::read_to_string(&linked_document)?;
    if !filtered_document.contains("not-mermaid") || !filtered_document.contains("WORKTREE") {
        return Err(io::Error::other(
            "clean/smudge fixture did not produce distinct filtered worktree bytes",
        )
        .into());
    }
    let filtered_status_output = git_command_for_root(&fixture_repo)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()?;
    if !filtered_status_output.status.success() || !filtered_status_output.stdout.is_empty() {
        return Err(io::Error::other(format!(
            "clean/smudge fixture did not preserve clean Git status: stdout={} stderr={}",
            String::from_utf8_lossy(&filtered_status_output.stdout),
            String::from_utf8_lossy(&filtered_status_output.stderr),
        ))
        .into());
    }
    let filtered_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !filtered_head_output.status.success() {
        return Err(io::Error::other("clean/smudge fixture HEAD lookup failed").into());
    }
    let filtered_head = String::from_utf8(filtered_head_output.stdout)?
        .trim()
        .to_owned();
    let (filtered_status, filtered_stderr, filtered_dispatch) = run_candidate_hook(&filtered_head)?;
    if !filtered_status || !filtered_dispatch.contains("--candidate-issue 549") {
        return Err(io::Error::other(format!(
            "candidate validation did not use submitted clean/smudge tree bytes:\nstatus={filtered_status}\nstderr={filtered_stderr}\ndispatch={filtered_dispatch}"
        ))
        .into());
    }

    let replacement_index = temp.path().join("replacement-index");
    let replacement_read_tree = git_command_for_root(&fixture_repo)
        .env("GIT_INDEX_FILE", &replacement_index)
        .args(["read-tree", &filtered_head])
        .output()?;
    if !replacement_read_tree.status.success() {
        return Err(io::Error::other(format!(
            "replacement-tree fixture could not read candidate tree: {}{}",
            String::from_utf8_lossy(&replacement_read_tree.stdout),
            String::from_utf8_lossy(&replacement_read_tree.stderr),
        ))
        .into());
    }
    let replacement_cacheinfo = format!("120000,{linked_blob},{LINKED_DOCUMENT_RELATIVE_PATH}");
    let replacement_update_index = git_command_for_root(&fixture_repo)
        .env("GIT_INDEX_FILE", &replacement_index)
        .args([
            "update-index",
            "--add",
            "--cacheinfo",
            &replacement_cacheinfo,
        ])
        .output()?;
    if !replacement_update_index.status.success() {
        return Err(io::Error::other(format!(
            "replacement-tree fixture could not write symlink entry: {}{}",
            String::from_utf8_lossy(&replacement_update_index.stdout),
            String::from_utf8_lossy(&replacement_update_index.stderr),
        ))
        .into());
    }
    let replacement_tree_output = git_command_for_root(&fixture_repo)
        .env("GIT_INDEX_FILE", &replacement_index)
        .args(["write-tree"])
        .output()?;
    if !replacement_tree_output.status.success() {
        return Err(io::Error::other(format!(
            "replacement-tree fixture could not write tree: {}{}",
            String::from_utf8_lossy(&replacement_tree_output.stdout),
            String::from_utf8_lossy(&replacement_tree_output.stderr),
        ))
        .into());
    }
    let replacement_tree = String::from_utf8(replacement_tree_output.stdout)?
        .trim()
        .to_owned();
    let mut replacement_commit_command = git_command_for_root(&fixture_repo);
    replacement_commit_command
        .args(["commit-tree", &replacement_tree, "-p", &base])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut replacement_commit = replacement_commit_command.spawn()?;
    replacement_commit
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("replacement-tree commit stdin was not piped"))?
        .write_all(b"replacement tree (#549)\n")?;
    let replacement_commit_output = replacement_commit.wait_with_output()?;
    if !replacement_commit_output.status.success() {
        return Err(io::Error::other(format!(
            "replacement-tree fixture could not write commit: {}{}",
            String::from_utf8_lossy(&replacement_commit_output.stdout),
            String::from_utf8_lossy(&replacement_commit_output.stderr),
        ))
        .into());
    }
    let replacement_commit_oid = String::from_utf8(replacement_commit_output.stdout)?
        .trim()
        .to_owned();
    git_success(
        &fixture_repo,
        &["replace", &filtered_head, &replacement_commit_oid],
    )?;
    let replaced_tree_output = git_command_for_root(&fixture_repo)
        .args([
            "ls-tree",
            &filtered_head,
            "--",
            LINKED_DOCUMENT_RELATIVE_PATH,
        ])
        .output()?;
    let original_tree_output = git_command_for_root(&fixture_repo)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args([
            "ls-tree",
            &filtered_head,
            "--",
            LINKED_DOCUMENT_RELATIVE_PATH,
        ])
        .output()?;
    if !replaced_tree_output.status.success()
        || !String::from_utf8_lossy(&replaced_tree_output.stdout).starts_with("120000 blob ")
        || !original_tree_output.status.success()
        || !String::from_utf8_lossy(&original_tree_output.stdout).starts_with("100644 blob ")
    {
        return Err(io::Error::other(format!(
            "replacement-tree fixture did not distinguish replacement from submitted tree:\nreplaced={}original={}",
            String::from_utf8_lossy(&replaced_tree_output.stdout),
            String::from_utf8_lossy(&original_tree_output.stdout),
        ))
        .into());
    }
    let (replacement_tree_status, replacement_tree_stderr, replacement_tree_dispatch) =
        run_candidate_hook(&filtered_head)?;
    if !replacement_tree_status || !replacement_tree_dispatch.contains("--candidate-issue 549") {
        return Err(io::Error::other(format!(
            "candidate validation did not ignore a replacement commit tree:\nstderr={replacement_tree_stderr}\ndispatch={replacement_tree_dispatch}",
        ))
        .into());
    }
    git_success(&fixture_repo, &["replace", "-d", &filtered_head])?;

    let replacement_document = temp.path().join("replacement-invalid.md");
    fs::write(
        &replacement_document,
        "## Issue task authority\n\nnot-mermaid replacement bytes\n",
    )?;
    let replacement_blob_output = git_command_for_root(&fixture_repo)
        .args(["hash-object", "-w"])
        .arg(&replacement_document)
        .output()?;
    if !replacement_blob_output.status.success() {
        return Err(io::Error::other(format!(
            "replacement-blob fixture could not write blob: {}{}",
            String::from_utf8_lossy(&replacement_blob_output.stdout),
            String::from_utf8_lossy(&replacement_blob_output.stderr),
        ))
        .into());
    }
    let replacement_blob = String::from_utf8(replacement_blob_output.stdout)?
        .trim()
        .to_owned();
    git_success(&fixture_repo, &["replace", &linked_blob, &replacement_blob])?;
    let replaced_blob_output = git_command_for_root(&fixture_repo)
        .args(["cat-file", "blob", &linked_blob])
        .output()?;
    let original_blob_output = git_command_for_root(&fixture_repo)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args(["cat-file", "blob", &linked_blob])
        .output()?;
    if !replaced_blob_output.status.success()
        || !String::from_utf8_lossy(&replaced_blob_output.stdout).contains("not-mermaid")
        || !original_blob_output.status.success()
        || String::from_utf8_lossy(&original_blob_output.stdout).contains("not-mermaid")
    {
        return Err(io::Error::other(
            "replacement-blob fixture did not distinguish replacement from submitted blob",
        )
        .into());
    }
    let (replacement_blob_status, replacement_blob_stderr, replacement_blob_dispatch) =
        run_candidate_hook(&filtered_head)?;
    if !replacement_blob_status || !replacement_blob_dispatch.contains("--candidate-issue 549") {
        return Err(io::Error::other(format!(
            "candidate validation did not ignore a replacement blob:\nstderr={replacement_blob_stderr}\ndispatch={replacement_blob_dispatch}",
        ))
        .into());
    }
    git_success(&fixture_repo, &["replace", "-d", &linked_blob])?;

    git_success(
        &fixture_repo,
        &["commit", "--allow-empty", "-m", "candidate (#549) (#547"],
    )?;
    let malformed_head_output = git_command_for_root(&fixture_repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !malformed_head_output.status.success() {
        return Err(io::Error::other("malformed-owner fixture HEAD lookup failed").into());
    }
    let malformed_head = String::from_utf8(malformed_head_output.stdout)?
        .trim()
        .to_owned();
    fs::write(&dispatch_log, "")?;
    let mut malformed_command = StdCommand::new(&shell);
    malformed_command
        .current_dir(&fixture_repo)
        .arg(&hook)
        .env("PATH", &test_path)
        .env("PROJECTATLAS_HOOK_DISPATCH_LOG", &dispatch_log)
        .env("PROJECTATLAS_ISSUE_PAYLOAD", &issue_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut malformed_child = malformed_command.spawn()?;
    malformed_child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("malformed-owner hook stdin was not piped"))?
        .write_all(
            format!(
                "refs/heads/feature {malformed_head} refs/heads/feature 3333333333333333333333333333333333333333\n"
            )
            .as_bytes(),
        )?;
    let malformed_output = malformed_child.wait_with_output()?;
    let malformed_dispatch = fs::read_to_string(&dispatch_log)?;
    let malformed_stderr = String::from_utf8_lossy(&malformed_output.stderr);
    if malformed_output.status.success()
        || !malformed_stderr.contains("owner resolution failed")
        || malformed_dispatch.contains("--candidate-issue")
    {
        return Err(io::Error::other(format!(
            "unmatched owner marker was not rejected before scoped IssueOps:\nstdout={}\\nstderr={malformed_stderr}\\ndispatch={malformed_dispatch}",
            String::from_utf8_lossy(&malformed_output.stdout),
        ))
        .into());
    }
    Ok(())
}

#[test]
fn pre_push_candidate_rejects_dirty_worktree_before_issueops() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let source_hook = workspace_root
        .join(GITHOOKS_DIR_NAME)
        .join(PRE_PUSH_HOOK_FILE_NAME);
    let shell = if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")
    } else {
        PathBuf::from("sh")
    };

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir_all(repo.join(GITHOOKS_DIR_NAME))?;
    fs::create_dir_all(repo.join(".github").join("scripts"))?;
    fs::create_dir_all(
        repo.join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join(ISSUEOPS_CHANGE_NAME),
    )?;
    fs::create_dir_all(&fake_path)?;
    fs::copy(
        &source_hook,
        repo.join(GITHOOKS_DIR_NAME).join(PRE_PUSH_HOOK_FILE_NAME),
    )?;
    fs::write(
        repo.join(".github")
            .join("scripts")
            .join(ISSUE_CHECKLISTS_SCRIPT_FILE_NAME),
        "",
    )?;
    fs::write(
        repo.join(OPENSPEC_DIR_NAME).join(ISSUE_MAP_FILE_NAME),
        "{\"schema_version\": 2, \"changes\": {}}\n",
    )?;
    fs::write(
        repo.join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join(ISSUEOPS_CHANGE_NAME)
            .join(TASKS_FILE_NAME),
        "- [x] 1.1 baseline\n",
    )?;
    fs::write(repo.join(CANDIDATE_FILE_NAME), "candidate\n")?;

    let python_stub = r#"#!/bin/sh
printf 'python3 %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
case " $* " in
  *"affected-ci-proof.py plan "*)
    printf '%s\n' '{"mode":"narrow","repository_contracts":["issueops","mermaid"],"rust_packages":[],"test_targets":[],"test_only":false,"jobs":{"rust":false}}'
    exit 0
    ;;
  *" -c "*) exec python "$@" ;;
  *" --owner-from-commits "*) printf '%s\n' 549 ;;
esac
exit 0
"#;
    let cargo_stub = r#"#!/bin/sh
printf 'cargo %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let npm_stub = r#"#!/bin/sh
printf 'npm %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
exit 0
"#;
    let gh_stub = r#"#!/bin/sh
printf 'gh %s\n' "$*" >> "$PROJECTATLAS_HOOK_DISPATCH_LOG"
if [ "${1:-}" = repo ] && [ "${2:-}" = view ]; then
  printf '%s\n' styler-ai/ProjectAtlas
fi
exit 0
"#;
    for (name, script) in [
        ("python3", python_stub),
        ("cargo", cargo_stub),
        ("npm", npm_stub),
        ("gh", gh_stub),
    ] {
        write_executable_script(&fake_path.join(name), script)?;
    }
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(fake_path).chain(std::env::split_paths(&current_path)),
    )?;

    git_success(&repo, &["init", "--initial-branch=main"])?;
    git_success(&repo, &["config", "user.email", "test@example.invalid"])?;
    git_success(&repo, &["config", "user.name", "ProjectAtlas test"])?;
    git_success(&repo, &["add", "."])?;
    git_success(&repo, &["commit", "-m", "baseline (#549)"])?;
    let base_output = git_command_for_root(&repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !base_output.status.success() {
        return Err(io::Error::other("fixture base commit lookup failed").into());
    }
    let base = String::from_utf8(base_output.stdout)?.trim().to_owned();
    git_success(&repo, &["checkout", "-b", "feature"])?;
    git_success(
        &repo,
        &["commit", "--allow-empty", "-m", "candidate (#549)"],
    )?;
    let head_output = git_command_for_root(&repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !head_output.status.success() {
        return Err(io::Error::other("fixture candidate commit lookup failed").into());
    }
    let head = String::from_utf8(head_output.stdout)?.trim().to_owned();
    git_success(&repo, &["update-ref", "refs/remotes/origin/main", &base])?;

    fs::write(
        repo.join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join(ISSUEOPS_CHANGE_NAME)
            .join(TASKS_FILE_NAME),
        "- [ ] 1.1 drifted checklist\n",
    )?;
    fs::write(
        repo.join(OPENSPEC_DIR_NAME).join(ISSUE_MAP_FILE_NAME),
        "{\"schema_version\": 2, \"changes\": {\"drift\": 549}}\n",
    )?;
    git_success(&repo, &["add", "openspec/issue-map.json"])?;
    fs::write(
        repo.join(OPENSPEC_DIR_NAME)
            .join(CHANGE_DIR_NAME)
            .join(ISSUEOPS_CHANGE_NAME)
            .join("untracked-notes.md"),
        "untracked relevant input\n",
    )?;

    let dispatch_log = temp.path().join(DISPATCH_LOG_FILE_NAME);
    fs::write(&dispatch_log, "")?;
    let mut command = StdCommand::new(&shell);
    command
        .current_dir(&repo)
        .arg(repo.join(GITHOOKS_DIR_NAME).join(PRE_PUSH_HOOK_FILE_NAME))
        .env("PATH", &test_path)
        .env("PROJECTATLAS_HOOK_DISPATCH_LOG", &dispatch_log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("dirty candidate hook stdin was not piped"))?
        .write_all(format!("refs/heads/feature {head} refs/heads/feature {head}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    let dispatch = fs::read_to_string(&dispatch_log)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success()
        || !stderr.contains("candidate branch worktree must be clean")
        || dispatch
            .lines()
            .any(|line| line.contains("issue-checklists.py --repo"))
        || dispatch.contains("--owner-from-commits")
    {
        return Err(io::Error::other(format!(
                "dirty candidate worktree did not fail before scoped IssueOps dispatch:\nstdout={}\nstderr={stderr}\ndispatch={dispatch}",
                String::from_utf8_lossy(&output.stdout),
            ))
            .into());
    }
    Ok(())
}

#[test]
fn macos_all_features_warning_gate_contract_is_exact() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let ci = fs::read_to_string(workspace_root.join(".github/workflows/ci.yml"))?;
    let planner = fs::read_to_string(workspace_root.join(".github/scripts/affected-ci-proof.py"))?;
    let e2e_smoke = workflow_job_block(&ci, "e2e-smoke")?;
    for required in [
        r#""macos-x64": "macos-15-intel""#,
        r#""macos-arm64": "macos-14""#,
    ] {
        if !planner.contains(required) {
            return Err(io::Error::other(format!(
                "affected planner omitted required macOS runner row {required:?}"
            ))
            .into());
        }
    }
    if !e2e_smoke.contains("matrix: ${{ fromJSON(needs.plan.outputs.platform_matrix) }}") {
        return Err(io::Error::other("e2e-smoke omitted its affected platform matrix").into());
    }

    let target_compile = workflow_job_step(&ci, "e2e-smoke", "Affected package target compile")?;
    if target_compile["if"].as_str()
        != Some(
            "contains(matrix.contracts, 'compile') && (fromJSON(needs.plan.outputs.plan).mode == 'narrow' || runner.os == 'Windows')",
        )
        || target_compile["shell"].as_str() != Some("bash")
        || target_compile["timeout-minutes"].as_i64() != Some(10)
    {
        return Err(io::Error::other(
            "affected target compile must cover narrow plans and full Windows fallback without duplicating full Linux or macOS proof",
        )
        .into());
    }
    let target_compile_run = target_compile["run"].as_str().unwrap_or_default();
    if target_compile_run
        .matches("sys.stdout.buffer.write")
        .count()
        != 2
        || target_compile_run.contains("sys.stdout.write")
    {
        return Err(io::Error::other(
            "affected target compile must emit planner lists without Windows CRLF translation",
        )
        .into());
    }
    for required in [
        r#"["rust_packages"]"#,
        r#"["test_targets"]"#,
        r#"["mode"]"#,
        "planner emitted unknown Rust package",
        "planner emitted unknown Rust test target",
        "target compile contract has no affected package",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo check \"${package_args[@]}\" --lib --bins --examples --all-features --locked",
        "cargo test \"${package_args[@]}\" --lib --bins --no-run --all-features --locked",
        "cargo check -p projectatlas-cli \"${target_args[@]}\" --all-features --locked",
    ] {
        if !target_compile_run.contains(required) {
            return Err(
                io::Error::other(format!("affected target compile omitted {required:?}")).into(),
            );
        }
    }
    for forbidden in [
        "cargo clippy",
        "--tests",
        "cargo check \"${package_args[@]}\" \"${target_args[@]}\"",
    ] {
        if target_compile_run.contains(forbidden) {
            return Err(io::Error::other(format!(
                "affected target compile repeats unrelated proof {forbidden:?}"
            ))
            .into());
        }
    }

    let step = workflow_job_step(&ci, "e2e-smoke", "macOS all-features warning gate")?;
    for (field, actual, expected) in [
        (
            "if",
            step["if"].as_str(),
            Some("runner.os == 'macOS' && contains(matrix.contracts, 'mac-quality')"),
        ),
        ("shell", step["shell"].as_str(), Some("bash")),
    ] {
        if actual != expected {
            return Err(io::Error::other(format!(
                "macOS warning gate field {field:?} must be {expected:?}, found {actual:?}"
            ))
            .into());
        }
    }
    if step["timeout-minutes"].as_i64() != Some(15) {
        return Err(io::Error::other(format!(
            "macOS warning gate timeout must be 15 minutes, found {:?}",
            step["timeout-minutes"]
        ))
        .into());
    }
    let expected_run =
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings";
    if step["run"].as_str().map(str::trim) != Some(expected_run) {
        return Err(io::Error::other(format!(
            "macOS warning gate commands drifted: found {:?}",
            step["run"].as_str()
        ))
        .into());
    }
    if step["run"]
        .as_str()
        .is_some_and(|run| run.contains("cargo check"))
    {
        return Err(io::Error::other(
            "macOS warning gate must not compile the workspace before clippy recompiles it",
        )
        .into());
    }

    let platform_regression = workflow_job_step(
        &ci,
        "e2e-smoke",
        "macOS optional parser worker platform regression",
    )?;
    for (field, actual, expected) in [
        (
            "if",
            platform_regression["if"].as_str(),
            Some("runner.os == 'macOS' && contains(matrix.contracts, 'parser')"),
        ),
        ("shell", platform_regression["shell"].as_str(), Some("bash")),
        (
            "run",
            platform_regression["run"].as_str().map(str::trim),
            Some(
                "cargo test --locked -p projectatlas-cli --all-features --test optional_parser_worker_platform",
            ),
        ),
    ] {
        if actual != expected {
            return Err(io::Error::other(format!(
                "macOS platform regression field {field:?} must be {expected:?}, found {actual:?}"
            ))
            .into());
        }
    }
    if platform_regression["timeout-minutes"].as_i64() != Some(15) {
        return Err(io::Error::other(format!(
            "macOS platform regression timeout must be 15 minutes, found {:?}",
            platform_regression["timeout-minutes"]
        ))
        .into());
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
    let runtime = mcp_contract_executable();
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
fn windows_installer_recovery_operation_preserves_config_selection() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let outside = temp.path().join("outside");
    fs::create_dir(&outside)?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let powershell = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?,
    )
    .join(WINDOWS_SYSTEM32_DIR)
    .join(WINDOWS_POWERSHELL_DIR)
    .join(WINDOWS_POWERSHELL_VERSION_DIR)
    .join(WINDOWS_POWERSHELL_EXECUTABLE);
    let script = temp.path().join("run-token-recovery.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_TEST_INSTALLER
foreach ($functionName in @(
        "Convert-ProjectAtlasVersionTag",
        "Get-ProjectAtlasMcpLaunchArguments",
        "Get-ProjectAtlasTokenLaunchArguments"
    )) {
    $match = [regex]::Match($installerSource, "(?ms)^function $functionName \{.*?^\}")
    if (-not $match.Success) { throw "Installer function missing: $functionName" }
    Invoke-Expression $match.Value
}
$arguments = @(Get-ProjectAtlasTokenLaunchArguments `
    $env:PROJECTATLAS_TEST_DB `
    $env:PROJECTATLAS_TEST_NESTED_CONFIG `
    $env:PROJECTATLAS_TEST_FLAT_CONFIG `
    $env:PROJECTATLAS_TEST_VERSION)
$expected = @(
    "--require-version", $env:PROJECTATLAS_TEST_VERSION,
    "--db", $env:PROJECTATLAS_TEST_DB
)
if (-not [string]::IsNullOrWhiteSpace($env:PROJECTATLAS_TEST_SELECTED_CONFIG)) {
    $expected += @("--config", $env:PROJECTATLAS_TEST_SELECTED_CONFIG)
}
$expected += @("token", "--view", "tui")
if (($arguments -join "`0") -cne ($expected -join "`0")) {
    throw "Unexpected recovery arguments: $($arguments -join ' ')"
}
& $env:PROJECTATLAS_TEST_RUNTIME @arguments
exit $LASTEXITCODE
"#,
    )?;

    for layout in ["nested", "flat", "none"] {
        let repo = temp.path().join(layout);
        let atlas_dir = repo.join(ATLAS_DIR_NAME);
        fs::create_dir_all(&atlas_dir)?;
        let nested_config = atlas_dir.join("config.toml");
        let flat_config = repo.join("projectatlas.toml");
        let selected_config = match layout {
            "nested" => Some(nested_config.as_path()),
            "flat" => Some(flat_config.as_path()),
            _ => None,
        };
        if let Some(config) = selected_config {
            fs::write(config, "[project]\nroot = \".\"\n")?;
        }
        let db = atlas_dir.join("projectatlas.db");
        let mut scan = StdCommand::new(&runtime);
        scan.current_dir(&outside)
            .args(["--require-version", env!("CARGO_PKG_VERSION")])
            .args(["--format", "json"])
            .arg("--db")
            .arg(&db);
        if let Some(config) = selected_config {
            scan.arg("--config").arg(config);
        }
        let scan_output = scan.arg("scan").arg(&repo).output()?;
        if !scan_output.status.success() {
            return Err(io::Error::other(format!(
                "{layout} recovery fixture scan failed: {}",
                String::from_utf8_lossy(&scan_output.stderr)
            ))
            .into());
        }
        let before = mcp_database_snapshot(&db)?;
        let output = StdCommand::new(&powershell)
            .current_dir(&outside)
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .env("PROJECTATLAS_TEST_INSTALLER", &installer)
            .env("PROJECTATLAS_TEST_RUNTIME", &runtime)
            .env("PROJECTATLAS_TEST_VERSION", env!("CARGO_PKG_VERSION"))
            .env("PROJECTATLAS_TEST_DB", &db)
            .env("PROJECTATLAS_TEST_NESTED_CONFIG", &nested_config)
            .env("PROJECTATLAS_TEST_FLAT_CONFIG", &flat_config)
            .env(
                "PROJECTATLAS_TEST_SELECTED_CONFIG",
                selected_config.unwrap_or_else(|| Path::new("")),
            )
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "{layout} recovery operation failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if mcp_database_snapshot(&db)? != before {
            return Err(io::Error::other(format!(
                "{layout} recovery operation changed the selected database"
            ))
            .into());
        }
        if outside
            .join(ATLAS_DIR_NAME)
            .join("projectatlas.db")
            .exists()
        {
            return Err(io::Error::other(format!(
                "{layout} recovery operation created an outside default database"
            ))
            .into());
        }
    }
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
    let fake_codex_state = isolated_home.join(FAKE_CODEX_REGISTRY_STATE_FILE_NAME);
    let fake_codex_stale_registry = isolated_home.join(FAKE_CODEX_REGISTRY_STALE_FILE_NAME);
    let fake_codex_current_registry = isolated_home.join(FAKE_CODEX_REGISTRY_CURRENT_FILE_NAME);
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"mcp\" if \"%2\"==\"add\" (\r\n  echo current>\"%PROJECTATLAS_FAKE_CODEX_STATE%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_STATE%\" (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT%\"\r\n  ) else (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE%\"\r\n  )\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
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
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let stable_runtime_dir = stable_runtime
        .parent()
        .ok_or_else(|| io::Error::other("stable runtime parent missing"))?;
    let parent_path = std::env::join_paths(
        std::iter::once(stable_runtime_dir.to_path_buf())
            .chain(std::env::split_paths(&inherited_path)),
    )?;

    let db = atlas_dir.join("projectatlas.db");
    let versioned_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("runtimes")
        .join(env!("CARGO_PKG_VERSION"))
        .join("x86_64-pc-windows-msvc")
        .join("projectatlas.exe");
    fs::write(
        &fake_codex_stale_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": stable_runtime,
                "args": ["--require-version", "0.3.15", "--db", "C:\\old\\.projectatlas\\projectatlas.db", "mcp"]
            }
        }))?,
    )?;
    fs::write(
        &fake_codex_current_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": versioned_runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", db,
                    "--config", atlas_dir.join("config.toml"),
                    "mcp"
                ]
            }
        }))?,
    )?;
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
    let mut current_lock_reaped = false;
    let mut stale_lock_process = None;

    let test_result = (|| -> Result<(), Box<dyn Error>> {
        let release_archive = create_windows_release_archive(temp.path(), &runtime)?;
        let release_asset_guard = lock_windows_release_asset_tests();
        let (release_base_url, release_server) = serve_release_assets(&release_archive, None)?;
        let workspace_root = workspace_root()?;
        let installer = workspace_root
            .join("plugins")
            .join("projectatlas")
            .join("scripts")
            .join("install-runtime.ps1");
        let standalone_installer_dir = temp.path().join("standalone-installer");
        fs::create_dir_all(&standalone_installer_dir)?;
        let standalone_installer = standalone_installer_dir.join("install-runtime.ps1");
        fs::copy(&installer, &standalone_installer)?;
        let output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&installer)
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
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
            .env(
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE",
                &fake_codex_stale_registry,
            )
            .env(
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
                &fake_codex_current_registry,
            )
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
        drop(release_asset_guard);
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
        if installer_output_text.contains("ProjectAtlas LocalAppData mirror is locked") {
            return Err(io::Error::other(format!(
                "installer tried to replace an already-current locked stable mirror\n{installer_output_text}"
            ))
            .into());
        }
        if !installer_output_text
            .contains("Active process resolves bare projectatlas to verified runtime")
        {
            return Err(io::Error::other(format!(
                "installer did not make its active process prefer the verified runtime\n{installer_output_text}"
            ))
            .into());
        }
        if !installer_output_text.trim_end().ends_with(
            "ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=true host_restart_required=false",
        ) || installer_output_text.contains("Existing host restart required:")
        {
            return Err(io::Error::other(format!(
                "installer misstated readiness for the already-current locked stable mirror\n{installer_output_text}"
            ))
            .into());
        }

        if !versioned_runtime.exists() {
            return Err(io::Error::other(format!(
                "release binary was not installed to the versioned runtime path: {}",
                versioned_runtime.display()
            ))
            .into());
        }
        if let Some(status) = locked_runtime.try_wait()? {
            return Err(io::Error::other(format!(
                "installer terminated the owned process holding the stable mirror lock: {status}"
            ))
            .into());
        }

        let sibling_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(
                "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; & projectatlas --require-version $env:PROJECTATLAS_VERSION --format json runtime-info",
            )
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let sibling_stdout = String::from_utf8_lossy(&sibling_output.stdout);
        if !sibling_output.status.success() {
            return Err(io::Error::other(format!(
                "later sibling from the unchanged parent failed:\n{sibling_stdout}\n{}",
                String::from_utf8_lossy(&sibling_output.stderr)
            ))
            .into());
        }
        let sibling_command = sibling_stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .ok_or_else(|| {
                io::Error::other("later sibling did not report its bare command path")
            })?;
        require_same_executable(
            sibling_command.trim(),
            &stable_runtime,
            "unchanged parent sibling",
        )?;

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
        let init = StdCommand::new(&versioned_runtime)
            .current_dir(&repo)
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--format")
            .arg("json")
            .arg("--db")
            .arg(&db)
            .arg("init")
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        if !init.status.success() {
            return Err(io::Error::other(format!(
                "versioned runtime could not initialize the locked-mirror fixture: {}",
                String::from_utf8_lossy(&init.stderr)
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

        locked_runtime.kill()?;
        locked_runtime.wait()?;
        current_lock_reaped = true;
        fs::write(&stable_runtime, b"stale ProjectAtlas stable mirror")?;
        stale_lock_process = Some(spawn_exclusive_file_lock(&stable_runtime)?);

        let stale_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&standalone_installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-RuntimePath")
            .arg(&versioned_runtime)
            .env_remove("PROJECTATLAS_VERSION")
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let stale_output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&stale_output.stdout),
            String::from_utf8_lossy(&stale_output.stderr)
        );
        let normalized_stale_output = stale_output_text.split_whitespace().collect::<String>();
        for required in [
            "ProjectAtlas LocalAppData mirror is locked",
            "verify durable absolute MCP configuration before attempting an exact obsolete-child handoff",
            "process_owner=inspection_failed",
            "ProjectAtlas convergence: update_state=partial stable_mirror_ready=false obsolete_mcp_handoff=inspection_failed",
        ] {
            if !normalized_stale_output.contains(&required.split_whitespace().collect::<String>()) {
                return Err(io::Error::other(format!(
                    "installer did not provide stale locked-mirror guidance {required:?}\n{stale_output_text}"
                ))
                .into());
            }
        }
        let stable_runtime_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&stable_runtime)?).replace('/', "\\");
        let versioned_runtime_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&versioned_runtime)?).replace('/', "\\");
        let db_diagnostic_path = normalize_native_path_display(&db).replace('/', "\\");
        let config_diagnostic_path =
            normalize_native_path_display(atlas_dir.join("config.toml")).replace('/', "\\");
        for required in [
            format!(
                "ProjectAtlas stale bare command: path={stable_runtime_diagnostic_path} observed_version=unavailable ready=false"
            ),
            format!(
                "verified_runtime={} target_version={}",
                versioned_runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime command: & '{}' --require-version '{}' --format json runtime-info",
                versioned_runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime operation: & '{}' '--require-version' '{}' '--db' '{}' '--config' '{}' 'token' '--view' 'tui'",
                versioned_runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION"),
                db_diagnostic_path,
                config_diagnostic_path
            ),
            "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=false"
                .to_string(),
            format!(
                "-ProjectAtlasVersion '{}' -RuntimePath '{}'",
                env!("CARGO_PKG_VERSION"),
                versioned_runtime_diagnostic_path
            ),
            format!(
                "projectatlas --require-version '{}' token --view tui",
                env!("CARGO_PKG_VERSION")
            ),
        ] {
            if !normalized_stale_output.contains(&required.split_whitespace().collect::<String>()) {
                return Err(io::Error::other(format!(
                    "installer omitted exact stale-command recovery field {required:?}\n{stale_output_text}"
                ))
                .into());
            }
        }
        if !stale_output.status.success()
            || !normalized_stale_output.ends_with(
                &"ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=false host_restart_required=false"
                    .split_whitespace()
                    .collect::<String>(),
            )
            || normalized_stale_output.contains(
                &"Existing host restart required:"
                    .split_whitespace()
                    .collect::<String>(),
            )
            || !normalized_stale_output.contains(
                &"restart alone will not repair it"
                    .split_whitespace()
                    .collect::<String>(),
            )
        {
            return Err(io::Error::other(format!(
                "installer did not report repair-required state for the unchanged stale parent mirror\n{stale_output_text}"
            ))
            .into());
        }
        if let Some(status) = stale_lock_process
            .as_mut()
            .ok_or_else(|| io::Error::other("stale mirror lock process missing"))?
            .try_wait()?
        {
            return Err(io::Error::other(format!(
                "installer terminated the owned stale-mirror lock process: {status}"
            ))
            .into());
        }

        let same_version_shadow = temp
            .path()
            .join("machine-shadow-bin")
            .join("projectatlas.exe");
        fs::create_dir_all(
            same_version_shadow
                .parent()
                .ok_or_else(|| io::Error::other("same-version shadow parent missing"))?,
        )?;
        fs::copy(&versioned_runtime, &same_version_shadow)?;
        let same_version_path = std::env::join_paths(
            std::iter::once(
                same_version_shadow
                    .parent()
                    .ok_or_else(|| io::Error::other("same-version shadow parent missing"))?
                    .to_path_buf(),
            )
            .chain(std::env::split_paths(&parent_path)),
        )?;
        let compatible_before_same_version_shadow = mcp_database_snapshot(&db)?;
        let same_version_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&standalone_installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-RuntimePath")
            .arg(&versioned_runtime)
            .env_remove("PROJECTATLAS_VERSION")
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &same_version_path)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let same_version_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&same_version_output.stdout),
            String::from_utf8_lossy(&same_version_output.stderr)
        );
        let normalized_same_version_text = same_version_text.split_whitespace().collect::<String>();
        let same_version_shadow_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&same_version_shadow)?)
                .replace('/', "\\");
        for required in [
            format!(
                "ProjectAtlas stale bare command: path={same_version_shadow_diagnostic_path} observed_version={} ready=false",
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "verified_runtime={versioned_runtime_diagnostic_path} target_version={}",
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime command: & '{}' --require-version '{}' --format json runtime-info",
                versioned_runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime operation: & '{}' '--require-version' '{}' '--db' '{}' '--config' '{}' 'token' '--view' 'tui'",
                versioned_runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION"),
                db_diagnostic_path,
                config_diagnostic_path
            ),
            "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=false"
                .to_string(),
            "stable_mirror_ready=false".to_string(),
            "parent_cli_ready=false".to_string(),
        ] {
            if !normalized_same_version_text
                .contains(&required.split_whitespace().collect::<String>())
            {
                return Err(io::Error::other(format!(
                    "same-version foreign PATH command suppressed recovery field {required:?}\n{same_version_text}"
                ))
                .into());
            }
        }
        if !same_version_output.status.success()
            || mcp_database_snapshot(&db)? != compatible_before_same_version_shadow
            || temp
                .path()
                .join(ATLAS_DIR_NAME)
                .join("projectatlas.db")
                .exists()
        {
            return Err(io::Error::other(format!(
                "same-version foreign PATH recovery changed state or failed\n{same_version_text}"
            ))
            .into());
        }
        if let Some(status) = stale_lock_process
            .as_mut()
            .ok_or_else(|| io::Error::other("stale mirror lock process missing"))?
            .try_wait()?
        {
            return Err(io::Error::other(format!(
                "same-version PATH recovery terminated the unrelated lock owner: {status}"
            ))
            .into());
        }
        if std::env::var("PROJECTATLAS_TEST_DISPOSABLE_RUNNER").as_deref() == Ok("github-hosted") {
            let restart_output = StdCommand::new("powershell")
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    r#"
$originalUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$exitCode = 1
try {
    $runtimeDir = Split-Path -Parent $env:PROJECTATLAS_TEST_RUNTIME
    [Environment]::SetEnvironmentVariable("Path", (@($runtimeDir, $originalUserPath) -join ";"), "User")
    & $env:PROJECTATLAS_TEST_INSTALLER `
        -ProjectRoot $env:PROJECTATLAS_TEST_ROOT `
        -ProjectAtlasVersion $env:PROJECTATLAS_TEST_VERSION `
        -RuntimePath $env:PROJECTATLAS_TEST_RUNTIME
    $exitCode = $LASTEXITCODE
}
finally {
    [Environment]::SetEnvironmentVariable("Path", $originalUserPath, "User")
}
exit $exitCode
"#,
                ])
                .env("PROJECTATLAS_TEST_INSTALLER", &standalone_installer)
                .env("PROJECTATLAS_TEST_ROOT", &repo)
                .env(
                    "PROJECTATLAS_TEST_VERSION",
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                )
                .env("PROJECTATLAS_TEST_RUNTIME", &versioned_runtime)
                .env("HOME", &isolated_home)
                .env("USERPROFILE", &isolated_home)
                .env("APPDATA", &app_data)
                .env("LOCALAPPDATA", &local_app_data)
                .env("PATH", &parent_path)
                .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
                .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
                .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
                .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
                .env(
                    "PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE",
                    &fake_codex_stale_registry,
                )
                .env(
                    "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
                    &fake_codex_current_registry,
                )
                .env("PROJECTATLAS_NO_TELEMETRY", "1")
                .output()?;
            let restart_text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&restart_output.stdout),
                String::from_utf8_lossy(&restart_output.stderr)
            );
            if !restart_output.status.success()
                || !restart_text.contains(
                    "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=true",
                )
                || !restart_text.contains("Existing host restart required:")
                || !restart_text.contains("host_restart_required=true")
                || restart_text.contains("restart alone will not repair it")
            {
                return Err(io::Error::other(format!(
                    "supplied-runtime install did not recognize persisted fresh-host PATH readiness:\n{restart_text}"
                ))
                .into());
            }
        }
        let compatible_before = mcp_database_snapshot(&db)?;
        let versioned_token = StdCommand::new(&versioned_runtime)
            .current_dir(temp.path())
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--db")
            .arg(&db)
            .arg("--config")
            .arg(atlas_dir.join("config.toml"))
            .args(["token", "--view", "tui"])
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        if !versioned_token.status.success() {
            return Err(io::Error::other(format!(
                "verified absolute runtime token TUI failed while the stale mirror stayed locked: {}",
                String::from_utf8_lossy(&versioned_token.stderr)
            ))
            .into());
        }
        if mcp_database_snapshot(&db)? != compatible_before {
            return Err(io::Error::other(
                "verified absolute runtime token TUI changed the compatible locked-mirror database",
            )
            .into());
        }
        if temp
            .path()
            .join(ATLAS_DIR_NAME)
            .join("projectatlas.db")
            .exists()
        {
            return Err(io::Error::other(
                "verified absolute runtime operation created a default database outside the selected project",
            )
            .into());
        }
        let locked_config_mcp = run_mcp_stdio(&mcp_command, &repo, &mcp_args, &messages)?;
        if !locked_config_mcp.contains(&expected_server_info) {
            return Err(io::Error::other(format!(
                "generated MCP config stopped using the verified runtime while the stale mirror stayed locked: {locked_config_mcp}"
            ))
            .into());
        }
        let stale_sibling = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(
                "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; try { & projectatlas --require-version $env:PROJECTATLAS_VERSION --format json runtime-info; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE } } catch { Write-Error $_; exit 1 }",
            )
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let stale_sibling_stdout = String::from_utf8_lossy(&stale_sibling.stdout);
        if stale_sibling.status.success() {
            return Err(io::Error::other(format!(
                "unchanged stale parent unexpectedly resolved a current bare runtime\n{stale_sibling_stdout}"
            ))
            .into());
        }
        let stale_sibling_command = stale_sibling_stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .ok_or_else(|| io::Error::other("stale sibling did not report its command path"))?;
        require_same_executable(
            stale_sibling_command.trim(),
            &stable_runtime,
            "stale unchanged parent sibling",
        )?;

        let versioned_runtime_dir = versioned_runtime
            .parent()
            .ok_or_else(|| io::Error::other("versioned runtime parent missing"))?;
        let known_stale_shim = app_data.join(NPM_SHIM_DIR).join("projectatlas.cmd");
        let known_stale_shim_dir = known_stale_shim
            .parent()
            .ok_or_else(|| io::Error::other("known stale shim parent missing"))?;
        fs::create_dir_all(known_stale_shim_dir)?;
        fs::write(
            &known_stale_shim,
            "@echo off\r\necho {\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.0.1\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}\r\n",
        )?;
        let fresh_host_path = std::env::join_paths(
            std::iter::once(known_stale_shim_dir.to_path_buf())
                .chain(std::iter::once(versioned_runtime_dir.to_path_buf()))
                .chain(std::env::split_paths(&parent_path)),
        )?;
        let post_quarantine_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-RuntimePath")
            .arg(&versioned_runtime)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &fresh_host_path)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let post_quarantine_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&post_quarantine_output.stdout),
            String::from_utf8_lossy(&post_quarantine_output.stderr)
        );
        if !post_quarantine_output.status.success()
            || !post_quarantine_text.trim_end().ends_with(
                "ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=true host_restart_required=false",
            )
            || post_quarantine_text.contains("ProjectAtlas stale bare command:")
            || post_quarantine_text.contains("Existing host restart required:")
        {
            return Err(io::Error::other(format!(
                "installer did not recognize the current runtime exposed after stale-shim quarantine\n{post_quarantine_text}"
            ))
            .into());
        }
        let known_stale_quarantine = stale_shim_quarantine_path(&known_stale_shim, "0.0.1");
        if known_stale_shim.exists()
            || !known_stale_quarantine.exists()
            || !post_quarantine_text.contains("Quarantined stale ProjectAtlas shim")
        {
            return Err(io::Error::other(format!(
                "installer did not quarantine the inherited stale shim before deriving restart state\n{post_quarantine_text}"
            ))
            .into());
        }
        if let Some(status) = stale_lock_process
            .as_mut()
            .ok_or_else(|| io::Error::other("stale mirror lock process missing"))?
            .try_wait()?
        {
            return Err(io::Error::other(format!(
                "post-quarantine install terminated the owned lock process: {status}"
            ))
            .into());
        }
        let fresh_sibling_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(
                "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; & projectatlas --require-version $env:PROJECTATLAS_VERSION --format json runtime-info",
            )
            .env("PATH", &fresh_host_path)
            .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let fresh_sibling_stdout = String::from_utf8_lossy(&fresh_sibling_output.stdout);
        if !fresh_sibling_output.status.success() {
            return Err(io::Error::other(format!(
                "fresh sibling failed to use the persisted runtime precedence:\n{fresh_sibling_stdout}\n{}",
                String::from_utf8_lossy(&fresh_sibling_output.stderr)
            ))
            .into());
        }
        let fresh_sibling_command = fresh_sibling_stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .ok_or_else(|| {
                io::Error::other("fresh sibling did not report its bare command path")
            })?;
        require_same_executable(
            fresh_sibling_command.trim(),
            &versioned_runtime,
            "fresh parent sibling",
        )?;

        let mut stale_lock = stale_lock_process
            .take()
            .ok_or_else(|| io::Error::other("stale mirror lock process missing before rerun"))?;
        stale_lock.kill()?;
        stale_lock.wait()?;
        let converged_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-RuntimePath")
            .arg(&versioned_runtime)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let converged_output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&converged_output.stdout),
            String::from_utf8_lossy(&converged_output.stderr)
        );
        if !converged_output.status.success()
            || !converged_output_text.contains("stable_mirror_ready=true")
            || !converged_output_text.trim_end().ends_with(
                "ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=true host_restart_required=false",
            )
            || converged_output_text.contains("ProjectAtlas stale bare command:")
        {
            return Err(io::Error::other(format!(
                "installer did not converge the stable mirror after its lock was released\n{converged_output_text}"
            ))
            .into());
        }
        let converged_before = mcp_database_snapshot(&db)?;
        let bare_token = StdCommand::new("powershell")
            .current_dir(&repo)
            .arg("-NoProfile")
            .arg("-Command")
            .arg(
                "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; & projectatlas --require-version $env:PROJECTATLAS_VERSION --format json runtime-info; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; & projectatlas --require-version $env:PROJECTATLAS_VERSION --db $env:PROJECTATLAS_DB token --view tui; exit $LASTEXITCODE",
            )
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
            .env("PROJECTATLAS_DB", &db)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()?;
        let bare_token_stdout = String::from_utf8_lossy(&bare_token.stdout);
        if !bare_token.status.success() {
            return Err(io::Error::other(format!(
                "converged bare runtime/version/token gate failed:\n{bare_token_stdout}\n{}",
                String::from_utf8_lossy(&bare_token.stderr)
            ))
            .into());
        }
        let bare_command = bare_token_stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .ok_or_else(|| io::Error::other("converged bare gate omitted its command path"))?;
        require_same_executable(
            bare_command.trim(),
            &stable_runtime,
            "converged bare command",
        )?;
        if mcp_database_snapshot(&db)? != converged_before {
            return Err(io::Error::other(
                "converged bare runtime/token gate changed the compatible database",
            )
            .into());
        }

        Ok(())
    })();

    if !current_lock_reaped {
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
    }
    if let Some(process) = stale_lock_process.as_mut() {
        let kill_result = process.kill();
        let wait_result = process.wait();
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
    }
    test_result
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_retires_only_exact_child_and_reports_retry_failure()
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

    let fixture_source = temp
        .path()
        .join(OBSOLETE_PROJECTATLAS_FIXTURE_SOURCE_FILE_NAME);
    fs::write(
        &fixture_source,
        r#"using System;
using System.Threading;

public static class Program
{
    public static int Main(string[] arguments)
    {
        if (Array.IndexOf(arguments, "runtime-info") >= 0)
        {
            Console.WriteLine("{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.3.26\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}");
            return 0;
        }
        if (Array.IndexOf(arguments, "mcp") >= 0)
        {
            Thread.Sleep(Timeout.Infinite);
            return 0;
        }
        return 2;
    }
}
"#,
    )?;
    let compile_output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(
            "Add-Type -Path $env:PROJECTATLAS_FIXTURE_SOURCE -OutputAssembly $env:PROJECTATLAS_FIXTURE_RUNTIME -OutputType ConsoleApplication",
        )
        .env("PROJECTATLAS_FIXTURE_SOURCE", &fixture_source)
        .env("PROJECTATLAS_FIXTURE_RUNTIME", &stable_runtime)
        .output()?;
    if !compile_output.status.success() {
        return Err(io::Error::other(format!(
            "failed to compile obsolete ProjectAtlas fixture runtime:\n{}",
            String::from_utf8_lossy(&compile_output.stderr)
        ))
        .into());
    }

    let db = atlas_dir.join("projectatlas.db");
    let codex_owner_fixture = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    compile_codex_mcp_owner_fixture(&codex_owner_fixture)?;
    let child_pid_file = temp.path().join("obsolete-mcp.pid");

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let parent_path =
        std::env::join_paths(std::env::split_paths(&inherited_path).filter(|entry| {
            ![
                "projectatlas.exe",
                "projectatlas.cmd",
                "projectatlas.bat",
                "projectatlas.ps1",
            ]
            .iter()
            .any(|candidate| entry.join(candidate).exists())
        }))?;
    let stale_parent_path = std::env::join_paths(
        std::iter::once(
            stable_runtime
                .parent()
                .ok_or_else(|| io::Error::other("stable runtime parent missing"))?
                .to_path_buf(),
        )
        .chain(std::env::split_paths(&parent_path)),
    )?;
    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex = isolated_home.join("codex.cmd");
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let plugin_cache = isolated_home.join(FAKE_CODEX_PLUGIN_CACHE_DIR);
    let plugin_manifest = plugin_cache
        .join(CODEX_PLUGIN_MANIFEST_DIR)
        .join("plugin.json");
    let plugin_skill = plugin_cache
        .join(PROJECTATLAS_SKILL_DIR)
        .join(PROJECTATLAS_SKILL_NAME)
        .join(SKILL_FILE_NAME);
    fs::create_dir_all(
        plugin_manifest
            .parent()
            .ok_or_else(|| io::Error::other("fake plugin manifest parent missing"))?,
    )?;
    fs::create_dir_all(
        plugin_skill
            .parent()
            .ok_or_else(|| io::Error::other("fake plugin skill parent missing"))?,
    )?;
    fs::write(
        &plugin_manifest,
        serde_json::to_vec(&json!({ "version": env!("CARGO_PKG_VERSION") }))?,
    )?;
    fs::write(&plugin_skill, FAKE_CODEX_SKILL_CONTENT)?;
    let fake_plugin_list = isolated_home.join(FAKE_CODEX_PLUGIN_LIST_FILE_NAME);
    fs::write(
        &fake_plugin_list,
        serde_json::to_vec(&json!({
            "installed": [{
                "pluginId": "projectatlas@projectatlas",
                "name": "projectatlas",
                "marketplaceName": "projectatlas",
                "version": env!("CARGO_PKG_VERSION"),
                "installed": true,
                "enabled": true,
                "marketplaceSource": {
                    "source": "https://github.com/styler-ai/ProjectAtlas.git"
                },
                "source": { "path": plugin_cache }
            }]
        }))?,
    )?;
    let stale_registry = isolated_home.join(FAKE_CODEX_REGISTRY_STALE_FILE_NAME);
    fs::write(
        &stale_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": stable_runtime,
                "args": ["--require-version", "0.3.26", "--db", "C:\\old\\.projectatlas\\projectatlas.db", "mcp"]
            }
        }))?,
    )?;
    let current_registry = isolated_home.join(FAKE_CODEX_REGISTRY_CURRENT_FILE_NAME);
    fs::write(
        &current_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", db,
                    "--config", atlas_dir.join("config.toml"),
                    "mcp"
                ]
            }
        }))?,
    )?;
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  type \"%PROJECTATLAS_FAKE_CODEX_PLUGIN_LIST%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"add\" (\r\n  echo current>\"%PROJECTATLAS_FAKE_CODEX_STATE%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_STATE%\" (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT%\"\r\n  ) else (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE%\"\r\n  )\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
    )?;
    let fake_codex_state = isolated_home.join("codex-state.txt");
    let production_installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let installer_plugin_root = production_installer
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("installer plugin root missing"))?;
    let installer = temp.path().join("install-runtime-owner-seam.ps1");
    write_installer_with_test_codex_identity_seam(&production_installer, &installer)?;
    let powershell = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?,
    )
    .join(WINDOWS_SYSTEM32_DIR)
    .join(WINDOWS_POWERSHELL_DIR)
    .join(WINDOWS_POWERSHELL_VERSION_DIR)
    .join(WINDOWS_POWERSHELL_EXECUTABLE);
    let run_installer = |process_ids: &[u32], process_path: &OsStr| {
        let mut process_ids = process_ids.to_vec();
        process_ids.push(std::process::id());
        let process_id_allowlist = windows_test_process_id_allowlist(&process_ids)?;
        StdCommand::new(&powershell)
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-RuntimePath")
            .arg(&runtime)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", process_path)
            .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_TEST_CODEX_OWNER", &codex_owner_fixture)
            .env("PROJECTATLAS_TEST_PROCESS_IDS", process_id_allowlist)
            .env(
                "PROJECTATLAS_TEST_INSTALLER_PLUGIN_ROOT",
                installer_plugin_root,
            )
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
            .env("PROJECTATLAS_FAKE_CODEX_PLUGIN_LIST", &fake_plugin_list)
            .env("PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE", &stale_registry)
            .env(
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
                &current_registry,
            )
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()
    };

    let run_production_installer = || {
        StdCommand::new(&powershell)
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&production_installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-RuntimePath")
            .arg(&runtime)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
            .env("PROJECTATLAS_FAKE_CODEX_PLUGIN_LIST", &fake_plugin_list)
            .env("PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE", &stale_registry)
            .env(
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
                &current_registry,
            )
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()
    };

    let readiness_started = Instant::now();
    let (mut codex_owner, obsolete_mcp_pid) = spawn_codex_owned_obsolete_mcp(
        &codex_owner_fixture,
        &stable_runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &child_pid_file,
        Some(CODEX_OWNER_DELAYED_PUBLICATION),
        None,
    )?;
    let readiness_elapsed = readiness_started.elapsed();
    let delayed_publication_max_elapsed =
        CODEX_OWNER_READINESS_TIMEOUT + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    let delayed_publication_crossed_former_budget = readiness_elapsed > Duration::from_secs(5)
        && readiness_elapsed < delayed_publication_max_elapsed;
    let mut second_codex_owner = None;
    let mut second_obsolete_mcp_pid = None;
    let mut non_codex_child = None;
    let test_result = (|| -> Result<(), Box<dyn Error>> {
        if !delayed_publication_crossed_former_budget {
            return Err(io::Error::other(format!(
                "delayed Codex owner publication did not cross the former five-second boundary within the one readiness deadline: elapsed={readiness_elapsed:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} scheduler_tolerance={CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE:?} max_elapsed={delayed_publication_max_elapsed:?}"
            ))
            .into());
        }
        let unsigned_output = run_production_installer()?;
        let unsigned_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&unsigned_output.stdout),
            String::from_utf8_lossy(&unsigned_output.stderr)
        );
        if !unsigned_output.status.success()
            || !unsigned_text.contains("obsolete_mcp_handoff=unsafe_owner")
            || !windows_process_is_alive(&obsolete_mcp_pid)?
        {
            return Err(io::Error::other(format!(
                "unsigned arbitrary codex.exe owner was not refused safely:\n{unsigned_text}"
            ))
            .into());
        }
        let output = run_installer(
            &[obsolete_mcp_pid.process_id, codex_owner.id()],
            &parent_path,
        )?;
        let installer_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let normalized_installer_output = installer_output.split_whitespace().collect::<String>();
        let expected_path_guidance = format!(
            "Configure {} first on PATH, then rerun this installer if convergence remains partial.",
            runtime
                .parent()
                .ok_or_else(|| io::Error::other("verified runtime parent missing"))?
                .display()
        );
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "installer failed exact obsolete MCP handoff:\n{installer_output}"
            ))
            .into());
        }
        if !normalized_installer_output.contains(
            &"Retired exact obsolete Codex-owned ProjectAtlas MCP process"
                .split_whitespace()
                .collect::<String>(),
        ) || !normalized_installer_output.contains(
            &"ProjectAtlas convergence: update_state=complete stable_mirror_ready=true obsolete_mcp_handoff=completed"
                .split_whitespace()
                .collect::<String>(),
        ) || !normalized_installer_output.ends_with(
            &"ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=false host_restart_required=false"
                .split_whitespace()
                .collect::<String>(),
        ) || !normalized_installer_output.contains(
            &expected_path_guidance
                .split_whitespace()
                .collect::<String>(),
        )
            || normalized_installer_output
                .contains("couldnotproveanexactretireableobsoleteMCPowner")
            || normalized_installer_output.contains("restarttheowninghost")
        {
            return Err(io::Error::other(format!(
                "installer did not report exact complete obsolete MCP convergence:\n{installer_output}"
            ))
            .into());
        }
        if windows_process_is_alive(&obsolete_mcp_pid)? {
            return Err(
                io::Error::other("installer left the exact obsolete MCP process running").into(),
            );
        }
        if codex_owner.try_wait()?.is_some() {
            return Err(io::Error::other("installer terminated the Codex owner process").into());
        }

        let runtime_info = StdCommand::new(&stable_runtime)
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--format")
            .arg("json")
            .arg("runtime-info")
            .output()?;
        if !runtime_info.status.success() {
            return Err(io::Error::other(format!(
                "stable mirror did not converge to the target runtime:\n{}",
                String::from_utf8_lossy(&runtime_info.stderr)
            ))
            .into());
        }
        let codex_config = read_json_file(&atlas_dir.join("projectatlas.mcp.json"))?;
        require_same_executable(
            json_string_at(&codex_config, &["mcpServers", "projectatlas", "command"])?,
            &runtime,
            "obsolete handoff codex config",
        )?;
        require_json_string(
            &codex_config,
            &["mcpServers", "projectatlas", "args", "1"],
            env!("CARGO_PKG_VERSION"),
        )?;
        let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
        if !fake_codex_calls.contains("mcp add projectatlas --")
            || !fake_codex_calls.contains(runtime.to_string_lossy().as_ref())
            || fake_codex_calls.contains(stable_runtime.to_string_lossy().as_ref())
        {
            return Err(io::Error::other(format!(
                "Codex registry did not converge before the exact MCP handoff:\n{fake_codex_calls}"
            ))
            .into());
        }

        fs::remove_file(&stable_runtime)?;
        let recompile_output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(
                "Add-Type -Path $env:PROJECTATLAS_FIXTURE_SOURCE -OutputAssembly $env:PROJECTATLAS_FIXTURE_RUNTIME -OutputType ConsoleApplication",
            )
            .env("PROJECTATLAS_FIXTURE_SOURCE", &fixture_source)
            .env("PROJECTATLAS_FIXTURE_RUNTIME", &stable_runtime)
            .output()?;
        if !recompile_output.status.success() {
            return Err(io::Error::other(format!(
                "failed to restore obsolete ProjectAtlas fixture runtime:\n{}",
                String::from_utf8_lossy(&recompile_output.stderr)
            ))
            .into());
        }
        let direct_child = StdCommand::new(&stable_runtime)
            .arg("--require-version")
            .arg("0.3.26")
            .arg("--db")
            .arg(&db)
            .arg("--config")
            .arg(atlas_dir.join("config.toml"))
            .arg("mcp")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let direct_child_id = direct_child.id();
        non_codex_child = Some(direct_child);
        let (owner, child_pid) = spawn_codex_owned_obsolete_mcp(
            &codex_owner_fixture,
            &stable_runtime,
            &db,
            Some(&atlas_dir.join("config.toml")),
            &temp.path().join("retry-obsolete-mcp.pid"),
            None,
            None,
        )?;
        second_codex_owner = Some(owner);
        second_obsolete_mcp_pid = Some(child_pid.clone());

        let retry_parent_id = second_codex_owner
            .as_ref()
            .ok_or_else(|| io::Error::other("second Codex owner fixture missing"))?
            .id();
        let retry_output = run_installer(
            &[child_pid.process_id, retry_parent_id, direct_child_id],
            &stale_parent_path,
        )?;
        let retry_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&retry_output.stdout),
            String::from_utf8_lossy(&retry_output.stderr)
        );
        let normalized_retry_text = retry_text.split_whitespace().collect::<String>();
        let stable_runtime_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&stable_runtime)?).replace('/', "\\");
        let runtime_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&runtime)?).replace('/', "\\");
        let db_diagnostic_path = normalize_native_path_display(&db).replace('/', "\\");
        let config_diagnostic_path =
            normalize_native_path_display(atlas_dir.join("config.toml")).replace('/', "\\");
        for required in [
            format!(
                "ProjectAtlas stale bare command: path={stable_runtime_diagnostic_path} observed_version=0.3.26 ready=false"
            ),
            format!(
                "verified_runtime={} target_version={}",
                runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime command: & '{}' --require-version '{}' --format json runtime-info",
                runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "ProjectAtlas verified absolute runtime operation: & '{}' '--require-version' '{}' '--db' '{}' '--config' '{}' 'token' '--view' 'tui'",
                runtime_diagnostic_path,
                env!("CARGO_PKG_VERSION"),
                db_diagnostic_path,
                config_diagnostic_path
            ),
            "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=false"
                .to_string(),
        ] {
            if !normalized_retry_text.contains(&required.split_whitespace().collect::<String>()) {
                return Err(io::Error::other(format!(
                    "obsolete locked-mirror retry omitted exact diagnostic {required:?}:\n{retry_text}"
                ))
                .into());
            }
        }
        if !retry_output.status.success()
            || !normalized_retry_text.contains(
                &"ProjectAtlas convergence: update_state=partial stable_mirror_ready=false obsolete_mcp_handoff=retry_failed codex_plugin_ready=true codex_registry_ready=true"
                    .split_whitespace()
                    .collect::<String>(),
            )
            || !runtime.exists()
            || !atlas_dir.join("projectatlas.mcp.json").exists()
            || !atlas_dir.join("projectatlas.claude.mcp.json").exists()
            || !atlas_dir.join("projectatlas.opencode.json").exists()
            || read_json_file(&atlas_dir.join("projectatlas.mcp.json"))? != codex_config
        {
            return Err(io::Error::other(format!(
                "installer did not preserve truthful partial readiness after the bounded retry failed:\n{retry_text}"
            ))
            .into());
        }
        if windows_process_is_alive(&child_pid)? {
            return Err(io::Error::other(
                "installer left the exact Codex-owned retry fixture running",
            )
            .into());
        }
        if second_codex_owner
            .as_mut()
            .ok_or_else(|| io::Error::other("second Codex owner fixture missing"))?
            .try_wait()?
            .is_some()
        {
            return Err(io::Error::other(
                "installer terminated the second Codex parent during retry failure",
            )
            .into());
        }
        if non_codex_child
            .as_mut()
            .ok_or_else(|| io::Error::other("non-Codex MCP fixture missing"))?
            .try_wait()?
            .is_some()
        {
            return Err(io::Error::other(
                "installer terminated the non-Codex process that kept the mirror locked",
            )
            .into());
        }
        let versioned_runtime_info = StdCommand::new(&runtime)
            .arg("--require-version")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("--format")
            .arg("json")
            .arg("runtime-info")
            .output()?;
        if !versioned_runtime_info.status.success() {
            return Err(io::Error::other(format!(
                "verified versioned runtime was unusable after retry failure:\n{}",
                String::from_utf8_lossy(&versioned_runtime_info.stderr)
            ))
            .into());
        }
        Ok(())
    })();

    if let Some(process_id) = second_obsolete_mcp_pid.as_ref()
        && windows_process_is_alive(process_id)?
    {
        let stop_result = stop_windows_fixture_process(process_id);
        if let Err(error) = stop_result
            && test_result.is_ok()
        {
            return Err(error);
        }
    }
    if let Some(process) = second_codex_owner.as_mut()
        && process.try_wait()?.is_none()
    {
        let kill_result = process.kill();
        let wait_result = process.wait();
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
    }
    if let Some(process) = non_codex_child.as_mut()
        && process.try_wait()?.is_none()
    {
        let kill_result = process.kill();
        let wait_result = process.wait();
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
    }
    if windows_process_is_alive(&obsolete_mcp_pid)? {
        let stop_result = stop_windows_fixture_process(&obsolete_mcp_pid);
        if let Err(error) = stop_result
            && test_result.is_ok()
        {
            return Err(error);
        }
    }
    if codex_owner.try_wait()?.is_none() {
        let kill_result = codex_owner.kill();
        let wait_result = codex_owner.wait();
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
    }
    test_result
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_preserves_unready_and_ambiguous_processes()
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
    let fixture_source = temp.path().join("obsolete-projectatlas-unready.cs");
    fs::write(
        &fixture_source,
        r#"using System;
using System.Threading;

public static class Program
{
    public static int Main(string[] arguments)
    {
        if (Array.IndexOf(arguments, "runtime-info") >= 0)
        {
            Console.WriteLine("{\"project\":\"ProjectAtlas\",\"major_version\":3,\"version\":\"0.3.26\",\"capabilities\":[\"mcp\"],\"text_format\":\"TOON\"}");
            return 0;
        }
        if (Array.IndexOf(arguments, "mcp") >= 0)
        {
            Thread.Sleep(Timeout.Infinite);
            return 0;
        }
        return 2;
    }
}
"#,
    )?;
    let compile_obsolete_runtime = || -> Result<(), Box<dyn Error>> {
        let output = StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(
                "Add-Type -Path $env:PROJECTATLAS_FIXTURE_SOURCE -OutputAssembly $env:PROJECTATLAS_FIXTURE_RUNTIME -OutputType ConsoleApplication",
            )
            .env("PROJECTATLAS_FIXTURE_SOURCE", &fixture_source)
            .env("PROJECTATLAS_FIXTURE_RUNTIME", &stable_runtime)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "failed to compile obsolete ProjectAtlas readiness fixture:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        Ok(())
    };
    compile_obsolete_runtime()?;

    let db = atlas_dir.join("projectatlas.db");
    let codex_owner_fixture = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    compile_codex_mcp_owner_fixture(&codex_owner_fixture)?;
    let first_child_pid_file = temp.path().join("first-obsolete-mcp.pid");

    let runtime_source = assert_cmd::cargo::cargo_bin("projectatlas");
    let runtime = temp.path().join("projectatlas-current.exe");
    fs::copy(&runtime_source, &runtime)?;
    let plugin_cache = isolated_home.join(FAKE_CODEX_PLUGIN_CACHE_DIR);
    let plugin_manifest = plugin_cache
        .join(CODEX_PLUGIN_MANIFEST_DIR)
        .join("plugin.json");
    let plugin_skill = plugin_cache
        .join(PROJECTATLAS_SKILL_DIR)
        .join(PROJECTATLAS_SKILL_NAME)
        .join(SKILL_FILE_NAME);
    fs::create_dir_all(
        plugin_manifest
            .parent()
            .ok_or_else(|| io::Error::other("fake plugin manifest parent missing"))?,
    )?;
    fs::create_dir_all(
        plugin_skill
            .parent()
            .ok_or_else(|| io::Error::other("fake plugin skill parent missing"))?,
    )?;
    fs::write(
        &plugin_manifest,
        serde_json::to_vec(&json!({ "version": env!("CARGO_PKG_VERSION") }))?,
    )?;
    fs::write(&plugin_skill, FAKE_CODEX_SKILL_CONTENT)?;
    let plugin_list = isolated_home.join(FAKE_CODEX_PLUGIN_LIST_FILE_NAME);
    fs::write(
        &plugin_list,
        serde_json::to_vec(&json!({ "installed": [] }))?,
    )?;
    let write_plugin_list =
        |version: &str, enabled: bool, marketplace_source: &str| -> Result<(), Box<dyn Error>> {
            fs::write(
                &plugin_list,
                serde_json::to_vec(&json!({
                    "installed": [{
                        "pluginId": "projectatlas@projectatlas",
                        "name": "projectatlas",
                        "marketplaceName": "projectatlas",
                        "version": version,
                        "installed": true,
                        "enabled": enabled,
                        "marketplaceSource": { "source": marketplace_source },
                        "source": { "path": &plugin_cache }
                    }]
                }))?,
            )?;
            Ok(())
        };
    let registry = isolated_home.join("codex-registry.json");
    fs::write(
        &registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": stable_runtime,
                "args": ["--require-version", "0.3.26", "--db", "C:\\old\\projectatlas.db", "mcp"]
            }
        }))?,
    )?;
    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let config_drift_trigger = isolated_home.join("drift-generated-config");
    let config_to_drift = atlas_dir.join("projectatlas.mcp.json");
    let runtime_drift_trigger = isolated_home.join("drift-runtime");
    let fake_codex = isolated_home.join("codex.cmd");
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_RUNTIME_DRIFT_TRIGGER%\" (\r\n    echo invalid>\"%PROJECTATLAS_FAKE_CODEX_RUNTIME_TO_DRIFT%\"\r\n    del /q \"%PROJECTATLAS_FAKE_CODEX_RUNTIME_DRIFT_TRIGGER%\"\r\n  )\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_CONFIG_DRIFT_TRIGGER%\" (\r\n    echo {}>\"%PROJECTATLAS_FAKE_CODEX_CONFIG_TO_DRIFT%\"\r\n    del /q \"%PROJECTATLAS_FAKE_CODEX_CONFIG_DRIFT_TRIGGER%\"\r\n  )\r\n  type \"%PROJECTATLAS_FAKE_CODEX_PLUGIN_LIST%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  if not exist \"%PROJECTATLAS_FAKE_CODEX_REGISTRY%\" exit /b 1\r\n  type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY%\"\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
    )?;
    let stable_runtime_dir = stable_runtime
        .parent()
        .ok_or_else(|| io::Error::other("stable runtime parent missing"))?;
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let parent_path = std::env::join_paths(
        std::iter::once(stable_runtime_dir.to_path_buf())
            .chain(std::env::split_paths(&inherited_path)),
    )?;
    let production_installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let installer_plugin_root = production_installer
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("installer plugin root missing"))?;
    let installer = temp.path().join("install-runtime-readiness-seam.ps1");
    write_installer_with_test_codex_identity_seam(&production_installer, &installer)?;
    let run_installer = |process_ids: &[u32]| {
        let mut process_ids = process_ids.to_vec();
        process_ids.push(std::process::id());
        let process_id_allowlist = windows_test_process_id_allowlist(&process_ids)?;
        StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&installer)
            .arg("-ProjectRoot")
            .arg(&repo)
            .arg("-ProjectAtlasVersion")
            .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
            .arg("-RuntimePath")
            .arg(&runtime)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("APPDATA", &app_data)
            .env("LOCALAPPDATA", &local_app_data)
            .env("PATH", &parent_path)
            .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
            .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
            .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
            .env("PROJECTATLAS_TEST_CODEX_OWNER", &codex_owner_fixture)
            .env("PROJECTATLAS_TEST_PROCESS_IDS", process_id_allowlist)
            .env(
                "PROJECTATLAS_TEST_INSTALLER_PLUGIN_ROOT",
                installer_plugin_root,
            )
            .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
            .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
            .env("PROJECTATLAS_FAKE_CODEX_PLUGIN_LIST", &plugin_list)
            .env("PROJECTATLAS_FAKE_CODEX_REGISTRY", &registry)
            .env(
                "PROJECTATLAS_FAKE_CODEX_CONFIG_DRIFT_TRIGGER",
                &config_drift_trigger,
            )
            .env("PROJECTATLAS_FAKE_CODEX_CONFIG_TO_DRIFT", &config_to_drift)
            .env(
                "PROJECTATLAS_FAKE_CODEX_RUNTIME_DRIFT_TRIGGER",
                &runtime_drift_trigger,
            )
            .env("PROJECTATLAS_FAKE_CODEX_RUNTIME_TO_DRIFT", &runtime)
            .env("PROJECTATLAS_NO_TELEMETRY", "1")
            .output()
    };

    let exact_registry = json!({
        "name": "projectatlas",
        "enabled": true,
        "transport": {
            "type": "stdio",
            "command": &runtime,
            "args": [
                "--require-version", env!("CARGO_PKG_VERSION"),
                "--db", &db,
                "--config", atlas_dir.join("config.toml"),
                "mcp"
            ]
        }
    });
    write_plugin_list(
        env!("CARGO_PKG_VERSION"),
        true,
        "https://github.com/styler-ai/ProjectAtlas.git",
    )?;
    fs::write(&registry, serde_json::to_vec(&exact_registry)?)?;
    fs::write(&config_drift_trigger, b"drift")?;
    let no_handoff_drift_output = run_installer(&[])?;
    let no_handoff_drift_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&no_handoff_drift_output.stdout),
        String::from_utf8_lossy(&no_handoff_drift_output.stderr)
    );
    if !no_handoff_drift_output.status.success()
        || !no_handoff_drift_text.contains("update_state=partial")
        || !no_handoff_drift_text.contains("obsolete_mcp_handoff=not_required")
        || !no_handoff_drift_text.contains("codex_plugin_ready=true codex_registry_ready=true")
        || !no_handoff_drift_text.contains("runtime_ready=true")
        || !no_handoff_drift_text.contains("generated_mcp_configs_ready=false")
        || !no_handoff_drift_text.contains("runtime_mcp_configs_ready=false")
        || !no_handoff_drift_text.contains("rerun this installer")
        || no_handoff_drift_text.contains("integration verified through generated MCP config")
        || no_handoff_drift_text.contains("The runtime and generated MCP configs are ready")
    {
        return Err(io::Error::other(format!(
            "installer reported stale final runtime/config readiness without a handoff:\n{no_handoff_drift_text}"
        ))
        .into());
    }
    let runtime_diagnostic_path =
        normalize_native_path_display(fs::canonicalize(&runtime)?).replace('/', "\\");
    fs::write(&runtime_drift_trigger, b"drift")?;
    let no_handoff_runtime_drift_output = run_installer(&[])?;
    let no_handoff_runtime_drift_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&no_handoff_runtime_drift_output.stdout),
        String::from_utf8_lossy(&no_handoff_runtime_drift_output.stderr)
    );
    let normalized_no_handoff_runtime_drift_text = no_handoff_runtime_drift_text
        .split_whitespace()
        .collect::<String>();
    if !no_handoff_runtime_drift_output.status.success()
        || !no_handoff_runtime_drift_text.contains("update_state=partial")
        || !no_handoff_runtime_drift_text.contains("obsolete_mcp_handoff=not_required")
        || !no_handoff_runtime_drift_text.contains("runtime_ready=false")
        || !no_handoff_runtime_drift_text.contains("generated_mcp_configs_ready=true")
        || !no_handoff_runtime_drift_text.contains("runtime_mcp_configs_ready=false")
        || !no_handoff_runtime_drift_text.contains("installer_cli_ready=false")
        || !normalized_no_handoff_runtime_drift_text.contains(
            &format!(
                "ProjectAtlas runtime failed final verification: path={runtime_diagnostic_path}"
            )
            .split_whitespace()
            .collect::<String>(),
        )
        || !normalized_no_handoff_runtime_drift_text.contains(
            &"ProjectAtlas PATH shadow report skipped because the requested absolute runtime failed final verification"
                .split_whitespace()
                .collect::<String>(),
        )
        || no_handoff_runtime_drift_text.contains("ProjectAtlas verified absolute runtime command:")
        || no_handoff_runtime_drift_text
            .contains("ProjectAtlas verified absolute runtime operation:")
        || no_handoff_runtime_drift_text.contains("integration verified through generated MCP config")
        || no_handoff_runtime_drift_text
            .contains("The runtime and generated MCP configs are ready")
    {
        return Err(io::Error::other(format!(
            "installer cached runtime readiness before final no-handoff probes:\n{no_handoff_runtime_drift_text}"
        ))
        .into());
    }
    fs::copy(&runtime_source, &runtime)?;
    fs::remove_file(&stable_runtime)?;
    fs::create_dir(&stable_runtime)?;
    let invalid_mirror_output = run_installer(&[])?;
    let invalid_mirror_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&invalid_mirror_output.stdout),
        String::from_utf8_lossy(&invalid_mirror_output.stderr)
    );
    let normalized_invalid_mirror_text = invalid_mirror_text.split_whitespace().collect::<String>();
    if !invalid_mirror_output.status.success()
        || !normalized_invalid_mirror_text.contains(
            &"ProjectAtlas convergence: update_state=partial stable_mirror_ready=false obsolete_mcp_handoff=inspection_failed codex_plugin_ready=true codex_registry_ready=true"
                .split_whitespace()
                .collect::<String>(),
        )
        || !normalized_invalid_mirror_text.contains("runtime_mcp_configs_ready=true")
        || !normalized_invalid_mirror_text
            .contains("Repairtheinvalidmirrorpathandrerunthisinstaller")
        || normalized_invalid_mirror_text
            .contains("RetiredexactobsoleteCodex-ownedProjectAtlasMCPprocess")
        || !stable_runtime.is_dir()
    {
        return Err(io::Error::other(format!(
            "installer trusted or aborted on a directory-shaped stable runtime mirror:\n{invalid_mirror_text}"
        ))
        .into());
    }
    for forbidden_state in [
        "obsolete_mcp_handoff=exited",
        "obsolete_mcp_handoff=retired",
        "obsolete_mcp_handoff=completed",
        "obsolete_mcp_handoff=retry_failed",
    ] {
        if normalized_invalid_mirror_text.contains(forbidden_state) {
            return Err(io::Error::other(format!(
                "invalid stable mirror entered forbidden handoff state {forbidden_state}:\n{invalid_mirror_text}"
            ))
            .into());
        }
    }
    fs::remove_dir_all(&stable_runtime)?;
    fs::write(
        &plugin_list,
        serde_json::to_vec(&json!({ "installed": [] }))?,
    )?;
    fs::write(
        &registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": &stable_runtime,
                "args": ["--require-version", "0.3.26", "--db", "C:\\old\\projectatlas.db", "mcp"]
            }
        }))?,
    )?;
    compile_obsolete_runtime()?;

    let (mut first_owner, first_obsolete_mcp_pid) = spawn_codex_owned_obsolete_mcp(
        &codex_owner_fixture,
        &stable_runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &first_child_pid_file,
        None,
        None,
    )?;
    let mut second_owner = None;
    let mut second_obsolete_mcp_pid = None;
    let test_result = (|| -> Result<(), Box<dyn Error>> {
        {
            let require_plugin_unready = |label: &str| -> Result<(), Box<dyn Error>> {
                let output = run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
                let text = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if !output.status.success()
                    || !text.contains("obsolete_mcp_handoff=codex_plugin_not_verified")
                    || !text.contains("codex_plugin_ready=false codex_registry_ready=false")
                    || !windows_process_is_alive(&first_obsolete_mcp_pid)?
                {
                    return Err(io::Error::other(format!(
                        "{label} plugin state was treated as readiness:\n{text}"
                    ))
                    .into());
                }
                Ok(())
            };
            require_plugin_unready("missing")?;
            fs::write(
                &plugin_list,
                serde_json::to_vec(&json!([{ "installed": [] }]))?,
            )?;
            require_plugin_unready("singleton-array-root")?;
            fs::write(
                &plugin_list,
                serde_json::to_vec(&json!({
                    "installed": [[{
                        "pluginId": "projectatlas@projectatlas",
                        "name": "projectatlas",
                        "marketplaceName": "projectatlas",
                        "version": env!("CARGO_PKG_VERSION"),
                        "installed": true,
                        "enabled": true,
                        "marketplaceSource": {
                            "source": "https://github.com/styler-ai/ProjectAtlas.git"
                        },
                        "source": { "path": &plugin_cache }
                    }]]
                }))?,
            )?;
            require_plugin_unready("singleton-array-plugin-entry")?;
            for (label, version, enabled, source) in [
                (
                    "stale",
                    "0.0.1",
                    true,
                    "https://github.com/styler-ai/ProjectAtlas.git",
                ),
                (
                    "disabled",
                    env!("CARGO_PKG_VERSION"),
                    false,
                    "https://github.com/styler-ai/ProjectAtlas.git",
                ),
                (
                    "wrong-source",
                    env!("CARGO_PKG_VERSION"),
                    true,
                    "https://github.com/example/ProjectAtlas.git",
                ),
            ] {
                write_plugin_list(version, enabled, source)?;
                require_plugin_unready(label)?;
            }
            write_plugin_list(
                env!("CARGO_PKG_VERSION"),
                true,
                "https://github.com/styler-ai/ProjectAtlas.git",
            )?;
            fs::write(&plugin_skill, b"stale skill")?;
            require_plugin_unready("wrong-skill")?;
            fs::write(&plugin_skill, FAKE_CODEX_SKILL_CONTENT)?;
            fs::write(
                &plugin_manifest,
                serde_json::to_vec(&json!([{
                    "version": env!("CARGO_PKG_VERSION")
                }]))?,
            )?;
            require_plugin_unready("singleton-array-source-manifest")?;
            fs::write(
                &plugin_manifest,
                serde_json::to_vec(&json!({
                    "version": env!("CARGO_PKG_VERSION")
                }))?,
            )?;
        }

        fs::write(
            &registry,
            serde_json::to_vec(&json!({
                "name": "projectatlas",
                "enabled": false,
                "transport": {
                    "type": "stdio",
                    "command": runtime,
                    "args": [
                        "--require-version", env!("CARGO_PKG_VERSION"),
                        "--db", db,
                        "--config", atlas_dir.join("config.toml"),
                        "mcp"
                    ]
                }
            }))?,
        )?;
        let disabled_registry_output =
            run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
        let disabled_registry_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&disabled_registry_output.stdout),
            String::from_utf8_lossy(&disabled_registry_output.stderr)
        );
        if !disabled_registry_output.status.success()
            || !disabled_registry_text.contains("obsolete_mcp_handoff=codex_registry_not_verified")
            || !disabled_registry_text
                .contains("codex_plugin_ready=true codex_registry_ready=false")
            || !windows_process_is_alive(&first_obsolete_mcp_pid)?
        {
            return Err(io::Error::other(format!(
                "disabled exact registry was treated as readiness:\n{disabled_registry_text}"
            ))
            .into());
        }

        fs::remove_file(&registry)?;
        let missing_registry_output =
            run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
        let missing_registry_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&missing_registry_output.stdout),
            String::from_utf8_lossy(&missing_registry_output.stderr)
        );
        if !missing_registry_output.status.success()
            || !missing_registry_text.contains("obsolete_mcp_handoff=codex_registry_not_verified")
            || !missing_registry_text.contains("codex_plugin_ready=true codex_registry_ready=false")
            || !windows_process_is_alive(&first_obsolete_mcp_pid)?
        {
            return Err(io::Error::other(format!(
                "skip flags treated missing registry as readiness:\n{missing_registry_text}"
            ))
            .into());
        }

        for (label, malformed_registry) in [
            ("singleton-array-root", json!([exact_registry.clone()])),
            (
                "singleton-array-transport",
                json!({
                    "name": "projectatlas",
                    "enabled": true,
                    "transport": [exact_registry["transport"].clone()]
                }),
            ),
        ] {
            fs::write(&registry, serde_json::to_vec(&malformed_registry)?)?;
            let malformed_output =
                run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
            let malformed_text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&malformed_output.stdout),
                String::from_utf8_lossy(&malformed_output.stderr)
            );
            if !malformed_output.status.success()
                || !malformed_text.contains("obsolete_mcp_handoff=codex_registry_not_verified")
                || !windows_process_is_alive(&first_obsolete_mcp_pid)?
            {
                return Err(io::Error::other(format!(
                    "{label} registry JSON shape was treated as readiness:\n{malformed_text}"
                ))
                .into());
            }
        }
        fs::write(&registry, serde_json::to_vec(&exact_registry)?)?;
        fs::write(&config_drift_trigger, b"drift")?;
        let drift_output = run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
        let drift_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&drift_output.stdout),
            String::from_utf8_lossy(&drift_output.stderr)
        );
        let normalized_drift_text = drift_text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !drift_output.status.success()
            || !drift_text.contains("update_state=partial")
            || !drift_text.contains("obsolete_mcp_handoff=replacement_readiness_changed")
            || !drift_text.contains("runtime_ready=true")
            || !drift_text.contains("generated_mcp_configs_ready=false")
            || !drift_text.contains("runtime_mcp_configs_ready=false")
            || !drift_text.contains("rerun this installer")
            || !normalized_drift_text.contains("ProjectAtlas stale bare command: path=")
            || !normalized_drift_text.contains("observed_version=0.3.26 ready=false")
            || !normalized_drift_text
                .contains(&format!("target_version={}", env!("CARGO_PKG_VERSION")))
            || !normalized_drift_text.contains("verified_runtime=")
            || !normalized_drift_text
                .contains("ProjectAtlas verified absolute runtime operation: & ")
            || !normalized_drift_text.contains(
                "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=",
            )
            || !normalized_drift_text.contains("--format json runtime-info")
            || !normalized_drift_text.contains("token --view tui")
            || drift_text.contains("integration verified through generated MCP config")
            || drift_text.contains("The runtime and generated MCP configs are ready")
            || !windows_process_is_alive(&first_obsolete_mcp_pid)?
            || first_owner.try_wait()?.is_some()
        {
            return Err(io::Error::other(format!(
                "installer reported stale final runtime/config readiness after config drift:\n{drift_text}"
            ))
            .into());
        }
        let runtime_diagnostic_path =
            normalize_native_path_display(fs::canonicalize(&runtime)?).replace('/', "\\");
        fs::write(&runtime_drift_trigger, b"drift")?;
        let runtime_drift_output =
            run_installer(&[first_obsolete_mcp_pid.process_id, first_owner.id()])?;
        let runtime_drift_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&runtime_drift_output.stdout),
            String::from_utf8_lossy(&runtime_drift_output.stderr)
        );
        let normalized_runtime_drift_text = runtime_drift_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !runtime_drift_output.status.success()
            || !runtime_drift_text.contains("update_state=partial")
            || !runtime_drift_text.contains("runtime_ready=false")
            || !runtime_drift_text.contains("generated_mcp_configs_ready=true")
            || !runtime_drift_text.contains("runtime_mcp_configs_ready=false")
            || !normalized_runtime_drift_text.contains("ProjectAtlas stale bare command: path=")
            || !normalized_runtime_drift_text.contains("observed_version=0.3.26 ready=false")
            || !normalized_runtime_drift_text
                .contains(&format!("target_version={}", env!("CARGO_PKG_VERSION")))
            || !normalized_runtime_drift_text.contains(&format!(
                "ProjectAtlas requested absolute runtime failed final verification: path={runtime_diagnostic_path}"
            ))
            || normalized_runtime_drift_text.contains("verified_runtime=")
            || normalized_runtime_drift_text
                .contains("ProjectAtlas verified absolute runtime command:")
            || normalized_runtime_drift_text
                .contains("ProjectAtlas verified absolute runtime operation:")
            || normalized_runtime_drift_text.contains(&format!(
                "-RuntimePath '{runtime_diagnostic_path}'"
            ))
            || runtime_drift_text.contains("integration verified through generated MCP config")
            || runtime_drift_text.contains("The runtime and generated MCP configs are ready")
            || !windows_process_is_alive(&first_obsolete_mcp_pid)?
            || first_owner.try_wait()?.is_some()
        {
            return Err(io::Error::other(format!(
                "installer advertised a runtime that failed final verification:\n{runtime_drift_text}"
            ))
            .into());
        }
        fs::copy(&runtime_source, &runtime)?;
        let (owner, process_id) = spawn_codex_owned_obsolete_mcp(
            &codex_owner_fixture,
            &stable_runtime,
            &db,
            Some(&atlas_dir.join("config.toml")),
            &temp.path().join("second-obsolete-mcp.pid"),
            None,
            None,
        )?;
        second_owner = Some(owner);
        second_obsolete_mcp_pid = Some(process_id);
        let second_identity = second_obsolete_mcp_pid
            .as_ref()
            .ok_or_else(|| io::Error::other("second obsolete MCP fixture missing"))?;
        let second_parent_id = second_owner
            .as_ref()
            .ok_or_else(|| io::Error::other("second Codex owner fixture missing"))?
            .id();
        let ambiguous_output = run_installer(&[
            first_obsolete_mcp_pid.process_id,
            first_owner.id(),
            second_identity.process_id,
            second_parent_id,
        ])?;
        let ambiguous_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&ambiguous_output.stdout),
            String::from_utf8_lossy(&ambiguous_output.stderr)
        );
        let second_alive = windows_process_is_alive(
            second_obsolete_mcp_pid
                .as_ref()
                .ok_or_else(|| io::Error::other("second obsolete MCP fixture missing"))?,
        )?;
        if !ambiguous_output.status.success()
            || !ambiguous_text.contains("obsolete_mcp_handoff=ambiguous")
            || !ambiguous_text.contains("codex_plugin_ready=true codex_registry_ready=true")
            || !windows_process_is_alive(&first_obsolete_mcp_pid)?
            || !second_alive
        {
            return Err(io::Error::other(format!(
                "ambiguous obsolete MCP owners were not preserved:\n{ambiguous_text}"
            ))
            .into());
        }
        let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
        for forbidden in [
            "plugin marketplace remove",
            "plugin remove",
            "plugin add",
            "mcp remove",
            "mcp add",
        ] {
            if fake_codex_calls.contains(forbidden) {
                return Err(io::Error::other(format!(
                    "skip flags allowed forbidden Codex mutation {forbidden:?}:\n{fake_codex_calls}"
                ))
                .into());
            }
        }
        Ok(())
    })();

    if let Some(process_id) = second_obsolete_mcp_pid.as_ref()
        && windows_process_is_alive(process_id)?
    {
        let stop_result = stop_windows_fixture_process(process_id);
        if let Err(error) = stop_result
            && test_result.is_ok()
        {
            return Err(error);
        }
    }
    if let Some(process) = second_owner.as_mut()
        && process.try_wait()?.is_none()
    {
        let kill_result = process.kill();
        let wait_result = process.wait();
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
    }
    if windows_process_is_alive(&first_obsolete_mcp_pid)? {
        let stop_result = stop_windows_fixture_process(&first_obsolete_mcp_pid);
        if let Err(error) = stop_result
            && test_result.is_ok()
        {
            return Err(error);
        }
    }
    if first_owner.try_wait()?.is_none() {
        let kill_result = first_owner.kill();
        let wait_result = first_owner.wait();
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
    }
    test_result
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_requires_exact_codex_plugin_state()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let script_dir = temp.path().join("scripts");
    fs::create_dir_all(&script_dir)?;
    let script = script_dir.join("test-codex-plugin-selection.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
foreach ($functionName in @(
        "Test-ProjectAtlasJsonObject",
        "Test-ProjectAtlasOfficialMarketplaceSource",
        "Get-ProjectAtlasCodexPluginInventory",
        "Get-ProjectAtlasCodexPlugin",
        "Get-ProjectAtlasCodexMarketplace",
        "Get-ProjectAtlasCodexPluginSourcePath",
        "Get-ProjectAtlasCodexPluginSourceManifestVersion",
        "Test-ProjectAtlasCodexPluginSourceManifest",
        "Test-ProjectAtlasCodexPluginReady"
    )) {
    $functionMatch = [regex]::Match(
        $installerSource,
        "(?ms)^function $functionName \{.*?^\}"
    )
    if (-not $functionMatch.Success) {
        throw "Installer Codex selector was not found: $functionName"
    }
    Invoke-Expression $functionMatch.Value
}
$script:pluginPayload = $null
function Invoke-ProjectAtlasBoundedJsonCommand {
    return ,$script:pluginPayload
}
$validPlugin = [pscustomobject]@{
    pluginId = "projectatlas@projectatlas"
    name = "projectatlas"
    marketplaceName = "projectatlas"
    version = "0.4.2"
    installed = $true
    enabled = $true
    marketplaceSource = [pscustomobject]@{
        source = "https://github.com/styler-ai/ProjectAtlas.git"
    }
    source = [pscustomobject]@{ path = $env:PROJECTATLAS_PLUGIN_CACHE }
}
$script:pluginPayload = [pscustomobject]@{ installed = @($validPlugin); available = @() }
if ($null -eq (Get-ProjectAtlasCodexPlugin "codex.exe")) {
    throw "Exact singleton ProjectAtlas plugin was rejected."
}
$invalidPluginPayloads = [System.Collections.Generic.List[object]]::new()
$invalidPluginPayloads.Add([object[]]@([pscustomobject]@{ installed = @($validPlugin) }))
$invalidPluginPayloads.Add([pscustomobject]@{ installed = $validPlugin })
$invalidPluginPayloads.Add([pscustomobject]@{ installed = @(
            $validPlugin,
            [pscustomobject]@{
                pluginId = 1
                name = "projectatlas"
                marketplaceName = "projectatlas"
            }
        ) })
$invalidPluginPayloads.Add([pscustomobject]@{ installed = @([pscustomobject]@{
                pluginId = 1
                name = "projectatlas"
                marketplaceName = "projectatlas"
            }) })
$invalidPluginPayloads.Add([pscustomobject]@{ installed = @("projectatlas") })
$invalidPluginPayloads.Add([pscustomobject]@{ installed = [object[]]@(, [object[]]@($validPlugin)) })
$invalidPluginPayloads.Add([pscustomobject]@{ installed = $null })
$invalidPluginPayloads.Add("projectatlas")
$invalidPluginLabels = @(
    "singleton-array-root",
    "scalar-installed",
    "valid-plus-colliding-record",
    "malformed-identity",
    "scalar-entry",
    "nested-entry-array",
    "null-installed",
    "scalar-root"
)
$caseIndex = 0
foreach ($payload in $invalidPluginPayloads) {
    $script:pluginPayload = $payload
    if ($null -ne (Get-ProjectAtlasCodexPlugin "codex.exe")) {
        throw "Malformed or ambiguous ProjectAtlas plugin selection was accepted: $($invalidPluginLabels[$caseIndex])"
    }
    $caseIndex += 1
}
$validMarketplace = [pscustomobject]@{
    name = "projectatlas"
    marketplaceSource = [pscustomobject]@{
        source = "https://github.com/styler-ai/ProjectAtlas.git"
    }
}
$validMarketplacePayload = [pscustomobject]@{ marketplaces = @($validMarketplace) }
if ($null -eq (Get-ProjectAtlasCodexMarketplace $validMarketplacePayload)) {
    throw "Exact singleton ProjectAtlas marketplace was rejected."
}
$invalidMarketplacePayloads = [System.Collections.Generic.List[object]]::new()
$invalidMarketplacePayloads.Add([object[]]@($validMarketplacePayload))
$invalidMarketplacePayloads.Add([pscustomobject]@{ marketplaces = $validMarketplace })
$invalidMarketplacePayloads.Add([pscustomobject]@{ marketplaces = @(
            $validMarketplace,
            [pscustomobject]@{ name = "projectatlas"; marketplaceSource = $null }
        ) })
$invalidMarketplacePayloads.Add([pscustomobject]@{ marketplaces = @("projectatlas") })
$invalidMarketplacePayloads.Add([pscustomobject]@{ marketplaces = [object[]]@(, [object[]]@($validMarketplace)) })
$invalidMarketplacePayloads.Add([pscustomobject]@{ marketplaces = $null })
$invalidMarketplacePayloads.Add("projectatlas")
$invalidMarketplaceLabels = @(
    "singleton-array-root",
    "scalar-marketplaces",
    "valid-plus-colliding-record",
    "scalar-entry",
    "nested-entry-array",
    "null-marketplaces",
    "scalar-root"
)
$caseIndex = 0
foreach ($payload in $invalidMarketplacePayloads) {
    if ($null -ne (Get-ProjectAtlasCodexMarketplace $payload)) {
        throw "Malformed or ambiguous ProjectAtlas marketplace selection was accepted: $($invalidMarketplaceLabels[$caseIndex])"
    }
    $caseIndex += 1
}
$manifestDirectory = Join-Path $env:PROJECTATLAS_PLUGIN_CACHE ".codex-plugin"
New-Item -ItemType Directory -Force -Path $manifestDirectory | Out-Null
$manifestPath = Join-Path $manifestDirectory "plugin.json"
Set-Content -LiteralPath $manifestPath -Value '{"name":"projectatlas","version":"0.4.2"}'
if (-not (Test-ProjectAtlasCodexPluginSourceManifest $validPlugin "0.4.2")) {
    throw "Exact object-root plugin source manifest was rejected."
}
Set-Content -LiteralPath $manifestPath -Value '[{"name":"projectatlas","version":"0.4.2"}]'
if ((Get-ProjectAtlasCodexPluginSourceManifestVersion $validPlugin) -eq "0.4.2" `
    -or (Test-ProjectAtlasCodexPluginSourceManifest $validPlugin "0.4.2")) {
    throw "Singleton-array plugin source manifest was accepted."
}
$script:pluginPayload = [pscustomobject]@{ installed = @($validPlugin); available = @() }
function Convert-ProjectAtlasVersionTag { return "0.4.2" }
function Resolve-ProjectAtlasCodexCommand { return "C:\Codex\codex.exe" }
function Get-ProjectAtlasCodexPlugin { return $validPlugin }
function Test-ProjectAtlasOfficialMarketplaceSource { return $true }
$script:sourceManifestError = $false
function Test-ProjectAtlasCodexPluginSourceManifest {
    if ($script:sourceManifestError) { throw "Unreadable plugin source manifest." }
    return $true
}
$script:pluginSourcePath = $null
function Get-ProjectAtlasCodexPluginSourcePath { return $script:pluginSourcePath }
function Get-ProjectAtlasSha256 { throw "Unreadable plugin artifact." }
$directoryArtifactRoot = Join-Path $env:PROJECTATLAS_PLUGIN_CACHE "directory-artifact"
$unreadableArtifactRoot = Join-Path $env:PROJECTATLAS_PLUGIN_CACHE "unreadable-artifact"
foreach ($pluginRoot in @($directoryArtifactRoot, $unreadableArtifactRoot)) {
    New-Item -ItemType Directory -Force -Path (Join-Path $pluginRoot ".codex-plugin") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $pluginRoot "skills\projectatlas") | Out-Null
    Set-Content -LiteralPath (Join-Path $pluginRoot ".codex-plugin\plugin.json") -Value "{}"
}
New-Item -ItemType Directory -Force -Path (Join-Path $directoryArtifactRoot "skills\projectatlas\SKILL.md") | Out-Null
Set-Content -LiteralPath (Join-Path $unreadableArtifactRoot "skills\projectatlas\SKILL.md") -Value "fixture"
$installerSkillPath = Join-Path (Split-Path -Parent $PSScriptRoot) "skills\projectatlas\SKILL.md"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $installerSkillPath) | Out-Null
Set-Content -LiteralPath $installerSkillPath -Value "fixture"
$script:pluginSourcePath = $directoryArtifactRoot
if (Test-ProjectAtlasCodexPluginReady "0.4.2") {
    throw "A plugin cache directory was accepted as a skill file."
}
$script:pluginSourcePath = $unreadableArtifactRoot
if (Test-ProjectAtlasCodexPluginReady "0.4.2") {
    throw "An unreadable plugin artifact was accepted as ready."
}
$script:sourceManifestError = $true
if (Test-ProjectAtlasCodexPluginReady "0.4.2") {
    throw "An unreadable plugin source manifest was accepted as ready."
}
$script:sourceManifestError = $false
$script:pluginSourcePath = "C:\bad$([char]0)path"
if (Test-ProjectAtlasCodexPluginReady "0.4.2") {
    throw "A malformed plugin source path was accepted as ready."
}
Write-Output "strict_plugin_marketplace_and_readiness"
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    for shell in ["powershell", "pwsh"] {
        let output = StdCommand::new(shell)
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .env("PROJECTATLAS_INSTALLER", &installer)
            .env(
                "PROJECTATLAS_PLUGIN_CACHE",
                temp.path().join(FAKE_CODEX_PLUGIN_CACHE_DIR),
            )
            .output()?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout)
                .contains("strict_plugin_marketplace_and_readiness")
        {
            return Err(io::Error::other(format!(
                "strict Codex plugin, marketplace, and readiness coverage failed under {shell}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_binds_generated_config_digest_to_validated_bytes()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_root = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = project_root.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let project_config = atlas_dir.join("config.toml");
    fs::write(&project_config, "[project]\nroot = \".\"\n")?;
    let generated = atlas_dir.join("projectatlas.mcp.json");
    let runtime = temp.path().join("projectatlas.exe");
    let db = atlas_dir.join("projectatlas.db");
    let valid = json!({
        "mcpServers": {
            "projectatlas": {
                "command": runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", db,
                    "--config", project_config,
                    "mcp"
                ],
                "cwd": project_root
            }
        }
    });
    let valid_bytes = serde_json::to_vec(&valid)?;
    fs::write(&generated, &valid_bytes)?;
    let singleton_root = serde_json::to_vec(&json!([valid]))?;
    let singleton_nested_object = serde_json::to_vec(&json!({
        "mcpServers": [valid["mcpServers"].clone()]
    }))?;
    let extra_arguments = serde_json::to_vec(&json!({
        "mcpServers": {
            "projectatlas": {
                "command": runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", db,
                    "--config", project_config,
                    "--extra", "value", "mcp"
                ],
                "cwd": project_root
            }
        }
    }))?;
    let extra_arguments_path = temp.path().join("generated-extra-arguments.json");
    let singleton_root_path = temp.path().join("generated-singleton-root.json");
    let singleton_nested_object_path = temp.path().join("generated-singleton-nested-object.json");
    fs::write(&extra_arguments_path, &extra_arguments)?;
    fs::write(&singleton_root_path, &singleton_root)?;
    fs::write(&singleton_nested_object_path, &singleton_nested_object)?;
    let script = temp.path().join("test-generated-config-snapshot.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
foreach ($functionName in @(
        "Assert-ProjectAtlasDirectPath",
        "Assert-ProjectAtlasDirectFilePath",
        "Convert-ProjectAtlasVersionTag",
        "Get-NormalizedPathEntry",
        "Get-ProjectAtlasComparablePath",
        "Get-ProjectAtlasMcpLaunchArguments",
        "Test-ProjectAtlasArgumentsUseAbsolutePaths",
        "Test-ProjectAtlasExactArguments",
        "Assert-ProjectAtlasEquivalentPath",
        "Get-ProjectAtlasSha256",
        "Get-ProjectAtlasSha256FromBytes",
        "Test-ProjectAtlasJsonObject",
        "Test-ProjectAtlasJsonStringArray",
        "Confirm-ProjectAtlasGeneratedMcpConfig",
        "Test-ProjectAtlasGeneratedMcpConfigReadiness"
    )) {
    $match = [regex]::Match($installerSource, "(?ms)^function $functionName \{.*?^\}")
    if (-not $match.Success) { throw "Installer function missing: $functionName" }
    Invoke-Expression $match.Value
}
$digest = Confirm-ProjectAtlasGeneratedMcpConfig `
    $env:PROJECTATLAS_GENERATED_CONFIG `
    "Codex" `
    $env:PROJECTATLAS_RUNTIME `
    $env:PROJECTATLAS_VERSION `
    $env:PROJECTATLAS_DB `
    $env:PROJECTATLAS_PROJECT_CONFIG `
    $env:PROJECTATLAS_FLAT_CONFIG `
    $env:PROJECTATLAS_PROJECT_ROOT
Write-Output "validated_digest=$digest"
$validatedBytes = [System.IO.File]::ReadAllBytes($env:PROJECTATLAS_GENERATED_CONFIG)
$configPaths = [string[]]@($env:PROJECTATLAS_GENERATED_CONFIG)
$expectedDigests = [string[]]@($digest)
if (-not (Test-ProjectAtlasGeneratedMcpConfigReadiness `
        $configPaths `
        $expectedDigests)) {
    throw "Validated generated config was not ready."
}
[System.IO.File]::WriteAllBytes(
    $env:PROJECTATLAS_GENERATED_CONFIG,
    [System.IO.File]::ReadAllBytes($env:PROJECTATLAS_EXTRA_ARGUMENTS)
)
if (Test-ProjectAtlasGeneratedMcpConfigReadiness `
    $configPaths `
    $expectedDigests) {
    throw "Changed generated config was reported ready."
}
[System.IO.File]::WriteAllBytes($env:PROJECTATLAS_GENERATED_CONFIG, $validatedBytes)
Microsoft.PowerShell.Management\Remove-Item `
    -LiteralPath $env:PROJECTATLAS_GENERATED_CONFIG `
    -Force
if (Test-ProjectAtlasGeneratedMcpConfigReadiness `
    $configPaths `
    $expectedDigests) {
    throw "Missing generated config was reported ready."
}
[System.IO.File]::WriteAllBytes($env:PROJECTATLAS_GENERATED_CONFIG, $validatedBytes)
Write-Output "final_runtime_config_drift_not_ready"
foreach ($invalidBytes in @(
        [System.IO.File]::ReadAllBytes($env:PROJECTATLAS_EXTRA_ARGUMENTS),
        [System.IO.File]::ReadAllBytes($env:PROJECTATLAS_SINGLETON_ROOT),
        [System.IO.File]::ReadAllBytes($env:PROJECTATLAS_SINGLETON_NESTED_OBJECT)
    )) {
    [System.IO.File]::WriteAllBytes($env:PROJECTATLAS_GENERATED_CONFIG, $invalidBytes)
    try {
        Confirm-ProjectAtlasGeneratedMcpConfig `
            $env:PROJECTATLAS_GENERATED_CONFIG `
            "Codex" `
            $env:PROJECTATLAS_RUNTIME `
            $env:PROJECTATLAS_VERSION `
            $env:PROJECTATLAS_DB `
            $env:PROJECTATLAS_PROJECT_CONFIG `
            $env:PROJECTATLAS_FLAT_CONFIG `
            $env:PROJECTATLAS_PROJECT_ROOT | Out-Null
        throw "Invalid generated config was accepted."
    }
    catch {
        if ($_.Exception.Message -eq "Invalid generated config was accepted.") { throw }
    }
}
Write-Output "invalid_snapshots_rejected"
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .env("PROJECTATLAS_INSTALLER", &installer)
        .env("PROJECTATLAS_GENERATED_CONFIG", &generated)
        .env("PROJECTATLAS_RUNTIME", &runtime)
        .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
        .env("PROJECTATLAS_DB", &db)
        .env("PROJECTATLAS_PROJECT_CONFIG", &project_config)
        .env(
            "PROJECTATLAS_FLAT_CONFIG",
            project_root.join("projectatlas.toml"),
        )
        .env("PROJECTATLAS_PROJECT_ROOT", &project_root)
        .env("PROJECTATLAS_EXTRA_ARGUMENTS", &extra_arguments_path)
        .env("PROJECTATLAS_SINGLETON_ROOT", &singleton_root_path)
        .env(
            "PROJECTATLAS_SINGLETON_NESTED_OBJECT",
            &singleton_nested_object_path,
        )
        .output()?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_digest = sha256_hex(&valid_bytes);
    if !output.status.success()
        || !output_text.contains(&format!("validated_digest={expected_digest}"))
        || !output_text.contains("final_runtime_config_drift_not_ready")
        || !output_text.contains("invalid_snapshots_rejected")
    {
        return Err(io::Error::other(format!(
            "generated config same-byte validation coverage failed:\n{output_text}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_rejects_changed_exited_and_inaccessible_processes()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let script = temp.path().join("test-obsolete-mcp-retirement.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
$sourceMatch = [regex]::Match(
    $installerSource,
    "(?s)Add-Type -TypeDefinition @'\r?\n(?<source>.*?)\r?\n'@"
)
if (-not $sourceMatch.Success) {
    throw "Installer runtime process source was not found."
}
Add-Type -TypeDefinition $sourceMatch.Groups["source"].Value
$hostPath = (Get-Process -Id $PID).Path
$parent = Get-Process -Id $PID
$parentCreationFileTime = $parent.StartTime.ToUniversalTime().ToFileTimeUtc()
$parentCim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $PID"
$parentArguments = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
    [string]$parentCim.CommandLine
)
$owned = Start-Process $hostPath `
    -ArgumentList @("-NoProfile", "-Command", "Start-Sleep -Seconds 30") `
    -WindowStyle Hidden `
    -PassThru
try {
    $creationFileTime = $owned.StartTime.ToUniversalTime().ToFileTimeUtc()
    $ownedCim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $($owned.Id)"
    $ownedArguments = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
        [string]$ownedCim.CommandLine
    )
    $imageSha256 = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ComputeImageSha256($hostPath)
    $changed = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime + 1000,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changed.State -ne "identity_changed_creation" -or $owned.HasExited) {
        throw "Changed process identity was not refused safely: $($changed.State)"
    }
    $wrongPath = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        (Join-Path (Split-Path -Parent $hostPath) "not-the-owner.exe"),
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($wrongPath.State -ne "identity_changed_path" -or $owned.HasExited) {
        throw "Changed process path was not refused safely: $($wrongPath.State)"
    }
    $wrongArguments = @($ownedArguments)
    $wrongArguments[$wrongArguments.Count - 1] = "runtime-info"
    $changedCommand = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $wrongArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changedCommand.State -ne "identity_changed_command" -or $owned.HasExited) {
        throw "Changed process command was not refused safely: $($changedCommand.State)"
    }
    $changedFile = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        ("0" * 64),
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changedFile.State -ne "identity_changed_file" -or $owned.HasExited) {
        throw "Changed process image was not refused safely: $($changedFile.State)"
    }
    $changedParent = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        ($PID + 1),
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changedParent.State -ne "identity_changed_parent" -or $owned.HasExited) {
        throw "Changed process parent identity was not refused safely: $($changedParent.State)"
    }
    $changedParentCreation = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        ($parentCreationFileTime + 1000),
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changedParentCreation.State -ne "identity_changed_parent_creation" -or $owned.HasExited) {
        throw "Changed parent creation was not refused safely: $($changedParentCreation.State)"
    }
    $changedParentPath = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        (Join-Path (Split-Path -Parent $hostPath) "not-the-parent.exe"),
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($changedParentPath.State -ne "identity_changed_parent_path" -or $owned.HasExited) {
        throw "Changed parent path was not refused safely: $($changedParentPath.State)"
    }
    $wrongParentArguments = @($parentArguments) + "identity-changed"
    $changedParentCommand = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $wrongParentArguments,
        $imageSha256,
        1000
    )
    if ($changedParentCommand.State -ne "identity_changed_parent_command" -or $owned.HasExited) {
        throw "Changed process parent command was not refused safely: $($changedParentCommand.State)"
    }
    $changedParentFile = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        ("0" * 64),
        1000
    )
    if ($changedParentFile.State -ne "identity_changed_parent_file" -or $owned.HasExited) {
        throw "Changed parent image was not refused safely: $($changedParentFile.State)"
    }
    $invalidExpectedPath = $hostPath + [char]0
    $invalidPath = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $invalidExpectedPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($invalidPath.State -ne "inspection_failed" -or $owned.HasExited) {
        throw "Invalid identity input was misclassified as process exit: $($invalidPath.State)"
    }
    $findMatch = [regex]::Match(
        $installerSource,
        "(?ms)^function Find-ProjectAtlasObsoleteStableMcpProcess \{.*?^\}"
    )
    if (-not $findMatch.Success) {
        throw "Installer obsolete MCP finder function was not found."
    }
    Invoke-Expression $findMatch.Value
    $syntheticStablePath = "C:\stable\projectatlas.exe"
    $syntheticCodexPath = "C:\signed\codex.exe"
    function Initialize-ProjectAtlasRuntimeProbe {}
    function Get-CimInstance {
        return @(
            [pscustomobject]@{
                ProcessId = 42
                Name = "projectatlas.exe"
                ExecutablePath = $syntheticStablePath
                CommandLine = '"C:\stable\projectatlas.exe" --require-version 0.3.26 --db C:\repo\projectatlas.db --config C:\repo\config.toml mcp'
                CreationDate = 100
                ParentProcessId = 43
            },
            [pscustomobject]@{
                ProcessId = 43
                Name = "codex.exe"
                ExecutablePath = $syntheticCodexPath
                CommandLine = '"C:\signed\codex.exe" app-server'
                CreationDate = 200
                ParentProcessId = 1
            }
        )
    }
    function Convert-ProjectAtlasVersionTag {
        param([string]$Version)
        return $Version.TrimStart("v")
    }
    function Get-NormalizedPathEntry {
        param([string]$Path)
        return $Path.ToLowerInvariant()
    }
    function Test-ProjectAtlasArgumentsUseAbsolutePaths { return $true }
    function Get-ProjectAtlasMcpLaunchArguments { return @() }
    function Test-ProjectAtlasExactArguments { return $true }
    function Convert-ProjectAtlasCimCreationFileTime {
        param([object]$Value)
        return [long]$Value
    }
    function Get-ProjectAtlasCodexImageIdentity { return ("c" * 64) }
    $reusedParentSelection = Find-ProjectAtlasObsoleteStableMcpProcess `
        $syntheticStablePath `
        "C:\repo\projectatlas.db" `
        "C:\repo\config.toml" `
        "C:\repo\projectatlas.toml" `
        "0.4.3"
    if ($reusedParentSelection.State -ne "unsafe_owner" -or $owned.HasExited -or $parent.HasExited) {
        throw "Parent created after its MCP child was not refused safely: $($reusedParentSelection.State)"
    }
    $handoffMatch = [regex]::Match(
        $installerSource,
        "(?ms)^function Invoke-ProjectAtlasObsoleteStableMcpHandoff \{.*?^\}"
    )
    if (-not $handoffMatch.Success) {
        throw "Installer obsolete MCP handoff function was not found."
    }
    Invoke-Expression $handoffMatch.Value
    function Test-ProjectAtlasRuntime { return $true }
    function Assert-ProjectAtlasDirectFilePath {}
    function Find-ProjectAtlasObsoleteStableMcpProcess {
        return [pscustomobject]@{
            State = "exact"
            ObservedVersion = "0.3.26"
            ImageSha256 = ("a" * 64)
        }
    }
    function Test-ProjectAtlasCodexPluginReady { return $true }
    function Test-ProjectAtlasCodexMcpRegistryReady { return $true }
    function Get-ProjectAtlasRuntimeImageSha256 { return ("a" * 64) }
    function Get-ProjectAtlasSha256 { return ("b" * 64) }
    function Get-ProjectAtlasRuntimeVersion { return "0.3.27" }
    function Convert-ProjectAtlasVersionTag { param([string]$Version); return $Version }
    $changedVersion = Invoke-ProjectAtlasObsoleteStableMcpHandoff `
        $hostPath `
        $hostPath `
        "0.4.3" `
        "C:\repo\.projectatlas\projectatlas.db" `
        "C:\repo\.projectatlas\config.toml" `
        "C:\repo\projectatlas.toml" `
        "C:\repo\.projectatlas\projectatlas.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.claude.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.opencode.json" `
        ("b" * 64) `
        ("b" * 64) `
        ("b" * 64)
    if ($changedVersion -ne "identity_changed_version" -or $owned.HasExited) {
        throw "Changed runtime version was not refused safely: $changedVersion"
    }
    $changedBeforeHandoff = Invoke-ProjectAtlasObsoleteStableMcpHandoff `
        $hostPath `
        $hostPath `
        "0.4.3" `
        "C:\repo\.projectatlas\projectatlas.db" `
        "C:\repo\.projectatlas\config.toml" `
        "C:\repo\projectatlas.toml" `
        "C:\repo\.projectatlas\projectatlas.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.claude.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.opencode.json" `
        ("e" * 64) `
        ("e" * 64) `
        ("e" * 64)
    if ($changedBeforeHandoff -ne "replacement_readiness_changed" -or $owned.HasExited) {
        throw "Pre-handoff replacement config was not refused safely: $changedBeforeHandoff"
    }
    function Find-ProjectAtlasObsoleteStableMcpProcess {
        return [pscustomobject]@{
            State = "exact"
            ObservedVersion = "0.3.26"
            ImageSha256 = ("a" * 64)
            ParentPath = $hostPath
            ParentImageSha256 = ("c" * 64)
        }
    }
    function Get-ProjectAtlasRuntimeVersion { return "0.3.26" }
    function Get-ProjectAtlasCodexImageIdentity { return ("c" * 64) }
    $script:configHashCalls = 0
    function Get-ProjectAtlasSha256 {
        $script:configHashCalls += 1
        if ($script:configHashCalls -gt 3) { return ("d" * 64) }
        return ("b" * 64)
    }
    $changedReplacement = Invoke-ProjectAtlasObsoleteStableMcpHandoff `
        $hostPath `
        $hostPath `
        "0.4.3" `
        "C:\repo\.projectatlas\projectatlas.db" `
        "C:\repo\.projectatlas\config.toml" `
        "C:\repo\projectatlas.toml" `
        "C:\repo\.projectatlas\projectatlas.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.claude.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.opencode.json" `
        ("b" * 64) `
        ("b" * 64) `
        ("b" * 64)
    if ($changedReplacement -ne "replacement_readiness_changed" -or $owned.HasExited) {
        throw "Changed replacement config was not refused safely: $changedReplacement"
    }
    function Get-ProjectAtlasSha256 { return ("b" * 64) }
    function Get-ProjectAtlasCodexImageIdentity { return ("d" * 64) }
    $changedOwnerDigest = Invoke-ProjectAtlasObsoleteStableMcpHandoff `
        $hostPath `
        $hostPath `
        "0.4.3" `
        "C:\repo\.projectatlas\projectatlas.db" `
        "C:\repo\.projectatlas\config.toml" `
        "C:\repo\projectatlas.toml" `
        "C:\repo\.projectatlas\projectatlas.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.claude.mcp.json" `
        "C:\repo\.projectatlas\projectatlas.opencode.json" `
        ("b" * 64) `
        ("b" * 64) `
        ("b" * 64)
    if ($changedOwnerDigest -ne "replacement_readiness_changed" -or $owned.HasExited) {
        throw "Changed final Codex owner digest was not refused safely: $changedOwnerDigest"
    }
    if (-not $owned.HasExited) {
        $owned.Kill()
        $owned.WaitForExit()
    }
    $exited = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $owned.Id,
        $creationFileTime,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($exited.State -ne "exited") {
        throw "Exited process race was not classified safely: $($exited.State)"
    }
    $accessDenied = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        4,
        0,
        $hostPath,
        $ownedArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    if ($accessDenied.State -ne "access_denied" -or $accessDenied.ErrorCode -ne 5) {
        throw "Access-denied process was not classified safely: $($accessDenied.State):$($accessDenied.ErrorCode)"
    }
    Write-Output "identity_changed_creation identity_changed_path identity_changed_command identity_changed_version replacement_readiness_changed identity_changed_file identity_changed_parent identity_changed_parent_creation identity_changed_parent_path identity_changed_parent_command identity_changed_parent_file parent_created_after_child inspection_failed exited access_denied"
}
finally {
    if (-not $owned.HasExited) {
        $owned.Kill()
        $owned.WaitForExit()
    }
}
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .env("PROJECTATLAS_INSTALLER", &installer)
        .output()?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .contains("identity_changed_creation identity_changed_path identity_changed_command identity_changed_version replacement_readiness_changed identity_changed_file identity_changed_parent identity_changed_parent_creation identity_changed_parent_path identity_changed_parent_command identity_changed_parent_file parent_created_after_child inspection_failed exited access_denied")
    {
        return Err(io::Error::other(format!(
            "bounded process-retirement failure coverage failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_classifies_exit_after_final_identity_check()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let script = temp.path().join("test-obsolete-mcp-exit-race.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
$sourceMatch = [regex]::Match(
    $installerSource,
    "(?s)Add-Type -TypeDefinition @'\r?\n(?<source>.*?)\r?\n'@"
)
if (-not $sourceMatch.Success) {
    throw "Installer runtime process source was not found."
}
$source = $sourceMatch.Groups["source"].Value
$seamPattern = '(?m)^                        if \(parent\.HasExited\)\r?\n' +
    '^                            return new ProcessRetirementResult\("owner_parent_exited", 0\);\r?\n' +
    '^                        if \(candidate\.HasExited\)\r?\n' +
    '^                            return new ProcessRetirementResult\("exited", 0\);\r?\n' +
    '^                        if \(!TerminateProcess\(handle, 0\)\)'
$seam = [regex]::new($seamPattern)
if ($seam.Matches($source).Count -ne 1) {
    throw "Installer retirement race seam was not unique."
}
$replacement = @'
                        if (parent.HasExited)
                            return new ProcessRetirementResult("owner_parent_exited", 0);
                        if (candidate.HasExited)
                            return new ProcessRetirementResult("exited", 0);
                        string exitGate = Environment.GetEnvironmentVariable("PROJECTATLAS_TEST_RETIRE_EXIT_GATE");
                        if (String.IsNullOrWhiteSpace(exitGate))
                            throw new InvalidOperationException("Test exit gate is missing.");
                        File.Delete(exitGate);
                        if (WaitForSingleObject(handle, 5000) != WaitObject0)
                            throw new TimeoutException("Test child did not exit after final identity validation.");
                        if (!TerminateProcess(handle, 0))
'@
$source = $seam.Replace($source, $replacement, 1)
Add-Type -TypeDefinition $source

$hostPath = (Get-Process -Id $PID).Path
$parent = Get-Process -Id $PID
$parentCreationFileTime = $parent.StartTime.ToUniversalTime().ToFileTimeUtc()
$parentCim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $PID"
$parentArguments = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
    [string]$parentCim.CommandLine
)
$imageSha256 = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ComputeImageSha256($hostPath)
$exitGate = Join-Path $PSScriptRoot "projectatlas-retire-exit-$PID-$([guid]::NewGuid()).gate"
Set-Content -LiteralPath $exitGate -Value "wait"
$env:PROJECTATLAS_TEST_RETIRE_EXIT_GATE = $exitGate
$child = Start-Process $hostPath `
    -ArgumentList @(
        "-NoProfile",
        "-Command",
        'while (Test-Path -LiteralPath $env:PROJECTATLAS_TEST_RETIRE_EXIT_GATE) { Start-Sleep -Milliseconds 10 }'
    ) `
    -WindowStyle Hidden `
    -PassThru
try {
    $childCreationFileTime = $child.StartTime.ToUniversalTime().ToFileTimeUtc()
    $childCim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $($child.Id)"
    $childArguments = [ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
        [string]$childCim.CommandLine
    )
    $result = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $child.Id,
        $childCreationFileTime,
        $hostPath,
        $childArguments,
        $imageSha256,
        $PID,
        $parentCreationFileTime,
        $hostPath,
        $parentArguments,
        $imageSha256,
        1000
    )
    $childExited = $child.WaitForExit(1000)
    if ($result.State -ne "exited" -or $result.ErrorCode -ne 0 -or -not $childExited) {
        throw "Exit after final identity validation was not classified safely: $($result.State):$($result.ErrorCode)"
    }
    if ($parent.HasExited) {
        throw "Exit-race classification terminated the parent process."
    }
    Write-Output "exit_after_final_identity_check"
}
finally {
    Remove-Item Env:PROJECTATLAS_TEST_RETIRE_EXIT_GATE -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $exitGate -Force -ErrorAction SilentlyContinue
    if (-not $child.HasExited) {
        $child.Kill()
        if (-not $child.WaitForExit(5000)) {
            throw "Exit-race child cleanup timed out."
        }
    }
}
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .env("PROJECTATLAS_INSTALLER", &installer)
        .output()?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains("exit_after_final_identity_check")
    {
        return Err(io::Error::other(format!(
            "post-identity child-exit race coverage failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_requires_trusted_authenticode_cmdlet()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let codex = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    fs::write(&codex, b"codex identity fixture")?;
    let script = temp.path().join("test-codex-image-identity.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$env:PSModulePath = [System.IO.Path]::Combine($PSHOME, "Modules")
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
$functionNames = @(
    "Test-ProjectAtlasAuthenticodeCodexSignature",
    "Test-ProjectAtlasStableCodexImageIdentity",
    "Get-ProjectAtlasCodexImageIdentity"
)
foreach ($functionName in $functionNames) {
    $functionMatch = [regex]::Match(
        $installerSource,
        "(?ms)^function $functionName \{.*?^\}"
    )
    if (-not $functionMatch.Success) {
        throw "Installer Codex identity function was not found: $functionName"
    }
    Invoke-Expression $functionMatch.Value
}
$script:imageHashCalls = 0
function Get-ProjectAtlasRuntimeImageSha256 {
    $script:imageHashCalls += 1
    return ("a" * 64)
}
$script:maliciousLookupCalled = $false
$script:maliciousSignatureCalled = $false
function Get-Command {
    $script:maliciousLookupCalled = $true
    throw "Malicious command resolver was called."
}
function Get-AuthenticodeSignature {
    $script:maliciousSignatureCalled = $true
    throw "Malicious signature probe was called."
}
$codexPath = [System.IO.Path]::GetFullPath($env:PROJECTATLAS_TEST_CODEX)
if ($null -ne (Get-ProjectAtlasCodexImageIdentity $codexPath)) {
    throw "Unsigned Codex image was trusted."
}
if ($script:maliciousLookupCalled) {
    throw "Unqualified Get-Command shadow was invoked."
}
if ($script:maliciousSignatureCalled) {
    throw "Unqualified Get-AuthenticodeSignature shadow was invoked."
}
if ($script:imageHashCalls -ne 2) {
    throw "Unsigned Codex image digest call count was $script:imageHashCalls instead of one pre-signature and one post-signature digest."
}

$script:identityMode = "valid"
function New-ProjectAtlasTestSignature {
    $certificate = [pscustomobject]@{}
    $certificate | Add-Member -MemberType ScriptMethod -Name GetNameInfo -Value {
        param($NameType, $ForIssuer)
        if ($script:identityMode -eq "wrong_signer") { return "Example Corp" }
        return "OpenAI OpCo, LLC"
    }
    return [pscustomobject]@{
        Status = if ($script:identityMode -eq "wrong_status") {
            [System.Management.Automation.SignatureStatus]::HashMismatch
        } else {
            [System.Management.Automation.SignatureStatus]::Valid
        }
        SignatureType = if ($script:identityMode -eq "wrong_type") {
            [System.Management.Automation.SignatureType]::Catalog
        } else {
            [System.Management.Automation.SignatureType]::Authenticode
        }
        SignerCertificate = $certificate
    }
}
if (-not (Test-ProjectAtlasAuthenticodeCodexSignature (New-ProjectAtlasTestSignature))) {
    throw "Valid OpenAI Authenticode signature facts were rejected."
}
$validSignature = New-ProjectAtlasTestSignature
if (-not (Test-ProjectAtlasStableCodexImageIdentity ("a" * 64) $validSignature ("a" * 64))) {
    throw "Matching pre/post Codex image digests were rejected."
}
if (Test-ProjectAtlasStableCodexImageIdentity ("a" * 64) $validSignature ("b" * 64)) {
    throw "Mismatched pre/post Codex image digests were trusted."
}
foreach ($mode in @("wrong_signer", "wrong_status", "wrong_type")) {
    $script:identityMode = $mode
    if (Test-ProjectAtlasAuthenticodeCodexSignature (New-ProjectAtlasTestSignature)) {
        throw "Invalid Codex signature $mode facts were accepted."
    }
}
Write-Output "trusted_authenticode_cmdlet_only"
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let system_root = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?,
    );
    let powershell_path = system_root
        .join(WINDOWS_SYSTEM32_DIR)
        .join(WINDOWS_POWERSHELL_DIR)
        .join(WINDOWS_POWERSHELL_VERSION_DIR)
        .join(WINDOWS_POWERSHELL_EXECUTABLE);
    let output = StdCommand::new(powershell_path)
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .env("PROJECTATLAS_INSTALLER", &installer)
        .env("PROJECTATLAS_TEST_CODEX", &codex)
        .output()?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains("trusted_authenticode_cmdlet_only")
    {
        return Err(io::Error::other(format!(
            "Codex Authenticode identity coverage failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_installer_obsolete_mcp_handoff_requires_exact_codex_registry()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let script = temp.path().join("test-codex-registry-readiness.ps1");
    fs::write(
        &script,
        r#"$ErrorActionPreference = "Stop"
$installerSource = Get-Content -Raw -LiteralPath $env:PROJECTATLAS_INSTALLER
foreach ($functionName in @(
        "Get-NormalizedPathEntry",
        "Test-ProjectAtlasArgumentsUseAbsolutePaths",
        "Test-ProjectAtlasExactArguments",
        "Test-ProjectAtlasJsonObject",
        "Test-ProjectAtlasJsonStringArray",
        "Test-ProjectAtlasCodexMcpRegistryEntry"
    )) {
    $match = [regex]::Match(
        $installerSource,
        "(?ms)^function $functionName \{.*?^\}"
    )
    if (-not $match.Success) {
        throw "Installer function was not found: $functionName"
    }
    Invoke-Expression $match.Value
}
$runtime = "C:\ProjectAtlas\runtime\projectatlas.exe"
$arguments = @(
    "--require-version", "0.4.1",
    "--db", "C:\repo\.projectatlas\projectatlas.db",
    "--config", "C:\repo\.projectatlas\config.toml",
    "mcp"
)
$exact = [pscustomobject]@{
    name = "projectatlas"
    enabled = $true
    transport = [pscustomobject]@{
        type = "stdio"
        command = $runtime
        args = $arguments
    }
}
if (-not (Test-ProjectAtlasCodexMcpRegistryEntry $exact $runtime $arguments)) {
    throw "Exact structured registry entry was rejected."
}
$singletonArrayRoot = [object[]]@($exact)
if (Test-ProjectAtlasCodexMcpRegistryEntry $singletonArrayRoot $runtime $arguments) {
    throw "Singleton-array registry root was accepted."
}
foreach ($invalidRoot in @("projectatlas", 1, $null)) {
    if (Test-ProjectAtlasCodexMcpRegistryEntry $invalidRoot $runtime $arguments) {
        throw "Scalar or null registry root was accepted."
    }
}
$nestedTransport = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$nestedTransport.transport = [object[]]@($nestedTransport.transport)
if (Test-ProjectAtlasCodexMcpRegistryEntry $nestedTransport $runtime $arguments) {
    throw "Array registry transport was accepted."
}
$disabled = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$disabled.enabled = $false
if (Test-ProjectAtlasCodexMcpRegistryEntry $disabled $runtime $arguments) {
    throw "Disabled exact registry entry was accepted."
}
foreach ($invalidEnabled in @("true", 1)) {
    $malformedEnabled = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    $malformedEnabled.enabled = $invalidEnabled
    if (Test-ProjectAtlasCodexMcpRegistryEntry $malformedEnabled $runtime $arguments) {
        throw "Non-Boolean registry enabled value was accepted: $invalidEnabled"
    }
}
$numericName = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$numericName.name = 1
if (Test-ProjectAtlasCodexMcpRegistryEntry $numericName $runtime $arguments) {
    throw "Non-string registry name was accepted."
}
$numericTransportType = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$numericTransportType.transport.type = 1
if (Test-ProjectAtlasCodexMcpRegistryEntry $numericTransportType $runtime $arguments) {
    throw "Non-string registry transport type was accepted."
}
$numericCommand = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$numericCommand.transport.command = 1
if (Test-ProjectAtlasCodexMcpRegistryEntry $numericCommand $runtime $arguments) {
    throw "Non-string registry command was accepted."
}
$scalarArguments = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$scalarArguments.transport.args = "--require-version"
if (Test-ProjectAtlasCodexMcpRegistryEntry $scalarArguments $runtime $arguments) {
    throw "Scalar registry arguments were accepted."
}
$numericArgument = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$numericArgument.transport.args[1] = 1
if (Test-ProjectAtlasCodexMcpRegistryEntry $numericArgument $runtime $arguments) {
    throw "Non-string registry argument was accepted."
}
$nestedArgument = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$nestedArgument.transport.args[1] = [object[]]@("0.4.1")
if (Test-ProjectAtlasCodexMcpRegistryEntry $nestedArgument $runtime $arguments) {
    throw "Nested registry argument array was accepted."
}
$substringCommand = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$substringCommand.transport.command = "$runtime.backup"
if (Test-ProjectAtlasCodexMcpRegistryEntry $substringCommand $runtime $arguments) {
    throw "Command substring false positive was accepted."
}
$substringValue = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$substringValue.transport.args[1] = "prefix-0.4.1-suffix"
if (Test-ProjectAtlasCodexMcpRegistryEntry $substringValue $runtime $arguments) {
    throw "Argument substring false positive was accepted."
}
$reordered = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$reordered.transport.args = @(
    "--db", "C:\repo\.projectatlas\projectatlas.db",
    "--require-version", "0.4.1",
    "--config", "C:\repo\.projectatlas\config.toml",
    "mcp"
)
if (Test-ProjectAtlasCodexMcpRegistryEntry $reordered $runtime $arguments) {
    throw "Reordered arguments were accepted."
}
$extra = $exact | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$extra.transport.args = @($arguments) + "--extra"
if (Test-ProjectAtlasCodexMcpRegistryEntry $extra $runtime $arguments) {
    throw "Extra arguments were accepted."
}
Write-Output "exact_json_registry_contract_verified"
"#,
    )?;
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.ps1");
    let output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .env("PROJECTATLAS_INSTALLER", &installer)
        .output()?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .contains("exact_json_registry_contract_verified")
    {
        return Err(io::Error::other(format!(
            "exact Codex registry verifier coverage failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

fn fake_codex_projectatlas_marketplace_root(codex_dir: &Path) -> PathBuf {
    codex_dir
        .join(".tmp")
        .join("marketplaces")
        .join("projectatlas")
}

fn fake_codex_projectatlas_plugin_source(codex_dir: &Path) -> PathBuf {
    fake_codex_projectatlas_marketplace_root(codex_dir)
        .join("plugins")
        .join("projectatlas")
}

fn fake_codex_projectatlas_installed_cache(codex_dir: &Path, version: &str) -> PathBuf {
    codex_dir
        .join("plugins")
        .join("cache")
        .join("projectatlas")
        .join("projectatlas")
        .join(version)
}

fn write_fake_codex_projectatlas_integration(
    codex_dir: &Path,
    installed_version: &str,
    source_manifest_version: &str,
    skill_content: &str,
) -> Result<(PathBuf, PathBuf, PathBuf), Box<dyn Error>> {
    let marketplace_root = fake_codex_projectatlas_marketplace_root(codex_dir);
    let plugin_source = fake_codex_projectatlas_plugin_source(codex_dir);
    let installed_cache = fake_codex_projectatlas_installed_cache(codex_dir, installed_version);
    let marketplace_manifest = marketplace_root
        .join(CODEX_MARKETPLACE_METADATA_DIR)
        .join("plugins")
        .join(CODEX_MARKETPLACE_MANIFEST_FILE_NAME);
    fs::create_dir_all(
        marketplace_manifest
            .parent()
            .ok_or_else(|| io::Error::other("fake marketplace manifest parent missing"))?,
    )?;
    fs::write(
        &marketplace_manifest,
        r#"{"name":"projectatlas","plugins":[{"name":"projectatlas","source":{"source":"local","path":"./plugins/projectatlas"},"policy":{"installation":"AVAILABLE","authentication":"ON_INSTALL"}}]}"#,
    )?;
    fs::write(
        marketplace_root.join(CODEX_MARKETPLACE_INSTALL_RECORD_FILE_NAME),
        format!(
            r#"{{"source_type":"git","source":"https://github.com/styler-ai/ProjectAtlas.git","ref_name":"v{installed_version}","sparse_paths":[],"revision":"prior-revision"}}"#
        ),
    )?;
    for (root, manifest_version) in [
        (&plugin_source, source_manifest_version),
        (&installed_cache, installed_version),
    ] {
        fs::create_dir_all(root.join(CODEX_PLUGIN_MANIFEST_DIR))?;
        fs::create_dir_all(
            root.join(PROJECTATLAS_SKILL_DIR)
                .join(PROJECTATLAS_SKILL_NAME),
        )?;
        fs::write(
            root.join(CODEX_PLUGIN_MANIFEST_DIR).join("plugin.json"),
            format!(r#"{{"name":"projectatlas","version":"{manifest_version}"}}"#),
        )?;
        fs::write(
            root.join(PROJECTATLAS_SKILL_DIR)
                .join(PROJECTATLAS_SKILL_NAME)
                .join(SKILL_FILE_NAME),
            skill_content,
        )?;
    }
    Ok((marketplace_root, plugin_source, installed_cache))
}

#[test]
fn installer_workflow_pin_reports_preserve_exact_rc_identity() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let workflow_dir = repo.join(".github").join("workflows");
    fs::create_dir_all(&workflow_dir)?;
    fs::write(
        workflow_dir.join("pins.yml"),
        "https://github.com/styler-ai/ProjectAtlas/releases/download/v1.2.3-rc12/current\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v1.2.2/stale-stable\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v1.2.3-rc2/stale-rc\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v1.2.3-rc12evil/malformed-rc\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v1.2.3evil/malformed-stable\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v$PROJECTATLAS_VERSION/dynamic-variable\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/v${PROJECTATLAS_VERSION}/dynamic-braced\n\
https://github.com/styler-ai/ProjectAtlas/releases/download/${{ inputs.version }}/dynamic-expression\n\
https://github.com/example/ProjectAtlas/releases/download/v9.9.9/unrelated\n",
    )?;
    let workspace = workspace_root()?;
    let output = if cfg!(windows) {
        let installer = fs::read_to_string(
            workspace
                .join("plugins")
                .join("projectatlas")
                .join("scripts")
                .join("install-runtime.ps1"),
        )?;
        let convert_start = installer
            .find("function Convert-ProjectAtlasVersionTag {")
            .ok_or_else(|| io::Error::other("PowerShell version conversion function missing"))?;
        let convert_end = installer[convert_start..]
            .find("function Initialize-ProjectAtlasRuntimeProbe")
            .map(|offset| convert_start + offset)
            .ok_or_else(|| io::Error::other("PowerShell version conversion boundary missing"))?;
        let report_start = installer
            .find("function Write-ProjectAtlasWorkflowPinReport {")
            .ok_or_else(|| io::Error::other("PowerShell workflow-pin report function missing"))?;
        let report_end = installer[report_start..]
            .find("function Get-ReleaseRuntimeInstallPath")
            .map(|offset| report_start + offset)
            .ok_or_else(|| io::Error::other("PowerShell workflow-pin report boundary missing"))?;
        let script = temp.path().join("workflow-pin-report.ps1");
        fs::write(
            &script,
            format!(
                "$ErrorActionPreference = 'Stop'\n{}\n{}\nWrite-ProjectAtlasWorkflowPinReport $env:PROJECTATLAS_FIXTURE_ROOT 'v1.2.3-rc12'\n",
                &installer[convert_start..convert_end],
                &installer[report_start..report_end]
            ),
        )?;
        StdCommand::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .env("PROJECTATLAS_FIXTURE_ROOT", &repo)
            .output()?
    } else {
        let installer = fs::read_to_string(
            workspace
                .join("plugins")
                .join("projectatlas")
                .join("scripts")
                .join("install-runtime.sh"),
        )?;
        let version_start = installer
            .find("expected_runtime_version() {")
            .ok_or_else(|| io::Error::other("POSIX version conversion function missing"))?;
        let version_end = installer[version_start..]
            .find("is_projectatlas_runtime() {")
            .map(|offset| version_start + offset)
            .ok_or_else(|| io::Error::other("POSIX version conversion boundary missing"))?;
        let report_start = installer
            .find("report_projectatlas_workflow_pins() {")
            .ok_or_else(|| io::Error::other("POSIX workflow-pin report function missing"))?;
        let report_end = installer[report_start..]
            .find("download_release_file() {")
            .map(|offset| report_start + offset)
            .ok_or_else(|| io::Error::other("POSIX workflow-pin report boundary missing"))?;
        let script = temp.path().join("workflow-pin-report.sh");
        fs::write(
            &script,
            format!(
                "set -eu\nprojectatlas_version=v1.2.3-rc12\nprojectatlas_bin=\n{}\n{}\nproject_root=$PROJECTATLAS_FIXTURE_ROOT\nreport_projectatlas_workflow_pins\n",
                &installer[version_start..version_end],
                &installer[report_start..report_end]
            ),
        )?;
        StdCommand::new("sh")
            .arg(script)
            .env("PROJECTATLAS_FIXTURE_ROOT", &repo)
            .output()?
    };
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized_report = report.split_whitespace().collect::<Vec<_>>().join(" ");
    if !output.status.success()
        || !normalized_report.contains("uses v1.2.2; expected v1.2.3-rc12")
        || !normalized_report.contains("uses v1.2.3-rc2; expected v1.2.3-rc12")
        || !normalized_report.contains("uses v1.2.3-rc12evil; expected v1.2.3-rc12")
        || !normalized_report.contains("uses v1.2.3evil; expected v1.2.3-rc12")
        || normalized_report.contains("uses v1.2.3-rc12;")
        || normalized_report.contains("uses v1.2.3;")
        || normalized_report.contains("PROJECTATLAS_VERSION")
        || normalized_report.contains("inputs.version")
        || normalized_report.contains("v9.9.9")
    {
        return Err(io::Error::other(format!(
            "installer workflow-pin report did not preserve exact RC identity:\n{report}"
        ))
        .into());
    }
    Ok(())
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
            "jobs:\n  smoke:\n    steps:\n      - run: curl -fsSL https://github.com/styler-ai/ProjectAtlas/releases/download/v0.0.1/projectatlas-v0.0.1-x86_64-unknown-linux-gnu.tar.gz -o projectatlas.tar.gz\n      - run: curl -fsSL https://github.com/styler-ai/ProjectAtlas/releases/download/v0.0.2-rc12/projectatlas-v0.0.2-rc12-x86_64-unknown-linux-gnu.tar.gz -o projectatlas-rc.tar.gz\n      - run: curl -fsSL https://github.com/styler-ai/ProjectAtlas/releases/download/{expected_release_tag}/projectatlas-{expected_release_tag}-x86_64-unknown-linux-gnu.tar.gz -o projectatlas-current.tar.gz\n      - run: curl -fsSL https://github.com/example/ProjectAtlas/releases/download/v9.9.9/projectatlas-v9.9.9-x86_64-unknown-linux-gnu.tar.gz -o projectatlas-fork.tar.gz\n"
        ),
    )?;
    fs::write(
        atlas_dir.join("kept-state.txt"),
        "existing project-local state must survive plugin updates\n",
    )?;
    let db = atlas_dir.join("projectatlas.db");
    let runtime = mcp_contract_executable();
    Command::new(&runtime)
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
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    let (_, fake_plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let fake_plugin_source_json =
        serde_json::to_string(&fake_plugin_source.to_string_lossy().to_string())?;
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_source_json
    );
    fs::write(
        isolated_home.join(FAKE_CODEX_REGISTRY_STALE_FILE_NAME),
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": if cfg!(windows) { "C:\\stale\\ProjectAtlas\\bin\\projectatlas.exe" } else { "/stale/ProjectAtlas/bin/projectatlas" },
                "args": ["--require-version", "0.0.1", "--db", if cfg!(windows) { "C:\\stale-repo\\.projectatlas\\projectatlas.db" } else { "/stale-repo/.projectatlas/projectatlas.db" }, "mcp"]
            }
        }))?,
    )?;
    fs::write(
        isolated_home.join(FAKE_CODEX_REGISTRY_CURRENT_FILE_NAME),
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", db,
                    "--config", atlas_dir.join("config.toml"),
                    "mcp"
                ]
            }
        }))?,
    )?;
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {plugin_list_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"add\" (\r\n  echo current>\"%PROJECTATLAS_FAKE_CODEX_STATE%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_STATE%\" (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT%\"\r\n  ) else (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE%\"\r\n  )\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{plugin_list_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"add\" ]; then\n  printf '%s\\n' current > \"$PROJECTATLAS_FAKE_CODEX_STATE\"\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  if [ -f \"$PROJECTATLAS_FAKE_CODEX_STATE\" ]; then\n    cat \"$PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT\"\n  else\n    cat \"$PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE\"\n  fi\n  exit 0\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;
    let safe_stale_runtime = if cfg!(windows) {
        isolated_home
            .join("AppData")
            .join("Roaming")
            .join(NPM_SHIM_DIR)
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
    let installer_output = run_plugin_installer_with_codex_fixture(
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
        || !installer_output_text.contains("v0.0.2-rc12")
        || !installer_output_text.contains(&expected_release_tag)
    {
        return Err(io::Error::other(format!(
            "plugin update installer did not report stale downstream workflow release pins:\n{installer_output_text}"
        ))
        .into());
    }
    let stale_pin_lines = installer_output_text
        .lines()
        .filter(|line| line.contains("Stale ProjectAtlas workflow release pin"))
        .collect::<Vec<_>>();
    if stale_pin_lines
        .iter()
        .any(|line| line.contains(&format!("uses {expected_release_tag};")))
    {
        return Err(io::Error::other(format!(
            "plugin update installer reported its exact current workflow release pin as stale:\n{installer_output_text}"
        ))
        .into());
    }
    if stale_pin_lines
        .iter()
        .any(|line| line.contains("uses v0.0.2;"))
    {
        return Err(io::Error::other(format!(
            "plugin update installer truncated a stale RC workflow release pin:\n{installer_output_text}"
        ))
        .into());
    }
    if let Some((stable_version, _)) = env!("CARGO_PKG_VERSION").split_once("-rc") {
        let truncated_tag = format!("v{stable_version}");
        if stale_pin_lines
            .iter()
            .any(|line| line.contains(&format!("uses {truncated_tag};")))
        {
            return Err(io::Error::other(format!(
                "plugin update installer truncated its exact RC workflow release pin:\n{installer_output_text}"
            ))
            .into());
        }
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
    fs::create_dir_all(isolated_home.join(CODEX_CONFIG_DIR))?;
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
    let installer_output = run_plugin_installer_with_codex_fixture(
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
fn plugin_update_leaves_current_codex_marketplace_untouched_and_repairs_stale_skill()
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
    let (_, fake_plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let plugin_skill = fake_plugin_source
        .join(PROJECTATLAS_SKILL_DIR)
        .join(PROJECTATLAS_SKILL_NAME)
        .join(SKILL_FILE_NAME);
    let fake_plugin_source_json =
        serde_json::to_string(&fake_plugin_source.to_string_lossy().to_string())?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_source_json
    );
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"plugin\" if \"%2\"==\"marketplace\" if \"%3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"list\" (\r\n  echo {plugin_list_json}\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"plugin\" if \"%2\"==\"add\" (\r\n  copy /Y \"%PROJECTATLAS_PACKAGED_SKILL%\" \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" >nul\r\n  if errorlevel 1 exit /b 1\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{plugin_list_json}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"add\" ]; then\n  cp \"$PROJECTATLAS_PACKAGED_SKILL\" \"$PROJECTATLAS_FAKE_PLUGIN_SKILL\"\n  exit $?\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let installer_output = run_plugin_installer_with_codex_fixture(
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
    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex_calls = fs::read_to_string(&fake_codex_log)?;
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
    for (label, skill_bytes) in [("missing", None), ("stale", Some(&b"stale skill"[..]))] {
        if let Some(skill_bytes) = skill_bytes {
            fs::write(&plugin_skill, skill_bytes)?;
        } else {
            fs::remove_file(&plugin_skill)?;
        }
        fs::write(&fake_codex_log, b"")?;
        let repair_output = run_plugin_installer_with_codex_fixture(
            &workspace_root,
            &repo,
            &runtime,
            &fake_path,
            &isolated_home,
        )?;
        let repair_output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&repair_output.stdout),
            String::from_utf8_lossy(&repair_output.stderr)
        );
        if !repair_output_text.contains("Codex ProjectAtlas plugin skill artifact does not match")
            || fs::read_to_string(&plugin_skill)? != FAKE_CODEX_SKILL_CONTENT
        {
            return Err(io::Error::other(format!(
                "installer did not repair the current-version {label} Codex skill artifact:\n{repair_output_text}"
            ))
            .into());
        }
        let repair_calls = fs::read_to_string(&fake_codex_log)?;
        for required in [
            "plugin remove projectatlas --marketplace projectatlas",
            "plugin add projectatlas --marketplace projectatlas",
        ] {
            if !repair_calls.contains(required) {
                return Err(io::Error::other(format!(
                    "{label} skill repair omitted {required:?}:\n{repair_calls}"
                ))
                .into());
            }
        }
        for forbidden in [
            "plugin marketplace remove projectatlas",
            "plugin marketplace add styler-ai/ProjectAtlas",
        ] {
            if repair_calls.contains(forbidden) {
                return Err(io::Error::other(format!(
                    "{label} skill repair mutated the current marketplace with {forbidden:?}:\n{repair_calls}"
                ))
                .into());
            }
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
    let (_, fake_plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        "0.0.1",
        FAKE_CODEX_SKILL_CONTENT,
    )?;
    let manifest_path = fake_plugin_source
        .join(CODEX_PLUGIN_MANIFEST_DIR)
        .join("plugin.json");
    let fake_plugin_source_json =
        serde_json::to_string(&fake_plugin_source.to_string_lossy().to_string())?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let plugin_list_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION"),
        fake_plugin_source_json
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
    let installer_output = run_plugin_installer_with_codex_fixture(
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
#[cfg(windows)]
fn windows_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail()
}

#[test]
#[cfg(unix)]
fn posix_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail()
}

fn assert_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail()
-> Result<(), Box<dyn Error>> {
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    for previous_ref in [expected_release_tag.as_str(), "v0.0.1"] {
        assert_failed_codex_replacement_preserves_prior_integration(previous_ref, true, false)?;
    }
    assert_failed_codex_replacement_preserves_prior_integration("v0.0.1", false, false)?;
    assert_failed_codex_replacement_preserves_prior_integration(&expected_release_tag, true, true)?;
    Ok(())
}

fn assert_failed_codex_replacement_preserves_prior_integration(
    previous_ref: &str,
    config_existed: bool,
    replacement_has_blank_source: bool,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    #[cfg(unix)]
    {
        // Exercise the `/var` to `/private/var` shape used by macOS temporary homes.
        let resolved_home = temp.path().join("resolved-home");
        fs::create_dir(&resolved_home)?;
        std::os::unix::fs::symlink(resolved_home, &isolated_home)?;
    }
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let expected_release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let codex_config = codex_dir.join("config.toml");
    if config_existed {
        fs::write(
            &codex_config,
            format!(
                "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{previous_ref}\"\n\n[plugins.\"projectatlas@projectatlas\"]\nenabled = true\n\n[mcp_servers.projectatlas]\ncommand = \"old-projectatlas-runtime\"\nargs = [\"--require-version\", \"0.0.1\", \"mcp\"]\n"
            ),
        )?;
    }
    let (marketplace_root, plugin_source, installed_cache) =
        write_fake_codex_projectatlas_integration(
            &codex_dir,
            "0.0.1",
            "0.0.1",
            "prior offline ProjectAtlas skill\n",
        )?;
    let unrelated_marketplace_state = marketplace_root.join("retained-state");
    fs::create_dir_all(unrelated_marketplace_state.join("empty-directory"))?;
    fs::write(
        unrelated_marketplace_state.join("metadata.bin"),
        b"unrelated validated marketplace bytes\n",
    )?;
    let prior_runtime_integration =
        r#"{"runtime":"old-projectatlas-runtime","version":"0.0.1","config":"prior"}"#;
    fs::write(
        plugin_source.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        prior_runtime_integration,
    )?;
    fs::write(
        installed_cache.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        prior_runtime_integration,
    )?;
    #[cfg(unix)]
    prepare_plugin_lock(&codex_dir)?;
    let state_before = repository_filesystem_snapshot(&codex_dir)?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let stale_plugin_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#
    );
    let blank_source_plugin_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":""}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let windows_replacement = if replacement_has_blank_source {
        ">\"%PROJECTATLAS_FAKE_CODEX_STATE%\" echo blank-source\r\nexit /b 0"
    } else {
        "exit /b 1"
    };
    let posix_replacement = if replacement_has_blank_source {
        "printf '%s\\n' blank-source > \"$PROJECTATLAS_FAKE_CODEX_STATE\"\nexit 0"
    } else {
        "exit 1"
    };
    let fake_codex_script = if cfg!(windows) {
        format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" goto plugin_list\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"remove\" goto destructive_marketplace_remove\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"remove\" goto destructive_plugin_remove\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"add\" goto replacement_failure\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" goto replacement_failure\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n:plugin_list\r\nif exist \"%PROJECTATLAS_FAKE_CODEX_STATE%\" (\r\n  echo {blank_source_plugin_json}\r\n  exit /b 0\r\n)\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\necho {stale_plugin_json}\r\nexit /b 0\r\n:plugin_absent\r\necho {{\"installed\":[],\"available\":[]}}\r\nexit /b 0\r\n:destructive_marketplace_remove\r\n>\"%PROJECTATLAS_FAKE_CODEX_CONFIG%\" echo mutated=true\r\nif exist \"%PROJECTATLAS_FAKE_MARKETPLACE_ROOT%\" rmdir /s /q \"%PROJECTATLAS_FAKE_MARKETPLACE_ROOT%\"\r\nexit /b 0\r\n:destructive_plugin_remove\r\n>\"%PROJECTATLAS_FAKE_CODEX_CONFIG%\" echo mutated=true\r\nif exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" rmdir /s /q \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\"\r\nexit /b 0\r\n:replacement_failure\r\n{windows_replacement}\r\n"
        )
    } else {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  if [ -f \"$PROJECTATLAS_FAKE_CODEX_STATE\" ]; then\n    printf '%s\\n' '{blank_source_plugin_json}'\n  elif [ -f \"$PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST\" ] && [ -f \"$PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD\" ] && [ -f \"$PROJECTATLAS_FAKE_PLUGIN_MANIFEST\" ] && [ -f \"$PROJECTATLAS_FAKE_PLUGIN_SKILL\" ] && [ -f \"$PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION\" ] && [ -f \"$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST\" ] && [ -f \"$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL\" ] && [ -f \"$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION\" ]; then\n    printf '%s\\n' '{stale_plugin_json}'\n  else\n    printf '%s\\n' '{{\"installed\":[],\"available\":[]}}'\n  fi\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"remove\" ]; then\n  printf '%s\\n' 'mutated=true' > \"$PROJECTATLAS_FAKE_CODEX_CONFIG\"\n  rm -rf -- \"$PROJECTATLAS_FAKE_MARKETPLACE_ROOT\"\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"remove\" ]; then\n  printf '%s\\n' 'mutated=true' > \"$PROJECTATLAS_FAKE_CODEX_CONFIG\"\n  rm -rf -- \"$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT\"\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && {{ [ \"${{2:-}}\" = \"add\" ] || {{ [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"add\" ]; }}; }}; then\n  {posix_replacement}\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then\n  exit 1\nfi\nexit 0\n"
        )
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = mcp_contract_executable();
    let verify_separate_state =
        previous_ref == expected_release_tag && config_existed && !replacement_has_blank_source;
    let generated_state_before = if verify_separate_state {
        let mut skip_command = projectatlas_plugin_installer_command_with_optional_path_and_home(
            &workspace_root,
            &repo,
            &runtime,
            Some(&fake_path),
            Some(&isolated_home),
        )?;
        skip_command
            .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
            .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1");
        let skip_output = require_successful_plugin_installer_output(skip_command.output()?)?;
        let skip_output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&skip_output.stdout),
            String::from_utf8_lossy(&skip_output.stderr)
        );
        let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
        let skip_codex_calls = if fake_codex_log.exists() {
            fs::read_to_string(&fake_codex_log)?
        } else {
            String::new()
        };
        let skipped_codex_state = repository_filesystem_snapshot(&codex_dir)?;
        if !skip_output_text.contains(
            "Codex ProjectAtlas plugin update skipped by PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE.",
        ) || skipped_codex_state != state_before
            || skip_codex_calls.lines().any(|call| {
                [
                    "plugin remove ",
                    "plugin add ",
                    "plugin marketplace remove ",
                    "plugin marketplace add ",
                ]
                .iter()
                .any(|mutation| call.starts_with(mutation))
            })
        {
            return Err(io::Error::other(format!(
                "explicit plugin skip mutated Codex state or hid its diagnostic:\n{skip_output_text}\nfake Codex calls:\n{skip_codex_calls}"
            ))
            .into());
        }
        if fake_codex_log.exists() {
            fs::remove_file(fake_codex_log)?;
        }
        Some(repository_filesystem_snapshot(&repo)?)
    } else {
        None
    };
    #[cfg(windows)]
    let runtime_state_before = if verify_separate_state {
        Some(repository_filesystem_snapshot(
            &isolated_home
                .join("AppData")
                .join("Local")
                .join(PROJECTATLAS_LOCAL_APPDATA_DIR),
        )?)
    } else {
        None
    };
    let installer_output = run_plugin_installer_with_codex_fixture(
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
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    if !installer_output_text.contains("Codex ProjectAtlas plugin update failed")
        || !installer_output_text.contains(
            "Codex MCP registry update skipped: no global projectatlas MCP server is configured",
        )
    {
        return Err(io::Error::other(format!(
            "installer did not preserve the plugin while checking the independent MCP registry:\n{installer_output_text}\nfake Codex calls:\n{fake_codex_calls}"
        ))
        .into());
    }
    if !fake_codex_calls.contains("mcp get projectatlas") {
        return Err(io::Error::other(format!(
            "failed plugin replacement suppressed independent MCP registry convergence:\n{fake_codex_calls}"
        ))
        .into());
    }
    let (required_remove, required_add, forbidden_add) = if previous_ref == expected_release_tag {
        (
            "plugin remove projectatlas --marketplace projectatlas --json",
            "plugin add projectatlas --marketplace projectatlas --json".to_string(),
            "plugin marketplace add",
        )
    } else {
        (
            "plugin marketplace remove projectatlas --json",
            format!(
                "plugin marketplace add styler-ai/ProjectAtlas --ref {expected_release_tag} --json"
            ),
            "plugin add projectatlas --marketplace projectatlas",
        )
    };
    for required in [required_remove, required_add.as_str()] {
        if !fake_codex_calls.contains(required) {
            return Err(io::Error::other(format!(
                "failed replacement omitted required call {required:?}:\n{fake_codex_calls}"
            ))
            .into());
        }
    }
    let prior_ref_add =
        format!("plugin marketplace add styler-ai/ProjectAtlas --ref {previous_ref} --json");
    if fake_codex_calls
        .lines()
        .filter(|call| *call == required_add.as_str())
        .count()
        != 1
        || (previous_ref != expected_release_tag
            && fake_codex_calls.lines().any(|call| call == prior_ref_add))
        || fake_codex_calls.contains(forbidden_add)
    {
        return Err(io::Error::other(format!(
            "failed replacement repeated acquisition or attempted network rollback:\n{fake_codex_calls}"
        ))
        .into());
    }
    if fake_codex_calls.contains("mcp remove projectatlas")
        || fake_codex_calls.contains("mcp add projectatlas")
    {
        return Err(io::Error::other(format!(
            "failed plugin replacement mutated the prior MCP runtime integration:\n{fake_codex_calls}"
        ))
        .into());
    }
    let state_after = repository_filesystem_snapshot(&codex_dir)?;
    if state_after != state_before {
        return Err(io::Error::other(format!(
            "failed replacement changed prior Codex marketplace/plugin/config/runtime state for {previous_ref}:\nbefore={state_before:#?}\nafter={state_after:#?}\ncalls:\n{fake_codex_calls}\ninstaller output:\n{installer_output_text}"
        ))
        .into());
    }
    if let Some(generated_state_before) = generated_state_before {
        let generated_state_after = repository_filesystem_snapshot(&repo)?;
        if generated_state_after != generated_state_before {
            return Err(io::Error::other(format!(
                "failed replacement changed generated ProjectAtlas host state:\nbefore={generated_state_before:#?}\nafter={generated_state_after:#?}"
            ))
            .into());
        }
    }
    #[cfg(windows)]
    if let Some(runtime_state_before) = runtime_state_before {
        let runtime_state_after = repository_filesystem_snapshot(
            &isolated_home
                .join("AppData")
                .join("Local")
                .join(PROJECTATLAS_LOCAL_APPDATA_DIR),
        )?;
        if runtime_state_after != runtime_state_before {
            return Err(io::Error::other(format!(
                "failed replacement changed the verified Windows runtime state:\nbefore={runtime_state_before:#?}\nafter={runtime_state_after:#?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn prepare_plugin_lock(codex_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(codex_dir.join(CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME), b"")?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn posix_plugin_lock_rejects_indirection_and_survives_crash() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{MetadataExt, symlink};

    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.sh");
    let installer_source = fs::read_to_string(installer)?;
    let acquire_start = installer_source
        .find("acquire_codex_projectatlas_plugin_update_lock() {")
        .ok_or_else(|| io::Error::other("POSIX plugin update lock acquisition function missing"))?;
    let acquire_end = installer_source[acquire_start..]
        .find("\nclear_codex_projectatlas_snapshot() {")
        .map(|offset| acquire_start + offset)
        .ok_or_else(|| io::Error::other("POSIX plugin update lock functions boundary drifted"))?;

    let temp = tempfile::tempdir()?;
    let lock_root = temp.path().join("codex-root");
    fs::create_dir(&lock_root)?;
    let lock_root = fs::canonicalize(lock_root)?;
    let lock_path = lock_root.join(CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME);
    let harness = temp.path().join("verify-plugin-lock-crash.sh");
    let runtime = mcp_contract_executable();
    fs::write(
        &harness,
        format!(
            r#"#!/bin/sh
set -eu
projectatlas_bin=${{PROJECTATLAS_TEST_RUNTIME:?}}
codex_plugin_update_lock_path=
codex_plugin_update_lock_root=
codex_plugin_update_lock_identity=
{}
mode=$1
lock_root=$2
acquire_codex_projectatlas_plugin_update_lock "$lock_root"
case "$mode" in
  hold)
    printf '%s\n' ready
    read -r ignored
    ;;
  survivor)
    sh -c 'printf "%s\n" "$$" > "$1"; while [ ! -e "$2" ]; do sleep 1; done' sh "$3" "$4" >/dev/null 2>&1 &
    ;;
  swap)
    mv -- "$lock_root/{CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME}" "$lock_root/held-lock"
    : > "$lock_root/{CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME}"
    if release_codex_projectatlas_plugin_update_lock; then exit 21; fi
    [ -f "$lock_root/held-lock" ]
    [ -f "$lock_root/{CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME}" ]
    ;;
  *)
    release_codex_projectatlas_plugin_update_lock
    ;;
esac
"#,
            &installer_source[acquire_start..acquire_end]
        ),
    )?;
    let harness_command = || {
        let mut command = StdCommand::new("sh");
        command
            .arg(&harness)
            .env("PROJECTATLAS_TEST_RUNTIME", &runtime);
        command
    };

    let outside = temp.path().join("outside-lock-target");
    fs::write(&outside, b"outside sentinel\n")?;
    symlink(&outside, &lock_path)?;
    let symlink_output = harness_command().arg("once").arg(&lock_root).output()?;
    if symlink_output.status.success() || fs::read(&outside)? != b"outside sentinel\n" {
        return Err(
            io::Error::other("POSIX plugin lock accepted or changed a symlink target").into(),
        );
    }
    fs::remove_file(&lock_path)?;

    fs::hard_link(&outside, &lock_path)?;
    let hard_link_output = harness_command().arg("once").arg(&lock_root).output()?;
    if hard_link_output.status.success() || fs::read(&outside)? != b"outside sentinel\n" {
        return Err(
            io::Error::other("POSIX plugin lock accepted or changed a hard-link target").into(),
        );
    }
    fs::remove_file(&lock_path)?;

    fs::create_dir(&lock_path)?;
    let directory_output = harness_command().arg("once").arg(&lock_root).output()?;
    if directory_output.status.success() {
        return Err(io::Error::other("POSIX plugin lock accepted a directory").into());
    }
    fs::remove_dir(&lock_path)?;

    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    write_executable_script(
        &fake_path.join(POSIX_FLOCK_EXECUTABLE_FILE_NAME),
        "#!/bin/sh\nexit 1\n",
    )?;
    let fake_runtime = fake_path.join("projectatlas");
    write_executable_script(&fake_runtime, "#!/bin/sh\nexit 1\n")?;
    let mut unavailable_command = harness_command();
    unavailable_command.arg("once").arg(&lock_root).env(
        "PATH",
        format!(
            "{}:{}",
            fake_path.display(),
            std::env::var_os("PATH")
                .unwrap_or_default()
                .to_string_lossy()
        ),
    );
    if cfg!(target_os = "macos") {
        unavailable_command.env("PROJECTATLAS_TEST_RUNTIME", &fake_runtime);
    }
    let unavailable_output = unavailable_command.output()?;
    if unavailable_output.status.success() {
        return Err(io::Error::other(
            "POSIX plugin lock proceeded after its native primitive failed",
        )
        .into());
    }

    #[cfg(target_os = "linux")]
    {
        let original_metadata = fs::metadata(&lock_path)?;
        let descriptor_swap_path = temp.path().join("descriptor-swap-path");
        fs::create_dir(&descriptor_swap_path)?;
        let captured_lock = lock_root.join("captured-lock");
        let detached_lock = lock_root.join("detached-lock");
        let replacement_lock = lock_root.join("replacement-lock");
        let flock_called = lock_root.join("flock-called");
        fs::write(&replacement_lock, b"replacement lock\n")?;
        let replacement_metadata = fs::metadata(&replacement_lock)?;
        if original_metadata.dev() == replacement_metadata.dev()
            && original_metadata.ino() == replacement_metadata.ino()
        {
            return Err(io::Error::other("Linux descriptor-swap fixture reused one inode").into());
        }
        write_executable_script(
            &descriptor_swap_path.join("id"),
            r#"#!/bin/sh
mv -- "$PROJECTATLAS_TEST_LOCK_PATH" "$PROJECTATLAS_TEST_CAPTURED_LOCK"
mv -- "$PROJECTATLAS_TEST_REPLACEMENT_LOCK" "$PROJECTATLAS_TEST_LOCK_PATH"
printf '%s\n' "$PROJECTATLAS_TEST_UID"
"#,
        )?;
        write_executable_script(
            &descriptor_swap_path.join("uname"),
            r#"#!/bin/sh
mv -- "$PROJECTATLAS_TEST_LOCK_PATH" "$PROJECTATLAS_TEST_DETACHED_LOCK"
mv -- "$PROJECTATLAS_TEST_CAPTURED_LOCK" "$PROJECTATLAS_TEST_LOCK_PATH"
printf '%s\n' Linux
"#,
        )?;
        write_executable_script(
            &descriptor_swap_path.join(POSIX_FLOCK_EXECUTABLE_FILE_NAME),
            r#"#!/bin/sh
: > "$PROJECTATLAS_TEST_FLOCK_CALLED"
exit 0
"#,
        )?;
        let descriptor_swap_output = harness_command()
            .arg("once")
            .arg(&lock_root)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    descriptor_swap_path.display(),
                    std::env::var_os("PATH")
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            )
            .env("PROJECTATLAS_TEST_LOCK_PATH", &lock_path)
            .env("PROJECTATLAS_TEST_CAPTURED_LOCK", &captured_lock)
            .env("PROJECTATLAS_TEST_REPLACEMENT_LOCK", &replacement_lock)
            .env("PROJECTATLAS_TEST_DETACHED_LOCK", &detached_lock)
            .env("PROJECTATLAS_TEST_FLOCK_CALLED", &flock_called)
            .env("PROJECTATLAS_TEST_UID", original_metadata.uid().to_string())
            .output()?;
        let restored_metadata = fs::metadata(&lock_path)?;
        let detached_metadata = fs::metadata(&detached_lock)?;
        if descriptor_swap_output.status.success()
            || flock_called.exists()
            || restored_metadata.dev() != original_metadata.dev()
            || restored_metadata.ino() != original_metadata.ino()
            || restored_metadata.nlink() != 1
            || detached_metadata.dev() != replacement_metadata.dev()
            || detached_metadata.ino() != replacement_metadata.ino()
        {
            return Err(io::Error::other(format!(
                "Linux plugin lock accepted a swapped inherited descriptor:\n{}\n{}",
                String::from_utf8_lossy(&descriptor_swap_output.stdout),
                String::from_utf8_lossy(&descriptor_swap_output.stderr)
            ))
            .into());
        }
        fs::remove_file(&detached_lock)?;
    }

    fs::write(&lock_path, b"orphaned owner\n")?;

    let mut owner = harness_command()
        .arg("hold")
        .arg(&lock_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut ready = String::new();
    BufReader::new(
        owner
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("POSIX lock owner stdout missing"))?,
    )
    .read_line(&mut ready)?;
    if ready.trim() != "ready" {
        let output = owner.wait_with_output()?;
        return Err(io::Error::other(format!(
            "POSIX lock owner did not acquire the lock:\n{}\n{}",
            ready,
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    owner.kill()?;
    owner.wait()?;

    let output = harness_command().arg("once").arg(&lock_root).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "POSIX plugin lock remained held after its owner crashed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let metadata = fs::symlink_metadata(&lock_path)?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(io::Error::other(
            "POSIX plugin lock did not remain one direct reusable file after crash recovery",
        )
        .into());
    }

    let child_ready = temp.path().join("surviving-child-ready");
    let child_release = temp.path().join("release-surviving-child");
    let survivor_output = harness_command()
        .arg("survivor")
        .arg(&lock_root)
        .arg(&child_ready)
        .arg(&child_release)
        .output()?;
    if !survivor_output.status.success() {
        return Err(io::Error::other(format!(
            "POSIX lock owner could not leave a mutation child running:\n{}\n{}",
            String::from_utf8_lossy(&survivor_output.stdout),
            String::from_utf8_lossy(&survivor_output.stderr)
        ))
        .into());
    }
    let child_deadline = Instant::now() + Duration::from_secs(2);
    while !child_ready.is_file() && Instant::now() < child_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !child_ready.is_file() {
        return Err(io::Error::other("POSIX mutation child did not report readiness").into());
    }
    let try_native_lock = || -> io::Result<bool> {
        let contender = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)?;
        match contender.try_lock() {
            Ok(()) => Ok(true),
            Err(fs::TryLockError::WouldBlock) => Ok(false),
            Err(fs::TryLockError::Error(source)) => Err(source),
        }
    };
    if try_native_lock()? {
        return Err(io::Error::other(
            "POSIX plugin lock was released while the mutation child still held its descriptor",
        )
        .into());
    }
    fs::write(&child_release, b"release\n")?;
    let release_deadline = Instant::now() + Duration::from_secs(3);
    let mut release_probe = try_native_lock()?;
    while !release_probe && Instant::now() < release_deadline {
        thread::sleep(Duration::from_millis(10));
        release_probe = try_native_lock()?;
    }
    if !release_probe {
        return Err(io::Error::other(
            "POSIX plugin lock remained held after releasing the mutation child",
        )
        .into());
    }
    let released_metadata = fs::symlink_metadata(&lock_path)?;
    if released_metadata.dev() != metadata.dev()
        || released_metadata.ino() != metadata.ino()
        || released_metadata.nlink() != 1
    {
        return Err(io::Error::other(
            "POSIX native lock probe replaced or linked the persistent lock inode",
        )
        .into());
    }
    let child_release_output = harness_command().arg("once").arg(&lock_root).output()?;
    if !child_release_output.status.success() {
        return Err(io::Error::other(format!(
            "POSIX plugin lock could not be reacquired after releasing the mutation child:\n{}\n{}",
            String::from_utf8_lossy(&child_release_output.stdout),
            String::from_utf8_lossy(&child_release_output.stderr)
        ))
        .into());
    }

    let swap_output = harness_command().arg("swap").arg(&lock_root).output()?;
    if !swap_output.status.success()
        || !lock_root.join("held-lock").is_file()
        || !lock_path.is_file()
    {
        return Err(io::Error::other(format!(
            "POSIX plugin lock release removed or accepted a replacement path:\n{}\n{}",
            String::from_utf8_lossy(&swap_output.stdout),
            String::from_utf8_lossy(&swap_output.stderr)
        ))
        .into());
    }
    if fs::read_dir(&lock_root)?.any(|entry| {
        entry.is_ok_and(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".projectatlas-plugin-update-candidate")
                || name.starts_with(".projectatlas-plugin-update.lock.stale")
        })
    }) {
        return Err(io::Error::other("POSIX plugin lock crash recovery left lock debris").into());
    }
    Ok(())
}

#[cfg(windows)]
struct WindowsSubstitutedDirectory {
    drive: String,
    root: PathBuf,
    active: bool,
}

#[cfg(windows)]
impl WindowsSubstitutedDirectory {
    fn create(target: &Path) -> Result<Self, Box<dyn Error>> {
        for letter in (b'D'..=b'Z').rev() {
            let drive = format!("{}:", char::from(letter));
            let root = PathBuf::from(format!(r"{drive}\"));
            if root.exists() {
                continue;
            }
            let status = StdCommand::new("subst.exe")
                .arg(&drive)
                .arg(target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if status.success() {
                return Ok(Self {
                    drive,
                    root,
                    active: true,
                });
            }
        }
        Err(io::Error::other("Windows alias test could not reserve a substituted drive").into())
    }

    fn release(mut self) -> Result<(), Box<dyn Error>> {
        let status = StdCommand::new("subst.exe")
            .args([self.drive.as_str(), "/D"])
            .status()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "Windows alias test could not release substituted drive {}",
                self.drive
            ))
            .into());
        }
        self.active = false;
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsSubstitutedDirectory {
    fn drop(&mut self) {
        if self.active {
            drop(
                StdCommand::new("subst.exe")
                    .args([self.drive.as_str(), "/D"])
                    .status(),
            );
        }
    }
}

#[cfg(windows)]
fn windows_short_path(path: &Path) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let path_text = path.to_string_lossy();
    let command_path = path_text.strip_prefix(r"\\?\").unwrap_or(&path_text);
    let command = format!(r#"for %I in ("{command_path}") do @echo %~sI"#);
    let output = StdCommand::new("cmd.exe")
        .args(["/D", "/C", &command])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Windows alias test could not query the short path for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let short_path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if short_path.as_os_str().is_empty()
        || !short_path.exists()
        || !short_path.to_string_lossy().contains('~')
        || short_path
            .to_string_lossy()
            .eq_ignore_ascii_case(command_path)
    {
        Ok(None)
    } else {
        Ok(Some(short_path))
    }
}

#[test]
#[cfg(windows)]
fn windows_plugin_update_serializes_restore_before_the_next_installer_reads_state()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_serializes_restore_before_the_next_installer_reads_state()
}

#[test]
#[cfg(unix)]
fn posix_plugin_update_serializes_restore_before_the_next_installer_reads_state()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_serializes_restore_before_the_next_installer_reads_state()
}

fn assert_plugin_update_serializes_restore_before_the_next_installer_reads_state()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let first_repo = temp.path().join("first-repo");
    let second_repo = temp.path().join("second-repo");
    fs::create_dir(&first_repo)?;
    fs::create_dir(&second_repo)?;
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
    let (_, plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        "0.0.1",
        "0.0.1",
        "prior offline ProjectAtlas skill\n",
    )?;
    #[cfg(windows)]
    let current_drive_alias = WindowsSubstitutedDirectory::create(&codex_dir)?;
    #[cfg(windows)]
    let aliased_codex_dir =
        windows_short_path(&codex_dir)?.unwrap_or_else(|| current_drive_alias.root.clone());
    #[cfg(windows)]
    {
        let physical_config_path = codex_dir.join("config.toml");
        let aliased_config_path = aliased_codex_dir.join("config.toml");
        let physical_config = fs::canonicalize(&physical_config_path).map_err(|error| {
            io::Error::other(format!(
                "could not canonicalize physical Windows config path {}: {error}",
                physical_config_path.display()
            ))
        })?;
        let aliased_config = fs::canonicalize(&aliased_config_path).map_err(|error| {
            io::Error::other(format!(
                "could not canonicalize aliased Windows config path {}: {error}",
                aliased_config_path.display()
            ))
        })?;
        if aliased_codex_dir
            .to_string_lossy()
            .eq_ignore_ascii_case(&codex_dir.to_string_lossy())
            || !aliased_config
                .to_string_lossy()
                .eq_ignore_ascii_case(&physical_config.to_string_lossy())
        {
            return Err(io::Error::other(format!(
                "Windows concurrency fixture did not address one physical Codex root through distinct aliases: physical={}, alias={}",
                codex_dir.display(),
                aliased_codex_dir.display()
            ))
            .into());
        }
    }
    #[cfg(unix)]
    prepare_plugin_lock(&codex_dir)?;
    let state_before = repository_filesystem_snapshot(&codex_dir)?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let stale_plugin_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#
    );
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let fake_codex_script = if cfg!(windows) {
        r#"@echo off
echo %PROJECTATLAS_FAKE_INSTALLER_ROLE% %*>>"%PROJECTATLAS_FAKE_CODEX_LOG%"
if "%~1"=="plugin" if "%~2"=="marketplace" if "%~3"=="list" goto marketplace_list
if "%~1"=="plugin" if "%~2"=="list" (
  >"%PROJECTATLAS_FAKE_INVENTORY_OUTPUT%" echo __STALE_PLUGIN_JSON__
  type "%PROJECTATLAS_FAKE_INVENTORY_OUTPUT%"
  exit /b 0
)
if "%~1"=="plugin" if "%~2"=="remove" goto plugin_remove
if "%~1"=="plugin" if "%~2"=="add" exit /b 1
if "%~1"=="mcp" if "%~2"=="get" exit /b 1
exit /b 0
:marketplace_list
if "%PROJECTATLAS_FAKE_INSTALLER_ROLE%"=="B" goto second_marketplace_list
echo {"marketplaces":[{"name":"projectatlas","marketplaceSource":{"source":"https://github.com/styler-ai/ProjectAtlas.git"}}]}
exit /b 0
:second_marketplace_list
if not exist "%PROJECTATLAS_FAKE_CODEX_CONFIG%" goto second_saw_mutation
if not exist "%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%" goto second_saw_mutation
if not exist "%PROJECTATLAS_FAKE_PLUGIN_SKILL%" goto second_saw_mutation
if not exist "%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST%" goto second_saw_mutation
if not exist "%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL%" goto second_saw_mutation
>"%PROJECTATLAS_FAKE_SECOND_OBSERVED%" echo restored
goto second_marketplace_response
:second_saw_mutation
>"%PROJECTATLAS_FAKE_SECOND_OBSERVED%" echo mutated
:second_marketplace_response
echo {"marketplaces":[]}
exit /b 0
:plugin_remove
>"%PROJECTATLAS_FAKE_CODEX_CONFIG%" echo mutated=true
if exist "%PROJECTATLAS_FAKE_PLUGIN_ROOT%" rmdir /s /q "%PROJECTATLAS_FAKE_PLUGIN_ROOT%"
if exist "%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%" rmdir /s /q "%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%"
>"%PROJECTATLAS_FAKE_FIRST_MUTATED%" echo mutated
:wait_for_release
if not exist "%PROJECTATLAS_FAKE_RELEASE_FIRST%" (
  >nul 2>nul ping 127.0.0.1 -n 2
  goto wait_for_release
)
exit /b 0
"#
        .replace("__STALE_PLUGIN_JSON__", &stale_plugin_json)
    } else {
        r#"#!/usr/bin/env sh
printf '%s %s\n' "${PROJECTATLAS_FAKE_INSTALLER_ROLE:-unknown}" "$*" >> "$PROJECTATLAS_FAKE_CODEX_LOG"
if [ "${1:-}" = "plugin" ] && [ "${2:-}" = "marketplace" ] && [ "${3:-}" = "list" ]; then
  if [ "${PROJECTATLAS_FAKE_INSTALLER_ROLE:-}" = B ]; then
    if [ -f "$PROJECTATLAS_FAKE_CODEX_CONFIG" ] &&
      [ -f "$PROJECTATLAS_FAKE_PLUGIN_MANIFEST" ] &&
      [ -f "$PROJECTATLAS_FAKE_PLUGIN_SKILL" ] &&
      [ -f "$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST" ] &&
      [ -f "$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL" ]; then
      printf '%s\n' restored > "$PROJECTATLAS_FAKE_SECOND_OBSERVED"
    else
      printf '%s\n' mutated > "$PROJECTATLAS_FAKE_SECOND_OBSERVED"
    fi
    printf '%s\n' '{"marketplaces":[]}'
  else
    printf '%s\n' '{"marketplaces":[{"name":"projectatlas","marketplaceSource":{"source":"https://github.com/styler-ai/ProjectAtlas.git"}}]}'
  fi
  exit 0
fi
if [ "${1:-}" = "plugin" ] && [ "${2:-}" = "list" ]; then
  printf '%s\n' '__STALE_PLUGIN_JSON__' > "$PROJECTATLAS_FAKE_INVENTORY_OUTPUT"
  cat -- "$PROJECTATLAS_FAKE_INVENTORY_OUTPUT"
  exit 0
fi
if [ "${1:-}" = "plugin" ] && [ "${2:-}" = "remove" ]; then
  printf '%s\n' mutated=true > "$PROJECTATLAS_FAKE_CODEX_CONFIG"
  rm -rf -- "$PROJECTATLAS_FAKE_PLUGIN_ROOT" "$PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT"
  printf '%s\n' mutated > "$PROJECTATLAS_FAKE_FIRST_MUTATED"
  while [ ! -f "$PROJECTATLAS_FAKE_RELEASE_FIRST" ]; do sleep 0.1; done
  exit 0
fi
if [ "${1:-}" = "plugin" ] && [ "${2:-}" = "add" ]; then exit 1; fi
if [ "${1:-}" = "mcp" ] && [ "${2:-}" = "get" ]; then exit 1; fi
exit 0
"#
        .replace("__STALE_PLUGIN_JSON__", &stale_plugin_json)
    };
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let workspace_root = workspace_root()?;
    let runtime = mcp_contract_executable();
    let first_mutated = isolated_home.join("first-mutated");
    let release_first = isolated_home.join("release-first");
    let second_observed = isolated_home.join("second-observed");
    let inventory_output = isolated_home.join("inventory-output.json");
    let mut first_command = projectatlas_plugin_installer_command_with_optional_path_and_home(
        &workspace_root,
        &first_repo,
        &runtime,
        Some(&fake_path),
        Some(&isolated_home),
    )?;
    let mut second_command = projectatlas_plugin_installer_command_with_optional_path_and_home(
        &workspace_root,
        &second_repo,
        &runtime,
        Some(&fake_path),
        Some(&isolated_home),
    )?;
    for command in [&mut first_command, &mut second_command] {
        command
            .env("PROJECTATLAS_FAKE_FIRST_MUTATED", &first_mutated)
            .env("PROJECTATLAS_FAKE_RELEASE_FIRST", &release_first)
            .env("PROJECTATLAS_FAKE_SECOND_OBSERVED", &second_observed)
            .env("PROJECTATLAS_FAKE_INVENTORY_OUTPUT", &inventory_output)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
    first_command.env("PROJECTATLAS_FAKE_INSTALLER_ROLE", "A");
    second_command.env("PROJECTATLAS_FAKE_INSTALLER_ROLE", "B");
    #[cfg(windows)]
    {
        first_command.current_dir(&first_repo);
        second_command
            .current_dir(&current_drive_alias.root)
            .env("CODEX_HOME", &aliased_codex_dir);
        let first_drive = first_repo
            .components()
            .next()
            .ok_or_else(|| io::Error::other("first Windows installer current drive is missing"))?;
        let second_drive = current_drive_alias
            .root
            .components()
            .next()
            .ok_or_else(|| io::Error::other("second Windows installer current drive is missing"))?;
        if first_drive
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&second_drive.as_os_str().to_string_lossy())
        {
            return Err(io::Error::other(
                "Windows alias fixture did not give the installers different current drives",
            )
            .into());
        }
    }

    let mut first_child = first_command.spawn()?;
    let first_deadline = Instant::now() + Duration::from_secs(15);
    while !first_mutated.is_file() {
        if let Some(status) = first_child.try_wait()? {
            let output = first_child.wait_with_output()?;
            let inventory = fs::read_to_string(&inventory_output).unwrap_or_default();
            let calls =
                fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE)).unwrap_or_default();
            return Err(io::Error::other(format!(
                "first installer exited before its destructive operation was held: {status}\nstdout:\n{}\nstderr:\n{}\ninventory:\n{inventory}\ncalls:\n{calls}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if Instant::now() >= first_deadline {
            drop(first_child.kill());
            drop(first_child.wait());
            return Err(io::Error::other(
                "first installer did not enter its held destructive operation",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }

    let second_child = second_command.spawn()?;
    let second_config = second_repo
        .join(ATLAS_DIR_NAME)
        .join("projectatlas.opencode.json");
    let second_ready_deadline = Instant::now() + Duration::from_secs(15);
    while !second_config.is_file() && Instant::now() < second_ready_deadline {
        thread::sleep(Duration::from_millis(25));
    }
    thread::sleep(Duration::from_secs(1));
    let calls_while_first_held = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    let queued_result = if !second_config.is_file() {
        Err(io::Error::other(
            "second installer did not reach its final generated-config boundary before the lock assertion",
        ))
    } else if second_observed.exists()
        || calls_while_first_held.contains("B plugin marketplace list")
    {
        Err(io::Error::other(format!(
            "second installer read Codex state before the first rollback completed:\n{calls_while_first_held}"
        )))
    } else {
        Ok(())
    };
    fs::write(&release_first, b"release")?;
    let first_output =
        wait_for_plugin_installer_output(first_child, "first", Duration::from_secs(45))?;
    let second_output =
        wait_for_plugin_installer_output(second_child, "second", Duration::from_secs(45))?;
    queued_result?;
    require_successful_plugin_installer_output(first_output)?;
    require_successful_plugin_installer_output(second_output)?;

    if fs::read_to_string(&second_observed)?.trim() != "restored" {
        return Err(io::Error::other(
            "second installer observed the first installer's incomplete mutation",
        )
        .into());
    }
    let state_after = repository_filesystem_snapshot(&codex_dir)?;
    if state_after != state_before {
        return Err(io::Error::other(format!(
            "serialized failed replacement did not restore exact prior state:\nbefore={state_before:#?}\nafter={state_after:#?}"
        ))
        .into());
    }
    if (cfg!(windows) && codex_dir.join(CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME).exists())
        || fs::read_dir(&codex_dir)?.any(|entry| {
            entry.is_ok_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".projectatlas-plugin-state")
            })
        })
    {
        return Err(io::Error::other(
            "serialized installer left an update lock or recovery snapshot behind",
        )
        .into());
    }
    #[cfg(windows)]
    current_drive_alias.release()?;
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_update_fails_closed_when_lock_root_cannot_be_canonicalized()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(&fake_path)?;
    fs::create_dir(&isolated_home)?;
    let fake_codex = fake_path.join("codex.cmd");
    write_executable_script(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nexit /b 1\r\n",
    )?;
    let blocked_ancestor = isolated_home.join("blocked-codex-root");
    fs::write(&blocked_ancestor, b"not a directory\n")?;
    let invalid_codex_home = blocked_ancestor.join(CODEX_CONFIG_DIR);
    let calls = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let mut command = projectatlas_plugin_installer_command_with_optional_path_and_home(
        &workspace_root()?,
        &repo,
        &mcp_contract_executable(),
        Some(&fake_path),
        Some(&isolated_home),
    )?;
    command
        .env("CODEX_HOME", &invalid_codex_home)
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = require_successful_plugin_installer_output(command.output()?)?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized_output_text = output_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !normalized_output_text.contains("update lock could not be acquired safely")
        || !normalized_output_text.contains("non-directory ancestor")
    {
        return Err(io::Error::other(format!(
            "Windows installer omitted the canonical lock failure diagnostic:\n{output_text}"
        ))
        .into());
    }
    if fs::read_to_string(&blocked_ancestor)? != "not a directory\n" || invalid_codex_home.exists()
    {
        return Err(io::Error::other(
            "Windows installer mutated the rejected Codex root or its blocking ancestor",
        )
        .into());
    }
    let codex_calls = fs::read_to_string(calls).unwrap_or_default();
    if [
        "plugin remove",
        "plugin add",
        "plugin marketplace remove",
        "plugin marketplace add",
    ]
    .iter()
    .any(|mutation| codex_calls.lines().any(|call| call.starts_with(mutation)))
    {
        return Err(io::Error::other(format!(
            "Windows installer reached a Codex mutation after canonical lock failure:\n{codex_calls}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_update_refuses_retained_recovery_state_before_mutation()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_refuses_retained_recovery_state_before_mutation()
}

#[test]
#[cfg(unix)]
fn posix_plugin_update_refuses_retained_recovery_state_before_mutation()
-> Result<(), Box<dyn Error>> {
    assert_plugin_update_refuses_retained_recovery_state_before_mutation()
}

fn assert_plugin_update_refuses_retained_recovery_state_before_mutation()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(&fake_path)?;
    fs::create_dir_all(&codex_dir)?;
    fs::write(isolated_home.join(FAKE_CODEX_LOG_FILE), b"")?;
    let retained_snapshot = codex_dir.join(if cfg!(windows) {
        ".projectatlas-plugin-state-crashed"
    } else {
        ".projectatlas-plugin-state.crashed"
    });
    fs::create_dir(&retained_snapshot)?;
    fs::write(retained_snapshot.join("config.toml"), b"prior=true\n")?;
    let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let fake_codex_script = if cfg!(windows) {
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nexit /b 1\r\n"
    } else {
        "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nexit 1\n"
    };
    write_executable_script(&fake_codex, fake_codex_script)?;

    let runtime = mcp_contract_executable();
    let output = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized_output_text = output_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !normalized_output_text.contains("retained recovery state requires inspection")
        || !normalized_output_text.contains(
            "Codex MCP registry update skipped: no global projectatlas MCP server is configured",
        )
    {
        return Err(io::Error::other(format!(
            "installer did not fail closed for retained crash recovery state:\n{output_text}"
        ))
        .into());
    }
    let calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    for forbidden in [
        "plugin add",
        "plugin remove",
        "plugin marketplace add",
        "plugin marketplace remove",
    ] {
        if calls.contains(forbidden) {
            return Err(io::Error::other(format!(
                "installer mutated Codex before addressing retained recovery state through {forbidden:?}:\n{calls}"
            ))
            .into());
        }
    }
    if !calls.contains("mcp get projectatlas") {
        return Err(io::Error::other(format!(
            "retained plugin recovery state suppressed independent MCP registry convergence:\n{calls}"
        ))
        .into());
    }
    let lock_exists = codex_dir.join(CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME).is_file();
    if !retained_snapshot.join("config.toml").is_file()
        || (cfg!(windows) && lock_exists)
        || (cfg!(unix) && !lock_exists)
    {
        return Err(io::Error::other(
            "installer changed retained recovery state or left its update lock behind",
        )
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_update_refuses_unavailable_or_ambiguous_inventory() -> Result<(), Box<dyn Error>>
{
    assert_plugin_update_refuses_unavailable_or_ambiguous_inventory()
}

#[test]
#[cfg(unix)]
fn posix_plugin_update_refuses_unavailable_or_ambiguous_inventory() -> Result<(), Box<dyn Error>> {
    assert_plugin_update_refuses_unavailable_or_ambiguous_inventory()
}

fn assert_plugin_update_refuses_unavailable_or_ambiguous_inventory() -> Result<(), Box<dyn Error>> {
    for inventory_case in [
        "nonzero",
        "malformed",
        "duplicate",
        "blank-version",
        "invalid-version",
        "blank-marketplace-source",
        "unofficial-marketplace-source",
        "blank-source-path",
        "relative-source-path",
    ] {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join(TEST_REPO_DIR);
        fs::create_dir(&repo)?;
        let fake_path = temp.path().join(FAKE_PATH_DIR);
        fs::create_dir(&fake_path)?;
        let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
        let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
        fs::create_dir_all(&codex_dir)?;
        fs::write(
            codex_dir.join("config.toml"),
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"v0.0.1\"\n",
        )?;
        let (_, plugin_source, _) = write_fake_codex_projectatlas_integration(
            &codex_dir,
            "0.0.1",
            "0.0.1",
            "prior offline ProjectAtlas skill\n",
        )?;
        let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
        let inventory_version = match inventory_case {
            "blank-version" => "",
            "invalid-version" => "../0.0.1",
            _ => "0.0.1",
        };
        let inventory_marketplace_source = match inventory_case {
            "blank-marketplace-source" => "",
            "unofficial-marketplace-source" => "https://github.com/example/ProjectAtlas.git",
            _ => "https://github.com/styler-ai/ProjectAtlas.git",
        };
        let inventory_source_path = match inventory_case {
            "blank-source-path" => "\"\"".to_string(),
            "relative-source-path" => "\"plugins/projectatlas\"".to_string(),
            _ => plugin_source_json,
        };
        let plugin_entry = format!(
            r#"{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":{},"installed":true,"enabled":true,"marketplaceSource":{{"source":{}}},"source":{{"path":{inventory_source_path}}}}}"#,
            serde_json::to_string(inventory_version)?,
            serde_json::to_string(inventory_marketplace_source)?,
        );
        let inventory_json = match inventory_case {
            "malformed" => "not-json".to_string(),
            "duplicate" => {
                format!(r#"{{"installed":[{plugin_entry},{plugin_entry}],"available":[]}}"#)
            }
            "nonzero" => String::new(),
            _ => format!(r#"{{"installed":[{plugin_entry}],"available":[]}}"#),
        };
        let fake_codex = fake_path.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
        let fake_codex_script = if cfg!(windows) {
            format!(
                "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" (\r\n  {inventory_command}\r\n)\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n",
                inventory_command = if inventory_case == "nonzero" {
                    "exit /b 17".to_string()
                } else {
                    format!("echo {inventory_json}\r\n  exit /b 0")
                }
            )
        } else {
            format!(
                "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"marketplace\" ] && [ \"${{3:-}}\" = \"list\" ]; then\n  printf '%s\\n' '{{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}'\n  exit 0\nfi\nif [ \"${{1:-}}\" = \"plugin\" ] && [ \"${{2:-}}\" = \"list\" ]; then\n  {inventory_command}\nfi\nif [ \"${{1:-}}\" = \"mcp\" ] && [ \"${{2:-}}\" = \"get\" ]; then exit 1; fi\nexit 0\n",
                inventory_command = if inventory_case == "nonzero" {
                    "exit 17".to_string()
                } else {
                    format!("printf '%s\\n' '{inventory_json}'\n  exit 0")
                }
            )
        };
        write_executable_script(&fake_codex, &fake_codex_script)?;
        #[cfg(unix)]
        prepare_plugin_lock(&codex_dir)?;
        let state_before = repository_filesystem_snapshot(&codex_dir)?;
        let runtime = mcp_contract_executable();
        let output = run_plugin_installer_with_codex_fixture(
            &workspace_root()?,
            &repo,
            &runtime,
            &fake_path,
            &isolated_home,
        )?;
        let output_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let normalized_output_text = output_text.split_whitespace().collect::<Vec<_>>().join(" ");
        let calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
        if !normalized_output_text
            .contains("installed plugin inventory could not be verified completely")
            || calls.contains("plugin remove projectatlas")
            || calls.contains("plugin add projectatlas")
            || calls.contains("plugin marketplace remove projectatlas")
            || calls.contains("plugin marketplace add")
        {
            return Err(io::Error::other(format!(
                "{inventory_case} inventory was not rejected before destructive mutation:\n{output_text}\ncalls:\n{calls}"
            ))
            .into());
        }
        let state_after = repository_filesystem_snapshot(&codex_dir)?;
        if state_after != state_before {
            return Err(io::Error::other(format!(
                "{inventory_case} inventory changed prior Codex state:\nbefore={state_before:#?}\nafter={state_after:#?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn posix_plugin_inventory_without_jq_rejects_split_object_fields() -> Result<(), Box<dyn Error>> {
    let installer = workspace_root()?
        .join("plugins")
        .join("projectatlas")
        .join("scripts")
        .join("install-runtime.sh");
    let installer_source = fs::read_to_string(&installer)?;
    let official_start = installer_source
        .find("official_projectatlas_marketplace_source() {")
        .ok_or_else(|| io::Error::other("POSIX installer omitted official source validator"))?;
    let official_end = installer_source[official_start..]
        .find("\ncodex_config_path() {")
        .map(|offset| official_start + offset)
        .ok_or_else(|| io::Error::other("POSIX official source validator boundary drifted"))?;
    let inventory_start = installer_source
        .find("codex_projectatlas_inventory_complete=false")
        .ok_or_else(|| io::Error::other("POSIX installer omitted plugin inventory state"))?;
    let inventory_end = installer_source[inventory_start..]
        .find("\ncodex_projectatlas_plugin_version() {")
        .map(|offset| inventory_start + offset)
        .ok_or_else(|| io::Error::other("POSIX plugin inventory boundary drifted"))?;

    let temp = tempfile::tempdir()?;
    let empty_path = temp.path().join("path-without-jq");
    fs::create_dir(&empty_path)?;
    let calls = temp.path().join("calls.txt");
    let fake_codex = temp.path().join(POSIX_CODEX_EXECUTABLE_FILE_NAME);
    write_executable_script(
        &fake_codex,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$PROJECTATLAS_FAKE_CODEX_LOG\"\nprintf '%s\\n' '{\"installed\":[{\"pluginId\":\"projectatlas@projectatlas\",\"name\":\"projectatlas\",\"marketplaceName\":\"projectatlas\",\"version\":\"0.0.1\",\"installed\":true,\"enabled\":true},{\"pluginId\":\"other@other\",\"marketplaceSource\":{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"},\"source\":{\"path\":\"/tmp/projectatlas\"}}],\"available\":[]}'\n",
    )?;
    let wrapper = temp.path().join("verify-no-jq.sh");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nset -eu\n{}\n{}\nPATH=$1\ncodex_bin=$2\nif load_codex_projectatlas_plugin_inventory; then\n  exit 17\nfi\n[ \"$codex_projectatlas_inventory_complete\" = false ]\n",
            &installer_source[official_start..official_end],
            &installer_source[inventory_start..inventory_end]
        ),
    )?;
    let output = StdCommand::new("bash")
        .arg(&wrapper)
        .arg(&empty_path)
        .arg(&fake_codex)
        .env("PROJECTATLAS_FAKE_CODEX_LOG", &calls)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "POSIX no-jq inventory did not fail closed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let calls = fs::read_to_string(calls)?;
    if calls.trim() != "plugin list --marketplace projectatlas --json"
        || calls.contains(" remove ")
        || calls.contains(" add ")
    {
        return Err(io::Error::other(format!(
            "POSIX no-jq split-object inventory reached mutation or rollback:\n{calls}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn posix_plugin_restore_rejects_hostile_paths_and_retains_recovery_state()
-> Result<(), Box<dyn Error>> {
    for fault in [
        "late-snapshot-source-symlink",
        "destination-ancestor-symlink",
        "destination-removal-failure",
        "destination-mounted-subtree",
        "snapshot-source-mounted-subtree",
        "cleanup-mounted-snapshot",
        "mount-after-permission-preflight",
        "mount-inventory-unavailable",
        "config-destination-directory",
        "prior-absent-config-removal-failure",
    ] {
        assert_posix_plugin_restore_rejects_hostile_path(fault)?;
    }
    #[cfg(target_os = "linux")]
    assert_posix_plugin_restore_rejects_hostile_path("mount-inventory-malformed")?;
    #[cfg(target_os = "macos")]
    for fault in ["mount-probe-unavailable", "mount-descendant-unavailable"] {
        assert_posix_plugin_restore_rejects_hostile_path(fault)?;
    }
    Ok(())
}

#[cfg(unix)]
fn assert_posix_plugin_restore_rejects_hostile_path(fault: &str) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    let outside = temp.path().join("outside-codex-home");
    fs::create_dir(&repo)?;
    fs::create_dir(&fake_path)?;
    fs::create_dir_all(&codex_dir)?;
    fs::create_dir(&outside)?;
    fs::write(
        outside.join(INSTALLER_OUTSIDE_SENTINEL_FILE_NAME),
        b"outside\n",
    )?;
    if fault != "prior-absent-config-removal-failure" {
        fs::write(
            codex_dir.join("config.toml"),
            format!(
                "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"v{}\"\n",
                env!("CARGO_PKG_VERSION")
            ),
        )?;
    }
    let (marketplace_root, plugin_source, installed_cache) =
        write_fake_codex_projectatlas_integration(
            &codex_dir,
            "0.0.1",
            "0.0.1",
            "prior offline ProjectAtlas skill\n",
        )?;
    let mounted_subtree = plugin_source.join("mounted-tree");
    let mount_canary = mounted_subtree.join(INSTALLER_CANARY_FILE_NAME);
    fs::create_dir(&mounted_subtree)?;
    fs::write(&mount_canary, b"mounted state\n")?;
    let live_source_before = repository_filesystem_snapshot(&plugin_source)?;
    let live_cache_before = repository_filesystem_snapshot(&installed_cache)?;
    let snapshot_mount_relative = mounted_subtree.strip_prefix(&marketplace_root)?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let stale_plugin_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#
    );
    let fake_codex = fake_path.join(POSIX_CODEX_EXECUTABLE_FILE_NAME);
    let fake_codex_script = r#"#!/usr/bin/env sh
printf '%s\n' "$*" >> "$PROJECTATLAS_FAKE_CODEX_LOG"
if [ "${1:-}" = plugin ] && [ "${2:-}" = marketplace ] && [ "${3:-}" = list ]; then
  printf '%s\n' '{"marketplaces":[{"name":"projectatlas","marketplaceSource":{"source":"https://github.com/styler-ai/ProjectAtlas.git"}}]}'
  exit 0
fi
if [ "${1:-}" = plugin ] && [ "${2:-}" = list ]; then
  printf '%s\n' '__STALE_PLUGIN_JSON__'
  exit 0
fi
if [ "${1:-}" = plugin ] && { [ "${2:-}" = remove ] || { [ "${2:-}" = marketplace ] && [ "${3:-}" = remove ]; }; }; then
  snapshot=$(find "$CODEX_HOME" -maxdepth 1 -type d -name '.projectatlas-plugin-state.*' -print -quit)
  [ -n "$snapshot" ] || exit 9
  case "$PROJECTATLAS_FAKE_RESTORE_FAULT" in
    late-snapshot-source-symlink)
      rm -rf -- "$PROJECTATLAS_FAKE_PLUGIN_ROOT" || exit 10
      ;;
    destination-ancestor-symlink)
      plugin_parent=$(dirname -- "$PROJECTATLAS_FAKE_PLUGIN_ROOT")
      rm -rf -- "$plugin_parent" || exit 12
      ln -s -- "$PROJECTATLAS_FAKE_OUTSIDE" "$plugin_parent" || exit 13
      ;;
    destination-removal-failure)
      chmod 500 -- "$PROJECTATLAS_FAKE_PLUGIN_ROOT" || exit 14
      ;;
    config-destination-directory)
      rm -f -- "$PROJECTATLAS_FAKE_CODEX_CONFIG" || exit 15
      mkdir -- "$PROJECTATLAS_FAKE_CODEX_CONFIG" || exit 16
      ;;
    prior-absent-config-removal-failure)
      printf '%s\n' untrusted > "$PROJECTATLAS_FAKE_CODEX_CONFIG" || exit 18
      chmod 500 -- "$CODEX_HOME" || exit 19
      ;;
    destination-mounted-subtree|mount-inventory-unavailable|mount-inventory-malformed)
      printf '%s\n' "$PROJECTATLAS_FAKE_MOUNT_TARGET" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 22
      : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
      ;;
    mount-after-permission-preflight)
      printf '%s\n' "$PROJECTATLAS_FAKE_MOUNT_TARGET" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 22
      ;;
    snapshot-source-mounted-subtree)
      printf '%s\n' "$snapshot/marketplace-root/$PROJECTATLAS_FAKE_SNAPSHOT_MOUNT_RELATIVE" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 23
      : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
      ;;
    mount-probe-unavailable)
      printf '%s\n' "$snapshot" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 23
      : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
      ;;
    mount-descendant-unavailable)
      printf '%s\n' "$snapshot/marketplace-root/$PROJECTATLAS_FAKE_SNAPSHOT_MOUNT_RELATIVE" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 23
      : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
      ;;
    cleanup-mounted-snapshot)
      ;;
    *) exit 17 ;;
  esac
  exit 0
fi
if [ "${1:-}" = plugin ] && { [ "${2:-}" = add ] || { [ "${2:-}" = marketplace ] && [ "${3:-}" = add ]; }; }; then exit 1; fi
if [ "${1:-}" = mcp ] && [ "${2:-}" = get ]; then exit 1; fi
exit 0
"#
    .replace("__STALE_PLUGIN_JSON__", &stale_plugin_json);
    write_executable_script(&fake_codex, &fake_codex_script)?;
    write_executable_script(
        &fake_path.join("cp"),
        r#"#!/usr/bin/env sh
PATH=$PROJECTATLAS_FAKE_REAL_PATH
export PATH
cp "$@" || exit $?
if [ "${PROJECTATLAS_FAKE_RESTORE_FAULT:-}" = late-snapshot-source-symlink ]; then
  case "${3:-}" in
    */.projectatlas-plugin-state.*/marketplace-root)
      snapshot=$(dirname -- "${3:-}")
      rm -rf -- "$snapshot/plugin-cache" || exit 20
      ln -s -- "$PROJECTATLAS_FAKE_OUTSIDE" "$snapshot/plugin-cache" || exit 21
      ;;
  esac
fi
if [ "${PROJECTATLAS_FAKE_RESTORE_FAULT:-}" = cleanup-mounted-snapshot ] &&
  [ "${1:-}" = -p ] && [ "${2:-}" = -- ]; then
  case "${4##*/}" in
    config.toml.projectatlas-restore.*)
      snapshot=$(find "$CODEX_HOME" -maxdepth 1 -type d -name '.projectatlas-plugin-state.*' -print -quit)
      [ -n "$snapshot" ] || exit 25
      printf '%s\n' "$snapshot/marketplace-root/$PROJECTATLAS_FAKE_SNAPSHOT_MOUNT_RELATIVE" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 26
      : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
      ;;
  esac
fi
"#,
    )?;
    write_executable_script(
        &fake_path.join(POSIX_FIND_EXECUTABLE_FILE_NAME),
        r#"#!/usr/bin/env sh
"$PROJECTATLAS_FAKE_REAL_FIND" "$@"
status=$?
if [ "$status" -eq 0 ] &&
  [ "${PROJECTATLAS_FAKE_RESTORE_FAULT:-}" = mount-after-permission-preflight ] &&
  [ "${2:-}" = -type ] && [ "${3:-}" = d ]; then
  printf '%s\n' "$PROJECTATLAS_FAKE_MOUNT_TARGET" > "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE" || exit 24
  : > "$PROJECTATLAS_FAKE_MOUNT_ACTIVE"
fi
exit "$status"
"#,
    )?;
    #[cfg(target_os = "linux")]
    write_executable_script(
        &fake_path.join("findmnt"),
        r#"#!/usr/bin/env sh
if [ ! -e "$PROJECTATLAS_FAKE_MOUNT_ACTIVE" ]; then
  PATH=$PROJECTATLAS_FAKE_REAL_PATH
  export PATH
  exec findmnt "$@"
fi
if [ "$PROJECTATLAS_FAKE_RESTORE_FAULT" = mount-inventory-unavailable ]; then
  exit 1
fi
if [ "$PROJECTATLAS_FAKE_RESTORE_FAULT" = mount-inventory-malformed ]; then
  printf '%s\n' '{"filesystems":[{"source":"untrusted"}]}'
  exit 0
fi
target=$(cat "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE") || exit 1
jq -n \
  --arg root "$PROJECTATLAS_FAKE_MOUNT_ROOT" \
  --arg target "$target" \
  '{filesystems: [{target: $root}, {target: $target}]}'
"#,
    )?;
    #[cfg(target_os = "macos")]
    write_executable_script(
        &fake_path.join("stat"),
        r#"#!/usr/bin/env sh
if [ -e "$PROJECTATLAS_FAKE_MOUNT_ACTIVE" ] &&
  [ "${1:-}" = -f ] && [ "${2:-}" = %d ]; then
  if [ "$PROJECTATLAS_FAKE_RESTORE_FAULT" = mount-inventory-unavailable ]; then
    exit 1
  fi
  target=$(cat "$PROJECTATLAS_FAKE_MOUNT_TARGET_FILE") || exit 1
  target=$(CDPATH= cd -P -- "$target" 2>/dev/null && pwd -P) || exit 1
  probe=$(CDPATH= cd -P -- "${3:-}" 2>/dev/null && pwd -P) || exit 1
  if { [ "$PROJECTATLAS_FAKE_RESTORE_FAULT" = mount-probe-unavailable ] ||
      [ "$PROJECTATLAS_FAKE_RESTORE_FAULT" = mount-descendant-unavailable ]; } &&
    [ "$probe" = "$target" ]; then
    exit 1
  fi
  case "$probe/" in
    "$target/"|"$target"/*)
      printf '%s\n' 2
      ;;
    *)
      printf '%s\n' 1
      ;;
  esac
  exit 0
fi
PATH=$PROJECTATLAS_FAKE_REAL_PATH
export PATH
exec stat "$@"
"#,
    )?;

    let runtime = mcp_contract_executable();
    let inherited_path = std::env::var_os("PATH")
        .ok_or_else(|| io::Error::other("POSIX hostile restore test requires PATH"))?;
    let real_find = std::env::split_paths(&inherited_path)
        .map(|directory| directory.join(POSIX_FIND_EXECUTABLE_FILE_NAME))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| io::Error::other("POSIX hostile restore test requires find"))?;
    let mount_active = temp.path().join("mount-active");
    let mount_target_file = temp.path().join("mount-target");
    let mut command = projectatlas_plugin_installer_command_with_optional_path_and_home(
        &workspace_root()?,
        &repo,
        &runtime,
        Some(&fake_path),
        Some(&isolated_home),
    )?;
    command
        .env("PROJECTATLAS_FAKE_RESTORE_FAULT", fault)
        .env("PROJECTATLAS_FAKE_OUTSIDE", &outside)
        .env("PROJECTATLAS_FAKE_REAL_PATH", &inherited_path)
        .env("PROJECTATLAS_FAKE_REAL_FIND", &real_find)
        .env("PROJECTATLAS_FAKE_MOUNT_ACTIVE", &mount_active)
        .env("PROJECTATLAS_FAKE_MOUNT_ROOT", &codex_dir)
        .env("PROJECTATLAS_FAKE_MOUNT_TARGET", &mounted_subtree)
        .env("PROJECTATLAS_FAKE_MOUNT_TARGET_FILE", &mount_target_file)
        .env(
            "PROJECTATLAS_FAKE_SNAPSHOT_MOUNT_RELATIVE",
            snapshot_mount_relative,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = wait_for_plugin_installer_output(
        command.spawn()?,
        "hostile POSIX restore",
        Duration::from_secs(45),
    )?;
    let output = require_successful_plugin_installer_output(output)?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_failure = match fault {
        "cleanup-mounted-snapshot" => "state snapshot cleanup failed",
        "mount-inventory-malformed" => "mount inventory is malformed",
        "mount-inventory-unavailable"
        | "mount-probe-unavailable"
        | "mount-descendant-unavailable" => "mount inventory cannot be read",
        _ => "could not be restored completely",
    };
    if !output_text.contains(expected_failure) {
        return Err(io::Error::other(format!(
            "POSIX installer accepted hostile restore fault {fault:?}:\n{output_text}"
        ))
        .into());
    }
    if matches!(
        fault,
        "destination-mounted-subtree"
            | "snapshot-source-mounted-subtree"
            | "cleanup-mounted-snapshot"
            | "mount-after-permission-preflight"
    ) && !output_text.contains("mounted filesystem")
    {
        return Err(io::Error::other(format!(
            "POSIX restore mount fault {fault:?} omitted its observed-mount diagnostic:\n{output_text}"
        ))
        .into());
    }
    let snapshots = fs::read_dir(&codex_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".projectatlas-plugin-state.")
        })
        .count();
    if snapshots != 1 {
        return Err(io::Error::other(format!(
            "POSIX hostile restore fault {fault:?} did not retain exactly one recovery snapshot"
        ))
        .into());
    }
    if fs::read(outside.join(INSTALLER_OUTSIDE_SENTINEL_FILE_NAME))? != b"outside\n"
        || fs::read_dir(&outside)?.count() != 1
    {
        return Err(io::Error::other(format!(
            "POSIX hostile restore fault {fault:?} mutated outside CODEX_HOME"
        ))
        .into());
    }
    if fault == "late-snapshot-source-symlink"
        && (repository_filesystem_snapshot(&plugin_source)? != live_source_before
            || repository_filesystem_snapshot(&installed_cache)? != live_cache_before)
    {
        return Err(io::Error::other(
            "POSIX restore changed live state before rejecting a late-swapped snapshot source",
        )
        .into());
    }
    if matches!(
        fault,
        "destination-mounted-subtree"
            | "snapshot-source-mounted-subtree"
            | "cleanup-mounted-snapshot"
            | "mount-after-permission-preflight"
            | "mount-inventory-unavailable"
            | "mount-inventory-malformed"
            | "mount-probe-unavailable"
            | "mount-descendant-unavailable"
    ) && (repository_filesystem_snapshot(&plugin_source)? != live_source_before
        || repository_filesystem_snapshot(&installed_cache)? != live_cache_before
        || fs::read(&mount_canary)? != b"mounted state\n")
    {
        return Err(io::Error::other(format!(
            "POSIX restore mutated live state after mount validation fault {fault:?}"
        ))
        .into());
    }
    let calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    if calls.contains("mcp remove projectatlas") || calls.contains("mcp add projectatlas") {
        return Err(io::Error::other(format!(
            "POSIX hostile restore fault {fault:?} mutated MCP state:\n{calls}"
        ))
        .into());
    }
    if fault == "destination-removal-failure" && plugin_source.exists() {
        fs::set_permissions(&plugin_source, fs::Permissions::from_mode(0o700))?;
    }
    if fault == "config-destination-directory"
        && fs::read_dir(codex_dir.join("config.toml"))?
            .next()
            .is_some()
    {
        return Err(io::Error::other(
            "POSIX config restore nested a temporary file inside the hostile destination directory",
        )
        .into());
    }
    if fault == "prior-absent-config-removal-failure" {
        fs::set_permissions(&codex_dir, fs::Permissions::from_mode(0o700))?;
        let lock = codex_dir.join(CODEX_PLUGIN_UPDATE_LOCK_FILE_NAME);
        if lock.exists() {
            fs::remove_file(lock)?;
        }
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_restore_rejects_config_directory_and_retains_recovery_snapshot()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(&fake_path)?;
    fs::create_dir_all(&codex_dir)?;
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"v{}\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )?;
    let (_, plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        "0.0.1",
        "0.0.1",
        "prior offline ProjectAtlas skill\n",
    )?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let stale_plugin_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"0.0.1","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#
    );
    let fake_codex = fake_path.join("codex.cmd");
    let fake_codex_script = format!(
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" (\r\n  echo {stale_plugin_json}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"remove\" (\r\n  del /f /q \"%PROJECTATLAS_FAKE_CODEX_CONFIG%\" >nul 2>nul\r\n  mkdir \"%PROJECTATLAS_FAKE_CODEX_CONFIG%\"\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" exit /b 1\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
    );
    write_executable_script(&fake_codex, &fake_codex_script)?;

    let runtime = mcp_contract_executable();
    let output = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output_text.contains("could not be restored completely") {
        return Err(io::Error::other(format!(
            "Windows installer accepted a config directory as restored state:\n{output_text}"
        ))
        .into());
    }
    let config_path = codex_dir.join("config.toml");
    if !config_path.is_dir() || fs::read_dir(&config_path)?.next().is_some() {
        return Err(io::Error::other(
            "Windows config restore changed the hostile destination or nested a temporary file inside it",
        )
        .into());
    }
    let snapshots = fs::read_dir(&codex_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".projectatlas-plugin-state-")
        })
        .count();
    if snapshots != 1 {
        return Err(io::Error::other(
            "Windows config restore failure did not retain exactly one recovery snapshot",
        )
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_restore_rejects_cache_junction_and_retains_recovery_snapshot()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let codex_config = codex_dir.join("config.toml");
    fs::write(
        &codex_config,
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{release_tag}\"\n"
        ),
    )?;
    let (_, plugin_source, installed_cache) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        "stale ProjectAtlas skill\n",
    )?;
    let runtime_integration = "prior offline runtime integration\n";
    fs::write(
        plugin_source.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        runtime_integration,
    )?;
    fs::write(
        installed_cache.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        runtime_integration,
    )?;
    let junction_target = isolated_home.join(FAKE_CODEX_JUNCTION_TARGET_DIR);
    fs::create_dir_all(&junction_target)?;
    fs::write(
        junction_target.join(INSTALLER_CANARY_FILE_NAME),
        "outside state\n",
    )?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let installed_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let fake_codex = fake_path.join("codex.cmd");
    let fake_script = format!(
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" goto plugin_list\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"remove\" goto plugin_remove\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" goto plugin_add\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n:plugin_list\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\necho {installed_json}\r\nexit /b 0\r\n:plugin_absent\r\necho {{\"installed\":[],\"available\":[]}}\r\nexit /b 0\r\n:plugin_remove\r\n>\"%PROJECTATLAS_FAKE_CODEX_CONFIG%\" echo mutated=true\r\nif exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" rmdir /s /q \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\"\r\nexit /b 0\r\n:plugin_add\r\nmklink /J \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" \"%PROJECTATLAS_FAKE_JUNCTION_TARGET%\" >nul\r\nexit /b 1\r\n"
    );
    write_executable_script(&fake_codex, &fake_script)?;

    let runtime = mcp_contract_executable();
    let output = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshots = fs::read_dir(&codex_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".projectatlas-plugin-state-")
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    if snapshots.len() != 1
        || !snapshots[0]
            .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
            .join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME)
            .is_file()
        || !output_text.contains("could not be restored completely")
        || !output_contains_path_name(&output_text, &snapshots[0])
    {
        return Err(io::Error::other(format!(
            "junction-swapped second restore destination did not retain one usable recovery snapshot:\n{output_text}\nsnapshots={snapshots:#?}"
        ))
        .into());
    }
    if fs::read_to_string(junction_target.join(INSTALLER_CANARY_FILE_NAME))? != "outside state\n" {
        return Err(io::Error::other("junction restore modified the outside canary").into());
    }
    let calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    if calls.contains("mcp remove projectatlas") || calls.contains("mcp add projectatlas") {
        return Err(io::Error::other(format!(
            "partial plugin restore mutated MCP state:\n{calls}"
        ))
        .into());
    }
    fs::remove_dir(&installed_cache)?;
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_snapshot_rejects_reparse_above_codex_home_before_mutation()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let real_home_parent = temp.path().join("real-home-parent");
    let linked_home_parent = temp.path().join("linked-home-parent");
    fs::create_dir(&real_home_parent)?;
    fs::write(
        real_home_parent.join(INSTALLER_CANARY_FILE_NAME),
        "outside state\n",
    )?;
    let link_output = StdCommand::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&linked_home_parent)
        .arg(&real_home_parent)
        .output()?;
    if !link_output.status.success() {
        return Err(io::Error::other(format!(
            "failed to create ancestor junction above CODEX_HOME:\n{}\n{}",
            String::from_utf8_lossy(&link_output.stdout),
            String::from_utf8_lossy(&link_output.stderr)
        ))
        .into());
    }
    let isolated_home = linked_home_parent.join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{release_tag}\"\n"
        ),
    )?;
    let (_, plugin_source, _) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        "stale ProjectAtlas skill\n",
    )?;
    #[cfg(unix)]
    prepare_plugin_lock(&codex_dir)?;
    let state_before = repository_filesystem_snapshot(&codex_dir)?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let installed_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let fake_codex = fake_path.join("codex.cmd");
    write_executable_script(
        &fake_codex,
        &format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" (\r\n  echo {installed_json}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
        ),
    )?;
    let runtime = mcp_contract_executable();
    let output = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized_output_text = output_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
    if !normalized_output_text.contains("reparse point in its Codex-root ancestry")
        || calls.contains("plugin remove projectatlas")
        || calls.contains("plugin add projectatlas")
        || calls.contains("plugin marketplace remove projectatlas")
        || calls.contains("plugin marketplace add")
        || repository_filesystem_snapshot(&codex_dir)? != state_before
        || fs::read_to_string(real_home_parent.join(INSTALLER_CANARY_FILE_NAME))?
            != "outside state\n"
    {
        return Err(io::Error::other(format!(
            "reparse ancestor above CODEX_HOME was not rejected before mutation:\n{output_text}\ncalls:\n{calls}"
        ))
        .into());
    }
    fs::remove_dir(&linked_home_parent)?;
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_snapshot_cleanup_refuses_path_swap_without_outside_deletion()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{release_tag}\"\n"
        ),
    )?;
    let (_, plugin_source, installed_cache) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        "stale ProjectAtlas skill\n",
    )?;
    fs::write(
        plugin_source.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        "current runtime integration\n",
    )?;
    fs::write(
        installed_cache.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
        "current runtime integration\n",
    )?;
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let installed_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let fake_codex = fake_path.join("codex.cmd");
    write_executable_script(
        &fake_codex,
        &format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" goto plugin_list\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"remove\" goto plugin_remove\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" goto plugin_add\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n:plugin_list\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\necho {installed_json}\r\nexit /b 0\r\n:plugin_absent\r\necho {{\"installed\":[],\"available\":[]}}\r\nexit /b 0\r\n:plugin_remove\r\nif exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" rmdir /s /q \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\"\r\nexit /b 0\r\n:plugin_add\r\ncopy /y \"%PROJECTATLAS_PACKAGED_SKILL%\" \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" >nul\r\nxcopy /e /i /q /y \"%PROJECTATLAS_FAKE_PLUGIN_ROOT%\" \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" >nul\r\nfor /d %%D in (\"%CODEX_HOME%\\.projectatlas-plugin-state-*\") do (\r\n  move \"%%~fD\" \"%PROJECTATLAS_FAKE_CLEANUP_SNAPSHOT_TARGET%\" >nul\r\n  >\"%PROJECTATLAS_FAKE_CLEANUP_SNAPSHOT_TARGET%\\outside-canary.txt\" echo outside state\r\n  mklink /J \"%%~fD\" \"%PROJECTATLAS_FAKE_CLEANUP_SNAPSHOT_TARGET%\" >nul\r\n)\r\nexit /b 0\r\n"
        ),
    )?;
    let runtime = mcp_contract_executable();
    let output = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    )?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cleanup_target = isolated_home.join(FAKE_CODEX_CLEANUP_SNAPSHOT_TARGET_DIR);
    let snapshot_links = fs::read_dir(&codex_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".projectatlas-plugin-state-")
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    if snapshot_links.len() != 1
        || !cleanup_target
            .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
            .join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME)
            .is_file()
        || fs::read_to_string(cleanup_target.join("outside-canary.txt"))? != "outside state\r\n"
        || !output_text.contains("state snapshot cleanup refused/path changed at")
        || output_text.contains("state snapshot cleanup failed; retained at")
        || !output_contains_path_name(&output_text, &snapshot_links[0])
        || !output_text.contains(&format!(
            "Codex ProjectAtlas plugin marketplace updated to {release_tag}."
        ))
    {
        return Err(io::Error::other(format!(
            "successful update did not refuse a swapped cleanup path truthfully:\n{output_text}\nlinks={snapshot_links:#?}\ntarget={cleanup_target:?}"
        ))
        .into());
    }
    fs::remove_dir(&snapshot_links[0])?;
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_plugin_snapshot_cleanup_failure_retains_usable_direct_snapshot()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    let fake_path = temp.path().join(FAKE_PATH_DIR);
    fs::create_dir(&fake_path)?;
    let isolated_home = temp.path().join(ISOLATED_HOME_DIR);
    let codex_dir = isolated_home.join(CODEX_CONFIG_DIR);
    fs::create_dir_all(&codex_dir)?;
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        codex_dir.join("config.toml"),
        format!(
            "[marketplaces.projectatlas]\nsource_type = \"git\"\nsource = \"https://github.com/styler-ai/ProjectAtlas.git\"\nref = \"{release_tag}\"\n"
        ),
    )?;
    let (_, plugin_source, installed_cache) = write_fake_codex_projectatlas_integration(
        &codex_dir,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        "stale ProjectAtlas skill\n",
    )?;
    for root in [&plugin_source, &installed_cache] {
        fs::write(
            root.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
            "current runtime integration\n",
        )?;
    }
    let plugin_source_json = serde_json::to_string(&plugin_source.to_string_lossy())?;
    let installed_json = format!(
        r#"{{"installed":[{{"pluginId":"projectatlas@projectatlas","name":"projectatlas","marketplaceName":"projectatlas","version":"{}","installed":true,"enabled":true,"marketplaceSource":{{"source":"https://github.com/styler-ai/ProjectAtlas.git"}},"source":{{"path":{plugin_source_json}}}}}],"available":[]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let fake_codex = fake_path.join("codex.cmd");
    write_executable_script(
        &fake_codex,
        &format!(
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" goto plugin_list\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"remove\" goto plugin_remove\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" goto plugin_add\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n:plugin_list\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL%\" goto plugin_absent\r\nif not exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION%\" goto plugin_absent\r\necho {installed_json}\r\nexit /b 0\r\n:plugin_absent\r\necho {{\"installed\":[],\"available\":[]}}\r\nexit /b 0\r\n:plugin_remove\r\nif exist \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" rmdir /s /q \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\"\r\nexit /b 0\r\n:plugin_add\r\ncopy /y \"%PROJECTATLAS_PACKAGED_SKILL%\" \"%PROJECTATLAS_FAKE_PLUGIN_SKILL%\" >nul\r\nxcopy /e /i /q /y \"%PROJECTATLAS_FAKE_PLUGIN_ROOT%\" \"%PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT%\" >nul\r\nfor /d %%D in (\"%CODEX_HOME%\\.projectatlas-plugin-state-*\") do (\r\n  icacls \"%%~fD\" /deny \"*S-1-1-0:(OI)(CI)(DE,DC)\" /T /C >nul\r\n  if errorlevel 1 exit /b 19\r\n)\r\nexit /b 0\r\n"
        ),
    )?;

    let runtime = mcp_contract_executable();
    let output_result = run_plugin_installer_with_codex_fixture(
        &workspace_root()?,
        &repo,
        &runtime,
        &fake_path,
        &isolated_home,
    );
    let snapshots = fs::read_dir(&codex_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".projectatlas-plugin-state-")
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let mut acl_restore_failures = Vec::new();
    for snapshot in &snapshots {
        let restore = StdCommand::new("icacls")
            .arg(snapshot)
            .args(["/remove:d", "*S-1-1-0", "/T", "/C"])
            .output()?;
        if !restore.status.success() {
            acl_restore_failures.push(format!(
                "{}:\n{}\n{}",
                snapshot.display(),
                String::from_utf8_lossy(&restore.stdout),
                String::from_utf8_lossy(&restore.stderr)
            ));
        }
    }
    if !acl_restore_failures.is_empty() {
        return Err(io::Error::other(format!(
            "failed to restore cleanup-fault ACLs:\n{}",
            acl_restore_failures.join("\n")
        ))
        .into());
    }
    let output = output_result?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if snapshots.len() != 1
        || !snapshots[0].join("config.toml").is_file()
        || !snapshots[0]
            .join(CODEX_MARKETPLACE_SNAPSHOT_DIR_NAME)
            .join("plugins")
            .join("projectatlas")
            .join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME)
            .is_file()
        || !snapshots[0]
            .join(FAKE_CODEX_PLUGIN_CACHE_DIR)
            .join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME)
            .is_file()
        || !snapshots[0]
            .join(CODEX_MARKETPLACE_SNAPSHOT_DIR_NAME)
            .join(CODEX_MARKETPLACE_METADATA_DIR)
            .join("plugins")
            .join(CODEX_MARKETPLACE_MANIFEST_FILE_NAME)
            .is_file()
        || !snapshots[0]
            .join(CODEX_MARKETPLACE_SNAPSHOT_DIR_NAME)
            .join(CODEX_MARKETPLACE_INSTALL_RECORD_FILE_NAME)
            .is_file()
        || !output_text.contains("state snapshot cleanup failed; retained at")
        || !output_contains_path_name(&output_text, &snapshots[0])
        || output_text.contains("state snapshot cleanup refused/path changed at")
        || !output_text.contains(&format!(
            "Codex ProjectAtlas plugin marketplace updated to {release_tag}."
        ))
    {
        return Err(io::Error::other(format!(
            "direct cleanup failure did not retain one complete usable recovery snapshot:\n{output_text}\nsnapshots={snapshots:#?}"
        ))
        .into());
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
        .join(WINDOWS_POWERSHELL_DIR)
        .join(WINDOWS_POWERSHELL_VERSION_DIR);
    let restricted_path = std::env::join_paths([
        system_root.join(WINDOWS_SYSTEM32_DIR),
        powershell_dir.clone(),
    ])?;
    let output = StdCommand::new(powershell_dir.join(WINDOWS_POWERSHELL_EXECUTABLE))
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
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let stable_runtime_dir = stable_runtime
        .parent()
        .ok_or_else(|| io::Error::other("stable runtime parent missing"))?;
    let parent_path = std::env::join_paths(
        std::iter::once(stable_runtime_dir.to_path_buf())
            .chain(std::env::split_paths(&inherited_path)),
    )?;

    let fake_codex_log = isolated_home.join(FAKE_CODEX_LOG_FILE);
    let fake_codex = isolated_home.join("codex.cmd");
    let fake_codex_state = isolated_home.join(FAKE_CODEX_REGISTRY_STATE_FILE_NAME);
    let fake_codex_stale_registry = isolated_home.join(FAKE_CODEX_REGISTRY_STALE_FILE_NAME);
    let fake_codex_current_registry = isolated_home.join(FAKE_CODEX_REGISTRY_CURRENT_FILE_NAME);
    let versioned_runtime = local_app_data
        .join(PROJECTATLAS_LOCAL_APPDATA_DIR)
        .join("runtimes")
        .join(env!("CARGO_PKG_VERSION"))
        .join("x86_64-pc-windows-msvc")
        .join("projectatlas.exe");
    fs::write(
        &fake_codex_stale_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": &stable_runtime,
                "args": [
                    "--require-version", "0.3.10",
                    "--db", temp.path().join("stale-project").join(ATLAS_DIR_NAME).join("projectatlas.db"),
                    "mcp"
                ]
            }
        }))?,
    )?;
    fs::write(
        &fake_codex_current_registry,
        serde_json::to_vec(&json!({
            "name": "projectatlas",
            "enabled": true,
            "transport": {
                "type": "stdio",
                "command": versioned_runtime,
                "args": [
                    "--require-version", env!("CARGO_PKG_VERSION"),
                    "--db", atlas_dir.join("projectatlas.db"),
                    "--config", atlas_dir.join("config.toml"),
                    "mcp"
                ]
            }
        }))?,
    )?;
    fs::write(
        &fake_codex,
        "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%1\"==\"mcp\" if \"%2\"==\"add\" (\r\n  echo current>\"%PROJECTATLAS_FAKE_CODEX_STATE%\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"mcp\" if \"%2\"==\"get\" (\r\n  if exist \"%PROJECTATLAS_FAKE_CODEX_STATE%\" (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT%\"\r\n  ) else (\r\n    type \"%PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE%\"\r\n  )\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
    )?;

    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let release_archive = create_windows_release_archive(temp.path(), &runtime)?;
    let release_asset_guard = lock_windows_release_asset_tests();
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
        .arg(&installer)
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
        .env("PATH", &parent_path)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
        .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
        .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
        .env(
            "PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE",
            &fake_codex_stale_registry,
        )
        .env(
            "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
            &fake_codex_current_registry,
        )
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
    drop(release_asset_guard);
    let installer_output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized_installer_output_text =
        installer_output_text.split_whitespace().collect::<String>();
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "release-binary installer failed\n{installer_output_text}"
        ))
        .into());
    }

    if !versioned_runtime.exists() {
        return Err(io::Error::other(format!(
            "release binary was not installed to the versioned runtime path: {}",
            versioned_runtime.display()
        ))
        .into());
    }
    if !normalized_installer_output_text.ends_with(
        &"ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=true host_restart_required=false"
            .split_whitespace()
            .collect::<String>(),
    ) || normalized_installer_output_text.contains(
        &"Existing host restart required:"
            .split_whitespace()
            .collect::<String>(),
    )
    {
        return Err(io::Error::other(format!(
            "installer did not report full readiness after synchronizing the unlocked mirror\n{installer_output_text}"
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
    let sibling_output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(
            "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; & projectatlas --require-version $env:PROJECTATLAS_VERSION --format json runtime-info",
        )
        .env("PATH", &parent_path)
        .env("PROJECTATLAS_VERSION", env!("CARGO_PKG_VERSION"))
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let sibling_stdout = String::from_utf8_lossy(&sibling_output.stdout);
    if !sibling_output.status.success() {
        return Err(io::Error::other(format!(
            "unchanged sibling failed after the stable mirror synchronized:\n{sibling_stdout}\n{}",
            String::from_utf8_lossy(&sibling_output.stderr)
        ))
        .into());
    }
    let sibling_command = sibling_stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| {
            io::Error::other("synchronized sibling did not report its bare command path")
        })?;
    require_same_executable(
        sibling_command.trim(),
        &stable_runtime,
        "synchronized parent sibling",
    )?;

    let stale_parent_bin = temp.path().join("stale-parent-bin");
    fs::create_dir_all(&stale_parent_bin)?;
    let stale_parent_runtime = stale_parent_bin.join("projectatlas.cmd");
    fs::write(
        &stale_parent_runtime,
        "@echo off\r\necho {\"version\":\"0.3.26\"}\r\nexit /b 0\r\n",
    )?;
    let stale_parent_path = std::env::join_paths(
        std::iter::once(stale_parent_bin).chain(std::env::split_paths(&inherited_path)),
    )?;
    let stale_parent_install = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&installer)
        .arg("-ProjectRoot")
        .arg(&repo)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .arg("-RuntimePath")
        .arg(&versioned_runtime)
        .env("HOME", &isolated_home)
        .env("USERPROFILE", &isolated_home)
        .env("APPDATA", &app_data)
        .env("LOCALAPPDATA", &local_app_data)
        .env("PATH", &stale_parent_path)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_CODEX_COMMAND", &fake_codex)
        .env("PROJECTATLAS_FAKE_CODEX_LOG", &fake_codex_log)
        .env("PROJECTATLAS_FAKE_CODEX_STATE", &fake_codex_state)
        .env(
            "PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE",
            &fake_codex_stale_registry,
        )
        .env(
            "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
            &fake_codex_current_registry,
        )
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    let stale_parent_install_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&stale_parent_install.stdout),
        String::from_utf8_lossy(&stale_parent_install.stderr)
    );
    let normalized_stale_parent_install_text = stale_parent_install_text
        .split_whitespace()
        .collect::<String>();
    if !stale_parent_install.status.success()
        || !normalized_stale_parent_install_text.ends_with(
            &"ProjectAtlas readiness: runtime_ready=true generated_mcp_configs_ready=true runtime_mcp_configs_ready=true installer_cli_ready=true parent_cli_ready=false host_restart_required=false"
                .split_whitespace()
                .collect::<String>(),
        )
        || normalized_stale_parent_install_text.contains(
            &"Existing host restart required:"
                .split_whitespace()
                .collect::<String>(),
        )
        || !normalized_stale_parent_install_text.contains(
            &"restart alone will not repair it"
                .split_whitespace()
                .collect::<String>(),
        )
    {
        return Err(io::Error::other(format!(
            "installer misstated repair requirements when the synchronized mirror was absent from the unchanged parent PATH\n{stale_parent_install_text}"
        ))
        .into());
    }
    let stale_sibling_output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(
            "$command = Get-Command projectatlas -ErrorAction Stop; Write-Output $command.Source; & projectatlas --format json runtime-info",
        )
        .env("PATH", &stale_parent_path)
        .output()?;
    let stale_sibling_stdout = String::from_utf8_lossy(&stale_sibling_output.stdout);
    if !stale_sibling_output.status.success() || !stale_sibling_stdout.contains("0.3.26") {
        return Err(io::Error::other(format!(
            "unchanged stale sibling did not demonstrate its stale bare-command resolution:\n{stale_sibling_stdout}\n{}",
            String::from_utf8_lossy(&stale_sibling_output.stderr)
        ))
        .into());
    }
    let stale_sibling_command = stale_sibling_stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| io::Error::other("stale sibling did not report its bare command path"))?;
    require_same_executable(
        stale_sibling_command.trim(),
        &stale_parent_runtime,
        "stale parent sibling",
    )?;

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
    if !normalized_installer_output_text.contains(
        &"Codex MCP registry updated to ProjectAtlas runtime"
            .split_whitespace()
            .collect::<String>(),
    ) {
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
    let release_asset_guard = lock_windows_release_asset_tests();
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
    drop(release_asset_guard);
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
    let release_asset_guard = lock_windows_release_asset_tests();
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
    drop(release_asset_guard);
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
fn packaged_cli_surface_preserves_frozen_routes_and_defaults() -> Result<(), Box<dyn Error>> {
    let executable = mcp_contract_executable();
    assert_mcp_contract_runtime_and_skill(&executable)?;
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cli-surfaces.json"))?;
    let current_key = format!("v{}", env!("CARGO_PKG_VERSION"));
    let current = fixture
        .get(&current_key)
        .ok_or_else(|| io::Error::other(format!("CLI fixture omitted {current_key}")))?;
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

    for legacy_version in ["v0.3.26", "v0.4.0"] {
        let legacy = json_at(&fixture, &[legacy_version])?;
        let legacy_commands = cli_surface_strings(legacy, &["commands"])?;
        if !legacy_commands
            .iter()
            .all(|command| current_commands.contains(command))
        {
            return Err(io::Error::other(format!(
                "packaged CLI removed a {legacy_version} command: current={current_commands:?} legacy={legacy_commands:?}"
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
                    "packaged CLI removed a {legacy_version} {parent} route: advertised={advertised:?} legacy={expected:?}"
                ))
                .into());
            }
        }
        if let Some(legacy_actions) = legacy.get("actions").and_then(Value::as_object) {
            for (route, expected) in legacy_actions {
                let expected = cli_value_strings(expected, route)?;
                let advertised = cli_help_surface(&executable, route)?.possible_values;
                if !expected.iter().all(|action| advertised.contains(action)) {
                    return Err(io::Error::other(format!(
                        "packaged CLI removed a {legacy_version} {route} action: advertised={advertised:?} legacy={expected:?}"
                    ))
                    .into());
                }
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
                    "packaged CLI removed or reordered a {legacy_version} default for {route:?}: advertised={advertised:?} legacy={expected:?}"
                ))
                .into());
            }
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
            output: CliContractOutput::JsonObject,
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

    Connection::open(&database)?.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let missing_root = temp.path().join(MISSING_INDEX_DIR_NAME);
    fs::create_dir(&missing_root)?;
    let wrong_root = temp.path().join(WRONG_PROJECT_OWNER_DIR_NAME);
    let wrong_atlas_dir = wrong_root.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&wrong_atlas_dir)?;
    let wrong_database = wrong_atlas_dir.join("projectatlas.db");
    fs::copy(&database, &wrong_database)?;
    let wrong_before = sqlite_compatibility_snapshot(&wrong_database)?;

    let writer = Connection::open(&database)?;
    writer.execute_batch("PRAGMA wal_autocheckpoint = 0")?;
    let supported_schema_version = writer.query_row(
        "SELECT CAST(value AS INTEGER) FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let future_schema_version = supported_schema_version
        .checked_add(1)
        .ok_or_else(|| io::Error::other("schema version overflowed"))?;
    writer.execute(
        "UPDATE metadata SET value = ?1 WHERE key = 'schema_version'",
        [future_schema_version],
    )?;
    let wal_path = sqlite_sidecar_path(&database, "-wal");
    if !wal_path.is_file() || fs::metadata(&wal_path)?.len() == 0 {
        return Err(io::Error::other(
            "packaged schema-mismatch fixture did not retain an active WAL",
        )
        .into());
    }
    let incompatible_before = sqlite_compatibility_snapshot(&database)?;

    let incompatible_cli = StdCommand::new(&executable)
        .current_dir(&repo)
        .arg("--require-version")
        .arg(env!("CARGO_PKG_VERSION"))
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&database)
        .arg("overview")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    if incompatible_cli.status.success() {
        return Err(io::Error::other("packaged CLI opened a newer-schema database").into());
    }
    let incompatible_cli_error: Value = serde_json::from_slice(&incompatible_cli.stderr)?;
    require_schema_version_mismatch(
        &incompatible_cli_error,
        future_schema_version,
        supported_schema_version,
    )?;
    let incompatible_cli_text = String::from_utf8_lossy(&incompatible_cli.stderr);
    for private in [database.display().to_string(), repo.display().to_string()] {
        if incompatible_cli_text.contains(&private) {
            return Err(io::Error::other(format!(
                "packaged CLI schema mismatch exposed private path {private}"
            ))
            .into());
        }
    }
    if sqlite_compatibility_snapshot(&database)? != incompatible_before {
        return Err(io::Error::other(
            "packaged CLI schema mismatch changed the active-WAL database",
        )
        .into());
    }

    let mut incompatible_session = McpContractSession::spawn(&executable, &repo, &database)?;
    let incompatible_mcp_result = (|| -> Result<(), Box<dyn Error>> {
        let mismatch_text = incompatible_session
            .call_tool_error("atlas_overview", &serde_json::json!({"project_path": repo}))?;
        let mismatch: Value = toon_format::decode_default(&mismatch_text)?;
        require_schema_version_mismatch(
            &mismatch,
            future_schema_version,
            supported_schema_version,
        )?;
        for private in [database.display().to_string(), repo.display().to_string()] {
            if mismatch_text.contains(&private) {
                return Err(io::Error::other(format!(
                    "packaged MCP schema mismatch exposed private path {private}"
                ))
                .into());
            }
        }

        let missing_text = incompatible_session.call_tool(
            "atlas_overview",
            &serde_json::json!({"project_path": missing_root}),
        )?;
        if !missing_text.contains("kind: init_required")
            || missing_root.join(ATLAS_DIR_NAME).exists()
        {
            return Err(io::Error::other(format!(
                "persistent MCP missing-index control mutated state or lost typed guidance: {missing_text}"
            ))
            .into());
        }

        let wrong_text = incompatible_session.call_tool(
            "atlas_overview",
            &serde_json::json!({"project_path": wrong_root}),
        )?;
        if !wrong_text.contains("kind: project_mismatch")
            || wrong_text.contains("kind: schema_version_mismatch")
            || sqlite_compatibility_snapshot(&wrong_database)? != wrong_before
        {
            return Err(io::Error::other(format!(
                "persistent MCP wrong-root control mutated state or lost typed ownership: {wrong_text}"
            ))
            .into());
        }

        let runtime_text =
            incompatible_session.call_tool("atlas_runtime_info", &serde_json::json!({}))?;
        if !runtime_text.contains(env!("CARGO_PKG_VERSION")) {
            return Err(io::Error::other(
                "persistent MCP session stopped responding after schema mismatch",
            )
            .into());
        }
        Ok(())
    })();
    complete_mcp_test_after_shutdown(incompatible_mcp_result, || incompatible_session.shutdown())?;
    if sqlite_compatibility_snapshot(&database)? != incompatible_before {
        return Err(io::Error::other(
            "packaged MCP schema mismatch changed the active-WAL database",
        )
        .into());
    }
    if writer.query_row::<i64, _, _>(
        "SELECT CAST(value AS INTEGER) FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )? != future_schema_version
    {
        return Err(io::Error::other("live WAL owner could not read the retained schema").into());
    }
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
    let linked = temp.path().join("mcp-contract-linked");
    let linked_output = git_command_for_root(&repo)
        .args(["worktree", "add", "-b", "mcp-contract-linked"])
        .arg(&linked)
        .output()?;
    if !linked_output.status.success() {
        return Err(io::Error::other(format!(
            "MCP contract linked-worktree setup failed: {}",
            String::from_utf8_lossy(&linked_output.stderr)
        ))
        .into());
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
            name: "atlas_worktree_list",
            arguments: serde_json::json!({"include_retired": false}),
            expected_marker: "worktrees:",
            payload_key: Some("worktrees"),
            effect: McpSqliteEffect::None,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_worktree_add",
            arguments: serde_json::json!({"worktree": "mcp-contract-linked", "alias": "contract-linked"}),
            expected_marker: "worktree:",
            payload_key: Some("worktree"),
            effect: McpSqliteEffect::WorktreeRegistryAdvance,
            telemetry_enabled: false,
        },
        McpToolContractCase {
            name: "atlas_worktree_remove",
            arguments: serde_json::json!({"worktree": "contract-linked"}),
            expected_marker: "worktree:",
            payload_key: Some("worktree"),
            effect: McpSqliteEffect::WorktreeRegistryAdvance,
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
    assert_frozen_mcp_surfaces_compatible(&inventory)?;
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
    if tools_digest != MCP_TOOLS_SHA256 {
        return Err(io::Error::other(format!(
            "MCP inventory/schema digest drifted: expected {MCP_TOOLS_SHA256}, found {tools_digest}"
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
            "atlas_scan" => fs::write(
                repo.join(SRC_DIR_NAME).join(SCANNED_RS_FILE_NAME),
                "pub fn rescanned_contract() { rescanned_contract(); }\n",
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

    let missing_index_root = temp.path().join(MISSING_INDEX_DIR_NAME);
    fs::create_dir(&missing_index_root)?;
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
        (
            "atlas_purpose_review",
            serde_json::json!({"project_path": repo_argument, "apply": true, "items": [{"path": "src/scanned.rs", "purpose": "Must not apply.", "task": "mcp-contract", "work_key": "incomplete"}]}),
            "entirely conditional",
        ),
        (
            "atlas_purpose_review",
            serde_json::json!({"project_path": parent_canary, "apply": true, "items": [{"path": "src/scanned.rs", "purpose": "Must not apply."}]}),
            "not a directory",
        ),
        (
            "atlas_purpose_review",
            serde_json::json!({"project_path": missing_index_root, "apply": true, "items": [{"path": "src/scanned.rs", "purpose": "Must not apply."}]}),
            "is missing",
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
    if missing_index_root.join(ATLAS_DIR_NAME).exists() {
        return Err(io::Error::other("missing-index MCP review created project state").into());
    }

    let stale_before = mcp_database_snapshot(&db)?;
    let (stale_response, stale_stdout) = run_mcp_contract_raw_call(
        &executable,
        &repo,
        &db,
        "atlas_purpose_review",
        &serde_json::json!({
            "project_path": repo_argument,
            "apply": true,
            "items": [{
                "path": "src/scanned.rs",
                "purpose": "Must not apply.",
                "task": "mcp-contract",
                "work_key": "0".repeat(64),
                "state_token": "0".repeat(64)
            }]
        }),
        false,
    )?;
    if stale_response
        .get("result")
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(io::Error::other(format!(
            "stale conditional MCP review returned an adapter error: {stale_response}"
        ))
        .into());
    }
    let stale_review: Value = toon_format::decode_default(&mcp_tool_text(&stale_stdout, 2)?)?;
    require_json_usize(&stale_review, &["purpose_review", "changed"], 0)?;
    require_json_usize(&stale_review, &["purpose_review", "conflicts"], 1)?;
    require_json_string(
        &stale_review,
        &["purpose_review_items", "0", "action"],
        "stale",
    )?;
    if mcp_database_snapshot(&db)? != stale_before {
        return Err(io::Error::other("stale conditional MCP review changed SQLite state").into());
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
        "pub fn indexed() {\n    helper();\n}\n\
         fn group_a_root() { group_a_one(); group_a_two(); }\n\
         fn group_a_one() { group_a_two(); }\n\
         fn group_a_two() { group_a_root(); }\n\
         fn group_b_root() { group_b_one(); group_b_two(); }\n\
         fn group_b_one() { group_b_two(); }\n\
         fn group_b_two() { group_b_root(); }\n\
         fn architecture_root() {\n\
             group_a_root(); group_a_one(); group_a_two();\n\
             group_b_root(); group_b_one(); group_b_two();\n\
         }\n\
         fn helper() {}\n",
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
        r#"{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","file":"src/lib.rs","symbol":"architecture_root","direction":"outbound","depth":3,"limit":100,"output_bytes":65536,"include_communities":true,"include_cycles":true}}}"#.to_string(),
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
        r#"{"jsonrpc":"2.0","id":35,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","file":"src/lib.rs","symbol":"architecture_root","direction":"outbound","depth":3,"limit":100,"output_bytes":65536,"include_communities":true,"include_cycles":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":36,"method":"tools/call","params":{"name":"atlas_symbol_relations","arguments":{"view":"analysis","file":"src/lib.rs","symbol":"architecture_root","direction":"outbound","depth":3,"limit":100,"edge_limit":1,"output_bytes":65536,"include_communities":true}}}"#.to_string(),
    ];
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let args = [
        "--db".to_string(),
        db.display().to_string(),
        "mcp".to_string(),
    ];
    let run_phase = |phase: &[String]| run_mcp_stdio(&executable, &repo, &args, phase);
    // MCP dispatches requests concurrently. Keep initialization/scan ahead of
    // later analysis, then reopen the same database for analysis requests.
    let mut stdout = run_phase(&messages[..4])?;
    let mut navigation_messages = vec![messages[0].clone(), messages[1].clone()];
    navigation_messages.extend(messages[4..21].iter().cloned());
    stdout.push_str(&run_phase(&navigation_messages)?);
    let mut analysis_messages = vec![messages[0].clone(), messages[1].clone()];
    analysis_messages.extend(messages[21..].iter().cloned());
    stdout.push_str(&run_phase(&analysis_messages)?);
    assert_frozen_mcp_surfaces_compatible(&stdout)?;
    let session_brief_text = mcp_tool_text(&stdout, 19)?;
    let analysis_text = mcp_tool_text(&stdout, 23)?;
    let repeated_analysis_text = mcp_tool_text(&stdout, 35)?;
    let bounded_analysis_text = mcp_tool_text(&stdout, 36)?;
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
    let analysis_payload: Value = toon_format::decode_default(&analysis_text)?;
    let repeated_analysis_payload: Value = toon_format::decode_default(&repeated_analysis_text)?;
    let first_communities = json_community_values(&analysis_payload)?;
    let repeated_communities = json_community_values(&repeated_analysis_payload)?;
    if serde_json::to_vec(&first_communities)? != serde_json::to_vec(&repeated_communities)? {
        return Err(io::Error::other(
            "repeated MCP community fields were not byte stable after TOON decoding",
        )
        .into());
    }
    assert_planted_community_values(&first_communities, "MCP")?;
    if bounded_analysis_text.len() > 65536 {
        return Err(io::Error::other(format!(
            "bounded community MCP TOON emitted {} bytes above its 65536-byte ceiling",
            bounded_analysis_text.len()
        ))
        .into());
    }
    let bounded_payload: Value = toon_format::decode_default(&bounded_analysis_text)?;
    require_json_bool(&bounded_payload, &["symbol_relations", "truncated"], true)?;
    require_json_usize(
        &bounded_payload,
        &["symbol_relations", "work", "rendered_output_bytes"],
        bounded_analysis_text.len(),
    )?;
    if bounded_payload
        .pointer("/symbol_relations/continuation")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(
            io::Error::other("bounded community MCP TOON omitted its continuation cursor").into(),
        );
    }
    let bounded_communities = json_community_values(&bounded_payload)?;
    if !bounded_communities.iter().any(|community| {
        community
            .get("coverage")
            .and_then(Value::as_str)
            .is_some_and(|coverage| coverage == "partial")
            && community
                .get("convergence")
                .and_then(Value::as_str)
                .is_some_and(|convergence| convergence == "inconclusive")
    }) || !bounded_payload
        .pointer("/symbol_relations/findings")
        .and_then(Value::as_array)
        .is_some_and(|findings| {
            findings.iter().any(|finding| {
                finding.get("kind").and_then(Value::as_str) == Some("community")
                    && finding.get("status").and_then(Value::as_str) == Some("inconclusive")
            })
        })
    {
        return Err(io::Error::other(
            "bounded MCP TOON lost the typed inconclusive community outcome",
        )
        .into());
    }
    let session_brief_has_ready_call = (session_brief_text.contains("target: atlas_file_summary")
        || session_brief_text.contains("target: atlas_symbol_relations"))
        && session_brief_text.contains("file: src/lib.rs");
    if !compact_session_brief_text
        .contains("recommended_subagent_reasoning: lowest_reliable_host_supported")
        || !compact_session_brief_text
            .contains("lowest reliable reasoning and cost tier the host supports")
    {
        return Err(io::Error::other(format!(
            "compact real MCP purpose handoff lost its reliable-tier instruction: {compact_session_brief_text}"
        ))
        .into());
    }
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
        || !stdout.contains("A V E R A G E   T O K E N S   A V O I D E D")
        || !stdout.contains("Total Tokens Avoided")
        || !stdout.contains("Average avoided")
        || !stdout.contains("Maximum avoided")
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
        "atlas_worktree_list",
        "atlas_worktree_add",
        "atlas_worktree_remove",
        "atlas_init",
        "atlas_session_brief",
        "atlas_file_summary",
        "atlas_symbol_relations",
        "atlas_slice",
        "atlas_token_report",
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
        "mcp" => require_json_array_len(payload, &["result", "tools"], 43)?,
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
            require_json_array_len(payload, &["mcp_tools"], 43)?;
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

#[cfg(windows)]
fn output_contains_path_name(output: &str, path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        output
            .replace(['\r', '\n'], "")
            .contains(name.to_string_lossy().as_ref())
    })
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
struct McpContractCleanupPacket {
    child: Child,
    stdout_reader: Option<thread::JoinHandle<()>>,
    stderr_reader: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
}

/// Reap one test-owned MCP contract packet after the observer has returned.
fn reap_mcp_contract_packet(mut packet: McpContractCleanupPacket) -> Result<(), Box<dyn Error>> {
    packet.child.stdin.take();
    if packet.child.try_wait()?.is_none() {
        packet.child.kill()?;
    }
    packet.child.wait()?;
    if let Some(reader) = packet.stdout_reader.take() {
        reader
            .join()
            .map_err(|_panic| io::Error::other("MCP contract stdout cleanup reader panicked"))?;
    }
    if let Some(reader) = packet.stderr_reader.take() {
        reader
            .join()
            .map_err(|_panic| io::Error::other("MCP contract stderr cleanup reader panicked"))??;
    }
    Ok(())
}

fn run_mcp_contract_inventory(
    executable: &Path,
    cwd: &Path,
    database: &Path,
) -> Result<String, Box<dyn Error>> {
    run_mcp_contract_inventory_with_test_delay(
        executable,
        cwd,
        database,
        Duration::from_secs(10),
        None,
        false,
    )
}

fn run_mcp_contract_inventory_with_test_delay(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
) -> Result<String, Box<dyn Error>> {
    run_mcp_contract_inventory_with_test_delay_and_kill(
        executable,
        cwd,
        database,
        timeout,
        observer_delay,
        hold_stdin_until_observation,
        &mut |child| child.kill(),
    )
}

fn run_mcp_contract_inventory_with_test_delay_and_kill(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
    kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
) -> Result<String, Box<dyn Error>> {
    run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
        executable,
        cwd,
        database,
        timeout,
        observer_delay,
        hold_stdin_until_observation,
        None,
        None,
        kill_child,
        None,
    )
}

/// Test-only variant that transfers a proven-live child and its readers to the
/// caller when injected termination cannot safely reap it here.
fn run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
    executable: &Path,
    cwd: &Path,
    database: &Path,
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
    exit_probe_error: Option<io::Error>,
    cleanup_probe_error: Option<io::Error>,
    kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
    handoff_live_child: Option<&mut dyn FnMut(McpContractCleanupPacket)>,
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
    complete_mcp_test_after_shutdown(operation_result, || {
        session.shutdown_with_test_delay_and_kill_and_handoff(
            timeout,
            observer_delay,
            hold_stdin_until_observation,
            exit_probe_error,
            cleanup_probe_error,
            kill_child,
            handoff_live_child,
        )
    })
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
    let telemetry = if telemetry_enabled { None } else { Some("1") };
    let (mut session, initialized) = McpContractSession::spawn_initialized(
        executable,
        cwd,
        database,
        &[("PROJECTATLAS_NO_TELEMETRY", telemetry)],
    )?;
    let operation_result = (|| -> Result<(Value, String), Box<dyn Error>> {
        let response = session.request(
            "tools/call",
            &serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        let stdout = format!(
            "{}\n{}\n",
            serde_json::to_string(&initialized)?,
            serde_json::to_string(&response)?
        );
        Ok((response, stdout))
    })();
    complete_mcp_test_after_shutdown(operation_result, || session.shutdown())
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
            | "file_content_classifications"
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

fn require_injected_exit_probe_failure<T>(
    result: &Result<T, Box<dyn Error>>,
    owner: &str,
    cause: &str,
    disposition: &str,
) -> Result<(), Box<dyn Error>> {
    let error = result
        .as_ref()
        .err()
        .ok_or_else(|| io::Error::other(format!("{owner} exit probe failure was not returned")))?;
    let io_error = error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other(format!("{owner} exit probe lost its io error")))?;
    let diagnostic = error.to_string();
    if io_error.kind() != io::ErrorKind::TimedOut
        || !diagnostic.contains(cause)
        || !diagnostic.contains(disposition)
    {
        return Err(io::Error::other(format!(
            "{owner} exit probe failure lost its classification, cause, or ownership disposition: {diagnostic}"
        ))
        .into());
    }
    Ok(())
}

fn require_reaped_probe_failure<T>(
    result: &Result<T, Box<dyn Error>>,
    owner: &str,
    cause: &str,
) -> Result<(), Box<dyn Error>> {
    require_injected_exit_probe_failure(result, owner, cause, ") status=")?;
    let diagnostic = result
        .as_ref()
        .err()
        .ok_or_else(|| {
            io::Error::other(format!("{owner} post-kill probe failure was not returned"))
        })?
        .to_string();
    if diagnostic.contains("cleanup incomplete") {
        return Err(io::Error::other(format!(
            "{owner} detached ownership after successful termination: {diagnostic}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn e2e_process_observers_reject_late_completion_and_preserve_in_time_success()
-> Result<(), Box<dyn Error>> {
    const OBSERVER_TIMEOUT: Duration = Duration::from_secs(2);
    const FIRST_OBSERVATION_DELAY: Duration = Duration::from_secs(3);
    const INJECTED_EXIT_PROBE_FAILURE: &str = "injected delayed-observer exit probe failure";

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let executable = mcp_contract_executable();
    run_mcp_contract_inventory_with_test_delay(
        &executable,
        &repo,
        &database,
        OBSERVER_TIMEOUT,
        None,
        false,
    )?;
    let late_shutdown = run_mcp_contract_inventory_with_test_delay(
        &executable,
        &repo,
        &database,
        OBSERVER_TIMEOUT,
        Some(FIRST_OBSERVATION_DELAY),
        false,
    );
    let late_shutdown_text = late_shutdown
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if late_shutdown.is_ok()
        || !late_shutdown_text.contains("completed after deadline")
        || late_shutdown_text.contains("still running at deadline")
    {
        return Err(io::Error::other(format!(
            "MCP session observer did not distinguish late completion from a running timeout: {late_shutdown:?}"
        ))
        .into());
    }
    let mut session_probe_kill_attempted = false;
    let mut session_probe_was_live = false;
    let session_probe_failure = run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
        &executable,
        &repo,
        &database,
        OBSERVER_TIMEOUT,
        Some(FIRST_OBSERVATION_DELAY),
        true,
        Some(io::Error::other(INJECTED_EXIT_PROBE_FAILURE)),
        None,
        &mut |child| {
            session_probe_kill_attempted = true;
            session_probe_was_live = child.try_wait()?.is_none();
            child.kill()
        },
        None,
    );
    require_injected_exit_probe_failure(
        &session_probe_failure,
        "MCP session observer",
        INJECTED_EXIT_PROBE_FAILURE,
        "cleanup complete: child reaped and readers joined",
    )?;
    if !session_probe_kill_attempted || !session_probe_was_live {
        return Err(io::Error::other(
            "MCP session synchronization failure skipped termination of a live child",
        )
        .into());
    }
    // Keep the real MCP server's owned pipe open so its stdio future is
    // causally pending when the zero-length deadline probe runs.
    let still_running_shutdown = run_mcp_contract_inventory_with_test_delay(
        &executable,
        &repo,
        &database,
        Duration::ZERO,
        None,
        true,
    );
    let still_running_shutdown_text = still_running_shutdown
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if still_running_shutdown.is_ok()
        || !still_running_shutdown_text.contains("still running at deadline")
        || !still_running_shutdown_text.contains("status=")
    {
        return Err(io::Error::other(format!(
            "MCP session observer did not terminate and reap a child still running at its deadline: {still_running_shutdown:?}"
        ))
        .into());
    }
    let mut session_cleanup_packet = None;
    let mut injected_session_kill =
        |_child: &mut Child| Err(io::Error::other("injected session kill failure"));
    let injected_session_failure = {
        let mut session_cleanup_handoff = |packet: McpContractCleanupPacket| {
            session_cleanup_packet = Some(packet);
        };
        run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
            &executable,
            &repo,
            &database,
            Duration::ZERO,
            None,
            true,
            None,
            None,
            &mut injected_session_kill,
            Some(&mut session_cleanup_handoff),
        )
    };
    let injected_session_error = injected_session_failure
        .as_ref()
        .err()
        .ok_or_else(|| io::Error::other("injected session kill failure was not returned"))?;
    let injected_session_io_error = injected_session_error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other("injected session failure lost its io error"))?;
    if injected_session_io_error.kind() != io::ErrorKind::TimedOut
        || !injected_session_error
            .to_string()
            .contains("still running at deadline")
        || !injected_session_error
            .to_string()
            .contains("injected session kill failure")
        || !injected_session_error
            .to_string()
            .contains("cleanup incomplete: operating system refused termination")
    {
        return Err(io::Error::other(format!(
            "MCP session observer did not preserve timeout classification for an injected kill failure: {injected_session_failure:?}"
        ))
        .into());
    }
    let mut session_cleanup_packet = session_cleanup_packet
        .take()
        .ok_or_else(|| io::Error::other("session cleanup was not synchronously transferred"))?;
    let (session_stdout_reader, session_stderr_reader) = match (
        session_cleanup_packet.stdout_reader.take(),
        session_cleanup_packet.stderr_reader.take(),
    ) {
        (Some(stdout_reader), Some(stderr_reader)) => (stdout_reader, stderr_reader),
        (stdout_reader, stderr_reader) => {
            reap_mcp_contract_packet(McpContractCleanupPacket {
                child: session_cleanup_packet.child,
                stdout_reader,
                stderr_reader,
            })?;
            return Err(io::Error::other("session readers were not transferred").into());
        }
    };
    let mut session_child = session_cleanup_packet.child;
    drop(session_child.stdin.take());
    if session_child.try_wait()?.is_none() {
        session_child.kill()?;
    }
    session_child.wait()?;
    session_stdout_reader
        .join()
        .map_err(|_panic| io::Error::other("session stdout cleanup reader panicked"))?;
    session_stderr_reader
        .join()
        .map_err(|_panic| io::Error::other("session stderr cleanup reader panicked"))??;
    let args = vec![
        "--db".to_string(),
        database.display().to_string(),
        "mcp".to_string(),
    ];
    let messages = vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "e2e-process-observer-deadlines", "version": "0.1.0"}
            }
        })
        .to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        })
        .to_string(),
    ];
    let in_time_mcp = run_mcp_stdio_with_env_and_test_delay(
        &executable,
        &repo,
        &args,
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        OBSERVER_TIMEOUT,
        None,
        false,
    )?;
    if mcp_response(&in_time_mcp, 1)?.get("result").is_none() {
        return Err(io::Error::other("in-time MCP observer omitted initialize result").into());
    }
    let late_mcp = run_mcp_stdio_with_env_and_test_delay(
        &executable,
        &repo,
        &args,
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        OBSERVER_TIMEOUT,
        Some(FIRST_OBSERVATION_DELAY),
        false,
    );
    let late_mcp_text = late_mcp
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if late_mcp.is_ok()
        || !late_mcp_text.contains("completed after deadline")
        || late_mcp_text.contains("still running at deadline")
    {
        return Err(io::Error::other(format!(
            "MCP stdio observer did not distinguish late completion from a running timeout: {late_mcp:?}"
        ))
        .into());
    }
    let mut stdio_probe_kill_attempted = false;
    let mut stdio_probe_was_live = false;
    let stdio_probe_failure = run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
        &executable,
        &repo,
        &args,
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        OBSERVER_TIMEOUT,
        Some(FIRST_OBSERVATION_DELAY),
        true,
        Some(io::Error::other(INJECTED_EXIT_PROBE_FAILURE)),
        None,
        &mut |child| {
            stdio_probe_kill_attempted = true;
            stdio_probe_was_live = child.try_wait()?.is_none();
            child.kill()
        },
        None,
    );
    require_injected_exit_probe_failure(
        &stdio_probe_failure,
        "MCP stdio observer",
        INJECTED_EXIT_PROBE_FAILURE,
        "cleanup complete: child reaped and readers joined",
    )?;
    if !stdio_probe_kill_attempted || !stdio_probe_was_live {
        return Err(io::Error::other(
            "MCP stdio synchronization failure skipped termination of a live child",
        )
        .into());
    }
    let still_running_mcp = run_mcp_stdio_with_env_and_test_delay(
        &executable,
        &repo,
        &args,
        &messages,
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        Duration::ZERO,
        None,
        true,
    );
    let still_running_mcp_text = still_running_mcp
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if still_running_mcp.is_ok()
        || !still_running_mcp_text.contains("still running at deadline")
        || !still_running_mcp_text.contains("status=")
    {
        return Err(io::Error::other(format!(
            "MCP stdio observer did not terminate and reap a child still running at its deadline: {still_running_mcp:?}"
        ))
        .into());
    }
    let mut stdio_cleanup_packet = None;
    let mut injected_stdio_kill =
        |_child: &mut Child| Err(io::Error::other("injected stdio kill failure"));
    let injected_stdio_failure = {
        let mut stdio_cleanup_handoff =
            |child: Child,
             stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
             stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>| {
                stdio_cleanup_packet = Some(McpStdioCleanupPacket {
                    child,
                    stdout_reader,
                    stderr_reader,
                });
            };
        run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
            &executable,
            &repo,
            &args,
            &messages,
            &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
            Duration::ZERO,
            None,
            true,
            None,
            None,
            &mut injected_stdio_kill,
            Some(&mut stdio_cleanup_handoff),
        )
    };
    let injected_stdio_error = injected_stdio_failure
        .as_ref()
        .err()
        .ok_or_else(|| io::Error::other("injected stdio kill failure was not returned"))?;
    let injected_stdio_io_error = injected_stdio_error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other("injected stdio failure lost its io error"))?;
    if injected_stdio_io_error.kind() != io::ErrorKind::TimedOut
        || !injected_stdio_error
            .to_string()
            .contains("still running at deadline")
        || !injected_stdio_error
            .to_string()
            .contains("injected stdio kill failure")
        || !injected_stdio_error
            .to_string()
            .contains("cleanup incomplete: operating system refused termination")
    {
        return Err(io::Error::other(format!(
            "MCP stdio observer did not preserve timeout classification for an injected kill failure: {injected_stdio_failure:?}"
        ))
        .into());
    }
    reap_mcp_stdio_packet(
        stdio_cleanup_packet
            .take()
            .ok_or_else(|| io::Error::other("stdio cleanup was not synchronously transferred"))?,
    )?;

    let in_time_installer = wait_for_plugin_installer_output_with_test_delay(
        StdCommand::new(&executable)
            .current_dir(&repo)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
        "in-time",
        OBSERVER_TIMEOUT,
        None,
    )?;
    if !in_time_installer.status.success() {
        return Err(io::Error::other("in-time installer observer rejected --version").into());
    }
    let late_installer = wait_for_plugin_installer_output_with_test_delay(
        StdCommand::new(&executable)
            .current_dir(&repo)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
        "late",
        OBSERVER_TIMEOUT,
        Some(FIRST_OBSERVATION_DELAY),
    );
    let late_installer_text = late_installer
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if late_installer.is_ok()
        || !late_installer_text.contains("completed after deadline")
        || late_installer_text.contains("still running at deadline")
    {
        return Err(io::Error::other(format!(
            "installer observer did not distinguish late completion from a running timeout: {late_installer:?}"
        ))
        .into());
    }
    let mut installer_probe_kill_attempted = false;
    let mut installer_probe_was_live = false;
    let installer_probe_failure =
        wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
            StdCommand::new(&executable)
                .current_dir(&repo)
                .arg("--db")
                .arg(&database)
                .arg("mcp")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
            "probe-failure",
            OBSERVER_TIMEOUT,
            Some(FIRST_OBSERVATION_DELAY),
            Some(io::Error::other(INJECTED_EXIT_PROBE_FAILURE)),
            None,
            &mut |child| {
                installer_probe_kill_attempted = true;
                installer_probe_was_live = child.try_wait()?.is_none();
                child.kill()
            },
            None,
        );
    require_injected_exit_probe_failure(
        &installer_probe_failure,
        "installer observer",
        INJECTED_EXIT_PROBE_FAILURE,
        "cleanup complete: child reaped and output drained",
    )?;
    if !installer_probe_kill_attempted || !installer_probe_was_live {
        return Err(io::Error::other(
            "installer synchronization failure skipped termination of a live child",
        )
        .into());
    }
    // The held stdio pipe keeps this real MCP child running until the observer
    // kills this exact process and wait_with_output drains both streams.
    let still_running_installer = wait_for_plugin_installer_output_with_test_delay(
        StdCommand::new(&executable)
            .current_dir(&repo)
            .arg("--db")
            .arg(&database)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
        "still-running",
        Duration::ZERO,
        None,
    );
    let still_running_installer_text = still_running_installer
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if still_running_installer.is_ok()
        || !still_running_installer_text.contains("still running at deadline")
        || !still_running_installer_text.contains("status=")
        || !still_running_installer_text.contains("stdout:")
        || !still_running_installer_text.contains("stderr:")
    {
        return Err(io::Error::other(format!(
            "installer observer did not terminate and drain a child still running at its deadline: {still_running_installer:?}"
        ))
        .into());
    }
    let mut installer_cleanup_child = None;
    let mut injected_installer_kill =
        |_child: &mut Child| Err(io::Error::other("injected installer kill failure"));
    let injected_installer_failure = {
        let mut installer_cleanup_handoff = |child: Child| {
            installer_cleanup_child = Some(child);
        };
        wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
            StdCommand::new(&executable)
                .current_dir(&repo)
                .arg("--db")
                .arg(&database)
                .arg("mcp")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
            "injected-installer",
            Duration::ZERO,
            None,
            None,
            None,
            &mut injected_installer_kill,
            Some(&mut installer_cleanup_handoff),
        )
    };
    let injected_installer_error = injected_installer_failure
        .as_ref()
        .err()
        .ok_or_else(|| io::Error::other("injected installer kill failure was not returned"))?;
    let injected_installer_io_error = injected_installer_error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other("injected installer failure lost its io error"))?;
    if injected_installer_io_error.kind() != io::ErrorKind::TimedOut
        || !injected_installer_error
            .to_string()
            .contains("still running at deadline")
        || !injected_installer_error
            .to_string()
            .contains("injected installer kill failure")
        || !injected_installer_error
            .to_string()
            .contains("cleanup incomplete: operating system refused termination")
    {
        return Err(io::Error::other(format!(
            "installer observer did not preserve timeout classification for an injected kill failure: {injected_installer_failure:?}"
        ))
        .into());
    }
    let mut installer_child = installer_cleanup_child
        .take()
        .ok_or_else(|| io::Error::other("installer cleanup was not synchronously transferred"))?;
    drop(installer_child.stdin.take());
    if installer_child.try_wait()?.is_none() {
        installer_child.kill()?;
    }
    // The real child may have consumed EOF and exited gracefully before this
    // test-owned cleanup probe runs. Reaping and draining it is the proof; its
    // final status is not part of the injected kill-failure contract.
    reap_plugin_installer_child(installer_child)?;
    Ok(())
}

#[test]
fn e2e_process_observers_reap_after_successful_kill_when_reprobe_fails()
-> Result<(), Box<dyn Error>> {
    const INJECTED_REPROBE_FAILURE: &str = "injected post-kill exit probe failure";

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let executable = mcp_contract_executable();

    let mut session_handoff_packet = None;
    let session_result = {
        let mut handoff = |packet: McpContractCleanupPacket| {
            session_handoff_packet = Some(packet);
        };
        run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
            &executable,
            &repo,
            &database,
            Duration::ZERO,
            None,
            true,
            Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
            None,
            &mut |child| child.kill(),
            Some(&mut handoff),
        )
    };
    require_reaped_probe_failure(
        &session_result,
        "MCP session observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if let Some(packet) = session_handoff_packet {
        reap_mcp_contract_packet(packet)?;
        return Err(io::Error::other(
            "MCP session detached after a successful injected termination",
        )
        .into());
    }

    let args = vec![
        "--db".to_string(),
        database.display().to_string(),
        "mcp".to_string(),
    ];
    let mut stdio_handoff_packet = None;
    let stdio_result = {
        let mut handoff =
            |child: Child,
             stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
             stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>| {
                stdio_handoff_packet = Some(McpStdioCleanupPacket {
                    child,
                    stdout_reader,
                    stderr_reader,
                });
            };
        run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
            &executable,
            &repo,
            &args,
            &[] as &[String],
            &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
            Duration::ZERO,
            None,
            true,
            Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
            None,
            &mut |child| child.kill(),
            Some(&mut handoff),
        )
    };
    require_reaped_probe_failure(
        &stdio_result,
        "MCP stdio observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if let Some(packet) = stdio_handoff_packet {
        reap_mcp_stdio_packet(packet)?;
        return Err(
            io::Error::other("MCP stdio detached after a successful injected termination").into(),
        );
    }

    let mut installer_handoff_child = None;
    let installer_result = {
        let mut handoff = |child: Child| {
            installer_handoff_child = Some(child);
        };
        wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
            StdCommand::new(&executable)
                .current_dir(&repo)
                .arg("--db")
                .arg(&database)
                .arg("mcp")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
            "installer post-kill probe",
            Duration::ZERO,
            None,
            Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
            None,
            &mut |child| child.kill(),
            Some(&mut handoff),
        )
    };
    require_reaped_probe_failure(
        &installer_result,
        "installer observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if let Some(child) = installer_handoff_child {
        reap_plugin_installer_child(child)?;
        return Err(
            io::Error::other("installer detached after a successful injected termination").into(),
        );
    }
    Ok(())
}

#[test]
fn e2e_process_observers_attempt_termination_when_cleanup_reprobe_fails()
-> Result<(), Box<dyn Error>> {
    const INJECTED_REPROBE_FAILURE: &str = "injected pre-termination exit probe failure";

    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    let database = atlas_dir.join("projectatlas.db");
    let executable = mcp_contract_executable();

    let mut session_kill_attempted = false;
    let mut session_was_live = false;
    let session_result = run_mcp_contract_inventory_with_test_delay_and_kill_and_handoff(
        &executable,
        &repo,
        &database,
        Duration::ZERO,
        None,
        true,
        None,
        Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
        &mut |child| {
            session_kill_attempted = true;
            session_was_live = child.try_wait()?.is_none();
            child.kill()
        },
        None,
    );
    require_reaped_probe_failure(
        &session_result,
        "MCP session observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if !session_kill_attempted || !session_was_live {
        return Err(io::Error::other(
            "MCP session cleanup probe failure skipped termination of a live child",
        )
        .into());
    }

    let args = vec![
        "--db".to_string(),
        database.display().to_string(),
        "mcp".to_string(),
    ];
    let mut stdio_kill_attempted = false;
    let mut stdio_was_live = false;
    let stdio_result = run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
        &executable,
        &repo,
        &args,
        &[] as &[String],
        &[("PROJECTATLAS_NO_TELEMETRY", Some("1"))],
        Duration::ZERO,
        None,
        true,
        None,
        Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
        &mut |child| {
            stdio_kill_attempted = true;
            stdio_was_live = child.try_wait()?.is_none();
            child.kill()
        },
        None,
    );
    require_reaped_probe_failure(
        &stdio_result,
        "MCP stdio observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if !stdio_kill_attempted || !stdio_was_live {
        return Err(io::Error::other(
            "MCP stdio cleanup probe failure skipped termination of a live child",
        )
        .into());
    }

    let mut installer_kill_attempted = false;
    let mut installer_was_live = false;
    let installer_result = wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
        StdCommand::new(&executable)
            .current_dir(&repo)
            .arg("--db")
            .arg(&database)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
        "installer pre-termination probe",
        Duration::ZERO,
        None,
        None,
        Some(io::Error::other(INJECTED_REPROBE_FAILURE)),
        &mut |child| {
            installer_kill_attempted = true;
            installer_was_live = child.try_wait()?.is_none();
            child.kill()
        },
        None,
    );
    require_reaped_probe_failure(
        &installer_result,
        "installer observer",
        INJECTED_REPROBE_FAILURE,
    )?;
    if !installer_kill_attempted || !installer_was_live {
        return Err(io::Error::other(
            "installer cleanup probe failure skipped termination of a live child",
        )
        .into());
    }
    Ok(())
}

#[test]
fn mcp_contract_shutdown_disconnects_saturated_responses_before_reader_join()
-> Result<(), Box<dyn Error>> {
    let executable = mcp_contract_executable();
    let mut child = StdCommand::new(&executable)
        .arg("--version")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("saturated-response fixture stdin was not piped"))?;
    let (sender, responses) =
        mpsc::sync_channel::<io::Result<String>>(MCP_CONTRACT_RESPONSE_CAPACITY);
    let (state_sender, state_receiver) = mpsc::channel();
    let stdout_reader = thread::spawn(move || {
        for id in 0..MCP_CONTRACT_RESPONSE_CAPACITY {
            if sender
                .send(Ok(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                })
                .to_string()))
                .is_err()
            {
                return;
            }
        }
        if state_sender.send("saturated").is_err() {
            return;
        }
        let disconnected = sender
            .send(Ok(serde_json::json!({
                "jsonrpc": "2.0",
                "id": MCP_CONTRACT_RESPONSE_CAPACITY,
                "result": {}
            })
            .to_string()))
            .is_err();
        let _terminal_state = state_sender.send(if disconnected {
            "disconnected"
        } else {
            "accepted"
        });
    });
    let stderr_reader = thread::spawn(|| Ok(Vec::new()));
    let session = McpContractSession {
        child: Some(child),
        stdin: Some(stdin),
        responses,
        stdout_reader: Some(stdout_reader),
        stderr_reader: Some(stderr_reader),
        next_request_id: 1,
    };
    let state = state_receiver.recv_timeout(Duration::from_secs(10))?;
    if state != "saturated" {
        return Err(io::Error::other(format!(
            "response reader did not fill the bounded channel: {state}"
        ))
        .into());
    }

    let shutdown = session.shutdown_with_test_delay(Duration::ZERO, None, false);
    let error = shutdown
        .as_ref()
        .err()
        .ok_or_else(|| io::Error::other("saturated response shutdown was not timed out"))?;
    let io_error = error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other("saturated response shutdown lost its io error"))?;
    if io_error.kind() != io::ErrorKind::TimedOut {
        return Err(io::Error::other(format!(
            "saturated response shutdown was not TimedOut: {error}"
        ))
        .into());
    }
    let terminal_state = state_receiver.try_recv()?;
    if terminal_state != "disconnected" {
        return Err(io::Error::other(format!(
            "stdout reader did not observe response disconnection: {terminal_state}"
        ))
        .into());
    }
    Ok(())
}

const MCP_CONTRACT_RESPONSE_CAPACITY: usize = 64;

/// Persistent real MCP session used by E2E contract clients.
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
        let (sender, responses) = mpsc::sync_channel(MCP_CONTRACT_RESPONSE_CAPACITY);
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

    /// Disconnect the bounded stdout sender before joining its reader.
    fn disconnect_responses(&mut self) {
        let (_sender, disconnected) = mpsc::channel();
        self.responses = disconnected;
    }

    /// Close stdin and require a clean bounded process exit.
    fn shutdown(self) -> Result<(), Box<dyn Error>> {
        self.shutdown_with_test_delay(Duration::from_secs(10), None, false)
    }

    fn shutdown_with_test_delay(
        self,
        timeout: Duration,
        observer_delay: Option<Duration>,
        hold_stdin_until_observation: bool,
    ) -> Result<(), Box<dyn Error>> {
        self.shutdown_with_test_delay_and_kill(
            timeout,
            observer_delay,
            hold_stdin_until_observation,
            &mut |child| child.kill(),
        )
    }

    fn shutdown_with_test_delay_and_kill(
        self,
        timeout: Duration,
        observer_delay: Option<Duration>,
        hold_stdin_until_observation: bool,
        kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
    ) -> Result<(), Box<dyn Error>> {
        self.shutdown_with_test_delay_and_kill_and_handoff(
            timeout,
            observer_delay,
            hold_stdin_until_observation,
            None,
            None,
            kill_child,
            None,
        )
    }

    /// Test-only seam for transferring a proven-live child and its readers.
    fn shutdown_with_test_delay_and_kill_and_handoff(
        mut self,
        timeout: Duration,
        observer_delay: Option<Duration>,
        hold_stdin_until_observation: bool,
        exit_probe_error: Option<io::Error>,
        cleanup_probe_error: Option<io::Error>,
        kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
        handoff_live_child: Option<&mut dyn FnMut(McpContractCleanupPacket)>,
    ) -> Result<(), Box<dyn Error>> {
        let mut exit_probe_error = exit_probe_error;
        let mut cleanup_probe_error = cleanup_probe_error;
        if !hold_stdin_until_observation {
            self.stdin.take();
        }
        if observer_delay.is_some() && (!hold_stdin_until_observation || exit_probe_error.is_some())
        {
            let synchronization = self.child.as_mut().map_or_else(
                || Err(io::Error::other("MCP contract child was consumed")),
                |child| {
                    synchronize_prompt_exit_before_delayed_observation(
                        child,
                        "MCP contract server",
                        exit_probe_error.take(),
                    )
                },
            );
            if let Err(error) = synchronization {
                let stdout_reader = self.stdout_reader.take();
                let stderr_reader = self.stderr_reader.take();
                let mut child = self
                    .child
                    .take()
                    .ok_or_else(|| io::Error::other("MCP contract child was consumed"))?;
                let kill_result = kill_child(&mut child);
                self.stdin.take();
                let status_after_kill = child.try_wait();
                if kill_result.is_err() && !matches!(&status_after_kill, Ok(Some(_))) {
                    let packet = McpContractCleanupPacket {
                        child,
                        stdout_reader,
                        stderr_reader,
                    };
                    if let Some(handoff) = handoff_live_child {
                        handoff(packet);
                    } else {
                        drop(packet);
                    }
                    let mut diagnostic = format!(
                        "MCP contract server exit synchronization failed before delayed observation: {error}; cleanup incomplete: child/readers detached"
                    );
                    if let Some(kill_error) = kill_result.as_ref().err() {
                        diagnostic.push_str("; termination failed: ");
                        diagnostic.push_str(&kill_error.to_string());
                    }
                    if let Err(probe_error) = status_after_kill {
                        diagnostic.push_str("; re-probe failed after termination attempt: ");
                        diagnostic.push_str(&probe_error.to_string());
                    }
                    return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                }
                let status = child.wait()?;
                self.disconnect_responses();
                if let Some(reader) = stdout_reader {
                    reader.join().map_err(|_panic| {
                        io::Error::other("MCP contract stdout reader panicked")
                    })?;
                }
                if let Some(reader) = stderr_reader {
                    reader.join().map_err(|_panic| {
                        io::Error::other("MCP contract stderr reader panicked")
                    })??;
                }
                let diagnostic = format!(
                    "MCP contract server exit synchronization failed before delayed observation: {error}; cleanup complete: child reaped and readers joined status={status}"
                );
                return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
            }
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("MCP contract shutdown deadline overflowed"))?;
        if let Some(delay) = observer_delay {
            thread::sleep(delay);
        }
        let mut timeout_reason = None;
        let mut accepted_completion = false;
        loop {
            if Instant::now() >= deadline {
                timeout_reason = Some("still running at deadline".to_string());
                break;
            }
            let (status, observed_at) = {
                let child = self
                    .child
                    .as_mut()
                    .ok_or_else(|| io::Error::other("MCP contract child was consumed"))?;
                let status = child.try_wait()?;
                let observed_at = Instant::now();
                (status, observed_at)
            };
            match status {
                Some(_) if observed_at < deadline => {
                    accepted_completion = true;
                    break;
                }
                Some(_) => {
                    timeout_reason = Some(format!(
                        "completed after deadline (observed_at={observed_at:?})"
                    ));
                    break;
                }
                None => {
                    let remaining = deadline.saturating_duration_since(observed_at);
                    if remaining.is_zero() {
                        timeout_reason = Some("still running at deadline".to_string());
                        break;
                    }
                    thread::sleep(Duration::from_millis(25).min(remaining));
                }
            }
        }

        let mut child = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("MCP contract child was consumed"))?;
        let mut pre_termination_probe_error = None;
        let mut post_termination_probe_error = None;
        if timeout_reason.is_some() {
            let (status, observed_at) = {
                let status = match cleanup_probe_error.take() {
                    Some(error) => {
                        pre_termination_probe_error = Some(error);
                        None
                    }
                    None => match child.try_wait() {
                        Ok(status) => status,
                        Err(error) => {
                            pre_termination_probe_error = Some(error);
                            None
                        }
                    },
                };
                let observed_at = Instant::now();
                (status, observed_at)
            };
            if status.is_none() {
                let kill_result = kill_child(&mut child);
                let status_after_kill = match exit_probe_error.take() {
                    Some(error) => Err(error),
                    None => child.try_wait(),
                };
                let status_after_kill = match status_after_kill {
                    Ok(status) => status,
                    Err(error) if kill_result.is_ok() => {
                        post_termination_probe_error = Some(error);
                        None
                    }
                    Err(error) => {
                        self.stdin.take();
                        let packet = McpContractCleanupPacket {
                            child,
                            stdout_reader: self.stdout_reader.take(),
                            stderr_reader: self.stderr_reader.take(),
                        };
                        if let Some(handoff) = handoff_live_child {
                            handoff(packet);
                        } else {
                            drop(packet);
                        }
                        let mut diagnostic = format!(
                            "MCP contract server did not exit after stdin closed: {} status=unknown (re-probe failed after termination attempt: {error}; cleanup incomplete: child/readers detached)",
                            timeout_reason.as_deref().unwrap_or("timeout")
                        );
                        if let Some(kill_error) = kill_result.as_ref().err() {
                            diagnostic.push_str("; termination failed: ");
                            diagnostic.push_str(&kill_error.to_string());
                        }
                        return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                    }
                };
                if let Err(error) = kill_result
                    && status_after_kill.is_none()
                {
                    self.stdin.take();
                    let packet = McpContractCleanupPacket {
                        child,
                        stdout_reader: self.stdout_reader.take(),
                        stderr_reader: self.stderr_reader.take(),
                    };
                    if let Some(handoff) = handoff_live_child {
                        handoff(packet);
                    } else {
                        drop(packet);
                    }
                    let diagnostic = format!(
                        "MCP contract server did not exit after stdin closed: {} status=still-running at deadline (termination failed: {error}; cleanup incomplete: operating system refused termination; child was not reaped; child/readers detached)",
                        timeout_reason.as_deref().unwrap_or("timeout")
                    );
                    return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                }
            } else if timeout_reason.as_deref() == Some("still running at deadline") {
                timeout_reason = Some(format!(
                    "completed after deadline (observed_at={observed_at:?})"
                ));
            }
        }
        self.stdin.take();
        let wait_result = child.wait();
        self.disconnect_responses();
        let stdout_result = self.stdout_reader.take().map(|reader| {
            reader
                .join()
                .map_err(|_panic| io::Error::other("MCP contract stdout reader panicked"))
        });
        let stderr_result = self
            .stderr_reader
            .take()
            .ok_or_else(|| io::Error::other("MCP contract stderr reader was consumed"))?
            .join()
            .map_err(|_panic| io::Error::other("MCP contract stderr reader panicked"))??;
        if let Some(reason) = timeout_reason {
            let mut diagnostic =
                format!("MCP contract server did not exit after stdin closed: {reason}");
            if let Some(error) = pre_termination_probe_error {
                diagnostic
                    .push_str(" status=unknown (re-probe failed before termination attempt: ");
                diagnostic.push_str(&error.to_string());
                diagnostic.push(')');
            }
            if let Some(error) = post_termination_probe_error {
                diagnostic
                    .push_str(" status=unknown (re-probe failed after successful termination: ");
                diagnostic.push_str(&error.to_string());
                diagnostic.push(')');
            }
            if let Ok(status) = &wait_result {
                diagnostic.push_str(" status=");
                diagnostic.push_str(&status.to_string());
            }
            if !stderr_result.is_empty() {
                diagnostic.push_str(" stderr=");
                diagnostic.push_str(&String::from_utf8_lossy(&stderr_result));
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
        }
        let status = wait_result?;
        stdout_result.transpose()?;
        if !accepted_completion || !status.success() {
            return Err(io::Error::other(format!(
                "MCP contract server failed: {}",
                String::from_utf8_lossy(&stderr_result)
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
        self.disconnect_responses();
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

#[test]
fn codex_schema_audit_rejects_every_definition_and_reference_form() {
    for schema in [
        serde_json::json!({"$ref": "#/properties/value"}),
        serde_json::json!({"items": {"$ref": "#/definitions/Value"}}),
        serde_json::json!({"$ref": "https://example.invalid/schema.json"}),
        serde_json::json!({"$defs": {"Value": {"type": "string"}}}),
        serde_json::json!({"definitions": {"Value": {"type": "string"}}}),
    ] {
        assert!(assert_self_contained_input_schema("fixture", &schema).is_err());
    }
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

/// Capture durable bytes, schema objects, and all logical rows without checkpointing.
fn sqlite_compatibility_snapshot(
    database: &Path,
) -> Result<SqliteCompatibilitySnapshot, Box<dyn Error>> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let schema_objects = {
        let mut statement = connection.prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '')
             FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )?;
        statement
            .query_map([], |row| {
                Ok(format!(
                    "{}\0{}\0{}\0{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let tables = sqlite_table_digests(&connection)?;
    drop(connection);
    let wal_path = sqlite_sidecar_path(database, "-wal");
    let mut sidecars = BTreeSet::new();
    for suffix in ["-wal", "-shm"] {
        if sqlite_sidecar_path(database, suffix).is_file() {
            sidecars.insert(suffix.to_string());
        }
    }
    Ok(SqliteCompatibilitySnapshot {
        database_bytes: fs::read(database)?,
        wal_bytes: wal_path.is_file().then(|| fs::read(wal_path)).transpose()?,
        sidecars,
        schema_objects,
        tables,
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
        McpSqliteEffect::WorktreeRegistryAdvance => {
            if authoritative != BTreeSet::from(["worktree_registrations".to_string()])
                || !usage.is_empty()
                || before.authored_purposes != after.authored_purposes
                || before.generation != after.generation
                || before.purpose_revision != after.purpose_revision
            {
                return Err(io::Error::other(format!(
                    "{name} escaped worktree-registry ownership: authoritative={authoritative:?} usage={usage:?}"
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
        "atlas_worktree_list" => {
            require_json_string(decoded, &["worktrees", "control_alias"], "main")?;
            require_json_usize_at_least(decoded, &["worktrees", "total_worktrees"], 2)?;
            require_json_bool(decoded, &["worktrees", "truncated"], false)?;
            require_json_array_len(decoded, &["worktrees", "worktrees"], 2)?;
        }
        "atlas_worktree_add" => {
            require_json_string(decoded, &["worktree", "operation"], "add")?;
            require_json_string(decoded, &["worktree", "status"], "registered")?;
            require_json_string(decoded, &["worktree", "alias"], "contract-linked")?;
            require_json_bool(decoded, &["worktree", "git_unchanged"], true)?;
            require_json_bool(decoded, &["worktree", "files_unchanged"], true)?;
        }
        "atlas_worktree_remove" => {
            require_json_string(decoded, &["worktree", "operation"], "remove")?;
            require_json_string(decoded, &["worktree", "status"], "retired")?;
            require_json_string(decoded, &["worktree", "alias"], "contract-linked")?;
            require_json_bool(decoded, &["worktree", "git_unchanged"], true)?;
            require_json_bool(decoded, &["worktree", "files_unchanged"], true)?;
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
            let tokens_avoided = json_at(decoded, &["token_savings", "tokens_avoided"])?
                .as_i64()
                .ok_or_else(|| {
                    io::Error::other("token_savings.tokens_avoided was not an integer")
                })?;
            let average_tokens = json_at(decoded, &["token_savings", "average_tokens_avoided"])?
                .as_i64()
                .ok_or_else(|| {
                    io::Error::other("token_savings.average_tokens_avoided was not an integer")
                })?;
            json_at(decoded, &["token_savings", "maximum_tokens_avoided"])?
                .as_i64()
                .ok_or_else(|| {
                    io::Error::other("token_savings.maximum_tokens_avoided was not an integer")
                })?;
            if tokens_avoided != average_tokens {
                return Err(io::Error::other(
                    "MCP token compatibility alias did not match the average",
                )
                .into());
            }
            require_json_usize(
                decoded,
                &[
                    "token_savings",
                    "average_policy",
                    "directory_walk_baseline_percent",
                ],
                50,
            )?;
            require_json_string(
                decoded,
                &["token_savings", "average_policy", "evidence"],
                "fixed_policy_estimate_not_benchmark_or_provider_measurement",
            )?;
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
            require_json_array_len(decoded, &["runtime", "mcp_tools"], 43)?;
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

/// Return one uniquely named step from a GitHub Actions job.
fn workflow_job_step(workflow: &str, job: &str, step_name: &str) -> Result<Yaml, Box<dyn Error>> {
    let documents = YamlLoader::load_from_str(workflow)?;
    let document = documents
        .first()
        .ok_or_else(|| io::Error::other("workflow document is empty"))?;
    let steps = document["jobs"][job]["steps"]
        .as_vec()
        .ok_or_else(|| io::Error::other(format!("workflow job {job:?} has no steps")))?;
    let mut found = None;
    for step in steps {
        if step["name"].as_str() != Some(step_name) {
            continue;
        }
        if found.is_some() {
            return Err(io::Error::other(format!(
                "workflow job {job:?} has duplicate step {step_name:?}"
            ))
            .into());
        }
        found = Some(step.clone());
    }
    found.ok_or_else(|| {
        io::Error::other(format!("workflow job {job:?} omitted step {step_name:?}")).into()
    })
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

/// Serialize Windows installer tests while their release-asset server is active.
#[cfg(windows)]
fn lock_windows_release_asset_tests() -> std::sync::MutexGuard<'static, ()> {
    match WINDOWS_RELEASE_ASSET_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Start one owned `PowerShell` process that holds an exclusive read lock.
#[cfg(windows)]
fn spawn_exclusive_file_lock(path: &Path) -> Result<Child, Box<dyn Error>> {
    let mut child = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(
            "$ErrorActionPreference='Stop'; $stream=[System.IO.File]::Open($env:PROJECTATLAS_TEST_LOCK_PATH,[System.IO.FileMode]::Open,[System.IO.FileAccess]::Read,[System.IO.FileShare]::None); [Console]::Out.WriteLine('locked'); [Console]::Out.Flush(); try { Start-Sleep -Seconds 300 } finally { $stream.Dispose() }",
        )
        .env("PROJECTATLAS_TEST_LOCK_PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("exclusive lock process stdout missing"))?;
    let mut ready = String::new();
    BufReader::new(stdout).read_line(&mut ready)?;
    if ready.trim() != "locked" {
        drop(child.kill());
        drop(child.wait());
        return Err(io::Error::other("exclusive lock process did not become ready").into());
    }
    Ok(child)
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

#[cfg(windows)]
fn compile_codex_mcp_owner_fixture(output: &Path) -> Result<(), Box<dyn Error>> {
    let source = output.with_extension("cs");
    fs::write(&source, CODEX_MCP_OWNER_FIXTURE_SOURCE)?;
    let compile_output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(
            "Add-Type -Path $env:PROJECTATLAS_FIXTURE_SOURCE -OutputAssembly $env:PROJECTATLAS_FIXTURE_RUNTIME -OutputType ConsoleApplication",
        )
        .env("PROJECTATLAS_FIXTURE_SOURCE", &source)
        .env("PROJECTATLAS_FIXTURE_RUNTIME", output)
        .output()?;
    if !compile_output.status.success() {
        return Err(io::Error::other(format!(
            "failed to compile Codex MCP owner fixture:\n{}",
            String::from_utf8_lossy(&compile_output.stderr)
        ))
        .into());
    }
    Ok(())
}

#[cfg(windows)]
fn compile_obsolete_projectatlas_fixture(output: &Path) -> Result<(), Box<dyn Error>> {
    let source = output.with_extension("cs");
    fs::write(
        &source,
        r#"using System;
using System.Threading;

public static class Program
{
    public static int Main(string[] arguments)
    {
        if (Array.IndexOf(arguments, "mcp") >= 0)
        {
            Thread.Sleep(Timeout.Infinite);
            return 0;
        }
        return 2;
    }
}
"#,
    )?;
    let compile_output = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(
            "Add-Type -Path $env:PROJECTATLAS_FIXTURE_SOURCE -OutputAssembly $env:PROJECTATLAS_FIXTURE_RUNTIME -OutputType ConsoleApplication",
        )
        .env("PROJECTATLAS_FIXTURE_SOURCE", &source)
        .env("PROJECTATLAS_FIXTURE_RUNTIME", output)
        .output()?;
    if !compile_output.status.success() {
        return Err(io::Error::other(format!(
            "failed to compile obsolete ProjectAtlas fixture runtime:\n{}",
            String::from_utf8_lossy(&compile_output.stderr)
        ))
        .into());
    }
    Ok(())
}

#[cfg(windows)]
fn write_installer_with_test_codex_identity_seam(
    installer: &Path,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(installer)?;
    let identity_start = source
        .find("function Get-ProjectAtlasCodexImageIdentity {")
        .ok_or_else(|| io::Error::other("installer Codex identity function missing"))?;
    let identity_end = source[identity_start..]
        .find("function Find-ProjectAtlasObsoleteStableMcpProcess {")
        .map(|offset| identity_start + offset)
        .ok_or_else(|| io::Error::other("installer obsolete MCP finder missing"))?;
    let test_identity = r"function Get-ProjectAtlasCodexImageIdentity {
    param([string]$FilePath)
    if ([string]::IsNullOrWhiteSpace($env:PROJECTATLAS_TEST_CODEX_OWNER) `
        -or -not [string]::Equals(
            [System.IO.Path]::GetFullPath($FilePath),
            [System.IO.Path]::GetFullPath($env:PROJECTATLAS_TEST_CODEX_OWNER),
            [System.StringComparison]::OrdinalIgnoreCase)) {
        return $null
    }
    return Get-ProjectAtlasRuntimeImageSha256 $FilePath
}

";
    let mut isolated = String::with_capacity(source.len());
    isolated.push_str(&source[..identity_start]);
    isolated.push_str(test_identity);
    isolated.push_str(&source[identity_end..]);
    let plugin_root_binding = "$installerPluginRoot = Split-Path -Parent $PSScriptRoot";
    if !isolated.contains(plugin_root_binding) {
        return Err(io::Error::other("installer plugin root binding missing").into());
    }
    let isolated = isolated.replace(
        plugin_root_binding,
        "$installerPluginRoot = $env:PROJECTATLAS_TEST_INSTALLER_PLUGIN_ROOT",
    );
    let process_snapshot_binding =
        "        $processes = @(Get-CimInstance -ClassName Win32_Process -OperationTimeoutSec 5)";
    let process_snapshot_occurrences = isolated.matches(process_snapshot_binding).count();
    if process_snapshot_occurrences != 1 {
        return Err(io::Error::other(format!(
            "installer process snapshot binding occurred {process_snapshot_occurrences} times instead of exactly once"
        ))
        .into());
    }
    let test_process_snapshot = r#"        $allowedProcessIdText = [string]$env:PROJECTATLAS_TEST_PROCESS_IDS
        if ([string]::IsNullOrWhiteSpace($allowedProcessIdText)) {
            throw "ProjectAtlas test process allowlist is missing."
        }
        $allowedProcessIds = @()
        foreach ($entry in $allowedProcessIdText.Split(',')) {
            $candidateProcessId = [uint32]0
            if ($entry -notmatch '^[1-9][0-9]*$' `
                -or -not [uint32]::TryParse($entry, [ref]$candidateProcessId) `
                -or $candidateProcessId -eq 0 `
                -or $candidateProcessId.ToString(
                    [System.Globalization.CultureInfo]::InvariantCulture
                ) -ne $entry `
                -or $allowedProcessIds -contains $candidateProcessId) {
                throw "ProjectAtlas test process allowlist contains an invalid process ID."
            }
            $allowedProcessIds += $candidateProcessId
        }
        $processFilter = ($allowedProcessIds | ForEach-Object {
                "ProcessId = $_"
            }) -join " OR "
        $observedProcesses = @(
            CimCmdlets\Get-CimInstance `
                -ClassName Win32_Process `
                -Filter $processFilter `
                -OperationTimeoutSec 5
        )
        $observedProcessIds = @()
        foreach ($process in $observedProcesses) {
            $observedProcessIdText = [string]$process.ProcessId
            $observedProcessId = [uint32]0
            if ($observedProcessIdText -notmatch '^[1-9][0-9]*$' `
                -or -not [uint32]::TryParse(
                    $observedProcessIdText,
                    [ref]$observedProcessId
                ) `
                -or $observedProcessId -eq 0 `
                -or $observedProcessId.ToString(
                    [System.Globalization.CultureInfo]::InvariantCulture
                ) -ne $observedProcessIdText `
                -or $observedProcessIds -contains $observedProcessId) {
                throw "ProjectAtlas test process snapshot contains an invalid process ID."
            }
            $observedProcessIds += $observedProcessId
        }
        if ($observedProcessIds.Count -ne $allowedProcessIds.Count) {
            throw "ProjectAtlas test process snapshot did not contain the exact allowlist."
        }
        foreach ($allowedProcessId in $allowedProcessIds) {
            if ($observedProcessIds -notcontains $allowedProcessId) {
                throw "ProjectAtlas test process snapshot did not contain the exact allowlist."
            }
        }
        $processes = @($observedProcesses)"#;
    let isolated = isolated.replacen(process_snapshot_binding, test_process_snapshot, 1);
    fs::write(output, isolated)?;
    Ok(())
}

#[cfg(windows)]
fn windows_test_process_id_allowlist(process_ids: &[u32]) -> io::Result<String> {
    if process_ids.is_empty() || process_ids.contains(&0) {
        return Err(io::Error::other(
            "Windows test process allowlist requires positive process IDs",
        ));
    }
    let mut process_ids = process_ids.to_vec();
    process_ids.sort_unstable();
    process_ids.dedup();
    Ok(process_ids
        .into_iter()
        .map(|process_id| process_id.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

#[cfg(windows)]
/// Derive the fixture-private identity record used when normal publication is absent.
fn codex_owner_retained_identity_path(child_identity_file: &Path) -> PathBuf {
    let mut retained_identity = child_identity_file.as_os_str().to_os_string();
    retained_identity.push(".owner");
    PathBuf::from(retained_identity)
}

#[cfg(windows)]
fn codex_owner_cleanup_deadline(started: Instant) -> Instant {
    started
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
        + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE
}

#[cfg(windows)]
fn codex_owner_cleanup_capture_deadline(cleanup_deadline: Instant) -> Instant {
    let now = Instant::now();
    let capture_budget = cleanup_deadline
        .saturating_duration_since(now)
        .saturating_sub(CODEX_OWNER_CHILD_STOP_BUDGET)
        .saturating_sub(CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET)
        .saturating_sub(CODEX_OWNER_CHILD_STOP_FINAL_BUDGET)
        .min(CODEX_OWNER_FAILURE_CLEANUP_BUDGET);
    now + capture_budget
}

#[cfg(windows)]
fn cleanup_codex_owner_processes_after_spawn_failure(
    parent: &mut Child,
    child_identity_file: &Path,
    stable_runtime: &Path,
    mut failures: Vec<String>,
    cleanup_deadline: Instant,
    identity_capture_delay: Option<Duration>,
    child_stop_delay: Option<Duration>,
    fail_helper_spawns: bool,
) -> Result<(), Box<dyn Error>> {
    let retained_identity_file = codex_owner_retained_identity_path(child_identity_file);
    let capture_deadline = codex_owner_cleanup_capture_deadline(cleanup_deadline);
    let mut capture_failures = Vec::new();
    let child_identity = match read_codex_owner_child_identity_with_test_delay(
        child_identity_file,
        stable_runtime,
        capture_deadline,
        identity_capture_delay,
    ) {
        Ok(identity) => Some(identity),
        Err(error) => {
            capture_failures.push(format!("normal child identity capture failed: {error}"));
            match read_codex_owner_child_identity_with_test_delay(
                &retained_identity_file,
                stable_runtime,
                capture_deadline,
                identity_capture_delay,
            ) {
                Ok(identity) => Some(identity),
                Err(error) => {
                    capture_failures
                        .push(format!("retained child identity capture failed: {error}"));
                    None
                }
            }
        }
    };
    let child_cleanup_result = match child_identity {
        Some(identity) => stop_windows_fixture_process_until_with_fallback_test_delay(
            &identity,
            cleanup_deadline,
            child_stop_delay,
            None,
            None,
            fail_helper_spawns,
            fail_helper_spawns,
        ),
        None => match read_codex_owner_identity_record(&retained_identity_file) {
            Ok(identity) => stop_windows_fixture_process_until_with_fallback_test_delay(
                &identity,
                cleanup_deadline,
                child_stop_delay,
                None,
                None,
                fail_helper_spawns,
                fail_helper_spawns,
            ),
            Err(error) => Err(io::Error::other(format!(
                "no retained child identity was available after capture failure ({}): {error}",
                capture_failures.join("; ")
            ))
            .into()),
        },
    };
    if let Err(error) = child_cleanup_result {
        failures.push(format!("could not retire its owned child safely: {error}"));
    }
    let kill_result = parent.kill();
    let wait_result = parent.wait();
    if let Err(error) = kill_result
        && error.kind() != io::ErrorKind::InvalidInput
    {
        failures.push(format!(
            "could not terminate the held owner process: {error}"
        ));
    }
    if let Err(error) = wait_result {
        failures.push(format!("could not reap the held owner process: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Codex owner fixture cleanup failed: {}",
            failures.join("; ")
        ))
        .into())
    }
}

#[cfg(windows)]
fn codex_owner_observation_failure(
    parent: &mut Child,
    child_identity_file: &Path,
    stable_runtime: &Path,
    error: impl std::fmt::Display,
    cleanup_deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    match cleanup_codex_owner_processes_after_spawn_failure(
        parent,
        child_identity_file,
        stable_runtime,
        Vec::new(),
        cleanup_deadline,
        None,
        None,
        false,
    ) {
        Ok(()) => Ok(()),
        Err(cleanup_error) => Err(io::Error::other(format!(
            "failed to preserve child-first cleanup after owner observation error ({error}): {cleanup_error}"
        ))
        .into()),
    }
}

#[cfg(windows)]
fn stop_codex_owner_after_spawn_failure(
    parent: &mut Child,
    child_identity_file: &Path,
    stable_runtime: &Path,
) -> Result<(), Box<dyn Error>> {
    stop_codex_owner_after_spawn_failure_with_test_delays(
        parent,
        child_identity_file,
        stable_runtime,
        None,
        None,
        None,
        false,
    )
}

#[cfg(windows)]
fn stop_codex_owner_after_spawn_failure_with_test_delays(
    parent: &mut Child,
    child_identity_file: &Path,
    stable_runtime: &Path,
    identity_capture_delay: Option<Duration>,
    child_stop_delay: Option<Duration>,
    owner_observation_delay: Option<Duration>,
    fail_helper_spawns: bool,
) -> Result<(), Box<dyn Error>> {
    let mut stop_file = child_identity_file.as_os_str().to_os_string();
    stop_file.push(".stop");
    let stop_result = fs::write(PathBuf::from(stop_file), b"stop");
    let cleanup_started = Instant::now();
    let cleanup_deadline = codex_owner_cleanup_deadline(cleanup_started);
    let observation_deadline = cleanup_started + CODEX_OWNER_FAILURE_CLEANUP_BUDGET;
    let mut observation_error = None;
    let mut failures = Vec::new();
    if stop_result.is_ok() {
        if let Some(delay) = owner_observation_delay {
            thread::sleep(delay);
        }
        loop {
            match parent.try_wait() {
                Ok(Some(status)) => {
                    if Instant::now() >= observation_deadline {
                        failures.push(format!(
                            "owner fixture exited after observation deadline: {status} (owner_observation_elapsed_ms={})",
                            cleanup_started.elapsed().as_millis()
                        ));
                        break;
                    }
                    return Ok(());
                }
                Ok(None) => {}
                Err(error) => {
                    observation_error = Some(error);
                    break;
                }
            }
            if Instant::now() >= observation_deadline {
                break;
            }
            let remaining = observation_deadline.saturating_duration_since(Instant::now());
            thread::sleep(remaining.min(Duration::from_millis(25)));
        }
    }

    if let Some(error) = observation_error {
        codex_owner_observation_failure(
            parent,
            child_identity_file,
            stable_runtime,
            error,
            cleanup_deadline,
        )?;
        return Ok(());
    }

    if let Err(error) = stop_result {
        failures.push(format!("could not signal the owner fixture: {error}"));
    } else {
        failures.push("owner fixture did not stop within five seconds".to_string());
    }
    cleanup_codex_owner_processes_after_spawn_failure(
        parent,
        child_identity_file,
        stable_runtime,
        failures,
        cleanup_deadline,
        identity_capture_delay,
        child_stop_delay,
        fail_helper_spawns,
    )
}

#[cfg(windows)]
/// Retire the exact published child, then kill and reap its owned parent.
fn cleanup_codex_owner_processes(
    mut parent: Child,
    child_identity: &WindowsProcessIdentity,
) -> Result<(), Box<dyn Error>> {
    let cleanup_deadline = codex_owner_cleanup_deadline(Instant::now());
    let child_cleanup_result =
        stop_windows_fixture_process_until(child_identity, cleanup_deadline, None, None);
    let kill_result = parent.kill();
    let wait_result = parent.wait();
    let mut failures = Vec::new();
    if let Err(error) = child_cleanup_result {
        failures.push(format!(
            "could not retire the published child safely: {error}"
        ));
    }
    if let Err(error) = kill_result
        && error.kind() != io::ErrorKind::InvalidInput
    {
        failures.push(format!(
            "could not terminate the held owner process: {error}"
        ));
    }
    if let Err(error) = wait_result {
        failures.push(format!("could not reap the held owner process: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(failures.join("; ")).into())
    }
}

#[cfg(windows)]
/// Preserve a negative assertion while cleaning up an unexpectedly accepted owner.
fn codex_owner_unexpected_acceptance_error(
    mode: &str,
    parent: Child,
    child_identity: &WindowsProcessIdentity,
) -> Box<dyn Error> {
    let cleanup_result = cleanup_codex_owner_processes(parent, child_identity);
    let mut message = format!("{mode} owner fixture publication was accepted");
    if let Err(error) = cleanup_result {
        message.push_str("; fixture cleanup also failed: ");
        message.push_str(&error.to_string());
    }
    io::Error::other(message).into()
}

#[cfg(windows)]
fn read_codex_owner_child_identity_with_test_delay(
    child_identity_file: &Path,
    stable_runtime: &Path,
    readiness_deadline: Instant,
    test_delay: Option<Duration>,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    if Instant::now() >= readiness_deadline {
        return Err(io::Error::other(
            "Windows fixture identity validation reached the readiness deadline before probing",
        )
        .into());
    }
    let published = read_codex_owner_identity_record(child_identity_file)?;
    let captured = capture_windows_process_identity_until(
        published.process_id,
        readiness_deadline,
        test_delay,
        None,
    )?;
    if captured.process_id != published.process_id
        || captured.creation_file_time_utc != published.creation_file_time_utc
    {
        return Err(io::Error::other(format!(
            "owner-published child identity differed: published_pid={} captured_pid={} published_creation={} captured_creation={}",
            published.process_id,
            captured.process_id,
            published.creation_file_time_utc,
            captured.creation_file_time_utc
        ))
        .into());
    }
    let owner_canonical =
        normalize_native_path_display(fs::canonicalize(&published.executable_path)?);
    let captured_canonical =
        normalize_native_path_display(fs::canonicalize(&captured.executable_path)?);
    let expected_canonical = normalize_native_path_display(fs::canonicalize(stable_runtime)?);
    if !captured_canonical.eq_ignore_ascii_case(&owner_canonical)
        || !captured_canonical.eq_ignore_ascii_case(&expected_canonical)
    {
        return Err(io::Error::other(format!(
            "owner-published child path differed: published_raw={} captured_raw={} expected_raw={} published_canonical={owner_canonical} captured_canonical={captured_canonical} expected_canonical={expected_canonical}",
            published.executable_path.display(),
            captured.executable_path.display(),
            stable_runtime.display()
        ))
        .into());
    }
    if Instant::now() >= readiness_deadline {
        return Err(io::Error::other(
            "Windows fixture identity validation completed after the readiness deadline",
        )
        .into());
    }
    Ok(captured)
}

#[cfg(windows)]
fn read_codex_owner_identity_record(
    child_identity_file: &Path,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    let identity_text = fs::read_to_string(child_identity_file)?;
    let mut identity_lines = identity_text.lines();
    let process_id = identity_lines
        .next()
        .ok_or_else(|| io::Error::other("owner fixture omitted child PID"))?
        .parse::<u32>()?;
    let owner_creation_file_time_utc = identity_lines
        .next()
        .ok_or_else(|| io::Error::other("owner fixture omitted child creation time"))?
        .parse::<i64>()?;
    let owner_executable_path = PathBuf::from(
        identity_lines
            .next()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| io::Error::other("owner fixture omitted child executable path"))?,
    );
    Ok(WindowsProcessIdentity {
        process_id,
        creation_file_time_utc: owner_creation_file_time_utc,
        executable_path: owner_executable_path,
    })
}

#[cfg(windows)]
fn codex_owner_spawn_error(
    parent: &mut Child,
    codex_fixture: &Path,
    child_identity_file: &Path,
    stable_runtime: &Path,
    error: impl std::fmt::Display,
) -> Box<dyn Error> {
    match stop_codex_owner_after_spawn_failure(parent, child_identity_file, stable_runtime) {
        Ok(()) => io::Error::other(format!(
            "{error}; owner={}; identity_file={}; expected_runtime={}",
            codex_fixture.display(),
            child_identity_file.display(),
            stable_runtime.display()
        ))
        .into(),
        Err(cleanup_error) => io::Error::other(format!(
            "{error}; owner={}; identity_file={}; expected_runtime={}; fixture cleanup also failed: {cleanup_error}",
            codex_fixture.display(),
            child_identity_file.display(),
            stable_runtime.display()
        ))
        .into(),
    }
}

#[cfg(windows)]
fn codex_owner_readiness_deadline(started: Instant) -> Result<Instant, Box<dyn Error>> {
    started
        .checked_add(CODEX_OWNER_READINESS_TIMEOUT)
        .ok_or_else(|| io::Error::other("Codex MCP owner readiness deadline overflow").into())
}

#[cfg(windows)]
fn spawn_codex_owned_obsolete_mcp(
    codex_fixture: &Path,
    stable_runtime: &Path,
    db: &Path,
    config: Option<&Path>,
    child_pid_file: &Path,
    publication_delay: Option<Duration>,
    publication_mode: Option<&str>,
) -> Result<(Child, WindowsProcessIdentity), Box<dyn Error>> {
    spawn_codex_owned_obsolete_mcp_with_test_delays(
        codex_fixture,
        stable_runtime,
        db,
        config,
        child_pid_file,
        publication_delay,
        publication_mode,
        None,
        None,
    )
}

#[cfg(windows)]
fn spawn_codex_owned_obsolete_mcp_with_test_delays(
    codex_fixture: &Path,
    stable_runtime: &Path,
    db: &Path,
    config: Option<&Path>,
    child_pid_file: &Path,
    publication_delay: Option<Duration>,
    publication_mode: Option<&str>,
    identity_capture_delay: Option<Duration>,
    observation_delay: Option<Duration>,
) -> Result<(Child, WindowsProcessIdentity), Box<dyn Error>> {
    let started = Instant::now();
    let deadline = codex_owner_readiness_deadline(started)?;
    let mut command = StdCommand::new(codex_fixture);
    command
        .arg(child_pid_file)
        .arg(stable_runtime)
        .arg(db)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(config) = config {
        command.arg(config).arg("--config").arg("model=\"o3\"");
    }
    if let Some(delay) = publication_delay {
        command.env(
            CODEX_OWNER_PUBLICATION_DELAY_ENV,
            delay.as_millis().to_string(),
        );
    }
    if let Some(mode) = publication_mode {
        command.env(CODEX_OWNER_PUBLICATION_MODE_ENV, mode);
    }
    let mut parent = command.spawn()?;
    if let Some(delay) = observation_delay {
        thread::sleep(delay);
    }
    loop {
        match parent.try_wait() {
            Ok(Some(status)) => {
                let observation_elapsed = started.elapsed();
                return Err(codex_owner_spawn_error(
                    &mut parent,
                    codex_fixture,
                    child_pid_file,
                    stable_runtime,
                    format!(
                        "Codex MCP owner fixture exited before publishing its child PID: {status} (owner_observation_elapsed_ms={})",
                        observation_elapsed.as_millis()
                    ),
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(codex_owner_spawn_error(
                    &mut parent,
                    codex_fixture,
                    child_pid_file,
                    stable_runtime,
                    format!("failed to inspect Codex MCP owner fixture: {error}"),
                ));
            }
        }
        if child_pid_file.is_file() {
            let observed_at = Instant::now();
            if observed_at >= deadline {
                let elapsed = started.elapsed();
                return Err(codex_owner_spawn_error(
                    &mut parent,
                    codex_fixture,
                    child_pid_file,
                    stable_runtime,
                    format!(
                        "Codex MCP owner fixture published its child PID after the readiness deadline (readiness_elapsed_ms={})",
                        elapsed.as_millis()
                    ),
                ));
            }
            // Publication and exact identity validation share this one readiness deadline.
            match read_codex_owner_child_identity_with_test_delay(
                child_pid_file,
                stable_runtime,
                deadline,
                identity_capture_delay,
            ) {
                Ok(captured) => return Ok((parent, captured)),
                Err(error) => {
                    return Err(codex_owner_spawn_error(
                        &mut parent,
                        codex_fixture,
                        child_pid_file,
                        stable_runtime,
                        format!(
                            "failed to validate published child identity (readiness_elapsed_ms={}): {error}",
                            started.elapsed().as_millis()
                        ),
                    ));
                }
            }
        }
        if Instant::now() >= deadline {
            let elapsed = started.elapsed();
            return Err(codex_owner_spawn_error(
                &mut parent,
                codex_fixture,
                child_pid_file,
                stable_runtime,
                format!(
                    "Codex MCP owner fixture did not publish its child PID within {CODEX_OWNER_READINESS_TIMEOUT:?} (elapsed={elapsed:?} readiness_elapsed_ms={})",
                    elapsed.as_millis()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[cfg(windows)]
fn windows_codex_owner_fixture_readiness_is_bounded_and_identity_safe() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    let atlas_dir = repo.join(ATLAS_DIR_NAME);
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let db = atlas_dir.join("projectatlas.db");
    let runtime = temp
        .path()
        .join(OBSOLETE_PROJECTATLAS_FIXTURE_EXECUTABLE_FILE_NAME);
    compile_obsolete_projectatlas_fixture(&runtime)?;
    let codex_fixture = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    compile_codex_mcp_owner_fixture(&codex_fixture)?;

    // Exercise the production readiness helper with prompt publication plus a real PowerShell
    // identity capture delay beyond the former five-second boundary, while remaining inside the
    // one absolute readiness deadline.
    let delayed_identity_file = temp.path().join("delayed-identity.pid");
    let delayed_started = Instant::now();
    let (delayed_parent, delayed_identity) = spawn_codex_owned_obsolete_mcp_with_test_delays(
        &codex_fixture,
        &runtime,
        &db,
        None,
        &delayed_identity_file,
        Some(CODEX_OWNER_DELAYED_PUBLICATION),
        None,
        Some(CODEX_OWNER_IDENTITY_CAPTURE_TEST_DELAY),
        None,
    )?;
    let delayed_elapsed = delayed_started.elapsed();
    if delayed_elapsed <= Duration::from_secs(5) || delayed_elapsed >= CODEX_OWNER_READINESS_TIMEOUT
    {
        let cleanup_result = cleanup_codex_owner_processes(delayed_parent, &delayed_identity);
        return Err(io::Error::other(format!(
            "delayed publication and identity capture did not stay inside one readiness deadline: elapsed={delayed_elapsed:?} publication_delay={CODEX_OWNER_DELAYED_PUBLICATION:?} capture_delay={CODEX_OWNER_IDENTITY_CAPTURE_TEST_DELAY:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} cleanup={cleanup_result:?}"
        ))
        .into());
    }
    let delayed_cleanup = cleanup_codex_owner_processes(delayed_parent, &delayed_identity);
    if delayed_cleanup.is_err() || windows_process_is_alive(&delayed_identity)? {
        return Err(io::Error::other(format!(
            "delayed publication and identity capture did not clean up its accepted child: {delayed_cleanup:?}"
        ))
        .into());
    }

    for (index, (mode, expected)) in [
        ("early-exit", "exited before publishing"),
        ("malformed", "failed to validate published child identity"),
        ("mismatched", "owner-published child identity differed"),
    ]
    .into_iter()
    .enumerate()
    {
        let identity_file = temp.path().join(format!("{mode}-{index}.pid"));
        let started = Instant::now();
        let result = spawn_codex_owned_obsolete_mcp(
            &codex_fixture,
            &runtime,
            &db,
            Some(&atlas_dir.join("config.toml")),
            &identity_file,
            None,
            Some(mode),
        );
        let error = match result {
            Ok((parent, child_identity)) => {
                return Err(codex_owner_unexpected_acceptance_error(
                    mode,
                    parent,
                    &child_identity,
                ));
            }
            Err(error) => error,
        };
        let elapsed = started.elapsed();
        let text = error.to_string();
        let early_exit_observation_elapsed = if mode == "early-exit" {
            Some(
                text.split("owner_observation_elapsed_ms=")
                    .nth(1)
                    .and_then(|value| value.split(')').next())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .map(Duration::from_millis)
                    .ok_or_else(|| {
                        io::Error::other(format!(
                            "early-exit owner fixture omitted its observation elapsed diagnostic:\n{text}"
                        ))
                    })?,
            )
        } else {
            None
        };
        if !text.contains(expected)
            || !text.contains(&format!("owner={}", codex_fixture.display()))
            || !text.contains(&format!("identity_file={}", identity_file.display()))
            || !text.contains(&format!("expected_runtime={}", runtime.display()))
            || (mode == "early-exit" && elapsed > codex_owner_early_exit_max_elapsed())
            || early_exit_observation_elapsed
                .as_ref()
                .is_some_and(|observed| *observed >= CODEX_OWNER_READINESS_TIMEOUT)
            || text.contains("fixture cleanup also failed")
        {
            return Err(io::Error::other(format!(
                "{mode} owner fixture failure was not bounded and diagnostic: elapsed={elapsed:?} observed={early_exit_observation_elapsed:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} max_early_exit={:?}\n{text}",
                codex_owner_early_exit_max_elapsed()
            ))
            .into());
        }
    }

    // Exercise the same branch a negative fixture would take if validation regressed.
    let accepted_identity_file = temp.path().join("unexpected-acceptance.pid");
    let accepted = spawn_codex_owned_obsolete_mcp(
        &codex_fixture,
        &runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &accepted_identity_file,
        None,
        None,
    )?;
    let accepted_identity = accepted.1.clone();
    let unexpected_error =
        codex_owner_unexpected_acceptance_error("mismatched", accepted.0, &accepted_identity);
    let unexpected_error_text = unexpected_error.to_string();
    if windows_process_is_alive(&accepted_identity)?
        || !unexpected_error_text.contains("mismatched owner fixture publication was accepted")
        || unexpected_error_text.contains("fixture cleanup also failed")
    {
        return Err(io::Error::other(format!(
            "unexpectedly accepted negative owner fixture did not clean up its owned processes: {unexpected_error}"
        ))
        .into());
    }

    let observation_identity_file = temp.path().join("observation-failure.pid");
    let (mut observation_parent, observation_identity) = spawn_codex_owned_obsolete_mcp(
        &codex_fixture,
        &runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &observation_identity_file,
        None,
        None,
    )?;
    // Inject only the observation decision while exercising the same child-first cleanup path;
    // the production caller retains the actual observation error in its spawn diagnostic.
    let observation_cleanup_deadline = Instant::now()
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
    let observation_cleanup_result = codex_owner_observation_failure(
        &mut observation_parent,
        &observation_identity_file,
        &runtime,
        "synthetic parent observation failure",
        observation_cleanup_deadline,
    );
    if observation_parent.try_wait()?.is_none()
        || windows_process_is_alive(&observation_identity)?
        || observation_cleanup_result.is_err()
    {
        return Err(io::Error::other(format!(
            "parent observation failure did not preserve exact child-first cleanup: {observation_cleanup_result:?}"
        ))
        .into());
    }

    // A promptly stopped owner can be observed after the five-second owner-observation
    // deadline when the polling caller is descheduled.  The late completion is a failure
    // classification, but it must still use the retained-identity child-first cleanup path.
    let late_observation_identity_file = temp.path().join("late-owner-observation.pid");
    let (mut late_observation_parent, late_observation_identity) = spawn_codex_owned_obsolete_mcp(
        &codex_fixture,
        &runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &late_observation_identity_file,
        None,
        None,
    )?;
    // Remove normal publication so cleanup must use the fixture-retained identity record.
    fs::remove_file(&late_observation_identity_file)?;
    let late_observation_result = stop_codex_owner_after_spawn_failure_with_test_delays(
        &mut late_observation_parent,
        &late_observation_identity_file,
        &runtime,
        None,
        None,
        Some(CODEX_OWNER_LATE_OWNER_OBSERVATION_TEST_DELAY),
        false,
    );
    let late_observation_text = late_observation_result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    let late_observation_elapsed = late_observation_text
        .split("owner_observation_elapsed_ms=")
        .nth(1)
        .and_then(|value| value.split(')').next())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .ok_or_else(|| {
            io::Error::other(format!(
                "late owner observation omitted its elapsed diagnostic: {late_observation_text}"
            ))
        })?;
    if late_observation_result.is_ok()
        || !late_observation_text.contains("owner fixture exited after observation deadline")
        || late_observation_elapsed < CODEX_OWNER_LATE_OWNER_OBSERVATION_TEST_DELAY
        || late_observation_elapsed
            > CODEX_OWNER_LATE_OWNER_OBSERVATION_TEST_DELAY
                + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE
        || late_observation_parent.try_wait()?.is_none()
        || windows_process_is_alive(&late_observation_identity)?
        || late_observation_text.contains("could not retire its owned child safely")
    {
        return Err(io::Error::other(format!(
            "late owner observation did not fail closed through exact child-first cleanup: result={late_observation_result:?} child_alive={}\n{late_observation_text}",
            windows_process_is_alive(&late_observation_identity)?
        ))
        .into());
    }

    // Exercise the real production helper when publication and PowerShell identity capture
    // compose beyond the same absolute readiness deadline. Cleanup must reap the exact retained
    // child and the held owner without creating a second readiness envelope.
    let boundary_identity_file = temp.path().join("readiness-boundary.pid");
    let boundary_started = Instant::now();
    let boundary_result = spawn_codex_owned_obsolete_mcp_with_test_delays(
        &codex_fixture,
        &runtime,
        &db,
        None,
        &boundary_identity_file,
        Some(CODEX_OWNER_READINESS_BOUNDARY_PUBLICATION_DELAY),
        None,
        Some(CODEX_OWNER_READINESS_BOUNDARY_CAPTURE_DELAY),
        None,
    );
    let boundary_elapsed = boundary_started.elapsed();
    let boundary_error = match boundary_result {
        Ok((parent, child_identity)) => {
            return Err(codex_owner_unexpected_acceptance_error(
                "readiness-boundary",
                parent,
                &child_identity,
            ));
        }
        Err(error) => error,
    };
    let boundary_text = boundary_error.to_string();
    let boundary_identity = read_codex_owner_identity_record(&codex_owner_retained_identity_path(
        &boundary_identity_file,
    ))?;
    let boundary_readiness_elapsed = boundary_text
        .split("readiness_elapsed_ms=")
        .nth(1)
        .and_then(|value| value.split(')').next())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .ok_or_else(|| {
            io::Error::other(format!(
                "readiness-boundary capture failure omitted total readiness elapsed: {boundary_text}"
            ))
        })?;
    let boundary_total_upper_bound = CODEX_OWNER_READINESS_TIMEOUT
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
        + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE
        + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    if boundary_elapsed < CODEX_OWNER_READINESS_TIMEOUT
        || boundary_elapsed > boundary_total_upper_bound
        || boundary_readiness_elapsed < CODEX_OWNER_READINESS_TIMEOUT
        || boundary_readiness_elapsed
            > CODEX_OWNER_READINESS_TIMEOUT + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE
        || !boundary_text.contains("timed out capturing Windows fixture process identity")
        || !boundary_text.contains("readiness_elapsed_ms=")
        || !boundary_text.contains(&format!("owner={}", codex_fixture.display()))
        || !boundary_text.contains(&format!(
            "identity_file={}",
            boundary_identity_file.display()
        ))
        || !boundary_text.contains(&format!("expected_runtime={}", runtime.display()))
        || boundary_text.contains("fixture cleanup also failed")
        || windows_process_is_alive(&boundary_identity)?
    {
        return Err(io::Error::other(format!(
            "readiness-boundary capture did not fail at the one bounded deadline or clean up its exact child: total_elapsed={boundary_elapsed:?} readiness_elapsed={boundary_readiness_elapsed:?} publication_delay={CODEX_OWNER_READINESS_BOUNDARY_PUBLICATION_DELAY:?} capture_delay={CODEX_OWNER_READINESS_BOUNDARY_CAPTURE_DELAY:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} total_upper_bound={boundary_total_upper_bound:?}\n{boundary_text}"
        ))
        .into());
    }

    let identity_file = temp.path().join("timeout.pid");
    let timeout_started = Instant::now();
    let result = spawn_codex_owned_obsolete_mcp(
        &codex_fixture,
        &runtime,
        &db,
        Some(&atlas_dir.join("config.toml")),
        &identity_file,
        None,
        Some("timeout-ignore-stop"),
    );
    let error = match result {
        Ok((parent, child_identity)) => {
            return Err(codex_owner_unexpected_acceptance_error(
                "timeout",
                parent,
                &child_identity,
            ));
        }
        Err(error) => error,
    };
    let timeout_elapsed = timeout_started.elapsed();
    let timeout_upper_bound = CODEX_OWNER_READINESS_TIMEOUT
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
        + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE
        + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    let text = error.to_string();
    let timeout_readiness_elapsed = text
        .split("readiness_elapsed_ms=")
        .nth(1)
        .and_then(|value| value.split(')').next())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .ok_or_else(|| {
            io::Error::other(format!(
                "true owner fixture timeout omitted its pre-cleanup readiness elapsed diagnostic:\n{text}"
            ))
        })?;
    let timeout_readiness_upper_bound =
        CODEX_OWNER_READINESS_TIMEOUT + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    let timeout_child_identity =
        read_codex_owner_identity_record(&codex_owner_retained_identity_path(&identity_file))?;
    if timeout_elapsed < CODEX_OWNER_READINESS_TIMEOUT
        || timeout_elapsed > timeout_upper_bound
        || timeout_readiness_elapsed < CODEX_OWNER_READINESS_TIMEOUT
        || timeout_readiness_elapsed > timeout_readiness_upper_bound
        || !text.contains("did not publish its child PID within 30s")
        || !text.contains("elapsed=")
        || !text.contains(&format!("owner={}", codex_fixture.display()))
        || !text.contains(&format!("identity_file={}", identity_file.display()))
        || !text.contains(&format!("expected_runtime={}", runtime.display()))
        || !text.contains("owner fixture did not stop within five seconds")
        || !text.contains("fixture cleanup also failed")
        || windows_process_is_alive(&timeout_child_identity)?
    {
        return Err(io::Error::other(format!(
            "true owner fixture timeout was not bounded and diagnostic: total_elapsed={timeout_elapsed:?} readiness_elapsed={timeout_readiness_elapsed:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} scheduler_tolerance={CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE:?} owner_observation_budget={CODEX_OWNER_FAILURE_CLEANUP_BUDGET:?} child_stop_budget={CODEX_OWNER_CHILD_STOP_BUDGET:?} cleanup_scheduler_tolerance={CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE:?} total_upper_bound={timeout_upper_bound:?} readiness_upper_bound={timeout_readiness_upper_bound:?}\n{text}"
        ))
        .into());
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Clone, Debug)]
struct WindowsProcessIdentity {
    process_id: u32,
    creation_file_time_utc: i64,
    executable_path: PathBuf,
}

#[cfg(windows)]
fn capture_windows_process_identity(
    process_id: u32,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    windows_native_process::capture_exact(process_id).map_err(|error| {
        io::Error::other(format!(
            "failed to capture exact Windows fixture process identity {process_id}: {error}"
        ))
        .into()
    })
}

#[cfg(windows)]
fn capture_windows_process_identity_with_timeout(
    process_id: u32,
    timeout: Duration,
    test_delay: Option<Duration>,
    observation_delay: Option<Duration>,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    if timeout.is_zero() {
        return Err(io::Error::other(format!(
            "Windows fixture identity capture budget expired before probing process {process_id}"
        ))
        .into());
    }
    let started = Instant::now();
    let deadline = started
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("Windows fixture identity capture deadline overflow"))?;
    let mut capture = spawn_windows_process_identity_capture(process_id, test_delay)?;
    if observation_delay.is_some() {
        // Release the intentional observer delay only after the small capture
        // process has exited, so host scheduling cannot masquerade as a late
        // completion.
        if let Err(error) = synchronize_prompt_exit_before_delayed_observation(
            &mut capture,
            "Windows fixture identity capture",
            None,
        ) {
            let kill_result = capture.kill();
            let wait_result = capture.wait();
            return Err(io::Error::other(format!(
                "Windows fixture identity capture did not complete before delayed observation: {error}; cleanup kill={kill_result:?} wait={wait_result:?}"
            ))
            .into());
        }
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            io::Error::other("Windows fixture identity capture deadline overflow")
        })?;
        return capture_windows_process_identity_from_child(
            process_id,
            capture,
            deadline,
            observation_delay,
        );
    }
    let capture =
        reject_windows_identity_capture_started_after_deadline(process_id, capture, deadline)?;
    capture_windows_process_identity_from_child(process_id, capture, deadline, None)
}

#[cfg(windows)]
fn capture_windows_process_identity_until(
    process_id: u32,
    deadline: Instant,
    test_delay: Option<Duration>,
    observation_delay: Option<Duration>,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    if Instant::now() >= deadline {
        return Err(io::Error::other(format!(
            "Windows fixture identity capture deadline expired before probing process {process_id}"
        ))
        .into());
    }
    let capture = spawn_windows_process_identity_capture(process_id, test_delay)?;
    let capture =
        reject_windows_identity_capture_started_after_deadline(process_id, capture, deadline)?;
    capture_windows_process_identity_from_child(process_id, capture, deadline, observation_delay)
}

#[cfg(windows)]
fn reject_windows_identity_capture_started_after_deadline(
    process_id: u32,
    mut capture: Child,
    deadline: Instant,
) -> Result<Child, Box<dyn Error>> {
    if Instant::now() >= deadline {
        let cleanup_result = terminate_windows_identity_capture(
            &mut capture,
            deadline
                .checked_add(CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE)
                .unwrap_or(deadline),
        );
        return Err(io::Error::other(format!(
            "Windows fixture identity capture process {process_id} started after the readiness deadline; cleanup={cleanup_result:?}"
        ))
        .into());
    }
    Ok(capture)
}

#[cfg(windows)]
fn spawn_windows_process_identity_capture(
    process_id: u32,
    test_delay: Option<Duration>,
) -> Result<Child, Box<dyn Error>> {
    let mut command = StdCommand::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(
            "$delay = 0; if ($env:PROJECTATLAS_TEST_CODEX_OWNER_IDENTITY_CAPTURE_DELAY_MS) { $delay = [int]$env:PROJECTATLAS_TEST_CODEX_OWNER_IDENTITY_CAPTURE_DELAY_MS }; if ($delay -gt 0) { Start-Sleep -Milliseconds $delay }; $process = Get-Process -Id $env:PROJECTATLAS_FIXTURE_PID -ErrorAction Stop; [pscustomobject]@{ process_id = [uint32]$process.Id; creation_file_time_utc = $process.StartTime.ToUniversalTime().ToFileTimeUtc(); executable_path = [System.IO.Path]::GetFullPath($process.Path) } | ConvertTo-Json -Compress",
        )
        .env("PROJECTATLAS_FIXTURE_PID", process_id.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(delay) = test_delay {
        command.env(
            CODEX_OWNER_IDENTITY_CAPTURE_DELAY_ENV,
            delay.as_millis().to_string(),
        );
    }
    Ok(command.spawn()?)
}

#[cfg(windows)]
fn terminate_windows_identity_capture(capture: &mut Child, deadline: Instant) -> io::Result<()> {
    let process_id = capture.id();
    let kill_error = match capture.kill() {
        Ok(()) => None,
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => None,
        Err(error) => Some(error),
    };
    loop {
        match capture.try_wait() {
            Ok(Some(_status)) => {
                return kill_error.map_or(Ok(()), |error| {
                    Err(io::Error::other(format!(
                        "could not terminate identity capture process {process_id}: {error}"
                    )))
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "identity capture process {process_id} was not reaped by its bounded cleanup deadline"
                    ),
                ));
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            Err(error) => {
                return Err(io::Error::other(format!(
                    "could not observe identity capture process {process_id} after termination: {error}"
                )));
            }
        }
    }
}

#[cfg(windows)]
fn capture_windows_process_identity_from_child(
    process_id: u32,
    mut capture: Child,
    deadline: Instant,
    observation_delay: Option<Duration>,
) -> Result<WindowsProcessIdentity, Box<dyn Error>> {
    let started = Instant::now();
    if let Some(delay) = observation_delay {
        thread::sleep(delay);
    }
    let output = loop {
        match capture.try_wait() {
            Ok(Some(_)) => {
                let output = capture.wait_with_output()?;
                let observed_at = Instant::now();
                if observed_at >= deadline {
                    return Err(io::Error::other(format!(
                        "Windows fixture identity capture completed after the readiness deadline (observed after {:?})",
                        observed_at.duration_since(started)
                    ))
                    .into());
                }
                break output;
            }
            Ok(None) if Instant::now() >= deadline => {
                let cleanup_result = terminate_windows_identity_capture(
                    &mut capture,
                    deadline
                        .checked_add(CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE)
                        .unwrap_or(deadline),
                );
                let mut cleanup = Vec::new();
                if let Err(error) = cleanup_result {
                    cleanup.push(format!("identity capture cleanup failed: {error}"));
                }
                let cleanup_detail = if cleanup.is_empty() {
                    String::new()
                } else {
                    format!("; {}", cleanup.join("; "))
                };
                return Err(io::Error::other(format!(
                    "timed out capturing Windows fixture process identity {process_id} at the readiness deadline (observed after {:?}){cleanup_detail}",
                    Instant::now().duration_since(started)
                ))
                .into());
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    let cleanup_result = terminate_windows_identity_capture(
                        &mut capture,
                        deadline
                            .checked_add(CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE)
                            .unwrap_or(deadline),
                    );
                    return Err(io::Error::other(format!(
                        "timed out capturing Windows fixture process identity {process_id} at the readiness deadline; cleanup={cleanup_result:?}"
                    ))
                    .into());
                }
                thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            Err(error) => {
                let cleanup_result = terminate_windows_identity_capture(
                    &mut capture,
                    Instant::now()
                        .checked_add(CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE)
                        .unwrap_or_else(Instant::now),
                );
                let cleanup_detail = format!("; identity-capture cleanup={cleanup_result:?}");
                return Err(io::Error::other(format!(
                    "failed to observe Windows fixture identity capture {process_id}: {error}{cleanup_detail}"
                ))
                .into());
            }
        }
    };
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "failed to capture Windows fixture process identity {process_id}:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let identity: Value = serde_json::from_slice(&output.stdout)?;
    let captured_process_id = identity["process_id"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| io::Error::other("Windows fixture identity omitted its process ID"))?;
    let creation_file_time_utc = identity["creation_file_time_utc"]
        .as_i64()
        .ok_or_else(|| io::Error::other("Windows fixture identity omitted its creation time"))?;
    let executable_path = identity["executable_path"]
        .as_str()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("Windows fixture identity omitted its executable path"))?;
    Ok(WindowsProcessIdentity {
        process_id: captured_process_id,
        creation_file_time_utc,
        executable_path,
    })
}

#[cfg(windows)]
fn windows_process_is_alive(identity: &WindowsProcessIdentity) -> Result<bool, Box<dyn Error>> {
    let status = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(
            "$process = Get-Process -Id $env:PROJECTATLAS_FIXTURE_PID -ErrorAction SilentlyContinue; if ($null -eq $process) { exit 1 }; try { $creation = $process.StartTime.ToUniversalTime().ToFileTimeUtc(); $path = [System.IO.Path]::GetFullPath($process.Path); if ($creation -eq [long]$env:PROJECTATLAS_FIXTURE_CREATION -and [string]::Equals($path, [System.IO.Path]::GetFullPath($env:PROJECTATLAS_FIXTURE_PATH), [System.StringComparison]::OrdinalIgnoreCase)) { exit 0 } } catch {}; exit 1",
        )
        .env(
            "PROJECTATLAS_FIXTURE_PID",
            identity.process_id.to_string(),
        )
        .env(
            "PROJECTATLAS_FIXTURE_CREATION",
            identity.creation_file_time_utc.to_string(),
        )
        .env("PROJECTATLAS_FIXTURE_PATH", &identity.executable_path)
        .status()?;
    Ok(status.success())
}

#[cfg(windows)]
fn stop_windows_fixture_process(identity: &WindowsProcessIdentity) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now()
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
    stop_windows_fixture_process_until(identity, deadline, None, None)
}

#[cfg(windows)]
const WINDOWS_FIXTURE_STOP_SCRIPT: &str = "$process = Get-Process -Id $env:PROJECTATLAS_FIXTURE_PID -ErrorAction SilentlyContinue; if ($null -eq $process) { exit 0 }; $result = 0; try { $heldHandle = $process.Handle; $creation = $process.StartTime.ToUniversalTime().ToFileTimeUtc(); $path = [System.IO.Path]::GetFullPath($process.Path); if ($creation -ne [long]$env:PROJECTATLAS_FIXTURE_CREATION -or -not [string]::Equals($path, [System.IO.Path]::GetFullPath($env:PROJECTATLAS_FIXTURE_PATH), [System.StringComparison]::OrdinalIgnoreCase)) { $result = 3 } elseif (-not $process.HasExited) { if ($env:PROJECTATLAS_TEST_CODEX_OWNER_STOP_DELAY_MS) { Start-Sleep -Milliseconds ([int]$env:PROJECTATLAS_TEST_CODEX_OWNER_STOP_DELAY_MS) }; $process.Kill(); if (-not $process.WaitForExit(5000)) { $result = 5 } } } catch { if (-not $process.HasExited) { $result = 4 } } finally { $process.Dispose() }; exit $result";

#[cfg(windows)]
fn spawn_windows_fixture_stop_helper(
    identity: &WindowsProcessIdentity,
    test_delay: Option<Duration>,
    fail_spawn: bool,
) -> Result<Child, Box<dyn Error>> {
    if fail_spawn {
        return Err(
            io::Error::other("test-injected Windows fixture stop-helper spawn failure").into(),
        );
    }
    let mut command = StdCommand::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(WINDOWS_FIXTURE_STOP_SCRIPT)
        .env("PROJECTATLAS_FIXTURE_PID", identity.process_id.to_string())
        .env(
            "PROJECTATLAS_FIXTURE_CREATION",
            identity.creation_file_time_utc.to_string(),
        )
        .env("PROJECTATLAS_FIXTURE_PATH", &identity.executable_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(delay) = test_delay {
        command.env(CODEX_OWNER_STOP_DELAY_ENV, delay.as_millis().to_string());
    }
    Ok(command.spawn()?)
}

#[cfg(windows)]
fn force_stop_windows_fixture_process(
    identity: &WindowsProcessIdentity,
    deadline: Instant,
    test_delay: Option<Duration>,
    fail_spawn: bool,
) -> Result<(), Box<dyn Error>> {
    let mut stop = spawn_windows_fixture_stop_helper(identity, test_delay, fail_spawn)?;
    let started = Instant::now();
    loop {
        match stop.try_wait() {
            Ok(Some(status)) if status.success() && Instant::now() < deadline => return Ok(()),
            Ok(Some(status)) if status.success() => {
                return Err(io::Error::other(format!(
                    "exact child cleanup fallback completed after deadline for Windows fixture process {} (observed after {:?})",
                    identity.process_id,
                    started.elapsed()
                ))
                .into());
            }
            Ok(Some(status)) => {
                return Err(io::Error::other(format!(
                    "exact child cleanup fallback refused Windows fixture process {} with status {status}",
                    identity.process_id
                ))
                .into());
            }
            Ok(None) if Instant::now() >= deadline => {
                let kill_result = stop.kill();
                let wait_result = stop.wait();
                return Err(io::Error::other(format!(
                    "exact child cleanup fallback timed out for Windows fixture process {} after {:?}; helper cleanup kill={kill_result:?} wait={wait_result:?}",
                    identity.process_id,
                    started.elapsed()
                ))
                .into());
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            Err(error) => {
                let kill_result = stop.kill();
                let wait_result = stop.wait();
                return Err(io::Error::other(format!(
                    "exact child cleanup fallback could not observe Windows fixture process {}: {error}; helper cleanup kill={kill_result:?} wait={wait_result:?}",
                    identity.process_id
                ))
                .into());
            }
        }
    }
}

#[cfg(windows)]
/// Provides the bounded, helper-free exact-process cleanup fallback.
#[allow(unsafe_code)]
mod windows_native_process {
    use super::WindowsProcessIdentity;
    use std::ffi::OsString;
    use std::fs;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::time::Instant;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_TERMINATE: u32 = 0x0001;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    const WAIT_FAILED: u32 = u32::MAX;
    const MAX_PROCESS_PATH: usize = 32_768;

    type Handle = *mut std::ffi::c_void;

    #[repr(C)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(handle: Handle) -> i32;
        fn GetExitCodeProcess(handle: Handle, exit_code: *mut u32) -> i32;
        fn GetProcessTimes(
            handle: Handle,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn QueryFullProcessImageNameW(
            handle: Handle,
            flags: u32,
            executable_path: *mut u16,
            path_length: *mut u32,
        ) -> i32;
        fn TerminateProcess(handle: Handle, exit_code: u32) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }

    /// Owns one exact process handle and closes it on every return path.
    struct ProcessHandle(Handle);

    impl ProcessHandle {
        fn open(process_id: u32) -> io::Result<Self> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
                    0,
                    process_id,
                )
            };
            if handle.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                    return Err(io::Error::new(io::ErrorKind::NotFound, error));
                }
                return Err(error);
            }
            Ok(Self(handle))
        }
    }

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Captures one process identity without starting a helper process.
    pub(super) fn capture_exact(process_id: u32) -> io::Result<WindowsProcessIdentity> {
        let handle = match ProcessHandle::open(process_id) {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(error),
            Err(error) => {
                return Err(io::Error::other(format!(
                    "native exact process identity could not open Windows fixture process {process_id}: {error}"
                )));
            }
        };
        let (creation_file_time_utc, executable_path) = query_identity(handle.0)?;
        Ok(WindowsProcessIdentity {
            process_id,
            creation_file_time_utc,
            executable_path,
        })
    }

    /// Stops only a process whose retained identity still matches, without spawning a helper.
    pub(super) fn stop_exact(
        identity: &WindowsProcessIdentity,
        deadline: Instant,
    ) -> io::Result<()> {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "native exact child cleanup deadline expired for Windows fixture process {}",
                    identity.process_id
                ),
            ));
        }
        let handle = match ProcessHandle::open(identity.process_id) {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(io::Error::other(format!(
                    "native exact child cleanup could not open Windows fixture process {}: {error}",
                    identity.process_id
                )));
            }
        };
        verify_identity(handle.0, identity)?;
        if process_exit_code(handle.0)? != STILL_ACTIVE {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "native exact child cleanup deadline expired before terminating Windows fixture process {}",
                    identity.process_id
                ),
            ));
        }
        let terminated = unsafe { TerminateProcess(handle.0, 1) } != 0;
        if !terminated {
            let error = io::Error::last_os_error();
            if process_exit_code(handle.0).is_ok_and(|exit_code| exit_code != STILL_ACTIVE) {
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "native exact child cleanup could not terminate Windows fixture process {}: {error}",
                identity.process_id
            )));
        }
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(u32::MAX)) as u32;
        let wait_result = unsafe { WaitForSingleObject(handle.0, timeout) };
        let observed_at = Instant::now();
        match wait_result {
            WAIT_OBJECT_0 if observed_at < deadline => Ok(()),
            WAIT_OBJECT_0 => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "native exact child cleanup observed Windows fixture process {} after its deadline",
                    identity.process_id
                ),
            )),
            WAIT_TIMEOUT => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "native exact child cleanup timed out waiting for Windows fixture process {}",
                    identity.process_id
                ),
            )),
            WAIT_FAILED => Err(io::Error::other(format!(
                "native exact child cleanup could not wait for Windows fixture process {}: {}",
                identity.process_id,
                io::Error::last_os_error()
            ))),
            result => Err(io::Error::other(format!(
                "native exact child cleanup returned unexpected wait status {result} for Windows fixture process {}",
                identity.process_id
            ))),
        }
    }

    fn process_exit_code(handle: Handle) -> io::Result<u32> {
        let mut exit_code = 0;
        if unsafe { GetExitCodeProcess(handle, &raw mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(exit_code)
    }

    fn verify_identity(handle: Handle, expected: &WindowsProcessIdentity) -> io::Result<()> {
        let (actual_creation_time, actual_path) = query_identity(handle)?;
        let actual_path = fs::canonicalize(actual_path)?;
        let expected_path = fs::canonicalize(&expected.executable_path)?;
        let actual_path = actual_path.to_str().ok_or_else(|| {
            io::Error::other("native exact child cleanup received a non-UTF-8 executable path")
        })?;
        let expected_path = expected_path.to_str().ok_or_else(|| {
            io::Error::other("native exact child cleanup retained a non-UTF-8 executable path")
        })?;
        if actual_creation_time != expected.creation_file_time_utc
            || !actual_path.eq_ignore_ascii_case(expected_path)
        {
            return Err(io::Error::other(format!(
                "native exact child cleanup refused Windows fixture process {} because its creation time or executable path did not match",
                expected.process_id
            )));
        }
        Ok(())
    }

    fn query_identity(handle: Handle) -> io::Result<(i64, PathBuf)> {
        let mut creation_time = FileTime {
            low_date_time: 0,
            high_date_time: 0,
        };
        let mut exit_time = FileTime {
            low_date_time: 0,
            high_date_time: 0,
        };
        let mut kernel_time = FileTime {
            low_date_time: 0,
            high_date_time: 0,
        };
        let mut user_time = FileTime {
            low_date_time: 0,
            high_date_time: 0,
        };
        if unsafe {
            GetProcessTimes(
                handle,
                &raw mut creation_time,
                &raw mut exit_time,
                &raw mut kernel_time,
                &raw mut user_time,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let actual_creation_time = (u64::from(creation_time.high_date_time) << 32
            | u64::from(creation_time.low_date_time)) as i64;
        let mut path = vec![0; MAX_PROCESS_PATH];
        let mut path_length = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &raw mut path_length) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        path.truncate(path_length as usize);
        Ok((
            actual_creation_time,
            PathBuf::from(OsString::from_wide(&path)),
        ))
    }
}

#[cfg(windows)]
fn stop_windows_fixture_process_native(
    identity: &WindowsProcessIdentity,
    deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    match windows_native_process::stop_exact(identity, deadline) {
        Ok(()) => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "native exact child cleanup failed for Windows fixture process {}: {error}",
            identity.process_id
        ))
        .into()),
    }
}

#[cfg(windows)]
fn windows_fixture_stop_failure_with_fallback(
    identity: &WindowsProcessIdentity,
    detail: impl std::fmt::Display,
    fallback_deadline: Instant,
    final_deadline: Instant,
    fallback_test_delay: Option<Duration>,
    fail_fallback_helper_spawn: bool,
) -> Box<dyn Error> {
    let fallback_result = force_stop_windows_fixture_process(
        identity,
        fallback_deadline,
        fallback_test_delay,
        fail_fallback_helper_spawn,
    );
    let fallback_succeeded = fallback_result.is_ok();
    let fallback_detail = match fallback_result.as_ref() {
        Ok(()) => "exact child cleanup fallback completed".to_string(),
        Err(error) => format!("exact child cleanup fallback failed: {error}"),
    };
    let final_detail = if fallback_succeeded {
        "exact child cleanup final stop skipped after fallback success".to_string()
    } else {
        match stop_windows_fixture_process_native(identity, final_deadline) {
            Ok(()) => "exact child cleanup final stop completed".to_string(),
            Err(error) => format!("exact child cleanup final stop failed: {error}"),
        }
    };
    io::Error::other(format!("{detail}; {fallback_detail}; {final_detail}")).into()
}

#[cfg(windows)]
fn stop_windows_fixture_process_until(
    identity: &WindowsProcessIdentity,
    deadline: Instant,
    test_delay: Option<Duration>,
    observation_delay: Option<Duration>,
) -> Result<(), Box<dyn Error>> {
    stop_windows_fixture_process_until_with_fallback_test_delay(
        identity,
        deadline,
        test_delay,
        observation_delay,
        None,
        false,
        false,
    )
}

#[cfg(windows)]
fn stop_windows_fixture_process_until_with_fallback_test_delay(
    identity: &WindowsProcessIdentity,
    deadline: Instant,
    test_delay: Option<Duration>,
    observation_delay: Option<Duration>,
    fallback_test_delay: Option<Duration>,
    fail_primary_helper_spawn: bool,
    fail_fallback_helper_spawn: bool,
) -> Result<(), Box<dyn Error>> {
    let primary_deadline = deadline
        .checked_sub(CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET)
        .and_then(|deadline| deadline.checked_sub(CODEX_OWNER_CHILD_STOP_FINAL_BUDGET))
        .unwrap_or(deadline);
    let fallback_deadline = deadline
        .checked_sub(CODEX_OWNER_CHILD_STOP_FINAL_BUDGET)
        .unwrap_or(deadline);
    let mut stop =
        match spawn_windows_fixture_stop_helper(identity, test_delay, fail_primary_helper_spawn) {
            Ok(stop) => stop,
            Err(error) => {
                return Err(windows_fixture_stop_failure_with_fallback(
                    identity,
                    format!(
                        "failed to start Windows fixture stop helper for {}: {error}",
                        identity.process_id
                    ),
                    fallback_deadline,
                    deadline,
                    fallback_test_delay,
                    fail_fallback_helper_spawn,
                ));
            }
        };
    let started = Instant::now();
    if let Some(delay) = observation_delay {
        thread::sleep(delay);
    }
    let status = loop {
        match stop.try_wait() {
            Ok(Some(status)) => {
                let observed_elapsed = started.elapsed();
                if Instant::now() >= primary_deadline {
                    let wait_result = stop.wait();
                    let reap_detail = wait_result
                        .err()
                        .map(|error| format!("; late stop-helper reap failed: {error}"))
                        .unwrap_or_default();
                    return Err(windows_fixture_stop_failure_with_fallback(
                        identity,
                        format!(
                            "timed out stopping Windows fixture process {} after {:?}; stop helper completed after deadline (observed after {observed_elapsed:?}){reap_detail}",
                            identity.process_id, observed_elapsed
                        ),
                        fallback_deadline,
                        deadline,
                        fallback_test_delay,
                        fail_fallback_helper_spawn,
                    ));
                }
                break status;
            }
            Ok(None) if Instant::now() >= primary_deadline => {
                let kill_result = stop.kill();
                let wait_result = stop.wait();
                let cleanup_detail = match (kill_result, wait_result) {
                    (Ok(()), Ok(_)) => String::new(),
                    (kill, wait) => {
                        format!("; stop-helper cleanup kill={kill:?} wait={wait:?}")
                    }
                };
                return Err(windows_fixture_stop_failure_with_fallback(
                    identity,
                    format!(
                        "timed out stopping Windows fixture process {} after {:?}{cleanup_detail}",
                        identity.process_id,
                        started.elapsed()
                    ),
                    fallback_deadline,
                    deadline,
                    fallback_test_delay,
                    fail_fallback_helper_spawn,
                ));
            }
            Ok(None) => {
                let remaining = primary_deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            Err(error) => {
                let kill_result = stop.kill();
                let wait_result = stop.wait();
                return Err(windows_fixture_stop_failure_with_fallback(
                    identity,
                    format!(
                        "failed to observe Windows fixture stop helper for {}: {error}; cleanup kill={kill_result:?} wait={wait_result:?}",
                        identity.process_id
                    ),
                    fallback_deadline,
                    deadline,
                    fallback_test_delay,
                    fail_fallback_helper_spawn,
                ));
            }
        }
    };
    if !status.success() {
        return Err(windows_fixture_stop_failure_with_fallback(
            identity,
            format!(
                "refused to stop Windows fixture process {} without its exact captured identity",
                identity.process_id
            ),
            fallback_deadline,
            deadline,
            fallback_test_delay,
            fail_fallback_helper_spawn,
        ));
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_identity_capture_is_bounded() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let started = Instant::now();
    let result = capture_windows_process_identity_with_timeout(
        identity.process_id,
        CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT,
        Some(CODEX_OWNER_IDENTITY_CAPTURE_TEST_DELAY),
        None,
    );
    let elapsed = started.elapsed();
    let kill_result = process.kill();
    let wait_result = process.wait();
    if let Err(error) = kill_result
        && error.kind() != io::ErrorKind::InvalidInput
    {
        return Err(error.into());
    }
    wait_result?;
    if result.is_ok()
        || elapsed
            > CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE
    {
        return Err(io::Error::other(format!(
            "stalled Windows fixture identity capture was not bounded: elapsed={elapsed:?} timeout={CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT:?} scheduler_tolerance={CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE:?} result={result:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_identity_capture_rejects_late_completion() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let result = capture_windows_process_identity_with_timeout(
        identity.process_id,
        CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT,
        None,
        Some(CODEX_OWNER_LATE_COMPLETION_TEST_DELAY),
    );
    let kill_result = process.kill();
    let wait_result = process.wait();
    if let Err(error) = kill_result
        && error.kind() != io::ErrorKind::InvalidInput
    {
        return Err(error.into());
    }
    wait_result?;
    let Err(error) = result else {
        return Err(io::Error::other("late identity capture completion was accepted").into());
    };
    if !error.to_string().contains("completed after") {
        return Err(io::Error::other(format!(
            "late identity capture did not exercise the completion-after-deadline branch: {error}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_stop_helper_is_bounded_and_child_safe() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let started = Instant::now();
    let deadline = started
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
    let result = stop_windows_fixture_process_until_with_fallback_test_delay(
        &identity, deadline, None, None, None, true, false,
    );
    let elapsed = started.elapsed();
    let child_alive_before_cleanup = windows_process_is_alive(&identity)?;
    let wait_result = if child_alive_before_cleanup {
        let kill_result = process.kill();
        let wait_result = process.wait();
        if let Err(error) = kill_result
            && error.kind() != io::ErrorKind::InvalidInput
        {
            return Err(error.into());
        }
        wait_result
    } else {
        process.wait()
    };
    wait_result?;
    let child_alive_after_cleanup = windows_process_is_alive(&identity)?;
    let result_text = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    if result.is_ok()
        || !result_text.contains("exact child cleanup fallback completed")
        || child_alive_after_cleanup
        || elapsed
            > CODEX_OWNER_CHILD_STOP_BUDGET
                + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
                + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE
    {
        return Err(io::Error::other(format!(
            "stalled Windows fixture stop helper was not bounded or left its child alive: elapsed={elapsed:?} timeout={CODEX_OWNER_CHILD_STOP_BUDGET:?} fallback={CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET:?} scheduler_tolerance={CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE:?} child_alive_before_cleanup={child_alive_before_cleanup} child_alive_after_cleanup={child_alive_after_cleanup} result={result:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_stop_helper_total_cleanup_deadline_is_bounded() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let started = Instant::now();
    let deadline = started
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
    let result = stop_windows_fixture_process_until_with_fallback_test_delay(
        &identity,
        deadline,
        Some(CODEX_OWNER_STOP_HELPER_TEST_DELAY),
        None,
        Some(CODEX_OWNER_STOP_HELPER_TEST_DELAY),
        false,
        false,
    );
    let elapsed = started.elapsed();
    let child_alive_after_cleanup = windows_process_is_alive(&identity)?;
    process.wait()?;
    let result_text = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    let helper_budget = CODEX_OWNER_CHILD_STOP_BUDGET + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET;
    let total_budget = helper_budget + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
    let lower_bound = helper_budget.saturating_sub(CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE);
    let upper_bound = total_budget + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
    if result.is_ok()
        || child_alive_after_cleanup
        || !result_text.contains("timed out stopping Windows fixture process")
        || !result_text.contains("exact child cleanup fallback timed out")
        || !result_text.contains("exact child cleanup final stop completed")
        || elapsed < lower_bound
        || elapsed > upper_bound
    {
        return Err(io::Error::other(format!(
            "stalled Windows fixture cleanup exceeded its absolute deadline or skipped a bounded phase: elapsed={elapsed:?} lower_bound={lower_bound:?} upper_bound={upper_bound:?} child_alive_after_cleanup={child_alive_after_cleanup} result={result:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_stop_helper_primary_spawn_failure_cleans_exact_child()
-> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let mut sentinel = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let sentinel_identity = match capture_windows_process_identity(sentinel.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            drop(sentinel.kill());
            drop(sentinel.wait());
            return Err(error);
        }
    };
    let test_result = (|| -> Result<(), Box<dyn Error>> {
        let started = Instant::now();
        let deadline = started
            + CODEX_OWNER_CHILD_STOP_BUDGET
            + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
            + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
        let result = stop_windows_fixture_process_until_with_fallback_test_delay(
            &identity, deadline, None, None, None, true, true,
        );
        let elapsed = started.elapsed();
        let exact_child_alive_before_cleanup = windows_process_is_alive(&identity)?;
        let mut mismatched_sentinel_identity = sentinel_identity.clone();
        mismatched_sentinel_identity.creation_file_time_utc += 1;
        let mismatch_result = stop_windows_fixture_process_until_with_fallback_test_delay(
            &mismatched_sentinel_identity,
            Instant::now() + CODEX_OWNER_CHILD_STOP_BUDGET,
            None,
            None,
            None,
            true,
            true,
        );
        let sentinel_alive_after_mismatch = windows_process_is_alive(&sentinel_identity)?;
        let result_text = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let mismatch_text = mismatch_result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let total_budget = CODEX_OWNER_CHILD_STOP_BUDGET
            + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
            + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET;
        let upper_bound = total_budget + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
        if result.is_ok()
            || result_text
                .matches("test-injected Windows fixture stop-helper spawn failure")
                .count()
                < 2
            || !result_text.contains("exact child cleanup fallback failed")
            || !result_text.contains("exact child cleanup final stop completed")
            || exact_child_alive_before_cleanup
            || mismatch_result.is_ok()
            || !mismatch_text.contains("native exact child cleanup failed")
            || !mismatch_text.contains("creation time or executable path did not match")
            || !sentinel_alive_after_mismatch
            || elapsed > upper_bound
        {
            return Err(io::Error::other(format!(
                "primary stop-helper spawn failure did not preserve bounded exact cleanup: elapsed={elapsed:?} upper_bound={upper_bound:?} exact_child_alive_before_cleanup={exact_child_alive_before_cleanup} sentinel_alive_after_mismatch={sentinel_alive_after_mismatch} mismatch_result={mismatch_result:?} result={result:?}"
            ))
            .into());
        }
        Ok(())
    })();
    let exact_cleanup_result = if windows_process_is_alive(&identity)? {
        let kill_result = process.kill();
        let wait_result = process.wait();
        if let Err(error) = kill_result
            && error.kind() != io::ErrorKind::InvalidInput
        {
            Err(error)
        } else {
            wait_result
        }
    } else {
        process.wait()
    };
    let sentinel_cleanup_result = if windows_process_is_alive(&sentinel_identity)? {
        let cleanup_result = stop_windows_fixture_process(&sentinel_identity);
        let wait_result = sentinel.wait();
        cleanup_result.and(wait_result.map_err(Into::into))
    } else {
        sentinel.wait().map_err(Into::into)
    };
    exact_cleanup_result?;
    sentinel_cleanup_result?;
    test_result
}

#[test]
#[cfg(windows)]
fn windows_codex_owner_native_cleanup_retires_child_when_helpers_fail() -> Result<(), Box<dyn Error>>
{
    windows_codex_owner_native_cleanup_with_injected_failure(
        WindowsNativeCleanupInjectedFailure::None,
    )
}

#[test]
#[cfg(windows)]
fn windows_codex_owner_native_cleanup_retires_child_when_sentinel_spawn_fails()
-> Result<(), Box<dyn Error>> {
    windows_codex_owner_native_cleanup_with_injected_failure(
        WindowsNativeCleanupInjectedFailure::SentinelSpawn,
    )
}

#[cfg(windows)]
#[test]
fn windows_codex_owner_native_cleanup_retires_processes_when_owner_identity_capture_fails()
-> Result<(), Box<dyn Error>> {
    windows_codex_owner_native_cleanup_with_injected_failure(
        WindowsNativeCleanupInjectedFailure::OwnerIdentityCapture,
    )
}

#[cfg(windows)]
#[test]
fn windows_codex_owner_native_cleanup_retires_processes_when_sentinel_identity_capture_fails()
-> Result<(), Box<dyn Error>> {
    windows_codex_owner_native_cleanup_with_injected_failure(
        WindowsNativeCleanupInjectedFailure::SentinelIdentityCapture,
    )
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsNativeCleanupInjectedFailure {
    None,
    OwnerIdentityCapture,
    SentinelSpawn,
    SentinelIdentityCapture,
}

#[cfg(windows)]
fn windows_codex_owner_native_cleanup_with_injected_failure(
    injected_failure: WindowsNativeCleanupInjectedFailure,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let runtime = temp
        .path()
        .join(OBSOLETE_PROJECTATLAS_FIXTURE_EXECUTABLE_FILE_NAME);
    let codex_fixture = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    let db = temp.path().join("projectatlas.db");
    let identity_file = temp.path().join("uncooperative-owner.pid");
    compile_obsolete_projectatlas_fixture(&runtime)?;
    compile_codex_mcp_owner_fixture(&codex_fixture)?;

    let (mut owner, child_identity) = spawn_codex_owned_obsolete_mcp(
        &codex_fixture,
        &runtime,
        &db,
        None,
        &identity_file,
        None,
        Some("ignore-stop"),
    )?;
    let injected_owner_identity = if injected_failure
        == WindowsNativeCleanupInjectedFailure::OwnerIdentityCapture
    {
        match capture_windows_process_identity(owner.id()) {
            Ok(identity) => Some(identity),
            Err(error) => {
                let cleanup_result = cleanup_codex_owner_processes(owner, &child_identity);
                return Err(io::Error::other(format!(
                    "failed to prepare the injected owner identity-capture failure: {error}; child-first owner cleanup={cleanup_result:?}"
                ))
                .into());
            }
        }
    } else {
        None
    };
    let owner_identity = match if injected_owner_identity.is_some() {
        Err(io::Error::other("test-injected Windows owner identity capture failure").into())
    } else {
        capture_windows_process_identity(owner.id())
    } {
        Ok(identity) => identity,
        Err(error) => {
            let cleanup_started = Instant::now();
            let cleanup_result = cleanup_codex_owner_processes(owner, &child_identity);
            if injected_failure == WindowsNativeCleanupInjectedFailure::OwnerIdentityCapture {
                let expected_owner_identity =
                    injected_owner_identity.as_ref().ok_or_else(|| {
                        io::Error::other(
                            "injected owner capture failure omitted its exact test identity",
                        )
                    })?;
                let child_alive_after_cleanup = windows_process_is_alive(&child_identity)?;
                let owner_alive_after_cleanup = windows_process_is_alive(expected_owner_identity)?;
                let cleanup_elapsed = cleanup_started.elapsed();
                let cleanup_upper_bound = CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                    + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                    + CODEX_OWNER_CHILD_STOP_BUDGET
                    + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
                    + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
                    + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
                let safety_child_cleanup = if child_alive_after_cleanup {
                    stop_windows_fixture_process(&child_identity)
                } else {
                    Ok(())
                };
                let safety_owner_cleanup = if owner_alive_after_cleanup {
                    stop_windows_fixture_process(expected_owner_identity)
                } else {
                    Ok(())
                };
                if !error
                    .to_string()
                    .contains("test-injected Windows owner identity capture failure")
                    || cleanup_result.is_err()
                    || child_alive_after_cleanup
                    || owner_alive_after_cleanup
                    || cleanup_elapsed > cleanup_upper_bound
                {
                    return Err(io::Error::other(format!(
                        "forced owner identity-capture failure did not preserve bounded child-first cleanup: elapsed={cleanup_elapsed:?} upper_bound={cleanup_upper_bound:?} child_alive_after_cleanup={child_alive_after_cleanup} owner_alive_after_cleanup={owner_alive_after_cleanup} cleanup={cleanup_result:?} safety_child_cleanup={safety_child_cleanup:?} safety_owner_cleanup={safety_owner_cleanup:?}"
                    ))
                    .into());
                }
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "failed to capture uncooperative Codex owner identity: {error}; child-first owner cleanup={cleanup_result:?}"
            ))
            .into());
        }
    };

    let mut sentinel = match if injected_failure
        == WindowsNativeCleanupInjectedFailure::SentinelSpawn
    {
        Err(io::Error::other(
            "test-injected mismatched Windows sentinel spawn failure",
        ))
    } else {
        StdCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg("Start-Sleep -Seconds 30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    } {
        Ok(sentinel) => sentinel,
        Err(error) => {
            // Force this setup-failure arm in one test without relying on host resource exhaustion.
            let cleanup_started = Instant::now();
            let cleanup_result = cleanup_codex_owner_processes(owner, &child_identity);
            let child_alive_after_cleanup = windows_process_is_alive(&child_identity)?;
            let owner_alive_after_cleanup = windows_process_is_alive(&owner_identity)?;
            let cleanup_upper_bound = CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                + CODEX_OWNER_CHILD_STOP_BUDGET
                + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
                + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
                + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
            if injected_failure == WindowsNativeCleanupInjectedFailure::SentinelSpawn {
                if !error
                    .to_string()
                    .contains("test-injected mismatched Windows sentinel spawn failure")
                    || cleanup_result.is_err()
                    || child_alive_after_cleanup
                    || owner_alive_after_cleanup
                    || cleanup_started.elapsed() > cleanup_upper_bound
                {
                    return Err(io::Error::other(format!(
                        "forced mismatched sentinel spawn failure did not preserve bounded child-first owner cleanup: elapsed={:?} upper_bound={cleanup_upper_bound:?} child_alive_after_cleanup={child_alive_after_cleanup} owner_alive_after_cleanup={owner_alive_after_cleanup} cleanup={cleanup_result:?}",
                        cleanup_started.elapsed()
                    ))
                    .into());
                }
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "failed to start mismatched Windows sentinel: {error}; child-first owner cleanup={cleanup_result:?}"
            ))
            .into());
        }
    };
    let injected_sentinel_identity = if injected_failure
        == WindowsNativeCleanupInjectedFailure::SentinelIdentityCapture
    {
        match capture_windows_process_identity(sentinel.id()) {
            Ok(identity) => Some(identity),
            Err(error) => {
                let owner_cleanup = cleanup_codex_owner_processes(owner, &child_identity);
                let sentinel_kill = sentinel.kill();
                let sentinel_wait = sentinel.wait();
                return Err(io::Error::other(format!(
                    "failed to prepare the injected sentinel identity-capture failure: {error}; child-first owner cleanup={owner_cleanup:?}; sentinel cleanup kill={sentinel_kill:?} wait={sentinel_wait:?}"
                ))
                .into());
            }
        }
    } else {
        None
    };
    let sentinel_identity = match if injected_sentinel_identity.is_some() {
        Err(io::Error::other("test-injected Windows sentinel identity capture failure").into())
    } else {
        capture_windows_process_identity(sentinel.id())
    } {
        Ok(identity) => identity,
        Err(error) => {
            let cleanup_started = Instant::now();
            let owner_cleanup = cleanup_codex_owner_processes(owner, &child_identity);
            let sentinel_kill = sentinel.kill();
            let sentinel_wait = sentinel.wait();
            if injected_failure == WindowsNativeCleanupInjectedFailure::SentinelIdentityCapture {
                let expected_sentinel_identity =
                    injected_sentinel_identity.as_ref().ok_or_else(|| {
                        io::Error::other(
                            "injected sentinel capture failure omitted its exact test identity",
                        )
                    })?;
                let child_alive_after_cleanup = windows_process_is_alive(&child_identity)?;
                let owner_alive_after_cleanup = windows_process_is_alive(&owner_identity)?;
                let sentinel_alive_after_cleanup =
                    windows_process_is_alive(expected_sentinel_identity)?;
                let cleanup_elapsed = cleanup_started.elapsed();
                let cleanup_upper_bound = CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                    + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
                    + CODEX_OWNER_CHILD_STOP_BUDGET
                    + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
                    + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
                    + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
                let safety_child_cleanup = if child_alive_after_cleanup {
                    stop_windows_fixture_process(&child_identity)
                } else {
                    Ok(())
                };
                let safety_owner_cleanup = if owner_alive_after_cleanup {
                    stop_windows_fixture_process(&owner_identity)
                } else {
                    Ok(())
                };
                let safety_sentinel_cleanup = if sentinel_alive_after_cleanup {
                    stop_windows_fixture_process(expected_sentinel_identity)
                } else {
                    Ok(())
                };
                if !error
                    .to_string()
                    .contains("test-injected Windows sentinel identity capture failure")
                    || owner_cleanup.is_err()
                    || sentinel_kill.is_err()
                    || sentinel_wait.is_err()
                    || child_alive_after_cleanup
                    || owner_alive_after_cleanup
                    || sentinel_alive_after_cleanup
                    || cleanup_elapsed > cleanup_upper_bound
                {
                    return Err(io::Error::other(format!(
                        "forced sentinel identity-capture failure did not preserve bounded child-first owner and sentinel cleanup: elapsed={cleanup_elapsed:?} upper_bound={cleanup_upper_bound:?} child_alive_after_cleanup={child_alive_after_cleanup} owner_alive_after_cleanup={owner_alive_after_cleanup} sentinel_alive_after_cleanup={sentinel_alive_after_cleanup} owner_cleanup={owner_cleanup:?} sentinel_kill={sentinel_kill:?} sentinel_wait={sentinel_wait:?} safety_child_cleanup={safety_child_cleanup:?} safety_owner_cleanup={safety_owner_cleanup:?} safety_sentinel_cleanup={safety_sentinel_cleanup:?}"
                    ))
                    .into());
                }
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "failed to capture mismatched Windows sentinel identity: {error}; child-first owner cleanup={owner_cleanup:?}; sentinel cleanup kill={sentinel_kill:?} wait={sentinel_wait:?}"
            ))
            .into());
        }
    };

    let test_result = (|| -> Result<(), Box<dyn Error>> {
        if !windows_process_is_alive(&child_identity)? || owner.try_wait()?.is_some() {
            return Err(io::Error::other(
                "uncooperative Codex owner did not retain a live exact child before cleanup",
            )
            .into());
        }
        let started = Instant::now();
        let result = stop_codex_owner_after_spawn_failure_with_test_delays(
            &mut owner,
            &identity_file,
            &runtime,
            None,
            None,
            None,
            true,
        );
        let elapsed = started.elapsed();
        let owner_reaped = owner.try_wait()?.is_some();
        let child_alive_after_cleanup = windows_process_is_alive(&child_identity)?;
        let result_text = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let mut mismatched_sentinel_identity = sentinel_identity.clone();
        mismatched_sentinel_identity.creation_file_time_utc += 1;
        let mismatch_started = Instant::now();
        let mismatch_result = stop_windows_fixture_process_until_with_fallback_test_delay(
            &mismatched_sentinel_identity,
            mismatch_started
                + CODEX_OWNER_CHILD_STOP_BUDGET
                + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
                + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET,
            None,
            None,
            None,
            true,
            true,
        );
        let mismatch_elapsed = mismatch_started.elapsed();
        let sentinel_alive_after_mismatch = windows_process_is_alive(&sentinel_identity)?;
        let total_budget = CODEX_OWNER_FAILURE_CLEANUP_BUDGET
            + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
            + CODEX_OWNER_CHILD_STOP_BUDGET
            + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
            + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
            + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
        let mismatch_budget = CODEX_OWNER_CHILD_STOP_BUDGET
            + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
            + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
            + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE;
        if result.is_ok()
            || !result_text.contains("owner fixture did not stop within five seconds")
            || result_text
                .matches("test-injected Windows fixture stop-helper spawn failure")
                .count()
                < 2
            || !result_text.contains("exact child cleanup fallback failed")
            || !result_text.contains("exact child cleanup final stop completed")
            || !owner_reaped
            || child_alive_after_cleanup
            || elapsed < CODEX_OWNER_FAILURE_CLEANUP_BUDGET
            || elapsed > total_budget
            || mismatch_result.is_ok()
            || !mismatch_result.as_ref().err().is_some_and(|error| {
                error
                    .to_string()
                    .contains("native exact child cleanup failed")
            })
            || !mismatch_result.as_ref().err().is_some_and(|error| {
                error
                    .to_string()
                    .contains("creation time or executable path did not match")
            })
            || !sentinel_alive_after_mismatch
            || mismatch_elapsed > mismatch_budget
        {
            return Err(io::Error::other(format!(
                "uncooperative Codex owner did not preserve native child-first cleanup: elapsed={elapsed:?} total_budget={total_budget:?} owner_reaped={owner_reaped} child_alive_after_cleanup={child_alive_after_cleanup} sentinel_alive_after_mismatch={sentinel_alive_after_mismatch} mismatch_elapsed={mismatch_elapsed:?} mismatch_budget={mismatch_budget:?} result={result:?} mismatch_result={mismatch_result:?}"
            ))
            .into());
        }
        Ok(())
    })();

    let owner_cleanup_result = if owner.try_wait()?.is_none() {
        let kill_result = owner.kill();
        let wait_result = owner.wait();
        if let Err(error) = kill_result
            && error.kind() != io::ErrorKind::InvalidInput
        {
            Err(error)
        } else {
            wait_result
        }
    } else {
        owner.wait()
    };
    let child_cleanup_result = if windows_process_is_alive(&child_identity)? {
        stop_windows_fixture_process(&child_identity)
    } else {
        Ok(())
    };
    let sentinel_cleanup_result = if windows_process_is_alive(&sentinel_identity)? {
        let cleanup_result = stop_windows_fixture_process(&sentinel_identity);
        let wait_result = sentinel.wait();
        cleanup_result.and(wait_result.map_err(Into::into))
    } else {
        sentinel.wait().map_err(Into::into)
    };
    owner_cleanup_result?;
    child_cleanup_result?;
    sentinel_cleanup_result?;
    test_result
}

#[test]
#[cfg(windows)]
fn windows_fixture_stop_helper_rejects_late_completion() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let deadline = Instant::now() + CODEX_OWNER_IDENTITY_CAPTURE_TEST_TIMEOUT;
    let result = stop_windows_fixture_process_until(
        &identity,
        deadline,
        None,
        Some(CODEX_OWNER_LATE_COMPLETION_TEST_DELAY),
    );
    let child_alive = windows_process_is_alive(&identity)?;
    if child_alive {
        drop(process.kill());
    }
    process.wait()?;
    if child_alive
        || result.as_ref().is_ok()
        || !result
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("completed after deadline"))
    {
        return Err(io::Error::other(format!(
            "late stop-helper completion did not fail closed or clean its child: child_alive={child_alive} result={result:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_identity_observed_after_readiness_is_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let runtime = temp
        .path()
        .join(OBSOLETE_PROJECTATLAS_FIXTURE_EXECUTABLE_FILE_NAME);
    let codex_fixture = temp.path().join(CODEX_FIXTURE_EXECUTABLE_FILE_NAME);
    let db = temp.path().join("projectatlas.db");
    let identity_file = temp.path().join("late-publication.pid");
    compile_obsolete_projectatlas_fixture(&runtime)?;
    compile_codex_mcp_owner_fixture(&codex_fixture)?;

    let started = Instant::now();
    let result = spawn_codex_owned_obsolete_mcp_with_test_delays(
        &codex_fixture,
        &runtime,
        &db,
        None,
        &identity_file,
        Some(CODEX_OWNER_DELAYED_PUBLICATION),
        Some("late-publication"),
        None,
        Some(CODEX_OWNER_OBSERVATION_TEST_DELAY),
    );
    let elapsed = started.elapsed();
    let error = match result {
        Ok((parent, child_identity)) => {
            return Err(codex_owner_unexpected_acceptance_error(
                "late-publication",
                parent,
                &child_identity,
            ));
        }
        Err(error) => error,
    };
    let text = error.to_string();
    let retained_identity =
        read_codex_owner_identity_record(&codex_owner_retained_identity_path(&identity_file))?;
    let total_upper_bound = CODEX_OWNER_READINESS_TIMEOUT
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_CHILD_STOP_BUDGET
        + CODEX_OWNER_CHILD_STOP_FALLBACK_BUDGET
        + CODEX_OWNER_CHILD_STOP_FINAL_BUDGET
        + CODEX_OWNER_CLEANUP_SCHEDULER_TOLERANCE
        + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    let readiness_elapsed = text
        .split("readiness_elapsed_ms=")
        .nth(1)
        .and_then(|value| value.split(')').next())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .ok_or_else(|| {
            io::Error::other(format!(
                "late-observed identity timeout omitted its readiness elapsed diagnostic: {text}"
            ))
        })?;
    // The helper starts its absolute deadline before spawning the fixture. Keep the wall-clock
    // assertion's process-start allowance bounded while preserving the 30-second readiness
    // contract and the explicit observer delay.
    let readiness_observation_upper_bound = CODEX_OWNER_OBSERVATION_TEST_DELAY
        + CODEX_OWNER_FAILURE_CLEANUP_BUDGET
        + CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE;
    if elapsed < CODEX_OWNER_READINESS_TIMEOUT
        || elapsed > total_upper_bound
        || readiness_elapsed <= CODEX_OWNER_READINESS_TIMEOUT
        || readiness_elapsed > readiness_observation_upper_bound
        || !text.contains("published its child PID after the readiness deadline")
        || !text.contains(&format!("owner={}", codex_fixture.display()))
        || !text.contains(&format!("identity_file={}", identity_file.display()))
        || !text.contains(&format!("expected_runtime={}", runtime.display()))
        || windows_process_is_alive(&retained_identity)?
    {
        return Err(io::Error::other(format!(
            "late-observed identity was accepted or not bounded: elapsed={elapsed:?} readiness_elapsed={readiness_elapsed:?} readiness={CODEX_OWNER_READINESS_TIMEOUT:?} readiness_observation_upper_bound={readiness_observation_upper_bound:?} readiness_scheduler_tolerance={CODEX_OWNER_READINESS_SCHEDULER_TOLERANCE:?} cleanup_deadline={total_upper_bound:?}\n{text}"
        ))
        .into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
fn windows_fixture_cleanup_requires_exact_process_identity() -> Result<(), Box<dyn Error>> {
    let mut process = StdCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("Start-Sleep -Seconds 30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let identity = match capture_windows_process_identity(process.id()) {
        Ok(identity) => identity,
        Err(error) => {
            drop(process.kill());
            drop(process.wait());
            return Err(error);
        }
    };
    let mut changed_identity = identity.clone();
    changed_identity.creation_file_time_utc += 1;
    let test_result = (|| -> Result<(), Box<dyn Error>> {
        if windows_process_is_alive(&changed_identity)? {
            return Err(io::Error::other("changed Windows fixture identity appeared live").into());
        }
        if stop_windows_fixture_process(&changed_identity).is_ok() {
            return Err(
                io::Error::other("cleanup accepted a changed Windows fixture identity").into(),
            );
        }
        if !windows_process_is_alive(&identity)? {
            return Err(io::Error::other("identity-safe cleanup stopped the wrong process").into());
        }
        Ok(())
    })();
    if windows_process_is_alive(&identity)? {
        let cleanup_result = stop_windows_fixture_process(&identity);
        if let Err(error) = cleanup_result
            && test_result.is_ok()
        {
            return Err(error);
        }
    }
    process.wait()?;
    test_result
}

/// Run the bundled plugin installer with an explicit runtime path.
fn run_projectatlas_plugin_installer(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_projectatlas_plugin_installer_with_optional_path(workspace_root, repo, runtime, None)
}

/// Run the bundled plugin installer against the isolated Codex fixture.
fn run_plugin_installer_with_codex_fixture(
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
    let mut command = projectatlas_plugin_installer_command_with_optional_path_and_home(
        workspace_root,
        repo,
        runtime,
        path_shadow,
        home,
    )?;
    require_successful_plugin_installer_output(command.output()?)
}

/// Build the bundled plugin installer command for tests that need process coordination.
fn projectatlas_plugin_installer_command_with_optional_path_and_home(
    workspace_root: &Path,
    repo: &Path,
    runtime: &Path,
    path_shadow: Option<&Path>,
    home: Option<&Path>,
) -> Result<StdCommand, Box<dyn Error>> {
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
        let codex_dir = home.join(CODEX_CONFIG_DIR);
        let fake_plugin_source = fake_codex_projectatlas_plugin_source(&codex_dir);
        let current_plugin_cache =
            fake_codex_projectatlas_installed_cache(&codex_dir, env!("CARGO_PKG_VERSION"));
        let fake_installed_plugin_cache = if current_plugin_cache.exists() {
            current_plugin_cache
        } else {
            fake_codex_projectatlas_installed_cache(&codex_dir, "0.0.1")
        };
        fs::create_dir_all(&app_data)?;
        fs::create_dir_all(&local_app_data)?;
        command
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("CODEX_HOME", &codex_dir)
            .env("APPDATA", app_data)
            .env("LOCALAPPDATA", local_app_data)
            .env(
                "PROJECTATLAS_FAKE_CODEX_LOG",
                home.join(FAKE_CODEX_LOG_FILE),
            )
            .env(
                "PROJECTATLAS_FAKE_CODEX_CONFIG",
                codex_dir.join("config.toml"),
            )
            .env("PROJECTATLAS_FAKE_PLUGIN_ROOT", &fake_plugin_source)
            .env(
                "PROJECTATLAS_FAKE_MARKETPLACE_ROOT",
                fake_codex_projectatlas_marketplace_root(&codex_dir),
            )
            .env(
                "PROJECTATLAS_FAKE_MARKETPLACE_MANIFEST",
                fake_codex_projectatlas_marketplace_root(&codex_dir)
                    .join(CODEX_MARKETPLACE_METADATA_DIR)
                    .join("plugins")
                    .join(CODEX_MARKETPLACE_MANIFEST_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_MARKETPLACE_INSTALL_RECORD",
                fake_codex_projectatlas_marketplace_root(&codex_dir)
                    .join(CODEX_MARKETPLACE_INSTALL_RECORD_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_INSTALLED_PLUGIN_ROOT",
                &fake_installed_plugin_cache,
            )
            .env(
                "PROJECTATLAS_FAKE_PLUGIN_MANIFEST",
                fake_plugin_source
                    .join(CODEX_PLUGIN_MANIFEST_DIR)
                    .join("plugin.json"),
            )
            .env(
                "PROJECTATLAS_FAKE_PLUGIN_SKILL",
                fake_plugin_source
                    .join(PROJECTATLAS_SKILL_DIR)
                    .join(PROJECTATLAS_SKILL_NAME)
                    .join(SKILL_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_PLUGIN_RUNTIME_INTEGRATION",
                fake_plugin_source.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_INSTALLED_PLUGIN_MANIFEST",
                fake_installed_plugin_cache
                    .join(CODEX_PLUGIN_MANIFEST_DIR)
                    .join("plugin.json"),
            )
            .env(
                "PROJECTATLAS_FAKE_INSTALLED_PLUGIN_SKILL",
                fake_installed_plugin_cache
                    .join(PROJECTATLAS_SKILL_DIR)
                    .join(PROJECTATLAS_SKILL_NAME)
                    .join(SKILL_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_INSTALLED_PLUGIN_RUNTIME_INTEGRATION",
                fake_installed_plugin_cache.join(CODEX_PLUGIN_RUNTIME_INTEGRATION_FILE_NAME),
            )
            .env(
                "PROJECTATLAS_FAKE_JUNCTION_TARGET",
                home.join(FAKE_CODEX_JUNCTION_TARGET_DIR),
            )
            .env(
                "PROJECTATLAS_FAKE_CLEANUP_SNAPSHOT_TARGET",
                home.join(FAKE_CODEX_CLEANUP_SNAPSHOT_TARGET_DIR),
            )
            .env(
                "PROJECTATLAS_PACKAGED_SKILL",
                workspace_root
                    .join("plugins")
                    .join("projectatlas")
                    .join(PROJECTATLAS_SKILL_DIR)
                    .join(PROJECTATLAS_SKILL_NAME)
                    .join(SKILL_FILE_NAME),
            );
        for (name, file_name) in [
            ("PROJECTATLAS_FAKE_CODEX_STATE", "codex-registry-state.txt"),
            (
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_STALE",
                "codex-registry-stale.json",
            ),
            (
                "PROJECTATLAS_FAKE_CODEX_REGISTRY_CURRENT",
                "codex-registry-current.json",
            ),
        ] {
            let path = home.join(file_name);
            if path.exists() || name == "PROJECTATLAS_FAKE_CODEX_STATE" {
                command.env(name, path);
            }
        }
    }
    Ok(command)
}

/// Reap one test-owned installer child after the observer has returned.
fn reap_plugin_installer_child(mut child: Child) -> Result<std::process::Output, Box<dyn Error>> {
    child.stdin.take();
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    Ok(child.wait_with_output()?)
}

/// Collect one coordinated installer process under an explicit deadline.
fn wait_for_plugin_installer_output(
    child: Child,
    label: &str,
    timeout: Duration,
) -> Result<std::process::Output, Box<dyn Error>> {
    wait_for_plugin_installer_output_with_test_delay(child, label, timeout, None)
}

fn wait_for_plugin_installer_output_with_test_delay(
    child: Child,
    label: &str,
    timeout: Duration,
    observer_delay: Option<Duration>,
) -> Result<std::process::Output, Box<dyn Error>> {
    wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
        child,
        label,
        timeout,
        observer_delay,
        None,
        None,
        &mut |child| child.kill(),
        None,
    )
}

/// Test-only variant that transfers a proven-live child to the caller when
/// injected termination cannot safely reap it here.
fn wait_for_plugin_installer_output_with_test_delay_and_kill_and_handoff(
    mut child: Child,
    label: &str,
    timeout: Duration,
    observer_delay: Option<Duration>,
    exit_probe_error: Option<io::Error>,
    cleanup_probe_error: Option<io::Error>,
    kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
    handoff_live_child: Option<&mut dyn FnMut(Child)>,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut exit_probe_error = exit_probe_error;
    let mut cleanup_probe_error = cleanup_probe_error;
    if observer_delay.is_some()
        && let Err(error) = synchronize_prompt_exit_before_delayed_observation(
            &mut child,
            label,
            exit_probe_error.take(),
        )
    {
        let kill_result = kill_child(&mut child);
        child.stdin.take();
        let status_after_kill = child.try_wait();
        if kill_result.is_err() && !matches!(&status_after_kill, Ok(Some(_))) {
            if let Some(handoff) = handoff_live_child {
                handoff(child);
            } else {
                drop(child);
            }
            let mut diagnostic = format!(
                "{label} plugin installer exit synchronization failed before delayed observation: {error}; cleanup incomplete: child detached"
            );
            if let Some(kill_error) = kill_result.as_ref().err() {
                diagnostic.push_str("; termination failed: ");
                diagnostic.push_str(&kill_error.to_string());
            }
            if let Err(probe_error) = status_after_kill {
                diagnostic.push_str("; re-probe failed after termination attempt: ");
                diagnostic.push_str(&probe_error.to_string());
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
        }
        let output = child.wait_with_output()?;
        let diagnostic = format!(
            "{label} plugin installer exit synchronization failed before delayed observation: {error}; cleanup complete: child reaped and output drained status={}",
            output.status
        );
        return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("plugin installer deadline overflowed"))?;
    if let Some(delay) = observer_delay {
        thread::sleep(delay);
    }
    loop {
        if Instant::now() >= deadline {
            let mut pre_termination_probe_error = None;
            let (status, _observed_at) = {
                let status = match cleanup_probe_error.take() {
                    Some(error) => {
                        pre_termination_probe_error = Some(error);
                        None
                    }
                    None => match child.try_wait() {
                        Ok(status) => status,
                        Err(error) => {
                            pre_termination_probe_error = Some(error);
                            None
                        }
                    },
                };
                let observed_at = Instant::now();
                (status, observed_at)
            };
            let still_running = status.is_none();
            let mut post_termination_probe_error = None;
            if still_running {
                let kill_result = kill_child(&mut child);
                let status_after_kill = match exit_probe_error.take() {
                    Some(error) => Err(error),
                    None => child.try_wait(),
                };
                let status_after_kill = match status_after_kill {
                    Ok(status) => status,
                    Err(error) if kill_result.is_ok() => {
                        post_termination_probe_error = Some(error);
                        None
                    }
                    Err(error) => {
                        child.stdin.take();
                        if let Some(handoff) = handoff_live_child {
                            handoff(child);
                        } else {
                            drop(child);
                        }
                        let mut diagnostic = format!(
                            "{label} plugin installer exceeded {timeout:?}: still running at deadline status=unknown (re-probe failed after termination attempt: {error}; cleanup incomplete: child detached)"
                        );
                        if let Some(kill_error) = kill_result.as_ref().err() {
                            diagnostic.push_str("; termination failed: ");
                            diagnostic.push_str(&kill_error.to_string());
                        }
                        return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                    }
                };
                if let Err(kill_error) = kill_result
                    && status_after_kill.is_none()
                {
                    child.stdin.take();
                    if let Some(handoff) = handoff_live_child {
                        handoff(child);
                    } else {
                        drop(child);
                    }
                    let diagnostic = format!(
                        "{label} plugin installer exceeded {timeout:?}: still running at deadline status=still-running at deadline (termination failed: {kill_error}; cleanup incomplete: operating system refused termination; child was not reaped; child detached)"
                    );
                    return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                }
            }
            let output = child.wait_with_output()?;
            let mut diagnostic = format!(
                "{label} plugin installer exceeded {timeout:?}: {}",
                if still_running {
                    "still running at deadline"
                } else {
                    "completed after deadline"
                }
            );
            if let Some(error) = post_termination_probe_error {
                diagnostic
                    .push_str(" status=unknown (re-probe failed after successful termination: ");
                diagnostic.push_str(&error.to_string());
                diagnostic.push(')');
            }
            if let Some(error) = pre_termination_probe_error {
                diagnostic
                    .push_str(" status=unknown (re-probe failed before termination attempt: ");
                diagnostic.push_str(&error.to_string());
                diagnostic.push(')');
            }
            diagnostic.push_str(" status=");
            diagnostic.push_str(&output.status.to_string());
            diagnostic.push_str("\nstdout:\n");
            diagnostic.push_str(&String::from_utf8_lossy(&output.stdout));
            diagnostic.push_str("\nstderr:\n");
            diagnostic.push_str(&String::from_utf8_lossy(&output.stderr));
            return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
        }
        let (status, observed_at) = {
            let status = child.try_wait()?;
            let observed_at = Instant::now();
            (status, observed_at)
        };
        if let Some(_status) = status {
            if observed_at < deadline {
                return Ok(child.wait_with_output()?);
            }
            let output = child.wait_with_output()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "{label} plugin installer exceeded {timeout:?}: completed after deadline (observed_at={observed_at:?}) status={}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            )
            .into());
        }
        let remaining = deadline.saturating_duration_since(observed_at);
        if remaining.is_zero() {
            continue;
        }
        thread::sleep(Duration::from_millis(25).min(remaining));
    }
}

/// Require a plugin installer process to exit successfully and preserve its output.
fn require_successful_plugin_installer_output(
    output: std::process::Output,
) -> Result<std::process::Output, Box<dyn Error>> {
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

/// Require the shared privacy-safe CLI/MCP schema mismatch contract.
fn require_schema_version_mismatch(
    value: &Value,
    found: i64,
    supported: i64,
) -> Result<(), Box<dyn Error>> {
    require_json_string(value, &["error", "kind"], "schema_version_mismatch")?;
    let mismatch = json_at(value, &["error", "schema_version_mismatch"])?;
    if mismatch.get("found_schema_version").and_then(Value::as_i64) != Some(found)
        || mismatch
            .get("supported_schema_version")
            .and_then(Value::as_i64)
            != Some(supported)
    {
        return Err(io::Error::other(format!(
            "schema mismatch versions differ: expected found={found} supported={supported}, actual={mismatch}"
        ))
        .into());
    }
    require_json_string(
        value,
        &["error", "schema_version_mismatch", "runtime_version"],
        env!("CARGO_PKG_VERSION"),
    )?;
    require_json_contains(
        value,
        &["error", "schema_version_mismatch", "recovery"],
        "do not reset",
    )
}
