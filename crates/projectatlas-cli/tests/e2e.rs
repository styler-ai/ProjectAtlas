//! Purpose: Validate `ProjectAtlas` 3 CLI end-to-end behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use projectatlas_core::PurposeSource;
use projectatlas_core::language::{BROAD_SOURCE_EXTENSIONS, detect_language_for_path};
use projectatlas_core::telemetry::{
    READ_AVOIDANCE_CONFIDENCE_MODELED, READ_AVOIDANCE_SCOPE, usage_from_estimates,
};
use projectatlas_db::{AtlasStore, HealthResolution};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as IoRead, Write as IoWrite};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TEST_REPO_DIR: &str = "repo";
const SRC_DIR_NAME: &str = "src";
const TESTS_DIR_NAME: &str = "tests";
const INSTALLER_RS_FILE_NAME: &str = "installer.rs";
const ATLAS_DIR_NAME: &str = ".projectatlas";
const GITHOOKS_DIR_NAME: &str = ".githooks";
const PRE_PUSH_HOOK_FILE_NAME: &str = "pre-push";
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
const PROJECTATLAS_SKILL_DIR: &str = "skills";
const PROJECTATLAS_SKILL_NAME: &str = "projectatlas";
const SKILL_FILE_NAME: &str = "SKILL.md";
const SUBDIR_CONFIG_DIR: &str = "config";
#[cfg(windows)]
const PROJECTATLAS_LOCAL_APPDATA_DIR: &str = "ProjectAtlas";

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
        repo.join(IGNORED_FIXTURE_DIR).join("hidden.rs"),
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
fn repository_guidance_keeps_legacy_toon_export_optional() -> Result<(), Box<dyn Error>> {
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
        if !verify.contains("lint_purpose_levels_require_agent_review_at_configured_scope") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must run the low/medium/strict purpose lint E2E"
            ))
            .into());
        }
        if !verify.contains("watch_once_preserves_unchanged_deep_summary_and_text_index") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must test reviewed purpose preservation across deep refresh"
            ))
            .into());
        }
        if !verify.contains("projectatlas-lints") || !verify.contains("strict-strings") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must run strict ProjectAtlas source string lints"
            ))
            .into());
        }
        if !verify.contains(
            "purpose review --from-file .projectatlas/projectatlas-purpose-review.json --apply",
        ) {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must replay reviewed purposes through ProjectAtlas before strict lint"
            ))
            .into());
        }
        if !verify.contains("lint --report-untracked --purpose-level strict") {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must enforce strict purpose lint after review replay"
            ))
            .into());
        }
        let scan_offset = verify.find("ProjectAtlas scan").unwrap_or(usize::MAX);
        let review_offset = verify
            .find("ProjectAtlas purpose review")
            .unwrap_or(usize::MAX);
        let lint_offset = verify.find("ProjectAtlas lint").unwrap_or(usize::MAX);
        if !(scan_offset < review_offset && review_offset < lint_offset) {
            return Err(io::Error::other(format!(
                "{workflow_name} verify job must run scan, purpose review, then strict lint in order"
            ))
            .into());
        }
    }
    let ci_install_smoke = workflow_job_block(&ci_workflow, "install-smoke")?;
    if ci_install_smoke.contains("map --force") {
        return Err(io::Error::other(
            "CI install smoke must not require the legacy TOON map export",
        )
        .into());
    }
    if ci_install_smoke.contains("--strict-folders") {
        return Err(io::Error::other(
            "CI install smoke must not require legacy folder .purpose linting",
        )
        .into());
    }

    let guidance_paths = [
        "AGENTS.md",
        "templates/AGENTS.md",
        "plugins/projectatlas/skills/projectatlas/SKILL.md",
        "skills/codex/ProjectAtlas.md",
        "skills/claude/ProjectAtlas.md",
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
    Ok(())
}

#[test]
fn repository_delivery_and_dependency_policy_is_enforced() -> Result<(), Box<dyn Error>> {
    let workspace_root = workspace_root()?;
    let workflow_dir = workspace_root.join(".github").join("workflows");
    let release_workflow = fs::read_to_string(workflow_dir.join("release.yml"))?;
    let auto_release_workflow = fs::read_to_string(workflow_dir.join("03-auto-release.yml"))?;
    let ci_workflow = fs::read_to_string(workflow_dir.join("ci.yml"))?;
    let docs_workflow = fs::read_to_string(workflow_dir.join("04-docs.yml"))?;
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

    for (name, workflow) in [
        ("release.yml", &release_workflow),
        ("03-auto-release.yml", &auto_release_workflow),
        ("ci.yml", &ci_workflow),
        ("04-docs.yml", &docs_workflow),
    ] {
        assert_actions_are_sha_pinned(name, workflow)?;
    }

    let cargo_update = dependabot
        .split("  - package-ecosystem: cargo")
        .nth(1)
        .and_then(|tail| tail.split("\n  - package-ecosystem:").next())
        .ok_or_else(|| io::Error::other("Dependabot Cargo update is missing"))?;
    let actions_update = dependabot
        .split("  - package-ecosystem: github-actions")
        .nth(1)
        .and_then(|tail| tail.split("\n  - package-ecosystem:").next())
        .ok_or_else(|| io::Error::other("Dependabot GitHub Actions update is missing"))?;
    for (ecosystem, update) in [("cargo", cargo_update), ("github-actions", actions_update)] {
        for required in ["directory: /", "target-branch: dev", "interval: weekly"] {
            if !update.contains(required) {
                return Err(io::Error::other(format!(
                    "Dependabot {ecosystem} update is missing {required:?}"
                ))
                .into());
            }
        }
    }
    for required in ["groups:", "update-types:", "- minor", "- patch"] {
        if !cargo_update.contains(required) {
            return Err(io::Error::other(format!(
                "Dependabot Cargo update is missing minor/patch grouping field {required:?}"
            ))
            .into());
        }
    }
    if cargo_update.contains("- major") {
        return Err(io::Error::other("Dependabot Cargo major updates must remain separate").into());
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
        "milestone_mapping_failures",
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
        "cargo fmt --all --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo test --workspace --all-features --locked",
        "cargo test --doc --workspace --all-features --locked",
        "RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --all-features --locked",
        "cargo deny --locked --all-features check -D warnings",
        "issue-checklists.py --self-test",
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
        "cargo test --doc --workspace --all-features --locked",
        "cargo deny --locked --all-features check -D warnings",
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
    {
        return Err(io::Error::other(
            "release must retain milestone completion and ordinary quality gates",
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
            "@echo off\r\necho %*>>\"%PROJECTATLAS_FAKE_CODEX_LOG%\"\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"marketplace\" if \"%~3\"==\"list\" (\r\n  echo {{\"marketplaces\":[{{\"name\":\"projectatlas\",\"marketplaceSource\":{{\"source\":\"https://github.com/styler-ai/ProjectAtlas.git\"}}}}]}}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"list\" (\r\n  echo {stale_plugin_json}\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"plugin\" if \"%~2\"==\"add\" (\r\n  if not exist \"%PROJECTATLAS_FAKE_FAILURE_MARKER%\" (\r\n    echo failed>\"%PROJECTATLAS_FAKE_FAILURE_MARKER%\"\r\n    exit /b 1\r\n  )\r\n  exit /b 0\r\n)\r\nif \"%~1\"==\"mcp\" if \"%~2\"==\"get\" exit /b 1\r\nexit /b 0\r\n"
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
    let reported_install_failure = installer_output_text
        .contains("Codex ProjectAtlas plugin update failed: could not install projectatlas plugin");
    let reported_version_mismatch = installer_output_text
        .contains("Codex ProjectAtlas plugin update failed: installed projectatlas plugin version");
    if !reported_install_failure && !reported_version_mismatch {
        return Err(io::Error::other(format!(
            "installer did not report the failed plugin reinstall:\n{installer_output_text}"
        ))
        .into());
    }
    if reported_install_failure && !failure_marker.exists() {
        return Err(io::Error::other("fake Codex plugin add failure was not exercised").into());
    }
    let fake_codex_calls = fs::read_to_string(isolated_home.join(FAKE_CODEX_LOG_FILE))?;
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
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_scan","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_scan","arguments":{"path":"."}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_watch_once","arguments":{"path":"."}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#.to_string(),
        outside_scan_message,
    ];
    let message_refs = messages.iter().map(String::as_str).collect::<Vec<_>>();
    let mcp_stdout = run_mcp_stdio(
        std::path::Path::new(command),
        &outside_cwd,
        &launch_args,
        &message_refs,
    )?;
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
            "F I L E   R E A D S   A V O I D E D",
        ))
        .stdout(predicate::str::contains("Observed"))
        .stdout(predicate::str::contains("Modeled narrowing"))
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

    Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&repo)
        .assert()
        .success();

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

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "relations", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));

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
        .stdout(predicate::str::contains("max_workers: 2"))
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
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    {
        let store = AtlasStore::open(&db)?;
        store.set_purpose(
            "src/lib.rs",
            "Imported Rust library purpose for MCP review.",
            PurposeSource::Imported,
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
        serde_json::json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"atlas_ignore_add","arguments":{"project_path":repo_argument,"kind":"dir-name","value":"generated-cache"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"atlas_ignore_remove","arguments":{"project_path":repo_argument,"kind":"dir-name","value":"generated-cache"}}}).to_string(),
        serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"atlas_lint","arguments":{"project_path":repo_argument,"purpose_level":"low"}}}).to_string(),
        r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"atlas_runtime_info","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"atlas_health","arguments":{"category":"missing-purpose","path_prefix":".","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"include_chart":true}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"atlas_purpose_review","arguments":{"apply":true,"items":[{"path":"src/lib.rs","confirm_existing":true}]}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"atlas_next","arguments":{"query":"indexed","limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"atlas_settings","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"atlas_session_brief","arguments":{"query":"indexed","folder_limit":1,"file_limit":1,"blocker_limit":1}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"atlas_task_status","arguments":{"task_id":"task-progress-contract"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"atlas_task_cancel","arguments":{"task_id":"task-progress-contract"}}}"#.to_string(),
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
    if !stdout.contains(r#""id":1"#)
        || !stdout.contains(r#""serverInfo":{"name":"ProjectAtlas","version":"#)
        || !stdout.contains(r#""name":"atlas_files""#)
        || !stdout.contains(r#""name":"atlas_next""#)
        || !stdout.contains(r#""name":"atlas_session_brief""#)
        || !stdout.contains(r#""name":"atlas_task_status""#)
        || !stdout.contains(r#""name":"atlas_task_cancel""#)
        || !stdout.contains("overview:")
        || !stdout.contains("files[1]")
        || !stdout.contains("next:")
        || !stdout.contains("mcp_session:")
        || !stdout.contains("path_scope: selected_project")
        || !stdout.contains("session_brief:")
        || !stdout.contains("task_status:")
        || !stdout.contains("task_cancel:")
        || !stdout.contains("task-progress-contract")
        || !stdout.contains("already_finished")
        || !stdout.contains("health:")
        || !stdout.contains("health_findings[1]")
        || !stdout.contains("next_start_index: 1")
        || !stdout.contains("ProjectAtlas")
        || !stdout.contains("Token Impact")
        || !stdout.contains("T O T A L   T O K E N S   A V O I D E D")
        || !stdout.contains("F I L E   R E A D S   A V O I D E D")
        || !stdout.contains("S I G N A L")
        || !stdout.contains("purpose_review:")
        || !stdout.contains("failed: 0")
        || !stdout.contains("src/lib.rs")
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
        "Imported Rust library purpose for MCP review.",
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
    require_json_contains(file_entry, &["reasons", "0"], "path matched")?;
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
    if !gitignore_text.contains(".projectatlas/*.db") {
        return Err(io::Error::other(format!(
            "created .gitignore did not protect ProjectAtlas runtime DBs: {gitignore_text}"
        ))
        .into());
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
fn scan_indexes_every_supported_language_extension() -> Result<(), Box<dyn Error>> {
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
        .stdout(predicate::str::contains("unchanged: 1"));

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
        .stdout(predicate::str::contains("unchanged: 1"));

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
        .stdout(predicate::str::contains("unchanged: 1"));

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
        .stdout(predicate::str::contains("stale_purposes: 1"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("health-check")
        .assert()
        .success()
        .stdout(predicate::str::contains("stale-purpose:src/a.rs:"));

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
        .args(["purpose", "queue", "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("purpose_curation:"))
        .stdout(predicate::str::contains("source_only: true"))
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
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();
    let stale_low = Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&db)
        .args(["lint", "--purpose-level", "low"])
        .output()?;
    if stale_low.status.success() {
        return Err(io::Error::other("low purpose lint missed stale high-impact file").into());
    }
    let stale_low_stderr = String::from_utf8(stale_low.stderr)?;
    if !stale_low_stderr.contains("[stale-purpose] Cargo.toml:") {
        return Err(io::Error::other(format!(
            "low purpose lint missed stale high-impact file:\n{stale_low_stderr}"
        ))
        .into());
    }
    if stale_low_stderr.contains("src/detail.rs") {
        return Err(io::Error::other(format!(
            "low purpose lint included low-value stale source file:\n{stale_low_stderr}"
        ))
        .into());
    }

    Ok(())
}

#[test]
fn search_and_symbol_slice_are_bounded_and_identity_safe() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join(TEST_REPO_DIR);
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(SRC_DIR_NAME))?;
    fs::write(repo.join(SRC_DIR_NAME).join("a.rs"), "needle one\n")?;
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
    require_json_usize(&search_json, &["returned"], 1)?;
    require_json_usize(&search_json, &["searched_files"], 1)?;
    require_json_bool(&search_json, &["truncated"], true)?;

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
fn skipped_symbol_builds_invalidate_stale_symbols() -> Result<(), Box<dyn Error>> {
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
        .success()
        .stdout(predicate::str::contains("too_large: 2"));

    let cargo_summary = json_summary_command(&repo, &db, "Cargo.toml")?;
    require_json_contains(&cargo_summary, &["content_summary"], "cargo manifest")?;
    require_json_string(&cargo_summary, &["summary_status"], "ok")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/main.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("old_too_large_symbol").not())
        .stdout(predicate::str::contains("new_too_large_symbol").not());

    fs::write(&source, "pub fn old_timeout_symbol() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    fs::write(&source, "pub fn new_timeout_symbol() {}\n")?;
    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["watch", ".", "--once", "--timeout-seconds", "0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("timed_out: 1"));

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["symbols", "list", "--file", "src/main.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("old_timeout_symbol").not())
        .stdout(predicate::str::contains("new_timeout_symbol").not());

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

/// Launch a real MCP stdio child and return stdout after stdin closes.
fn run_mcp_stdio(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
) -> Result<String, Box<dyn Error>> {
    let input = format!(
        "{}\n",
        messages
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut child = StdCommand::new(executable)
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

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
    format!("{:x}", Sha256::digest(bytes))
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
        return Err(io::Error::other(format!("summary command failed for {file}")).into());
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
