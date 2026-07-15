//! Verify installers never execute source discovered inside a target repository.

use assert_cmd::Command;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(30);
const RELEASE_BASE_URL: &str = "http://127.0.0.1:9/projectatlas-test";

#[test]
fn installers_do_not_derive_source_installs_from_the_target_root() -> Result<(), Box<dyn Error>> {
    let root = workspace_root()?;
    let powershell =
        fs::read_to_string(root.join("plugins/projectatlas/scripts/install-runtime.ps1"))?;
    let posix = fs::read_to_string(root.join("plugins/projectatlas/scripts/install-runtime.sh"))?;

    for (name, source) in [("PowerShell", powershell), ("POSIX", posix)] {
        require(
            !source.contains("cargo install --path")
                && !source.contains("\"install\", \"--path\"")
                && !source.contains("$project_root/crates/projectatlas-cli/Cargo.toml")
                && !source.contains("Join-Path $ProjectRoot \"crates\\projectatlas-cli"),
            format!("{name} installer can execute source inferred from the target repository"),
        )?;
    }

    Ok(())
}

#[cfg(windows)]
#[test]
fn powershell_installer_treats_a_hostile_target_as_data() -> Result<(), Box<dyn Error>> {
    let fixture = HostileTarget::new()?;
    let fake_bin = fixture.temp.path().join("fake-bin");
    fs::create_dir(&fake_bin)?;
    fs::write(
        fake_bin.join("cargo.cmd"),
        "@echo off\r\necho %* > \"%PROJECTATLAS_FAKE_CARGO_LOG%\"\r\nexit /b 23\r\n",
    )?;

    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.ps1");
    let mut command = Command::new("powershell");
    configure_hostile_install(&mut command, &fixture, &fake_bin)?;
    let output = command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(&fixture.target)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .arg("-ReleaseBaseUrl")
        .arg(RELEASE_BASE_URL)
        .output()?;

    fixture.verify_official_fallback_without_execution(output.status)?;
    Ok(())
}

#[cfg(windows)]
#[test]
fn powershell_installer_rejects_a_project_state_junction() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target");
    let outside = temp.path().join("outside");
    let sentinel = outside.join("sentinel.txt");
    fs::create_dir(&target)?;
    fs::create_dir(&outside)?;
    fs::write(&sentinel, "outside-state\n")?;
    let junction = target.join(".projectatlas");
    let junction_output = Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()?;
    require(
        junction_output.status.success(),
        format!(
            "failed to create junction fixture: {}",
            String::from_utf8_lossy(&junction_output.stderr)
        ),
    )?;

    let output = run_powershell_runtime_install(&target, temp.path())?;
    require(
        !output.status.success(),
        "installer accepted a project-state junction",
    )?;
    let normalized_stderr = String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    require(
        normalized_stderr.contains("must not be a symlink, junction, or reparse point"),
        format!(
            "installer did not explain the rejected junction: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    require(
        fs::read_to_string(&sentinel)? == "outside-state\n"
            && !outside.join("projectatlas.mcp.json").exists(),
        "installer wrote through the project-state junction",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn posix_installer_treats_a_hostile_target_as_data() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = HostileTarget::new()?;
    let fake_bin = fixture.temp.path().join("fake-bin");
    fs::create_dir(&fake_bin)?;
    let fake_cargo = fake_bin.join("cargo");
    fs::write(
        &fake_cargo,
        "#!/usr/bin/env sh\nprintf '%s\\n' \"$*\" > \"$PROJECTATLAS_FAKE_CARGO_LOG\"\nexit 23\n",
    )?;
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))?;

    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.sh");
    let mut command = Command::new("bash");
    configure_hostile_install(&mut command, &fixture, &fake_bin)?;
    let output = command.arg(installer).arg(&fixture.target).output()?;

    fixture.verify_official_fallback_without_execution(output.status)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn posix_installer_rejects_a_project_state_symlink() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target");
    let outside = temp.path().join("outside");
    let sentinel = outside.join("sentinel.txt");
    fs::create_dir(&target)?;
    fs::create_dir(&outside)?;
    fs::write(&sentinel, "outside-state\n")?;
    symlink(&outside, target.join(".projectatlas"))?;

    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.sh");
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let output = Command::new("bash")
        .timeout(INSTALL_TIMEOUT)
        .arg(installer)
        .arg(&target)
        .env("HOME", temp.path())
        .env("PROJECTATLAS_RUNTIME_PATH", runtime)
        .env(
            "PROJECTATLAS_VERSION",
            format!("v{}", env!("CARGO_PKG_VERSION")),
        )
        .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .output()?;
    require(
        !output.status.success(),
        "installer accepted a project-state symlink",
    )?;
    require(
        String::from_utf8_lossy(&output.stderr).contains("must not be a symlink"),
        format!(
            "installer did not explain the rejected symlink: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    require(
        fs::read_to_string(&sentinel)? == "outside-state\n"
            && !outside.join("projectatlas.mcp.json").exists(),
        "installer wrote through the project-state symlink",
    )?;
    Ok(())
}

struct HostileTarget {
    temp: tempfile::TempDir,
    target: PathBuf,
    marker: PathBuf,
    cargo_log: PathBuf,
}

impl HostileTarget {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("hostile-target");
        let package = target.join("crates/projectatlas-cli");
        let marker = temp.path().join("hostile-build-script-executed");
        let cargo_log = temp.path().join("cargo-arguments.log");
        fs::create_dir_all(package.join("src"))?;
        fs::write(
            package.join("Cargo.toml"),
            "[package]\nname = \"projectatlas-cli\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[[bin]]\nname = \"projectatlas\"\npath = \"src/main.rs\"\n",
        )?;
        let marker_literal = serde_json::to_string(marker.to_string_lossy().as_ref())?;
        fs::write(
            package.join("build.rs"),
            format!("fn main() {{ std::fs::write({marker_literal}, \"executed\").unwrap(); }}\n"),
        )?;
        fs::write(package.join("src/main.rs"), "fn main() {}\n")?;
        fs::write(
            target.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"projectatlas-cli\"\nversion = \"0.0.0\"\n",
        )?;

        Ok(Self {
            temp,
            target,
            marker,
            cargo_log,
        })
    }

    fn verify_official_fallback_without_execution(
        &self,
        status: std::process::ExitStatus,
    ) -> Result<(), Box<dyn Error>> {
        require(
            !status.success(),
            "fake Cargo failure must stop installation",
        )?;
        require(
            !self.marker.exists(),
            "the target repository build script was executed",
        )?;
        let arguments = fs::read_to_string(&self.cargo_log)?;
        require(
            arguments.contains("install --git https://github.com/styler-ai/ProjectAtlas")
                && arguments.contains("projectatlas-cli --locked --force"),
            format!("installer did not use the explicit official repository fallback: {arguments}"),
        )?;
        require(
            !arguments.contains("--path")
                && !arguments.contains(self.target.to_string_lossy().as_ref()),
            format!("installer passed target-controlled source to Cargo: {arguments}"),
        )?;
        Ok(())
    }
}

fn configure_hostile_install(
    command: &mut Command,
    fixture: &HostileTarget,
    fake_bin: &Path,
) -> Result<(), Box<dyn Error>> {
    let app_data = fixture.temp.path().join("app-data");
    let local_app_data = fixture.temp.path().join("local-app-data");
    let codex_home = fixture.temp.path().join(".codex");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;
    fs::create_dir_all(&codex_home)?;
    command
        .timeout(INSTALL_TIMEOUT)
        .env("PATH", prepend_path(fake_bin)?)
        .env("HOME", fixture.temp.path())
        .env("USERPROFILE", fixture.temp.path())
        .env("APPDATA", app_data)
        .env("LOCALAPPDATA", local_app_data)
        .env("CODEX_HOME", codex_home)
        .env("PROJECTATLAS_FAKE_CARGO_LOG", &fixture.cargo_log)
        .env(
            "PROJECTATLAS_VERSION",
            format!("v{}", env!("CARGO_PKG_VERSION")),
        )
        .env("PROJECTATLAS_RELEASE_BASE_URL", RELEASE_BASE_URL)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1")
        .env_remove("PROJECTATLAS_RUNTIME_PATH")
        .env_remove("PROJECTATLAS_RELEASE_BINARY_ONLY");
    Ok(())
}

#[cfg(windows)]
fn run_powershell_runtime_install(
    project_root: &Path,
    isolated_root: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.ps1");
    let runtime = assert_cmd::cargo::cargo_bin("projectatlas");
    let app_data = isolated_root.join("app-data");
    let local_app_data = isolated_root.join("local-app-data");
    let codex_home = isolated_root.join(".codex");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;
    fs::create_dir_all(&codex_home)?;
    let mut command = Command::new("powershell");
    command
        .timeout(INSTALL_TIMEOUT)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(project_root)
        .arg("-RuntimePath")
        .arg(runtime)
        .arg("-ProjectAtlasVersion")
        .arg(format!("v{}", env!("CARGO_PKG_VERSION")))
        .env("HOME", isolated_root)
        .env("USERPROFILE", isolated_root)
        .env("APPDATA", app_data)
        .env("LOCALAPPDATA", local_app_data)
        .env("CODEX_HOME", codex_home)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1");
    Ok(command.output()?)
}

fn prepend_path(path: &Path) -> Result<OsString, env::JoinPathsError> {
    let mut paths = vec![path.to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    env::join_paths(paths)
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}
