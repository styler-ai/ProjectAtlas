//! Verify release bootstraps treat target projects as untrusted installer input.

use assert_cmd::Command;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Maximum runtime for one installer subprocess.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Unreachable release endpoint that makes any remaining source fallback observable.
const UNREACHABLE_RELEASE_BASE_URL: &str = "http://127.0.0.1:9/projectatlas-test";
/// Project-local MCP config files written by both release bootstraps.
const GENERATED_MCP_CONFIG_FILES: [&str; 3] = [
    "projectatlas.mcp.json",
    "projectatlas.claude.mcp.json",
    "projectatlas.opencode.json",
];

#[test]
fn bootstraps_never_derive_executable_source_from_the_target_project() -> Result<(), Box<dyn Error>>
{
    let root = workspace_root()?;
    let sources = [
        (
            "PowerShell",
            fs::read_to_string(root.join("plugins/projectatlas/scripts/install-runtime.ps1"))?,
        ),
        (
            "POSIX",
            fs::read_to_string(root.join("plugins/projectatlas/scripts/install-runtime.sh"))?,
        ),
    ];

    for (host, source) in sources {
        require(
            !source.contains("cargo install --path")
                && !source.contains("\"install\", \"--path\"")
                && !source.contains("$project_root/crates/projectatlas-cli/Cargo.toml")
                && !source.contains("Join-Path $ProjectRoot \"crates\\projectatlas-cli"),
            format!("{host} bootstrap can execute source inferred from its target project"),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn powershell_bootstrap_treats_hostile_target_source_as_data() -> Result<(), Box<dyn Error>> {
    let fixture = HostileTarget::create()?;
    let fake_bin = fixture.root.path().join("fake-bin");
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
        .arg(release_version())
        .arg("-ReleaseBaseUrl")
        .arg(UNREACHABLE_RELEASE_BASE_URL)
        .output()?;

    fixture.verify_target_source_remained_inert(output.status)
}

#[cfg(windows)]
#[test]
fn powershell_bootstrap_rejects_project_state_reparse_points_before_writing()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    let outside = root.path().join("outside");
    fs::create_dir(&target)?;
    fs::create_dir(&outside)?;
    let sentinel = outside.join("sentinel.txt");
    fs::write(&sentinel, "unchanged\n")?;
    let stable_runtime = root
        .path()
        .join("local-app-data/ProjectAtlas/bin/projectatlas.exe");

    let junction = target.join(".projectatlas");
    let junction_output = Command::new("cmd")
        .timeout(INSTALL_TIMEOUT)
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

    let output = run_powershell_runtime_install(&target, root.path())?;
    require(
        !output.status.success(),
        "PowerShell bootstrap accepted a project-state reparse point",
    )?;
    require(
        String::from_utf8_lossy(&output.stderr)
            .contains("must not be a symlink, junction, or reparse point"),
        format!(
            "PowerShell bootstrap did not explain the rejected reparse point: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    require(
        fs::read_to_string(sentinel)? == "unchanged\n"
            && !outside.join("projectatlas.mcp.json").exists()
            && !stable_runtime.exists(),
        "PowerShell bootstrap mutated state before rejecting the project-state reparse point",
    )
}

#[cfg(windows)]
#[test]
fn powershell_bootstrap_rejects_redirected_config_outputs() -> Result<(), Box<dyn Error>> {
    for config_name in GENERATED_MCP_CONFIG_FILES {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        let atlas_dir = target.join(".projectatlas");
        let outside = root.path().join("outside");
        fs::create_dir_all(&atlas_dir)?;
        fs::create_dir(&outside)?;
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, "unchanged\n")?;
        let redirected_output = atlas_dir.join(config_name);

        let link_output = Command::new("cmd")
            .timeout(INSTALL_TIMEOUT)
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&redirected_output)
            .arg(&outside)
            .output()?;
        require(
            link_output.status.success(),
            format!(
                "failed to create config reparse-point fixture for {config_name}: {}",
                String::from_utf8_lossy(&link_output.stderr)
            ),
        )?;

        let output = run_powershell_runtime_install(&target, root.path())?;
        require(
            !output.status.success(),
            format!("PowerShell bootstrap accepted redirected output {config_name}"),
        )?;
        require(
            String::from_utf8_lossy(&output.stderr)
                .contains("must not be a symlink, junction, or reparse point"),
            format!(
                "PowerShell bootstrap did not explain redirected output {config_name}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        require(
            fs::read_to_string(&sentinel)? == "unchanged\n",
            format!("PowerShell bootstrap overwrote redirected output {config_name}"),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn powershell_bootstrap_atomically_replaces_hard_linked_config_outputs()
-> Result<(), Box<dyn Error>> {
    installer_atomically_replaces_hard_linked_config_outputs(
        "PowerShell",
        run_powershell_runtime_install,
    )
}

#[cfg(unix)]
#[test]
fn posix_bootstrap_treats_hostile_target_source_as_data() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = HostileTarget::create()?;
    let fake_bin = fixture.root.path().join("fake-bin");
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

    fixture.verify_target_source_remained_inert(output.status)
}

#[cfg(unix)]
#[test]
fn posix_bootstrap_rejects_project_state_symlinks_before_writing() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    let outside = root.path().join("outside");
    fs::create_dir(&target)?;
    fs::create_dir(&outside)?;
    let sentinel = outside.join("sentinel.txt");
    fs::write(&sentinel, "unchanged\n")?;
    let installed_runtime = root.path().join(".local/bin/projectatlas");
    symlink(&outside, target.join(".projectatlas"))?;

    let output = run_posix_runtime_install(&target, root.path())?;
    require(
        !output.status.success(),
        "POSIX bootstrap accepted a project-state symlink",
    )?;
    require(
        String::from_utf8_lossy(&output.stderr).contains("must not be a symlink"),
        format!(
            "POSIX bootstrap did not explain the rejected symlink: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    require(
        fs::read_to_string(sentinel)? == "unchanged\n"
            && !outside.join("projectatlas.mcp.json").exists()
            && !installed_runtime.exists(),
        "POSIX bootstrap mutated state before rejecting the project-state symlink",
    )
}

#[cfg(unix)]
#[test]
fn posix_bootstrap_rejects_redirected_config_outputs() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    for config_name in GENERATED_MCP_CONFIG_FILES {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        let atlas_dir = target.join(".projectatlas");
        let outside = root.path().join("outside");
        fs::create_dir_all(&atlas_dir)?;
        fs::create_dir(&outside)?;
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, "unchanged\n")?;
        symlink(&sentinel, atlas_dir.join(config_name))?;

        let output = run_posix_runtime_install(&target, root.path())?;
        require(
            !output.status.success(),
            format!("POSIX bootstrap accepted redirected output {config_name}"),
        )?;
        require(
            String::from_utf8_lossy(&output.stderr).contains("must not be a symlink"),
            format!(
                "POSIX bootstrap did not explain redirected output {config_name}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        require(
            fs::read_to_string(&sentinel)? == "unchanged\n",
            format!("POSIX bootstrap overwrote redirected output {config_name}"),
        )?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn posix_bootstrap_atomically_replaces_hard_linked_config_outputs() -> Result<(), Box<dyn Error>> {
    installer_atomically_replaces_hard_linked_config_outputs("POSIX", run_posix_runtime_install)
}

/// Verify publication replaces hostile hard links without changing their external inodes.
fn installer_atomically_replaces_hard_linked_config_outputs(
    host: &str,
    run_installer: fn(&Path, &Path) -> Result<std::process::Output, Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    let atlas_dir = target.join(".projectatlas");
    let outside = root.path().join("outside");
    fs::create_dir_all(&atlas_dir)?;
    fs::create_dir(&outside)?;

    let mut sentinels = Vec::with_capacity(GENERATED_MCP_CONFIG_FILES.len());
    for config_name in GENERATED_MCP_CONFIG_FILES {
        let sentinel = outside.join(format!("{config_name}.sentinel"));
        fs::write(&sentinel, "unchanged\n")?;
        fs::hard_link(&sentinel, atlas_dir.join(config_name))?;
        sentinels.push((config_name, sentinel));
    }

    let output = run_installer(&target, root.path())?;
    require(
        output.status.success(),
        format!(
            "{host} bootstrap failed to replace hard-linked config outputs: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    for (config_name, sentinel) in sentinels {
        require(
            fs::read_to_string(&sentinel)? == "unchanged\n"
                && fs::read_to_string(atlas_dir.join(config_name))? != "unchanged\n",
            format!("{host} bootstrap did not safely replace hard-linked output {config_name}"),
        )?;
    }
    Ok(())
}

/// A target project whose apparent CLI package would execute a marker-writing build script.
struct HostileTarget {
    /// Isolated fixture root.
    root: tempfile::TempDir,
    /// Installer target project.
    target: PathBuf,
    /// Marker written only if target-controlled Rust executes.
    execution_marker: PathBuf,
    /// Captured arguments from the fake Cargo fallback.
    cargo_log: PathBuf,
}

impl HostileTarget {
    /// Create a target-controlled package that must remain inert installer input.
    fn create() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("hostile-target");
        let package = target.join("crates/projectatlas-cli");
        let execution_marker = root.path().join("target-source-executed");
        let cargo_log = root.path().join("cargo-arguments.log");
        fs::create_dir_all(package.join("src"))?;
        fs::write(
            package.join("Cargo.toml"),
            "[package]\nname = \"projectatlas-cli\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[[bin]]\nname = \"projectatlas\"\npath = \"src/main.rs\"\n",
        )?;
        let marker_literal = serde_json::to_string(execution_marker.to_string_lossy().as_ref())?;
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
            root,
            target,
            execution_marker,
            cargo_log,
        })
    }

    /// Verify target-controlled Rust remains inert under every supported fallback choice.
    fn verify_target_source_remained_inert(
        &self,
        status: std::process::ExitStatus,
    ) -> Result<(), Box<dyn Error>> {
        require(
            !status.success(),
            "fake Cargo failure must stop installation",
        )?;
        require(
            !self.execution_marker.exists(),
            "installer executed the target project's build script",
        )?;
        if self.cargo_log.is_file() {
            let arguments = fs::read_to_string(&self.cargo_log)?;
            require(
                !arguments.contains("--path")
                    && !arguments.contains(self.target.to_string_lossy().as_ref()),
                format!("installer passed target-controlled source to Cargo: {arguments}"),
            )?;
        }
        Ok(())
    }
}

/// Configure an installer process that reaches a logged, failing Cargo fallback.
fn configure_hostile_install(
    command: &mut Command,
    fixture: &HostileTarget,
    fake_bin: &Path,
) -> Result<(), Box<dyn Error>> {
    configure_isolated_environment(command, fixture.root.path())?;
    command
        .env("PATH", prepend_path(fake_bin)?)
        .env("PROJECTATLAS_FAKE_CARGO_LOG", &fixture.cargo_log)
        .env("PROJECTATLAS_VERSION", release_version())
        .env(
            "PROJECTATLAS_RELEASE_BASE_URL",
            UNREACHABLE_RELEASE_BASE_URL,
        )
        .env_remove("PROJECTATLAS_RUNTIME_PATH")
        .env_remove("PROJECTATLAS_RELEASE_BINARY_ONLY");
    Ok(())
}

#[cfg(windows)]
/// Run the `PowerShell` bootstrap with an already-built runtime and isolated host state.
fn run_powershell_runtime_install(
    project_root: &Path,
    isolated_root: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.ps1");
    let mut command = isolated_command("powershell", isolated_root)?;
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(installer)
        .arg("-ProjectRoot")
        .arg(project_root)
        .arg("-RuntimePath")
        .arg(assert_cmd::cargo::cargo_bin("projectatlas"))
        .arg("-ProjectAtlasVersion")
        .arg(release_version());
    Ok(command.output()?)
}

#[cfg(unix)]
/// Run the POSIX bootstrap with an already-built runtime and isolated host state.
fn run_posix_runtime_install(
    project_root: &Path,
    isolated_root: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let installer = workspace_root()?.join("plugins/projectatlas/scripts/install-runtime.sh");
    let mut command = isolated_command("bash", isolated_root)?;
    command
        .arg(installer)
        .arg(project_root)
        .env(
            "PROJECTATLAS_RUNTIME_PATH",
            assert_cmd::cargo::cargo_bin("projectatlas"),
        )
        .env("PROJECTATLAS_VERSION", release_version());
    Ok(command.output()?)
}

/// Create a subprocess with all installer-owned host state redirected into a fixture root.
fn isolated_command(program: &str, root: &Path) -> Result<Command, Box<dyn Error>> {
    let mut command = Command::new(program);
    configure_isolated_environment(&mut command, root)?;
    Ok(command)
}

/// Redirect installer-owned host paths and disable unrelated registry updates.
fn configure_isolated_environment(
    command: &mut Command,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let app_data = root.join("app-data");
    let local_app_data = root.join("local-app-data");
    let codex_home = root.join(".codex");
    fs::create_dir_all(&app_data)?;
    fs::create_dir_all(&local_app_data)?;
    fs::create_dir_all(&codex_home)?;
    command
        .timeout(INSTALL_TIMEOUT)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("APPDATA", app_data)
        .env("LOCALAPPDATA", local_app_data)
        .env("CODEX_HOME", codex_home)
        .env("PROJECTATLAS_SKIP_USER_PATH_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE", "1")
        .env("PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE", "1")
        .env("PROJECTATLAS_NO_TELEMETRY", "1");
    Ok(())
}

/// Prepend one fixture executable directory to the inherited process path.
fn prepend_path(path: &Path) -> Result<OsString, env::JoinPathsError> {
    let mut paths = vec![path.to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    env::join_paths(paths)
}

/// Return the release version expected by the checked-in bootstraps.
fn release_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// Convert a failed invariant into a test error without panicking.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Resolve the Cargo workspace containing this integration test.
fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}
