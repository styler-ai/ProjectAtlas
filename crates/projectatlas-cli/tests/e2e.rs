//! Purpose: Validate `ProjectAtlas` 3 CLI end-to-end behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use projectatlas_core::PurposeSource;
use projectatlas_core::language::{BROAD_SOURCE_EXTENSIONS, detect_language_for_path};
use projectatlas_core::telemetry::usage_from_estimates;
use projectatlas_db::{AtlasStore, HealthResolution};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn runtime_info_does_not_create_projectatlas_directory() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    let atlas_dir = repo.join(".projectatlas");
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
            .join(".codex-plugin")
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
        "Get-KnownProjectAtlasShimPaths",
        "Quarantine-ProjectAtlasStaleShims",
        "Test-ProjectAtlasRuntime $candidate $null",
        "[string]$RuntimePath",
        "PROJECTATLAS_RUNTIME_PATH",
        "Find-ProjectAtlas $ProjectAtlasVersion",
        "System.Text.UTF8Encoding",
        r#"$installArgs += @("projectatlas-cli", "--locked", "--force")"#,
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
        r#"Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code""#,
        r#"Write-ProjectAtlasMcpConfig $opencodeConfigPath "opencode""#,
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
        "runtime_override=${PROJECTATLAS_RUNTIME_PATH:-}",
        "runtime_version=$(printf",
        "[ \"$runtime_version\" = \"$expected_version\" ]",
        "cargo install --git \"$repository\" --tag \"$projectatlas_version\" projectatlas-cli --locked --force",
        "projectatlas.claude.mcp.json",
        "projectatlas.opencode.json",
        "write_mcp_config \"$claude_mcp_config_path\" claude-code",
        "write_mcp_config \"$opencode_config_path\" opencode",
    ] {
        if !posix_installer.contains(required) {
            return Err(io::Error::other(format!(
                "POSIX installer is missing runtime version guard {required:?}"
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
    if !e2e_smoke.contains(
        "windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked",
    ) {
        return Err(io::Error::other(
            "Windows CI smoke must run the locked stable mirror release-binary regression",
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
    let gitignore = fs::read_to_string(workspace_root.join(".gitignore"))?;
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
    Ok(())
}

#[test]
fn plugin_installer_writes_real_harness_configs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    let workspace_root = workspace_root()?;
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    run_projectatlas_plugin_installer(&workspace_root, &repo, &runtime)?;

    let atlas_dir = repo.join(".projectatlas");
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
#[cfg(windows)]
fn windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let atlas_dir = repo.join(".projectatlas");
    fs::create_dir_all(&atlas_dir)?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;

    let isolated_home = temp.path().join("isolated-home");
    let app_data = isolated_home.join("AppData").join("Roaming");
    let local_app_data = isolated_home.join("AppData").join("Local");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;

    let stable_runtime = local_app_data
        .join("ProjectAtlas")
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
        let (release_base_url, release_server) =
            serve_single_release_asset(fs::read(&release_archive)?)?;
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
        if !installer_output_text.contains("ProjectAtlas LocalAppData mirror skipped") {
            return Err(io::Error::other(format!(
                "installer did not report the locked LocalAppData mirror\n{installer_output_text}"
            ))
            .into());
        }

        let versioned_runtime = local_app_data
            .join("ProjectAtlas")
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    let atlas_dir = repo.join(".projectatlas");
    fs::create_dir_all(&atlas_dir)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("a.rs"), "pub fn a() {}\n")?;
    fs::write(repo.join("src").join("b.rs"), "pub fn b() {}\n")?;
    fs::write(
        atlas_dir.join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        atlas_dir.join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n",
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
    let isolated_home = temp.path().join("isolated-home");
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
fn bare_relative_projectatlas_config_path_drives_scan_map_and_lint() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\n",
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(
        repo.join(".purpose"),
        "Repository root for bare config path regression tests\n",
    )?;
    fs::write(
        repo.join("src").join(".purpose"),
        "Rust source folder for bare config path regression tests\n",
    )?;
    fs::write(
        repo.join("src").join("main.rs"),
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

    let store = AtlasStore::open(&repo.join(".projectatlas").join("projectatlas.db"))?;
    for (path, purpose) in [
        (".", "Agent-reviewed bare config regression root"),
        (
            ".projectatlas",
            "Agent-reviewed ProjectAtlas metadata folder for bare config tests",
        ),
        (
            "src",
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
        repo.join(".projectatlas").join("projectatlas.toon"),
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
    let map = fs::read_to_string(repo.join(".projectatlas").join("projectatlas.toon"))?;
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    let mut source = "fn main() {\n    println!(\"hello\");\n}\n".to_string();
    for index in 0..120 {
        writeln!(
            &mut source,
            "fn helper_{index}() {{ println!(\"helper {index}\"); }}"
        )?;
    }
    fs::write(repo.join("src").join("main.rs"), source)?;
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
        .args(["folders", "src"])
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
        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"atlas_scan","arguments":{{"path":{}}}}}}}"#,
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
    if !mcp_stdout.contains("outside the MCP-bound project root") {
        return Err(io::Error::other(format!(
            "generated mcp config allowed an outside repository path: {mcp_stdout}"
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
        .stdout(predicate::str::contains("token_savings:"));
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
    let buckets = token_json["buckets"]
        .as_array()
        .ok_or_else(|| io::Error::other("token buckets missing from json report"))?;
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "full_file_compression"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "full_file"
            && bucket["confidence"] == "observed"
    }) {
        return Err(io::Error::other("full-file compression token bucket missing").into());
    }
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "navigation_avoidance"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "directory_walk"
            && bucket["confidence"] == "policy_estimate"
    }) {
        return Err(io::Error::other("directory-walk navigation token bucket missing").into());
    }
    if !buckets.iter().any(|bucket| {
        bucket["token_savings_bucket"] == "navigation_avoidance"
            && bucket["accuracy"] == "heuristic_estimate"
            && bucket["baseline_kind"] == "selected_candidates"
            && bucket["confidence"] == "inferred"
    }) {
        return Err(io::Error::other("selected-candidates navigation token bucket missing").into());
    }
    let calls_before = token_json["calls"]
        .as_u64()
        .ok_or_else(|| io::Error::other("token calls missing before no-telemetry check"))?;
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
    if calls_before != calls_after {
        return Err(io::Error::other(format!(
            "no-telemetry overview mutated call count: before {calls_before}, after {calls_after}"
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
        .stdout(predicate::str::contains("ProjectAtlas Token Savings"))
        .stdout(predicate::str::contains("heuristic chars/bytes / 4"))
        .stdout(predicate::str::contains("not model billing tokens"))
        .stdout(predicate::str::contains("Without PA"))
        .stdout(predicate::str::contains("With PA"))
        .stdout(predicate::str::contains("Saved"))
        .stdout(predicate::str::contains("Buckets"))
        .stdout(predicate::str::contains("heuristic_estimate"))
        .stdout(predicate::str::contains("saved tokens"))
        .stdout(predicate::str::contains("wrong folders, wrong files"))
        .stdout(predicate::str::contains("unnecessary full reads"));
    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui", "--trend", "month"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ProjectAtlas Token Trends"))
        .stdout(predicate::str::contains("Window    month"))
        .stdout(predicate::str::contains("saved"));
    Ok(())
}

#[test]
fn root_and_metadata_validation_flow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("a.rs"), "pub fn a() {}\n")?;
    fs::write(repo.join("src").join("b.rs"), "pub fn b() {}\n")?;

    Command::cargo_bin("projectatlas")?
        .arg("--format")
        .arg("json")
        .args(["root", "set"])
        .arg(&repo)
        .assert()
        .success();

    let db = repo.join(".projectatlas").join("projectatlas.db");
    let config = repo.join(".projectatlas").join("config.toml");
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
        .arg(other_repo.join(".projectatlas").join("projectatlas.db"))
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
        fs::create_dir_all(repo.join("src"))?;
        fs::write(
            repo.join("src").join("main.rs"),
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
        serde_json::to_string(&repo_b.join("src").join("main.rs").to_string_lossy())?
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
    if !output_a.contains("token_savings:") || !output_a.contains("estimate_kind: heuristic") {
        return Err(io::Error::other(format!(
            "repo A MCP token report did not include heuristic token telemetry: {output_a}"
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("main.rs"),
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
        ("src", "Rust source folder for no-telemetry CLI smoke."),
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
        vec!["folders", "src", "--limit", "5"],
        vec!["files", "main", "--folder", "src", "--limit", "5"],
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
    fs::create_dir(repo.join("src"))?;
    for module in 0..MODULES {
        let module_dir = repo.join("src").join(format!("module_{module:02}"));
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("lib.rs"),
        "pub struct Atlas;\n\nimpl Atlas {\n    pub fn sail(&self) {\n        helper();\n    }\n}\n\nfn helper() {}\n",
    )?;
    fs::write(repo.join("src").join(".purpose"), "Rust source folder\n")?;
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
    if repo.join("src").join(".purpose").exists() {
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("src").join("rust").join("no_alias"))?;
    fs::create_dir_all(repo.join("src").join("rust").join("module_alias"))?;
    fs::create_dir_all(repo.join("src").join("rust").join("function_alias"))?;
    fs::create_dir_all(repo.join("src").join("ts").join("no_alias"))?;
    fs::create_dir_all(repo.join("src").join("ts").join("named_alias"))?;
    fs::create_dir_all(repo.join("src").join("ts").join("api"))?;
    fs::create_dir_all(repo.join("src").join("py").join("package"))?;
    fs::create_dir_all(repo.join("src").join("py").join("package_entry"))?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("no_alias")
            .join("service.rs"),
        "pub fn run_no_alias() -> &'static str {\n    \"rust-no-alias\"\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("no_alias")
            .join("main.rs"),
        "use crate::rust::no_alias::service;\n\nfn start_rust_no_alias() {\n    service::run_no_alias();\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("module_alias")
            .join("service.rs"),
        "pub fn run_module_alias() -> &'static str {\n    \"rust-module-alias\"\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("module_alias")
            .join("main.rs"),
        "use crate::rust::module_alias::service as rust_service;\n\nfn start_rust_module_alias() {\n    rust_service::run_module_alias();\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("function_alias")
            .join("service.rs"),
        "pub fn run_function_alias() -> &'static str {\n    \"rust-function-alias\"\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("rust")
            .join("function_alias")
            .join("main.rs"),
        "use crate::rust::function_alias::service::run_function_alias as run_rust_function;\n\nfn start_rust_function_alias() {\n    run_rust_function();\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("ts")
            .join("no_alias")
            .join("service.ts"),
        "export function runTsNoAlias(): string {\n  return \"typescript-no-alias\";\n}\n",
    )?;
    fs::write(
        repo.join("src").join("ts").join("no_alias_main.ts"),
        "import { runTsNoAlias } from \"./no_alias/service\";\n\nexport function startTsNoAlias(): string {\n  return runTsNoAlias();\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("ts")
            .join("named_alias")
            .join("service.ts"),
        "export function runTsNamedAlias(): string {\n  return \"typescript-named-alias\";\n}\n",
    )?;
    fs::write(
        repo.join("src").join("ts").join("named_alias_main.ts"),
        "import { runTsNamedAlias as runAlias } from \"./named_alias/service\";\n\nexport function startTsNamedAlias(): string {\n  return runAlias();\n}\n",
    )?;
    fs::write(
        repo.join("src").join("ts").join("api").join("index.ts"),
        "export function runTsNamespace(): string {\n  return \"typescript-namespace\";\n}\n",
    )?;
    fs::write(
        repo.join("src").join("ts").join("namespace_main.ts"),
        "import * as api from \"./api\";\n\nexport function startTsNamespace(): string {\n  return api.runTsNamespace();\n}\n",
    )?;
    fs::write(
        repo.join("src")
            .join("py")
            .join("package")
            .join("no_alias.py"),
        "def run_py_no_alias():\n    return \"python-no-alias\"\n",
    )?;
    fs::write(
        repo.join("src").join("py").join("no_alias_main.py"),
        "from py.package.no_alias import run_py_no_alias\n\n\ndef start_py_no_alias():\n    return run_py_no_alias()\n",
    )?;
    fs::write(
        repo.join("src")
            .join("py")
            .join("package")
            .join("named_alias.py"),
        "def run_py_named_alias():\n    return \"python-named-alias\"\n",
    )?;
    fs::write(
        repo.join("src").join("py").join("named_alias_main.py"),
        "from py.package.named_alias import run_py_named_alias as run_alias\n\n\ndef start_py_named_alias():\n    return run_alias()\n",
    )?;
    fs::write(
        repo.join("src")
            .join("py")
            .join("package")
            .join("module_alias.py"),
        "def run_py_module_alias():\n    return \"python-module-alias\"\n",
    )?;
    fs::write(
        repo.join("src").join("py").join("module_alias_main.py"),
        "import py.package.module_alias as py_service\n\n\ndef start_py_module_alias():\n    return py_service.run_py_module_alias()\n",
    )?;
    fs::write(
        repo.join("src")
            .join("py")
            .join("package_entry")
            .join("__init__.py"),
        "def run_py_entry():\n    return \"python-entry\"\n",
    )?;
    fs::write(
        repo.join("src").join("py").join("entry_main.py"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("lib.rs"),
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

    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"atlas_health","arguments":{"category":"missing-purpose","path_prefix":".","limit":1}}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"atlas_token_report","arguments":{"include_chart":true}}}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"atlas_purpose_review","arguments":{"apply":true,"items":[{"path":"src/lib.rs","confirm_existing":true}]}}}"#,
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
        || !stdout.contains("overview:")
        || !stdout.contains("files[1]")
        || !stdout.contains("health:")
        || !stdout.contains("health_findings[1]")
        || !stdout.contains("next_start_index: 1")
        || !stdout.contains("ProjectAtlas Token Savings")
        || !stdout.contains("purpose_review:")
        || !stdout.contains("failed: 0")
        || !stdout.contains("src/lib.rs")
    {
        return Err(io::Error::other(format!(
            "mcp stdout did not include expected payloads: {stdout}"
        ))
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
fn indexed_reads_use_scanned_project_root_from_any_cwd() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let outside = temp.path().join("outside");
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&repo)?;
    fs::create_dir(&outside)?;
    fs::create_dir(&unrelated)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        outside.join("projectatlas.toml"),
        "[project]\nroot = \"../unrelated\"\n\n[scan]\nexclude_dir_names = [\"src\"]\n",
    )?;
    fs::write(
        repo.join("src").join("lib.rs"),
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
        repo.join("src").join("lib.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::create_dir_all(repo.join("src").join("api"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\", \"generated\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join("src").join("engine.rs"),
        "pub fn build_project_atlas() {}\n",
    )?;
    fs::write(
        repo.join("src").join("api").join("live.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("src"))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::create_dir_all(repo.join("local-cache"))?;
    fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;
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

    let config_text = fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
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
    if nested.join(".projectatlas").join("config.toml").exists() {
        return Err(io::Error::other("nested ignore command created a nested config").into());
    }
    let nested_config_text = fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
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

    let updated_config_text = fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
    if updated_config_text.contains("local-cache") || updated_config_text.contains(r#""src""#) {
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
    let removed_config_text = fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
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
        repo.join(".projectatlas").join("config.toml"),
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
        fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir_all(repo.join("src").join("api"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::write(
        repo.join("src").join("engine.rs"),
        "pub fn active_engine() {}\n",
    )?;
    fs::write(
        repo.join("src").join("api").join("live.rs"),
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
        repo.join(".projectatlas").join("config.toml"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("src"))?;
    fs::write(
        repo.join("src").join("ProductPanel.vue"),
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
        &repo.join(".projectatlas").join("projectatlas.db"),
        "src/ProductPanel.vue",
    )?;
    require_json_string(&summary_json, &["parser_kind"], "structural-symbol-graph")?;
    require_json_string(&summary_json, &["summary_status"], "ok")?;
    Ok(())
}

#[test]
fn javascript_summary_ignores_locals_and_object_stub_methods() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir_all(repo.join(".github").join("workflows"))?;
    fs::create_dir_all(repo.join("app").join("styles"))?;
    fs::create_dir_all(repo.join("app").join("public").join("data"))?;
    fs::create_dir_all(repo.join("public"))?;
    fs::create_dir_all(repo.join("src"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n",
    )?;
    fs::write(repo.join(".projectatlas").join("projectatlas.db"), b"db")?;
    fs::write(
        repo.join(".projectatlas").join("projectatlas.toon"),
        "generated map\n",
    )?;
    fs::write(
        repo.join(".projectatlas").join("projectatlas.mcp.json"),
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
        repo.join("src").join("empty.rs"),
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::create_dir_all(repo.join("docs").join("api"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(
        repo.join(".purpose"),
        "Repository root for prefix map tests\n",
    )?;
    fs::write(
        repo.join("src").join(".purpose"),
        "Rust source folder for prefix map tests\n",
    )?;
    fs::write(
        repo.join("docs").join(".purpose"),
        "Documentation folder for prefix map tests\n",
    )?;
    fs::write(
        repo.join("src").join("engine.rs"),
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

    let map = fs::read_to_string(repo.join(".projectatlas").join("projectatlas.toon"))?;
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\"]\n",
    )?;
    fs::write(
        repo.join(".projectatlas").join("projectatlas.toon"),
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
    fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n",
    )?;
    fs::write(
        repo.join("src").join("main.rs"),
        "// Purpose: Legacy header purpose that should only seed empty rows.\nfn main() {}\n",
    )?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--config")
        .arg(repo.join(".projectatlas").join("config.toml"))
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
        .arg(repo.join(".projectatlas").join("config.toml"))
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
    let repo = temp.path().join("repo");
    let outside = temp.path().join("outside");
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&repo)?;
    fs::create_dir(&outside)?;
    fs::create_dir(&unrelated)?;
    fs::create_dir(repo.join("src"))?;
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
        repo.join("src").join("engine.rs"),
        "pub fn flat_config_engine() {}\n",
    )?;
    fs::write(
        repo.join("generated").join("noise.rs"),
        "pub fn flat_config_noise() {}\n",
    )?;
    let atlas_dir = repo.join(".projectatlas");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("src").join("nested"))?;
    fs::write(
        repo.join("src").join("nested").join("handler.rs"),
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
    let repo = temp.path().join("target").join("repo");
    fs::create_dir_all(repo.join("src"))?;
    fs::write(repo.join("src").join("main.rs"), "pub fn main_entry() {}\n")?;
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("lib.rs"), "pub fn initial() {}\n")?;
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
        repo.join("src").join("lib.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("main.rs"),
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
            "src",
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
            "src",
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("empty.rs"), "// comment only\n")?;
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("lib.rs"), "pub fn existing() {}\n")?;
    let db = temp.path().join("projectatlas.db");

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["scan", "."])
        .assert()
        .success();

    fs::create_dir_all(repo.join("src").join("feature"))?;
    fs::write(
        repo.join("src").join("feature").join("new_file.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::create_dir_all(repo.join("crates").join("atlas_core").join("src"))?;
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
        repo.join("src").join("main.rs"),
        "mod service;\nconst CONTENT_ONLY_ROUTE: &str = \"contentOnlyRoute\";\nfn main() {\n    service::run();\n}\n",
    )?;
    fs::write(
        repo.join("src").join("service.rs"),
        "pub struct Runner;\n\nimpl Runner {\n    pub fn execute(&self) {\n        helper();\n    }\n}\n\npub fn run() {\n    Runner.execute();\n}\n\nfn helper() {}\n",
    )?;
    fs::write(
        repo.join("crates")
            .join("atlas_core")
            .join("src")
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
        .args(["files", "service", "--folder", "src", "--limit", "5"])
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
            "src",
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("lib.rs"),
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
        ("src", "Rust source folder for clean parity alias tests."),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("a.rs"), "pub fn alpha() {}\n")?;
    fs::write(repo.join("src").join("b.rs"), "pub fn beta() {}\n")?;
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
        ("src", "Rust source folder for purpose gate tests."),
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
        repo.join("src").join("a.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("service.rs"),
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
            "src",
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("detail.rs"),
        "pub fn trusted_detail() {}\n",
    )?;
    fs::write(
        repo.join("src").join("service.rs"),
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join("src").join("customers"))?;
    fs::create_dir_all(repo.join("src").join("settings"))?;
    fs::create_dir(repo.join("assets"))?;
    fs::write(
        repo.join("src").join("customers").join("service.rs"),
        "pub struct CustomerService;\n\nimpl CustomerService {\n    pub fn reconcile(&self) {}\n}\n",
    )?;
    fs::write(
        repo.join("src").join("settings").join("service.rs"),
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::create_dir(repo.join("assets"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\"]\n\n[purpose.styles_by_extension]\n\".toml\" = \"line-comment\"\n",
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n  assets/logo.svg,SVG brand asset for purpose lint strictness tests\n",
    )?;
    fs::write(repo.join(".gitignore"), ".projectatlas/\n")?;
    fs::write(
        repo.join("Cargo.toml"),
        "# Purpose: Rust manifest for purpose lint strictness tests.\n[package]\nname = \"purpose-lint-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        repo.join("src").join("detail.rs"),
        "// Purpose: Rust implementation detail for purpose lint strictness tests.\npub fn detail() {}\n",
    )?;
    fs::write(
        repo.join("assets").join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )?;
    let db = temp.path().join("projectatlas.db");
    let config = repo.join(".projectatlas").join("config.toml");

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
        ("src", "Imported source folder purpose"),
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
        ("src", "Agent-reviewed source folder purpose"),
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

    Ok(())
}

#[test]
fn search_and_symbol_slice_are_bounded_and_identity_safe() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("a.rs"), "needle one\n")?;
    fs::write(repo.join("src").join("b.rs"), "needle two\n")?;
    fs::write(
        repo.join("src").join("lib.rs"),
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

    Ok(())
}

#[test]
fn skipped_symbol_builds_invalidate_stale_symbols() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    let source = repo.join("src").join("main.rs");
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
    messages: &[&str],
) -> Result<String, Box<dyn Error>> {
    let input = format!("{}\n", messages.join("\n"));
    let mut child = StdCommand::new(executable)
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("mcp stdin was not piped"))?
        .write_all(input.as_bytes())?;
    drop(child.stdin.take());

    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
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
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "projectatlas mcp failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
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

/// Serve a single local release asset request for a `PowerShell` installer smoke test.
#[cfg(windows)]
fn serve_single_release_asset(
    asset: Vec<u8>,
) -> Result<(String, thread::JoinHandle<Result<(), io::Error>>), Box<dyn Error>> {
    use std::io::Read as _;

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
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
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        asset.len()
                    )?;
                    stream.write_all(&asset)?;
                    return Ok(());
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
        .env("PROJECTATLAS_RUNTIME_PATH", runtime);
    if let Some(path_shadow) = path_shadow {
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let shadowed_path = std::env::join_paths(
            std::iter::once(path_shadow.to_path_buf()).chain(std::env::split_paths(&current_path)),
        )?;
        command.env("PATH", shadowed_path);
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
            .env("LOCALAPPDATA", local_app_data);
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
    let repo = temp.path().join("repo");
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
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;
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
    let generated_config = fs::read_to_string(repo.join(".projectatlas").join("config.toml"))?;
    if generated_config.contains("purpose_filename") || generated_config.contains(".purpose") {
        return Err(io::Error::other(format!(
            "init config advertised legacy purpose files: {generated_config}"
        ))
        .into());
    }
    if repo.join(".purpose").exists() || repo.join("src").join(".purpose").exists() {
        return Err(io::Error::other("init created legacy .purpose files").into());
    }
    fs::write(
        repo.join(".projectatlas")
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
        ("src", "Rust source folder for CLI integration tests"),
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
    let store = AtlasStore::open(&repo.join(".projectatlas").join("projectatlas.db"))?;
    if !store
        .load_nodes_by_paths(&[".projectatlas".to_string()])?
        .is_empty()
    {
        store.set_purpose(
            ".projectatlas",
            "Agent-reviewed ProjectAtlas metadata folder for CLI integration tests",
            PurposeSource::Agent,
        )?;
    }

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["map", "--force"])
        .assert()
        .success();

    let map = fs::read_to_string(repo.join(".projectatlas").join("projectatlas.toon"))?;
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
