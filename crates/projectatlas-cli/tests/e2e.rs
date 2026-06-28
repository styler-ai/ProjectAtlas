//! Purpose: Validate `ProjectAtlas` 3 CLI end-to-end behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as IoWrite};
use std::process::{Command as StdCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
        "--db",
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "2"],
        "--config",
    )?;
    require_json_string(
        &mcp_config_json,
        &["mcpServers", "projectatlas", "args", "4"],
        "mcp",
    )?;
    let mcp_args = mcp_config_json["mcpServers"]["projectatlas"]["args"]
        .as_array()
        .ok_or_else(|| io::Error::other("mcp args missing"))?;
    let config_path = mcp_args
        .get(3)
        .ok_or_else(|| io::Error::other("mcp config path missing"))?
        .as_str()
        .ok_or_else(|| io::Error::other("mcp config path missing"))?;
    if !std::path::Path::new(config_path).is_absolute() {
        return Err(io::Error::other("mcp config path was not absolute").into());
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
    let expected_root = repo.canonicalize()?;
    if actual_root != expected_root {
        return Err(io::Error::other(format!(
            "mcp config repo root mismatch: expected {expected_root:?}, got {actual_root:?}"
        ))
        .into());
    }

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
    require_json_usize_at_least(&token_json, &["calls"], 7)?;
    require_json_usize_greater_than(&token_json, &["estimated_without_projectatlas"], 0)?;
    require_json_usize_greater_than(&token_json, &["estimated_with_projectatlas"], 0)?;
    require_json_i64_greater_than(&token_json, &["estimated_saved"], 0)?;
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

    Command::cargo_bin("projectatlas")?
        .arg("--db")
        .arg(&db)
        .args(["token", "--view", "tui"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ProjectAtlas Token Savings"))
        .stdout(predicate::str::contains("wrong-file opens"))
        .stdout(predicate::str::contains("unnecessary full-code reads"));
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

    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-e2e","version":"0.1.0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_overview","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"atlas_files","arguments":{"file_pattern":"*.rs","limit":1}}}"#,
    ];
    let input = format!("{}\n", messages.join("\n"));
    let executable = assert_cmd::cargo::cargo_bin("projectatlas");
    let mut child = StdCommand::new(executable)
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .arg("mcp")
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
    let stdout = String::from_utf8(output.stdout)?;
    if !stdout.contains(r#""id":1"#)
        || !stdout.contains(r#""name":"atlas_files""#)
        || !stdout.contains("overview:")
        || !stdout.contains("files[1]")
        || !stdout.contains("src/lib.rs")
    {
        return Err(io::Error::other(format!(
            "mcp stdout did not include expected payloads: {stdout}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn indexed_reads_use_scanned_project_root_from_any_cwd() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let outside = temp.path().join("outside");
    fs::create_dir(&repo)?;
    fs::create_dir(&outside)?;
    fs::create_dir(repo.join("src"))?;
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
    Ok(())
}

#[test]
fn scan_honors_configured_excludes_and_cli_fuzzy_search() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::create_dir(repo.join("src"))?;
    fs::create_dir_all(repo.join("generated"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        "[project]\nroot = \".\"\n\n[scan]\nexclude_dir_names = [\".git\", \".projectatlas\", \"target\", \"node_modules\", \"generated\"]\n",
    )?;
    fs::write(
        repo.join("src").join("engine.rs"),
        "pub fn build_project_atlas() {}\n",
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
    require_json_usize(&scan_json, &["overview", "files"], 1)?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "noise"])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated/noise.rs").not());

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["files", "--file-pattern", "src\\*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/engine.rs"))
        .stdout(predicate::str::contains("generated/noise.rs").not());

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
    let before_summary = json_at(&before_json, &["observed_summary"])?
        .as_str()
        .ok_or_else(|| io::Error::other("observed summary before watch was not a string"))?
        .to_string();
    if !before_summary.contains("helper") {
        return Err(io::Error::other("summary before watch did not include symbol facts").into());
    }

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
    require_json_string(&after_json, &["observed_summary"], &before_summary)?;

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
        "mod service;\nfn main() {\n    service::run();\n}\n",
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
        .failure()
        .stdout(predicate::str::contains("parity:"))
        .stdout(predicate::str::contains(
            "profile: \"repository-intelligence\"",
        ))
        .stdout(predicate::str::contains("5 suggested"));

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
                .find(|entry| entry["node"]["path"] == "src/service.rs")
        })
        .ok_or_else(|| io::Error::other("service file entry was missing"))?;
    require_json_string(
        file_entry,
        &["summary"],
        "rust source defining type and function Service, run.",
    )?;
    require_json_string(file_entry, &["purpose", "status"], "suggested")?;
    require_json_string(
        file_entry,
        &["purpose", "purpose"],
        "Provide service.rs behavior: rust source defining type and function Service, run",
    )?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("--db")
        .arg(&db)
        .args(["summary", "src/service.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("file_summary:"))
        .stdout(predicate::str::contains("purpose_status: suggested"))
        .stdout(predicate::str::contains("observed_summary:"))
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
    require_json_string(&summary_json, &["purpose_status"], "suggested")?;
    require_json_string(&summary_json, &["purpose_source"], "generated")?;
    require_json_string(&summary_json, &["docstring"], "Service module docs.")?;
    require_json_usize(&summary_json, &["total_exports"], 2)?;
    require_json_string(&summary_json, &["exports", "0"], "Service")?;
    require_json_string(&summary_json, &["exports", "1"], "run")?;
    require_json_string(
        &summary_json,
        &["observed_summary"],
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
                .find(|entry| entry["node"]["path"] == "src/service.rs")
        })
        .ok_or_else(|| io::Error::other("approved service file entry was missing"))?;
    require_json_string(file_entry, &["purpose", "status"], "approved")?;
    require_json_string(file_entry, &["purpose", "source"], "agent")?;
    require_json_string(
        file_entry,
        &["purpose", "purpose"],
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
    require_json_string(&summary_json, &["purpose_status"], "approved")?;
    require_json_string(&summary_json, &["purpose_source"], "agent")?;
    require_json_string(
        &summary_json,
        &["purpose"],
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
        .stdout(predicate::str::contains("too_large: 1"));

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
fn init_map_and_lint_flow_uses_rust_implementation() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir(repo.join("src"))?;
    fs::write(
        repo.join("src").join("main.rs"),
        "// Purpose: Provide a tiny Rust entry point for ProjectAtlas tests.\nfn main() {}\n",
    )?;
    fs::write(
        repo.join("README.md"),
        "# Purpose: Demo readme for Rust map lint tests\n",
    )?;
    fs::write(repo.join("logo.png"), b"png")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .args(["init", "--seed-purpose"])
        .assert()
        .success();
    fs::write(
        repo.join(".purpose"),
        "Demo repository for Rust map lint tests\n",
    )?;
    fs::write(
        repo.join("src").join(".purpose"),
        "Rust source folder for CLI integration tests\n",
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n  # path,summary\n  logo.png,Demo asset for Rust map lint tests\n",
    )?;

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
        .stdout(predicate::str::contains("approved_purposes: 5"));

    Ok(())
}
