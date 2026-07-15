//! Closed configuration and environment policy for bounded local Git subprocesses.

use crate::bounded_process_supervisor::{SupervisionError, run_supervised};
use processkit::{Command, Stdin};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tempfile::{Builder as TempDirBuilder, TempDir};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::{
    ffi::{OsStrExt as _, OsStringExt as _},
    fs::PermissionsExt as _,
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;

/// Maximum accepted size of a linked-worktree `.git` metadata file.
const GIT_METADATA_FILE_LIMIT: u64 = 4 * 1024;
/// Maximum accepted bytes in one Git inventory.
const GIT_INVENTORY_BYTE_LIMIT: usize = 8 * 1024 * 1024;
/// Maximum accepted paths in one Git inventory.
const GIT_INVENTORY_ENTRY_LIMIT: usize = 100_000;
/// Maximum accepted bytes in one repository-relative Git path.
const GIT_PATH_BYTE_LIMIT: usize = 16 * 1024;
/// Maximum aggregate bytes accepted from tracked symlink materializations.
const GIT_LITERAL_HASH_BYTE_LIMIT: usize = 1024 * 1024;
/// Maximum accepted size of one resolved Git executable.
const GIT_EXECUTABLE_FILE_LIMIT: u64 = 256 * 1024 * 1024;
/// Deadline for one repository-bound Git query.
const GIT_QUERY_TIMEOUT: Duration = Duration::from_secs(30);
/// Retained stdout/stderr ceiling for one repository-bound Git query.
const GIT_QUERY_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
/// Prefix for an explicit Git metadata-directory binding.
const GIT_DIRECTORY_ARGUMENT_PREFIX: &str = "--git-dir=";
/// Prefix for an explicit Git worktree binding.
const GIT_WORK_TREE_ARGUMENT_PREFIX: &str = "--work-tree=";
/// Conversion-free query for the committed tree.
const RAW_HEAD_TREE_QUERY: &[&str] = &["ls-tree", "-r", "-z", "--full-tree", "HEAD"];
/// Conversion-free query for staged index entries.
const RAW_INDEX_QUERY: &[&str] = &["ls-files", "--stage", "-z"];
/// Query for index state flags such as skip-worktree and assume-unchanged.
const INDEX_FLAGS_QUERY: &[&str] = &["ls-files", "-v", "-z"];
/// Import exact stage rows into the private sanitized index.
const SANITIZED_INDEX_IMPORT_QUERY: &[&str] = &["update-index", "-z", "--index-info"];
/// Query untracked paths using only checked-in ignore rules and sanitized config.
const SANITIZED_UNTRACKED_QUERY: &[&str] = &["ls-files", "--others", "--exclude-standard", "-z"];
/// Hash worktree paths with built-in Git clean conversion and no filter drivers.
const SANITIZED_HASH_QUERY: &[&str] = &["hash-object", "--stdin-paths"];
/// Hash private literal files without applying path-based conversion.
const SANITIZED_LITERAL_HASH_QUERY: &[&str] = &["hash-object", "--no-filters", "--stdin-paths"];
/// Regular non-executable Git index mode.
const MODE_REGULAR: &[u8] = b"100644";
/// Regular executable Git index mode.
const MODE_EXECUTABLE: &[u8] = b"100755";
/// Git index mode for a symbolic link.
const MODE_SYMLINK: &[u8] = b"120000";
/// Windows file attribute identifying a reparse point.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Git configuration overrides that disable executable helpers and recursive work.
const CONFIGURATION_OVERRIDES: &[(&str, &str)] = &[
    ("core.fsmonitor", "false"),
    ("core.untrackedCache", "false"),
    ("diff.external", ""),
    ("diff.ignoreSubmodules", "all"),
    ("status.submoduleSummary", "false"),
    ("submodule.recurse", "false"),
];

/// Git variables retained after clearing the inherited environment.
const ENVIRONMENT_OVERRIDES: &[(&str, &str)] = &[
    ("GIT_CONFIG_COUNT", "0"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_LITERAL_PATHSPECS", "1"),
    ("GIT_NO_LAZY_FETCH", "1"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_PAGER", "cat"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
];

/// Failures while resolving or executing one repository-bound Git probe.
#[derive(Debug, Error)]
pub(crate) enum RepositoryGitError {
    /// Repository, executable, or command evidence violated the closed Git policy.
    #[error("repository Git policy failed: {0}")]
    Policy(String),
    /// A filesystem operation failed while binding the repository or executable.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The shared process-tree supervisor failed.
    #[error(transparent)]
    Supervision(#[from] SupervisionError),
}

/// Canonical Git executable and repository paths for bounded read-only probes.
pub(crate) struct RepositoryGitProbe {
    /// Canonical executable used for every probe query.
    executable: PathBuf,
    /// SHA-256 digest of the exact executable bytes resolved from `PATH`.
    executable_sha256: String,
    /// Canonical repository worktree bound by every query.
    work_tree: PathBuf,
    /// Canonical repository metadata directory bound by every query.
    git_directory: PathBuf,
}

impl RepositoryGitProbe {
    /// Resolve one canonical repository and one unambiguous Git executable identity.
    pub(crate) fn resolve(root: &Path) -> Result<Self, RepositoryGitError> {
        let work_tree = fs::canonicalize(root)?;
        if !fs::metadata(&work_tree)?.is_dir() {
            return Err(RepositoryGitError::Policy(
                "Git worktree root is not a canonical directory".into(),
            ));
        }
        let search_path = env::var_os("PATH")
            .ok_or_else(|| RepositoryGitError::Policy("Git PATH is not defined".into()))?;
        let (executable, executable_sha256) = resolve_git_executable(&work_tree, &search_path)?;
        let git_directory = resolve_git_directory(&work_tree)?;
        Ok(Self {
            executable,
            executable_sha256,
            work_tree,
            git_directory,
        })
    }

    /// Return the canonical Git executable path bound by this probe.
    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    /// Return the SHA-256 digest of the bound Git executable bytes.
    pub(crate) fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }

    /// Run one repository-bound Git query with closed config, environment, and process bounds.
    pub(crate) async fn output_bytes(
        &self,
        arguments: &[&str],
    ) -> Result<Vec<u8>, RepositoryGitError> {
        let mut command_arguments =
            repository_bound_git_arguments(&self.git_directory, &self.work_tree)?;
        command_arguments.extend(arguments.iter().map(OsString::from));
        self.run_command(&self.work_tree, command_arguments, None)
            .await
    }

    /// Compute one filter-free HEAD/index/worktree state twice and reject concurrent drift.
    pub(crate) async fn worktree_state(&self) -> Result<Vec<u8>, RepositoryGitError> {
        let first = self.worktree_state_pass().await?;
        let second = self.worktree_state_pass().await?;
        consistent_worktree_state(&first, second)
    }

    /// Compute one filter-free repository state pass through a private sanitized index.
    async fn worktree_state_pass(&self) -> Result<SanitizedWorktreeEvidence, RepositoryGitError> {
        let head = self.output_bytes(raw_head_tree_query()).await?;
        let index = self.output_bytes(raw_index_query()).await?;
        let index_flags = self.output_bytes(index_flags_query()).await?;
        let plan =
            plan_sanitized_worktree_comparison(&self.work_tree, &head, &index, &index_flags)?;
        let workspace = SanitizedGitWorkspace::create(
            &self.git_directory,
            &self.work_tree,
            plan.object_format(),
        )?;
        let literal_input =
            workspace.materialize_literal_hash_inputs(plan.literal_hash_inputs())?;
        self.run_command(
            &self.work_tree,
            workspace.command_arguments(sanitized_index_import_query())?,
            Some(plan.index_input()),
        )
        .await?;
        let hashes = self
            .run_command(
                &self.work_tree,
                workspace.command_arguments(sanitized_hash_query())?,
                Some(plan.hash_input()),
            )
            .await?;
        let literal_hashes = self
            .run_command(
                workspace.literal_directory(),
                workspace.command_arguments(sanitized_literal_hash_query())?,
                Some(&literal_input),
            )
            .await?;
        let untracked = self
            .run_command(
                &self.work_tree,
                workspace.command_arguments(sanitized_untracked_query())?,
                None,
            )
            .await?;
        Ok(plan.finish(&hashes, &literal_hashes, &untracked)?)
    }

    /// Execute one already-bound Git command with the shared process policy.
    async fn run_command(
        &self,
        current_directory: &Path,
        command_arguments: Vec<OsString>,
        stdin: Option<&[u8]>,
    ) -> Result<Vec<u8>, RepositoryGitError> {
        let executable_directory = self.executable.parent().ok_or_else(|| {
            RepositoryGitError::Policy("Git executable has no parent directory".into())
        })?;
        let mut command = Command::new(&self.executable)
            .args(&command_arguments)
            .current_dir(current_directory)
            .env_clear()
            .env("PATH", executable_directory)
            .env("GIT_CONFIG_GLOBAL", git_null_device())
            .env("GIT_CONFIG_SYSTEM", git_null_device());
        for (name, value) in closed_git_environment() {
            command = command.env(name, value);
        }
        #[cfg(windows)]
        for name in ["SYSTEMROOT", "WINDIR"] {
            if let Some(value) = env::var_os(name) {
                command = command.env(name, value);
            }
        }
        if let Some(bytes) = stdin {
            command = command.stdin(Stdin::from_bytes(bytes));
        }
        let output = run_supervised(command, GIT_QUERY_TIMEOUT, GIT_QUERY_OUTPUT_LIMIT).await?;
        if output.output_truncated {
            return Err(RepositoryGitError::Policy(
                "Git query exceeded the retained output limit".into(),
            ));
        }
        if !output.is_success() {
            return Err(RepositoryGitError::Policy(format!(
                "Git query failed: {}",
                String::from_utf8_lossy(&output.stderr.retained).trim()
            )));
        }
        Ok(output.stdout.retained)
    }
}

/// Resolve one canonical Git executable identity and reject ambiguous `PATH` entries.
fn resolve_git_executable(
    root: &Path,
    search_path: &std::ffi::OsStr,
) -> Result<(PathBuf, String), RepositoryGitError> {
    let executable_name = format!("git{}", env::consts::EXE_SUFFIX);
    let mut paths = BTreeSet::new();
    let mut selected: Option<(PathBuf, String)> = None;
    for directory in env::split_paths(search_path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(&executable_name);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > GIT_EXECUTABLE_FILE_LIMIT {
            continue;
        }
        let canonical = fs::canonicalize(candidate)?;
        if canonical.starts_with(root) {
            return Err(RepositoryGitError::Policy(
                "Git executable resolves inside the repository".into(),
            ));
        }
        if !paths.insert(canonical.clone()) {
            continue;
        }
        let executable_bytes = read_bounded_file(
            &canonical,
            usize::try_from(GIT_EXECUTABLE_FILE_LIMIT).map_err(|source| {
                RepositoryGitError::Policy(format!(
                    "Git executable byte limit does not fit this platform: {source}"
                ))
            })?,
        )?;
        let sha256 = format!("{:x}", Sha256::digest(executable_bytes));
        if let Some((_, selected_sha256)) = &selected {
            if selected_sha256 != &sha256 {
                return Err(RepositoryGitError::Policy(
                    "Git executable identity is ambiguous across PATH".into(),
                ));
            }
        } else {
            selected = Some((canonical, sha256));
        }
    }
    selected.ok_or_else(|| RepositoryGitError::Policy("Git executable not found".into()))
}

/// Require both complete sanitized observations to match before returning state bytes.
fn consistent_worktree_state(
    first: &SanitizedWorktreeEvidence,
    second: SanitizedWorktreeEvidence,
) -> Result<Vec<u8>, RepositoryGitError> {
    if first != &second {
        return Err(RepositoryGitError::Policy(
            "sanitized Git worktree state changed between verification passes".into(),
        ));
    }
    Ok(second.into_state())
}

/// Build the global arguments that close Git configuration and helper execution.
pub(crate) fn closed_git_arguments() -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--no-pager"),
        OsString::from("--no-optional-locks"),
        OsString::from("--literal-pathspecs"),
    ];
    for (key, value) in CONFIGURATION_OVERRIDES {
        arguments.push(OsString::from("-c"));
        arguments.push(OsString::from(format!("{key}={value}")));
    }
    arguments.push(OsString::from("-c"));
    arguments.push(OsString::from(format!(
        "core.hooksPath={}",
        git_null_device()
    )));
    arguments
}

/// Build Git arguments bound to one explicit metadata directory and worktree.
pub(crate) fn repository_bound_git_arguments(
    git_directory: &Path,
    work_tree: &Path,
) -> io::Result<Vec<OsString>> {
    let mut arguments = closed_git_arguments();
    let mut git_directory_argument = OsString::from(GIT_DIRECTORY_ARGUMENT_PREFIX);
    git_directory_argument.push(git_path_argument(git_directory)?);
    arguments.push(git_directory_argument);
    let mut work_tree_argument = OsString::from(GIT_WORK_TREE_ARGUMENT_PREFIX);
    work_tree_argument.push(git_path_argument(work_tree)?);
    arguments.push(work_tree_argument);
    Ok(arguments)
}

/// Return the conversion-free committed-tree query.
pub(crate) const fn raw_head_tree_query() -> &'static [&'static str] {
    RAW_HEAD_TREE_QUERY
}

/// Return the conversion-free staged-index query.
pub(crate) const fn raw_index_query() -> &'static [&'static str] {
    RAW_INDEX_QUERY
}

/// Return the index-state flag query.
pub(crate) const fn index_flags_query() -> &'static [&'static str] {
    INDEX_FLAGS_QUERY
}

/// Return the private sanitized-index import command.
pub(crate) const fn sanitized_index_import_query() -> &'static [&'static str] {
    SANITIZED_INDEX_IMPORT_QUERY
}

/// Return the sanitized untracked-path query.
pub(crate) const fn sanitized_untracked_query() -> &'static [&'static str] {
    SANITIZED_UNTRACKED_QUERY
}

/// Return the built-in-conversion worktree hash command.
pub(crate) const fn sanitized_hash_query() -> &'static [&'static str] {
    SANITIZED_HASH_QUERY
}

/// Return the literal-blob hash command used for tracked symlinks.
pub(crate) const fn sanitized_literal_hash_query() -> &'static [&'static str] {
    SANITIZED_LITERAL_HASH_QUERY
}

/// Object identifier algorithm used by one repository index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitObjectFormat {
    /// Forty-hex-digit object identifiers.
    Sha1,
    /// Sixty-four-hex-digit object identifiers.
    Sha256,
}

impl GitObjectFormat {
    /// Resolve one format from an already validated object identifier.
    fn from_oid(oid: &[u8]) -> io::Result<Self> {
        match oid.len() {
            40 => Ok(Self::Sha1),
            64 => Ok(Self::Sha256),
            _ => Err(invalid_git_metadata(
                "Git object identifier has an unsupported width",
            )),
        }
    }

    /// Return the repository-format config required by native Git.
    const fn repository_format_version(self) -> u8 {
        match self {
            Self::Sha1 => 0,
            Self::Sha256 => 1,
        }
    }

    /// Return the optional object-format config block.
    const fn extension_config(self) -> &'static str {
        match self {
            Self::Sha1 => "",
            Self::Sha256 => "[extensions]\nobjectFormat = sha256\n",
        }
    }
}

/// One committed or staged path identity.
#[derive(Clone, Debug, Eq, PartialEq)]
struct GitPathIdentity {
    /// Git mode bytes.
    mode: Vec<u8>,
    /// Object identifier bytes.
    oid: Vec<u8>,
}

/// One worktree file that must hash to its staged object identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
struct HashExpectation {
    /// Repository-relative Git path bytes.
    path: Vec<u8>,
    /// Expected Git-clean blob object identifier.
    oid: Vec<u8>,
}

/// Parsed stage-zero index plus the complete path inventory.
struct ParsedIndex {
    /// Stage-zero entries used for worktree comparison.
    entries: BTreeMap<Vec<u8>, GitPathIdentity>,
    /// Every path present at any index stage.
    paths: BTreeSet<Vec<u8>>,
}

/// Comparison plan using sanitized built-in Git clean conversion.
#[derive(Debug)]
pub(crate) struct SanitizedWorktreeComparison {
    /// Repository object format inferred from validated inventories.
    object_format: GitObjectFormat,
    /// Exact stage rows imported into the private sanitized index.
    index_input: Vec<u8>,
    /// Newline-delimited paths accepted by `git hash-object --stdin-paths`.
    hash_input: Vec<u8>,
    /// Hash expectations in exact input order.
    expectations: Vec<HashExpectation>,
    /// Exact tracked symlink materializations hashed as literal blob bytes.
    literal_hash_inputs: Vec<Vec<u8>>,
    /// Literal blob expectations in exact input order.
    literal_expectations: Vec<HashExpectation>,
    /// Canonical dirty or unsupported-state records discovered before hashing.
    findings: Vec<Vec<u8>>,
    /// Length-framed HEAD, index, flag, hash, and untracked inventory bytes.
    binding: Vec<u8>,
}

/// One complete sanitized worktree observation.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SanitizedWorktreeEvidence {
    /// Canonical dirty or unsupported-state records.
    state: Vec<u8>,
    /// Length-framed inventories and built-in-conversion hash output.
    binding: Vec<u8>,
}

impl SanitizedWorktreeEvidence {
    /// Consume the evidence and return its canonical worktree-state bytes.
    pub(crate) fn into_state(self) -> Vec<u8> {
        self.state
    }
}

impl SanitizedWorktreeComparison {
    /// Return the validated repository object format.
    pub(crate) const fn object_format(&self) -> GitObjectFormat {
        self.object_format
    }

    /// Return exact stage rows for the private sanitized index.
    pub(crate) fn index_input(&self) -> &[u8] {
        &self.index_input
    }

    /// Return bounded path input for built-in-conversion hashing.
    pub(crate) fn hash_input(&self) -> &[u8] {
        &self.hash_input
    }

    /// Return exact literal blob inputs for tracked symlink materializations.
    pub(crate) fn literal_hash_inputs(&self) -> &[Vec<u8>] {
        &self.literal_hash_inputs
    }

    /// Reconcile hashes and sanitized untracked paths into deterministic state.
    pub(crate) fn finish(
        mut self,
        observed_hashes: &[u8],
        observed_literal_hashes: &[u8],
        untracked_rows: &[u8],
    ) -> io::Result<SanitizedWorktreeEvidence> {
        let observed = observed_hashes
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
            .map(|row| row.strip_suffix(b"\r").unwrap_or(row))
            .collect::<Vec<_>>();
        if observed.len() != self.expectations.len() {
            return Err(invalid_git_data(format!(
                "sanitized worktree hash count drifted: expected {}, observed {}",
                self.expectations.len(),
                observed.len()
            )));
        }
        for (expectation, observed_oid) in self.expectations.iter().zip(observed) {
            validate_oid(observed_oid)?;
            if GitObjectFormat::from_oid(observed_oid)? != self.object_format {
                return Err(invalid_git_metadata(
                    "sanitized worktree hash object format drifted",
                ));
            }
            if observed_oid != expectation.oid {
                self.findings.push(finding(
                    b"modified",
                    &expectation.path,
                    &[expectation.oid.as_slice(), observed_oid].concat(),
                ));
            }
        }
        let observed_literals = observed_literal_hashes
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
            .map(|row| row.strip_suffix(b"\r").unwrap_or(row))
            .collect::<Vec<_>>();
        if observed_literals.len() != self.literal_expectations.len() {
            return Err(invalid_git_data(format!(
                "sanitized literal hash count drifted: expected {}, observed {}",
                self.literal_expectations.len(),
                observed_literals.len()
            )));
        }
        for (expectation, observed_oid) in self.literal_expectations.iter().zip(observed_literals) {
            validate_oid(observed_oid)?;
            if GitObjectFormat::from_oid(observed_oid)? != self.object_format {
                return Err(invalid_git_metadata(
                    "sanitized literal hash object format drifted",
                ));
            }
            if observed_oid != expectation.oid {
                self.findings.push(finding(
                    b"modified",
                    &expectation.path,
                    &[expectation.oid.as_slice(), observed_oid].concat(),
                ));
            }
        }
        append_untracked_findings(untracked_rows, &mut self.findings)?;
        self.findings.sort_unstable();
        append_binding_field(&mut self.binding, observed_hashes);
        append_binding_field(&mut self.binding, observed_literal_hashes);
        append_binding_field(&mut self.binding, untracked_rows);
        Ok(SanitizedWorktreeEvidence {
            state: self.findings.concat(),
            binding: self.binding,
        })
    }
}

/// Build a sanitized HEAD/index/worktree comparison plan.
pub(crate) fn plan_sanitized_worktree_comparison(
    work_tree: &Path,
    head_rows: &[u8],
    index_rows: &[u8],
    index_flag_rows: &[u8],
) -> io::Result<SanitizedWorktreeComparison> {
    if !work_tree.is_absolute() || !fs::metadata(work_tree)?.is_dir() {
        return Err(invalid_git_metadata(
            "sanitized worktree comparison root is not a canonical directory",
        ));
    }
    for rows in [head_rows, index_rows, index_flag_rows] {
        if rows.len() > GIT_INVENTORY_BYTE_LIMIT {
            return Err(invalid_git_metadata("Git inventory exceeds the byte limit"));
        }
    }
    let mut object_format = None;
    let head = parse_head_rows(head_rows, &mut object_format)?;
    let mut findings = Vec::new();
    let index = parse_index_rows(index_rows, &mut findings, &mut object_format)?;
    parse_index_flags(index_flag_rows, &index.paths, &mut findings)?;
    compare_head_and_index(&head, &index.entries, &mut findings);
    let object_format = object_format
        .ok_or_else(|| invalid_git_metadata("Git inventory has no object identifiers"))?;

    let mut hash_input = Vec::new();
    let mut expectations = Vec::new();
    let mut literal_hash_inputs = Vec::new();
    let mut literal_expectations = Vec::new();
    let mut literal_bytes = 0_usize;
    let mut native_identities = BTreeMap::new();
    for (path, identity) in &index.entries {
        let native_identity = native_path_identity(path)?;
        if let Some(previous) = native_identities.insert(native_identity, path) {
            return Err(invalid_git_data(format!(
                "Git paths alias one native worktree path: {:?} and {:?}",
                String::from_utf8_lossy(previous),
                String::from_utf8_lossy(path)
            )));
        }
        if path.contains(&b'\n') {
            findings.push(finding(b"unsupported-newline-path", path, &[]));
            continue;
        }
        if identity.mode == MODE_SYMLINK {
            match inspect_worktree_path(work_tree, path)? {
                WorktreePathState::Regular(metadata) => {
                    literal_bytes = literal_bytes.saturating_add(metadata.len() as usize);
                    if literal_bytes > GIT_LITERAL_HASH_BYTE_LIMIT {
                        return Err(invalid_git_metadata(
                            "tracked symlink materializations exceed the byte limit",
                        ));
                    }
                    literal_hash_inputs.push(read_bounded_file(
                        &work_tree.join(path_from_git_bytes(path)?),
                        GIT_LITERAL_HASH_BYTE_LIMIT,
                    )?);
                    literal_expectations.push(HashExpectation {
                        path: path.clone(),
                        oid: identity.oid.clone(),
                    });
                }
                #[cfg(unix)]
                WorktreePathState::Symlink(path_on_disk) => {
                    let target = fs::read_link(path_on_disk)?;
                    let bytes = target.as_os_str().as_bytes().to_vec();
                    literal_bytes = literal_bytes.saturating_add(bytes.len());
                    if literal_bytes > GIT_LITERAL_HASH_BYTE_LIMIT {
                        return Err(invalid_git_metadata(
                            "tracked symlink materializations exceed the byte limit",
                        ));
                    }
                    literal_hash_inputs.push(bytes);
                    literal_expectations.push(HashExpectation {
                        path: path.clone(),
                        oid: identity.oid.clone(),
                    });
                }
                #[cfg(windows)]
                WorktreePathState::Symlink(path_on_disk) => {
                    let _ = path_on_disk;
                    findings.push(finding(b"unsafe-worktree-path", path, &[]));
                }
                #[cfg(windows)]
                WorktreePathState::Unsafe => {
                    findings.push(finding(b"unsafe-worktree-path", path, &[]));
                }
                #[cfg(unix)]
                WorktreePathState::Unsafe => {
                    findings.push(finding(b"unsafe-worktree-path", path, &[]));
                }
                WorktreePathState::Missing => findings.push(finding(b"missing", path, &[])),
            }
            continue;
        }
        if identity.mode != MODE_REGULAR && identity.mode != MODE_EXECUTABLE {
            findings.push(finding(b"unsupported-mode", path, &identity.mode));
            continue;
        }
        match inspect_worktree_path(work_tree, path)? {
            WorktreePathState::Regular(metadata) => {
                #[cfg(unix)]
                {
                    let expected_executable = identity.mode == MODE_EXECUTABLE;
                    let observed_executable = metadata.permissions().mode() & 0o100 != 0;
                    if expected_executable != observed_executable {
                        findings.push(finding(b"worktree-mode", path, &identity.mode));
                    }
                }
                #[cfg(not(unix))]
                let _ = metadata;
                hash_input.extend_from_slice(path);
                hash_input.push(b'\n');
                expectations.push(HashExpectation {
                    path: path.clone(),
                    oid: identity.oid.clone(),
                });
            }
            WorktreePathState::Missing => findings.push(finding(b"missing", path, &[])),
            WorktreePathState::Unsafe | WorktreePathState::Symlink(_) => {
                findings.push(finding(b"unsafe-worktree-path", path, &[]));
            }
        }
    }

    let mut binding = Vec::new();
    for field in [head_rows, index_rows, index_flag_rows] {
        append_binding_field(&mut binding, field);
    }
    Ok(SanitizedWorktreeComparison {
        object_format,
        index_input: index_rows.to_vec(),
        hash_input,
        expectations,
        literal_hash_inputs,
        literal_expectations,
        findings,
        binding,
    })
}

/// Private Git directory with no inherited filter, attribute, hook, or exclude config.
pub(crate) struct SanitizedGitWorkspace {
    /// Own the directory until all bounded Git calls complete.
    _directory: TempDir,
    /// Canonical private Git metadata directory.
    git_directory: PathBuf,
    /// Private directory containing bounded literal blob inputs.
    literal_directory: PathBuf,
    /// Canonical repository worktree inspected by native Git.
    work_tree: PathBuf,
}

impl SanitizedGitWorkspace {
    /// Create a private Git directory that exposes original objects only as an alternate.
    pub(crate) fn create(
        source_git_directory: &Path,
        work_tree: &Path,
        object_format: GitObjectFormat,
    ) -> io::Result<Self> {
        let parent = work_tree
            .parent()
            .ok_or_else(|| invalid_git_metadata("worktree has no parent directory"))?;
        let directory = TempDirBuilder::new()
            .prefix(".projectatlas-git-evidence-")
            .tempdir_in(parent)?;
        let root = fs::canonicalize(directory.path())?;
        if root.starts_with(work_tree) {
            return Err(invalid_git_metadata(
                "sanitized Git directory was created inside the worktree",
            ));
        }
        let git_directory = root.join("git");
        let literal_directory = root.join("literal-inputs");

        fs::create_dir_all(git_directory.join("objects").join("info"))?;
        fs::create_dir_all(git_directory.join("refs").join("heads"))?;
        fs::create_dir_all(git_directory.join("info"))?;
        fs::create_dir(&literal_directory)?;
        fs::write(git_directory.join("HEAD"), b"ref: refs/heads/sanitized\n")?;
        fs::write(git_directory.join("info").join("exclude"), b"")?;

        // Canonical input conversion accepts platform line-ending materialization while the
        // private config still leaves every external filter driver undefined and unexecutable.
        let config = format!(
            "[core]\nrepositoryformatversion = {}\nbare = false\nfilemode = {}\nsymlinks = {}\nignorecase = {}\nlogallrefupdates = false\nautocrlf = input\nsafecrlf = false\nfsmonitor = false\nuntrackedCache = false\nhooksPath = {}\nattributesFile = {}\nexcludesFile = {}\n{}",
            object_format.repository_format_version(),
            cfg!(unix),
            cfg!(unix),
            cfg!(windows),
            git_null_device(),
            git_null_device(),
            git_null_device(),
            object_format.extension_config(),
        );
        fs::write(git_directory.join("config"), config.as_bytes())?;

        let common_directory = resolve_git_common_directory(source_git_directory)?;
        let objects = common_directory.join("objects");
        let metadata = fs::symlink_metadata(&objects)?;
        if is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(invalid_git_metadata(
                "Git object directory is not a real directory",
            ));
        }
        let objects = fs::canonicalize(objects)?;
        let mut alternate = git_file_path_bytes(&objects)?;
        alternate.push(b'\n');
        fs::write(
            git_directory
                .join("objects")
                .join("info")
                .join("alternates"),
            alternate,
        )?;

        Ok(Self {
            _directory: directory,
            git_directory,
            literal_directory,
            work_tree: work_tree.to_owned(),
        })
    }

    /// Build closed arguments bound to the private Git directory and real worktree.
    pub(crate) fn command_arguments(&self, query: &[&str]) -> io::Result<Vec<OsString>> {
        let mut arguments = repository_bound_git_arguments(&self.git_directory, &self.work_tree)?;
        arguments.extend(query.iter().map(OsString::from));
        Ok(arguments)
    }

    /// Materialize bounded literal blobs and return newline-delimited private filenames.
    pub(crate) fn materialize_literal_hash_inputs(
        &self,
        inputs: &[Vec<u8>],
    ) -> io::Result<Vec<u8>> {
        let mut paths = Vec::new();
        for (index, bytes) in inputs.iter().enumerate() {
            let name = format!("literal-{index:08}");
            let path = self.literal_directory.join(&name);
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            options.open(path)?.write_all(bytes)?;
            paths.extend_from_slice(name.as_bytes());
            paths.push(b'\n');
        }
        Ok(paths)
    }

    /// Return the private working directory used for literal blob hashing.
    pub(crate) fn literal_directory(&self) -> &Path {
        &self.literal_directory
    }
}

/// Resolve `.git` directories and linked-worktree metadata files without invoking Git.
pub(crate) fn resolve_git_directory(root: &Path) -> io::Result<PathBuf> {
    let dot_git = root.join(".git");
    let metadata = fs::symlink_metadata(&dot_git)?;
    if is_link_or_reparse(&metadata) {
        return Err(invalid_git_metadata(
            "Git metadata entry is a symlink or reparse point",
        ));
    }
    let declared = if metadata.is_dir() {
        dot_git
    } else {
        if !metadata.is_file() || metadata.len() > GIT_METADATA_FILE_LIMIT {
            return Err(invalid_git_metadata(
                "Git metadata entry is not a bounded file or directory",
            ));
        }
        let value = fs::read_to_string(&dot_git)?;
        let mut lines = value.lines();
        let first = lines
            .next()
            .and_then(|line| line.strip_prefix("gitdir: "))
            .filter(|path| !path.is_empty())
            .ok_or_else(|| invalid_git_metadata("Git metadata file is malformed"))?;
        if lines.next().is_some() {
            return Err(invalid_git_metadata("Git metadata file has extra records"));
        }
        let path = PathBuf::from(first);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    let canonical = fs::canonicalize(declared)?;
    if !canonical.is_absolute() || !fs::metadata(&canonical)?.is_dir() {
        return Err(invalid_git_metadata(
            "Git metadata directory is not canonical",
        ));
    }
    Ok(canonical)
}

/// Resolve a linked worktree's common Git directory without loading config.
fn resolve_git_common_directory(git_directory: &Path) -> io::Result<PathBuf> {
    let commondir = git_directory.join("commondir");
    let declared = match fs::symlink_metadata(&commondir) {
        Ok(metadata) => {
            if is_link_or_reparse(&metadata)
                || !metadata.is_file()
                || metadata.len() > GIT_METADATA_FILE_LIMIT
            {
                return Err(invalid_git_metadata(
                    "Git common-directory metadata is not a bounded real file",
                ));
            }
            let value = fs::read_to_string(&commondir)?;
            let mut lines = value.lines();
            let first = lines
                .next()
                .filter(|path| !path.is_empty())
                .ok_or_else(|| invalid_git_metadata("Git common-directory file is malformed"))?;
            if lines.next().is_some() {
                return Err(invalid_git_metadata(
                    "Git common-directory file has extra records",
                ));
            }
            let path = PathBuf::from(first);
            if path.is_absolute() {
                path
            } else {
                git_directory.join(path)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => git_directory.to_owned(),
        Err(error) => return Err(error),
    };
    let canonical = fs::canonicalize(declared)?;
    if !canonical.is_absolute() || !fs::metadata(&canonical)?.is_dir() {
        return Err(invalid_git_metadata(
            "Git common directory is not canonical",
        ));
    }
    Ok(canonical)
}

/// Render a canonical native path for Git's line-oriented alternates file.
fn git_file_path_bytes(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&b'\n') || bytes.contains(&b'\r') {
            return Err(invalid_git_metadata(
                "Git object directory contains a line separator",
            ));
        }
        Ok(bytes.to_vec())
    }
    #[cfg(windows)]
    {
        let text = git_path_argument(path)?
            .into_string()
            .map_err(|_path| invalid_git_metadata("Git object directory is not Unicode"))?;
        if text.contains(['\n', '\r']) {
            return Err(invalid_git_metadata(
                "Git object directory contains a line separator",
            ));
        }
        Ok(text.replace('\\', "/").into_bytes())
    }
}

/// Return the environment entries that remain authoritative after `env_clear`.
pub(crate) const fn closed_git_environment() -> &'static [(&'static str, &'static str)] {
    ENVIRONMENT_OVERRIDES
}

/// Return the platform null path used to suppress Git configuration and hooks.
pub(crate) const fn git_null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// Render a canonical path for native Git, which rejects Windows verbatim prefixes.
fn git_path_argument(path: &Path) -> io::Result<OsString> {
    #[cfg(unix)]
    if path.as_os_str().as_bytes().contains(&0) {
        return Err(invalid_git_metadata("Git path contains a NUL byte"));
    }
    #[cfg(windows)]
    {
        let text = path
            .to_str()
            .ok_or_else(|| invalid_git_metadata("Git path is not valid Unicode"))?;
        if text.contains('\0') {
            return Err(invalid_git_metadata("Git path contains a NUL character"));
        }
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(OsString::from(format!(r"\\{unc}")));
        }
        if let Some(plain) = text.strip_prefix(r"\\?\") {
            return Ok(OsString::from(plain));
        }
    }
    Ok(path.as_os_str().to_owned())
}

/// One filesystem observation for an indexed regular file path.
pub(crate) enum WorktreePathState {
    /// Every parent is a real directory and the leaf is a regular file.
    Regular(fs::Metadata),
    /// Every parent is real and the leaf is a symbolic link.
    Symlink(PathBuf),
    /// The indexed path is absent.
    Missing,
    /// A parent or leaf is link-like or has an incompatible type.
    Unsafe,
}

/// Parse committed tree rows without consulting worktree conversion rules.
fn parse_head_rows(
    rows: &[u8],
    object_format: &mut Option<GitObjectFormat>,
) -> io::Result<BTreeMap<Vec<u8>, GitPathIdentity>> {
    require_nul_terminated(rows, "committed-tree inventory")?;
    let mut entries = BTreeMap::new();
    for row in rows.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        require_entry_capacity(entries.len(), "committed-tree inventory")?;
        let (metadata, path) = split_metadata_and_path(row, "committed-tree row")?;
        validate_git_path(path)?;
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 || (fields[1] != b"blob" && fields[1] != b"commit") {
            return Err(invalid_git_metadata("committed-tree row is malformed"));
        }
        validate_mode(fields[0])?;
        validate_oid(fields[2])?;
        observe_object_format(fields[2], object_format)?;
        let identity = GitPathIdentity {
            mode: fields[0].to_vec(),
            oid: fields[2].to_vec(),
        };
        if entries.insert(path.to_vec(), identity).is_some() {
            return Err(invalid_git_metadata(
                "committed-tree inventory contains a duplicate path",
            ));
        }
    }
    Ok(entries)
}

/// Parse staged index rows while retaining merge conflicts as dirty findings.
fn parse_index_rows(
    rows: &[u8],
    findings: &mut Vec<Vec<u8>>,
    object_format: &mut Option<GitObjectFormat>,
) -> io::Result<ParsedIndex> {
    require_nul_terminated(rows, "index inventory")?;
    let mut entries = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut stages = BTreeSet::new();
    for row in rows.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        require_entry_capacity(stages.len(), "index inventory")?;
        let (metadata, path) = split_metadata_and_path(row, "index row")?;
        validate_git_path(path)?;
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 || fields[2].len() != 1 || !matches!(fields[2][0], b'0'..=b'3') {
            return Err(invalid_git_metadata("index row is malformed"));
        }
        validate_mode(fields[0])?;
        validate_oid(fields[1])?;
        if fields[1].iter().all(|byte| *byte == b'0') {
            return Err(invalid_git_metadata(
                "index inventory contains an intent-to-add object identifier",
            ));
        }
        observe_object_format(fields[1], object_format)?;
        if !stages.insert((path.to_vec(), fields[2][0])) {
            return Err(invalid_git_metadata(
                "index inventory contains a duplicate path and stage",
            ));
        }
        paths.insert(path.to_vec());
        if fields[2] != b"0" {
            findings.push(finding(b"index-conflict", path, metadata));
            continue;
        }
        let identity = GitPathIdentity {
            mode: fields[0].to_vec(),
            oid: fields[1].to_vec(),
        };
        if entries.insert(path.to_vec(), identity).is_some() {
            return Err(invalid_git_metadata(
                "index inventory contains a duplicate stage-zero path",
            ));
        }
    }
    Ok(ParsedIndex { entries, paths })
}

/// Parse index flags and reject sparse, assume-unchanged, or unresolved state.
fn parse_index_flags(
    rows: &[u8],
    expected_paths: &BTreeSet<Vec<u8>>,
    findings: &mut Vec<Vec<u8>>,
) -> io::Result<()> {
    require_nul_terminated(rows, "index-flag inventory")?;
    let mut observed_paths = BTreeSet::new();
    for row in rows.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        require_entry_capacity(observed_paths.len(), "index-flag inventory")?;
        if row.len() < 3 || row[1] != b' ' {
            return Err(invalid_git_metadata("index-flag row is malformed"));
        }
        let path = &row[2..];
        validate_git_path(path)?;
        if !observed_paths.insert(path.to_vec()) {
            return Err(invalid_git_metadata(
                "index-flag inventory contains a duplicate path",
            ));
        }
        if row[0] != b'H' {
            findings.push(finding(b"unsupported-index-flag", path, &row[..1]));
        }
    }
    if observed_paths != *expected_paths {
        return Err(invalid_git_metadata(
            "index-flag inventory does not match the staged path inventory",
        ));
    }
    Ok(())
}

/// Add deterministic staged add/delete/change findings.
fn compare_head_and_index(
    head: &BTreeMap<Vec<u8>, GitPathIdentity>,
    index: &BTreeMap<Vec<u8>, GitPathIdentity>,
    findings: &mut Vec<Vec<u8>>,
) {
    for (path, committed) in head {
        match index.get(path) {
            None => findings.push(finding(b"staged-delete", path, &committed.oid)),
            Some(staged) if staged != committed => findings.push(finding(
                b"staged-change",
                path,
                &[
                    committed.mode.as_slice(),
                    committed.oid.as_slice(),
                    staged.mode.as_slice(),
                    staged.oid.as_slice(),
                ]
                .concat(),
            )),
            Some(_) => {}
        }
    }
    for (path, staged) in index {
        if !head.contains_key(path) {
            findings.push(finding(b"staged-add", path, &staged.oid));
        }
    }
}

/// Add deterministic untracked-path findings from `git ls-files --others`.
fn append_untracked_findings(rows: &[u8], findings: &mut Vec<Vec<u8>>) -> io::Result<()> {
    if rows.len() > GIT_INVENTORY_BYTE_LIMIT {
        return Err(invalid_git_metadata(
            "untracked-path inventory exceeds the byte limit",
        ));
    }
    require_nul_terminated(rows, "untracked-path inventory")?;
    let mut paths = BTreeSet::new();
    for path in rows.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        require_entry_capacity(paths.len(), "untracked-path inventory")?;
        validate_git_path(path)?;
        if !paths.insert(path.to_vec()) {
            return Err(invalid_git_metadata(
                "untracked-path inventory contains a duplicate path",
            ));
        }
        findings.push(finding(b"untracked", path, &[]));
    }
    Ok(())
}

/// Inspect every path component without following a symlink or reparse point.
pub(crate) fn inspect_worktree_path(root: &Path, git_path: &[u8]) -> io::Result<WorktreePathState> {
    let root_metadata = fs::symlink_metadata(root)?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Ok(WorktreePathState::Unsafe);
    }
    let relative = path_from_git_bytes(git_path)?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Ok(WorktreePathState::Unsafe);
    }
    let component_count = relative.components().count();
    let mut current = root.to_owned();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(name) = component else {
            return Ok(WorktreePathState::Unsafe);
        };
        current.push(name);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(WorktreePathState::Missing);
            }
            Err(error) => return Err(error),
        };
        let is_leaf = index + 1 == component_count;
        if is_link_or_reparse(&metadata) {
            return Ok(if is_leaf && metadata.file_type().is_symlink() {
                WorktreePathState::Symlink(current)
            } else {
                WorktreePathState::Unsafe
            });
        }
        if is_leaf {
            return Ok(if metadata.is_file() {
                WorktreePathState::Regular(metadata)
            } else {
                WorktreePathState::Unsafe
            });
        }
        if !metadata.is_dir() {
            return Ok(WorktreePathState::Unsafe);
        }
    }
    Ok(WorktreePathState::Unsafe)
}

/// Read one already-inspected regular file with an explicit byte ceiling.
fn read_bounded_file(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid_git_metadata(
            "file exceeds the requested byte limit",
        ));
    }
    Ok(bytes)
}

/// Convert exact Git path bytes into one native relative path.
pub(crate) fn path_from_git_bytes(path: &[u8]) -> io::Result<PathBuf> {
    validate_git_path(path)?;
    #[cfg(unix)]
    {
        Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
    }
    #[cfg(windows)]
    {
        String::from_utf8(path.to_vec())
            .map(PathBuf::from)
            .map_err(|_error| invalid_git_metadata("Git path is not valid UTF-8 on Windows"))
    }
}

/// Validate exact Git path bytes before native path adaptation.
fn validate_git_path(path: &[u8]) -> io::Result<()> {
    if path.is_empty() || path.len() > GIT_PATH_BYTE_LIMIT {
        return Err(invalid_git_metadata(
            "Git path has an unsupported byte length",
        ));
    }
    if path.contains(&0) {
        return Err(invalid_git_metadata("Git path contains a NUL byte"));
    }
    if path.starts_with(b"/")
        || path.ends_with(b"/")
        || path
            .split(|byte| *byte == b'/')
            .any(|component| component.is_empty() || component == b"." || component == b"..")
    {
        return Err(invalid_git_metadata(
            "Git path is not a normal repository-relative path",
        ));
    }
    #[cfg(windows)]
    validate_windows_git_path(path)?;
    Ok(())
}

/// Return the identity used to detect native path aliases.
fn native_path_identity(path: &[u8]) -> io::Result<Vec<u8>> {
    validate_git_path(path)?;
    #[cfg(unix)]
    {
        Ok(path.to_vec())
    }
    #[cfg(windows)]
    {
        let text = std::str::from_utf8(path)
            .map_err(|_error| invalid_git_metadata("Git path is not valid UTF-8 on Windows"))?;
        Ok(text.to_lowercase().replace('/', "\\").into_bytes())
    }
}

/// Reject Windows separators, streams, device names, and lossy path aliases.
#[cfg(windows)]
fn validate_windows_git_path(path: &[u8]) -> io::Result<()> {
    let text = std::str::from_utf8(path)
        .map_err(|_error| invalid_git_metadata("Git path is not valid UTF-8 on Windows"))?;
    for component in text.split('/') {
        if component.ends_with([' ', '.'])
            || component.chars().any(|character| {
                character <= '\u{1f}'
                    || matches!(character, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*')
            })
        {
            return Err(invalid_git_metadata(
                "Git path cannot be represented exactly on Windows",
            ));
        }
        let device_stem = component
            .split('.')
            .next()
            .unwrap_or(component)
            .to_ascii_uppercase();
        if matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || device_stem
                .strip_prefix("COM")
                .or_else(|| device_stem.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                })
        {
            return Err(invalid_git_metadata(
                "Git path resolves to a reserved Windows device name",
            ));
        }
    }
    Ok(())
}

/// Split one NUL-delimited Git row at its metadata/path boundary.
fn split_metadata_and_path<'a>(
    row: &'a [u8],
    description: &'static str,
) -> io::Result<(&'a [u8], &'a [u8])> {
    let Some(position) = row.iter().position(|byte| *byte == b'\t') else {
        return Err(invalid_git_data(format!(
            "{description} has no path separator"
        )));
    };
    let (metadata, path_with_separator) = row.split_at(position);
    let path = &path_with_separator[1..];
    if metadata.is_empty() || path.is_empty() {
        return Err(invalid_git_data(format!("{description} is empty")));
    }
    Ok((metadata, path))
}

/// Require exact NUL framing for a Git inventory.
fn require_nul_terminated(rows: &[u8], description: &'static str) -> io::Result<()> {
    if !rows.is_empty() && rows.last() != Some(&0) {
        return Err(invalid_git_data(format!(
            "{description} is not NUL terminated"
        )));
    }
    Ok(())
}

/// Validate one six-digit Git mode.
fn validate_mode(mode: &[u8]) -> io::Result<()> {
    if mode.len() != 6 || !mode.iter().all(u8::is_ascii_digit) {
        return Err(invalid_git_metadata("Git mode is malformed"));
    }
    Ok(())
}

/// Validate one SHA-1 or SHA-256 Git object identifier.
fn validate_oid(oid: &[u8]) -> io::Result<()> {
    if !matches!(oid.len(), 40 | 64) || !oid.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid_git_metadata("Git object identifier is malformed"));
    }
    Ok(())
}

/// Require one consistent object format across HEAD and index inventories.
fn observe_object_format(oid: &[u8], observed: &mut Option<GitObjectFormat>) -> io::Result<()> {
    let format = GitObjectFormat::from_oid(oid)?;
    if observed.is_some_and(|current| current != format) {
        return Err(invalid_git_metadata(
            "Git inventories mix object identifier formats",
        ));
    }
    *observed = Some(format);
    Ok(())
}

/// Enforce a deterministic upper bound before inserting another inventory row.
fn require_entry_capacity(current: usize, description: &'static str) -> io::Result<()> {
    if current >= GIT_INVENTORY_ENTRY_LIMIT {
        return Err(invalid_git_data(format!(
            "{description} exceeds the entry limit"
        )));
    }
    Ok(())
}

/// Encode one deterministic dirty-state record without interpreting Git path bytes.
fn finding(kind: &[u8], path: &[u8], detail: &[u8]) -> Vec<u8> {
    let mut record = Vec::with_capacity(kind.len() + path.len() + detail.len() + 3);
    record.extend_from_slice(kind);
    record.push(b'\t');
    record.extend_from_slice(path);
    record.push(b'\t');
    record.extend_from_slice(detail);
    record.push(0);
    record
}

/// Append one exact byte field with an architecture-independent length prefix.
fn append_binding_field(binding: &mut Vec<u8>, field: &[u8]) {
    binding.extend_from_slice(&(field.len() as u64).to_le_bytes());
    binding.extend_from_slice(field);
}

/// Return whether a metadata entry can redirect traversal through a link-like object.
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Construct one typed invalid-data error for malformed repository metadata.
fn invalid_git_metadata(message: &'static str) -> io::Error {
    invalid_git_data(message)
}

/// Construct one typed invalid-data error with owned diagnostic context.
fn invalid_git_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Return a typed test failure without panicking inside `Result`-returning tests.
    fn verify(condition: bool, message: &'static str) -> io::Result<()> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message))
        }
    }

    /// Equivalent executable copies preserve the first canonical `PATH` identity.
    #[test]
    fn git_executable_resolution_accepts_identical_bytes() -> Result<(), RepositoryGitError> {
        let repository = tempdir()?;
        let root = fs::canonicalize(repository.path())?;
        let executables = tempdir()?;
        let first_directory = executables.path().join("first");
        let second_directory = executables.path().join("second");
        fs::create_dir(&first_directory)?;
        fs::create_dir(&second_directory)?;
        let executable_name = format!("git{}", env::consts::EXE_SUFFIX);
        let first = first_directory.join(&executable_name);
        let second = second_directory.join(&executable_name);
        fs::copy(env::current_exe()?, &first)?;
        fs::copy(env::current_exe()?, &second)?;
        let search_path = env::join_paths([&first_directory, &second_directory])
            .map_err(|source| RepositoryGitError::Policy(source.to_string()))?;

        let (resolved, digest) = resolve_git_executable(&root, &search_path)?;
        let expected_digest = format!(
            "{:x}",
            Sha256::digest(read_bounded_file(
                &first,
                usize::try_from(GIT_EXECUTABLE_FILE_LIMIT)
                    .map_err(|source| RepositoryGitError::Policy(source.to_string()))?,
            )?)
        );
        if resolved != fs::canonicalize(first)? || digest != expected_digest {
            return Err(RepositoryGitError::Policy(
                "identical Git executable bytes did not preserve PATH order".into(),
            ));
        }
        Ok(())
    }

    /// Equal dirty-state bytes cannot hide drift in the bound repository inventories.
    #[test]
    fn worktree_consistency_rejects_different_bindings() -> io::Result<()> {
        let first = SanitizedWorktreeEvidence {
            state: Vec::new(),
            binding: b"first binding".to_vec(),
        };
        let second = SanitizedWorktreeEvidence {
            state: Vec::new(),
            binding: b"second binding".to_vec(),
        };
        verify(
            consistent_worktree_state(&first, second).is_err(),
            "different repository bindings were accepted because state bytes matched",
        )
    }

    /// Matching HEAD, index, flags, and sanitized hashes produce clean state.
    #[test]
    fn sanitized_worktree_comparison_accepts_matching_regular_file() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        fs::write(root.join("file.txt"), b"fixture")?;
        let oid = "a".repeat(40);
        let head = format!("100644 blob {oid}\tfile.txt\0");
        let index = format!("100644 {oid} 0\tfile.txt\0");
        let plan = plan_sanitized_worktree_comparison(
            &root,
            head.as_bytes(),
            index.as_bytes(),
            b"H file.txt\0",
        )?;
        verify(
            plan.hash_input() == b"file.txt\n",
            "regular hash input drifted",
        )?;
        verify(
            plan.finish(format!("{oid}\n").as_bytes(), b"", b"")?
                .into_state()
                .is_empty(),
            "matching regular file was reported dirty",
        )?;
        Ok(())
    }

    /// A tracked symlink placeholder is hashed as literal target bytes.
    #[test]
    fn sanitized_worktree_comparison_accepts_symlink_materialization() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        fs::write(root.join("link.txt"), b"target.txt")?;
        let oid = "a".repeat(40);
        let head = format!("120000 blob {oid}\tlink.txt\0");
        let index = format!("120000 {oid} 0\tlink.txt\0");
        let plan = plan_sanitized_worktree_comparison(
            &root,
            head.as_bytes(),
            index.as_bytes(),
            b"H link.txt\0",
        )?;
        verify(
            plan.hash_input().is_empty(),
            "symlink entered regular hash input",
        )?;
        verify(
            plan.literal_hash_inputs() == [b"target.txt".to_vec()],
            "symlink literal input drifted",
        )?;
        verify(
            plan.finish(b"", format!("{oid}\n").as_bytes(), b"")?
                .into_state()
                .is_empty(),
            "matching symlink was reported dirty",
        )?;
        Ok(())
    }

    /// Sanitized hash drift and untracked paths become deterministic records.
    #[test]
    fn sanitized_worktree_comparison_reports_modified_content() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        fs::write(root.join("file.txt"), b"fixture")?;
        let expected = "a".repeat(40);
        let observed = "b".repeat(40);
        let head = format!("100644 blob {expected}\tfile.txt\0");
        let index = format!("100644 {expected} 0\tfile.txt\0");
        let status = plan_sanitized_worktree_comparison(
            &root,
            head.as_bytes(),
            index.as_bytes(),
            b"H file.txt\0",
        )?
        .finish(format!("{observed}\n").as_bytes(), b"", b"untracked.txt\0")?;
        let status = status.into_state();
        verify(
            status.starts_with(b"modified\tfile.txt\t"),
            "modified file state is absent",
        )?;
        verify(
            status
                .windows(b"untracked\tuntracked.txt\t".len())
                .any(|window| window == b"untracked\tuntracked.txt\t"),
            "untracked file state is absent",
        )?;
        Ok(())
    }

    /// Staged add, change, and delete states are all retained.
    #[test]
    fn sanitized_worktree_comparison_reports_every_staged_change() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        for path in ["added.txt", "changed.txt", "same.txt"] {
            fs::write(root.join(path), b"fixture")?;
        }
        let old = "a".repeat(40);
        let new = "b".repeat(40);
        let head = format!(
            "100644 blob {old}\tchanged.txt\0\
             100644 blob {old}\tdeleted.txt\0\
             100644 blob {old}\tsame.txt\0"
        );
        let index = format!(
            "100644 {new} 0\tadded.txt\0\
             100644 {new} 0\tchanged.txt\0\
             100644 {old} 0\tsame.txt\0"
        );
        let flags = b"H added.txt\0H changed.txt\0H same.txt\0";
        let observed = format!("{new}\n{new}\n{old}\n");
        let state =
            plan_sanitized_worktree_comparison(&root, head.as_bytes(), index.as_bytes(), flags)?
                .finish(observed.as_bytes(), b"", b"")?
                .into_state();
        for kind in [b"staged-add".as_slice(), b"staged-change", b"staged-delete"] {
            verify(
                state.windows(kind.len()).any(|window| window == kind),
                "staged state is absent",
            )?;
        }
        Ok(())
    }

    /// Sparse and assume-unchanged state is explicitly ineligible.
    #[test]
    fn sanitized_worktree_comparison_rejects_nonstandard_index_flags() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        fs::write(root.join("file.txt"), b"fixture")?;
        let oid = "a".repeat(40);
        let head = format!("100644 blob {oid}\tfile.txt\0");
        let index = format!("100644 {oid} 0\tfile.txt\0");
        for flag in *b"Sh" {
            let flags = [
                flag, b' ', b'f', b'i', b'l', b'e', b'.', b't', b'x', b't', 0,
            ];
            let state = plan_sanitized_worktree_comparison(
                &root,
                head.as_bytes(),
                index.as_bytes(),
                &flags,
            )?
            .finish(format!("{oid}\n").as_bytes(), b"", b"")?
            .into_state();
            verify(
                state.starts_with(b"unsupported-index-flag\tfile.txt\t"),
                "nonstandard index flag was accepted",
            )?;
        }
        Ok(())
    }

    /// Intent-to-add, malformed framing, and truncated hashes fail closed.
    #[test]
    fn sanitized_worktree_comparison_rejects_malformed_evidence() -> io::Result<()> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        fs::write(root.join("file.txt"), b"fixture")?;
        let oid = "a".repeat(40);
        let head = format!("100644 blob {oid}\tfile.txt");
        let index = format!("100644 {oid} 0\tfile.txt\0");
        verify(
            plan_sanitized_worktree_comparison(
                &root,
                head.as_bytes(),
                index.as_bytes(),
                b"H file.txt\0",
            )
            .is_err(),
            "unterminated HEAD evidence was accepted",
        )?;

        let head = format!("100644 blob {oid}\tfile.txt\0");
        let plan = plan_sanitized_worktree_comparison(
            &root,
            head.as_bytes(),
            index.as_bytes(),
            b"H file.txt\0",
        )?;
        verify(
            plan.finish(b"", b"", b"").is_err(),
            "truncated hash evidence was accepted",
        )?;
        let zero = "0".repeat(40);
        let intent = format!("100644 {zero} 0\tfile.txt\0");
        verify(
            plan_sanitized_worktree_comparison(
                &root,
                head.as_bytes(),
                intent.as_bytes(),
                b"H file.txt\0",
            )
            .is_err(),
            "intent-to-add evidence was accepted",
        )?;
        Ok(())
    }

    /// Native path adaptation rejects empty, absolute, parent, and NUL-bearing Git paths.
    #[test]
    fn native_git_path_adaptation_rejects_invalid_bytes() -> io::Result<()> {
        for path in [
            b"".as_slice(),
            b"/absolute.rs",
            b"../parent.rs",
            b"nul\0path.rs",
        ] {
            verify(
                path_from_git_bytes(path).is_err(),
                "invalid Git path bytes were adapted to a native path",
            )?;
        }
        verify(
            path_from_git_bytes(b"src/lib.rs")? == Path::new("src/lib.rs"),
            "valid Git path bytes changed during native adaptation",
        )?;
        verify(
            git_path_argument(Path::new("nul\0path.rs")).is_err(),
            "NUL-bearing native Git path was accepted",
        )?;
        verify(
            git_path_argument(Path::new("src/lib.rs"))? == "src/lib.rs",
            "valid native Git path changed during argument adaptation",
        )
    }

    /// Windows path streams, separators, devices, and case aliases fail closed.
    #[cfg(windows)]
    #[test]
    fn sanitized_worktree_comparison_rejects_windows_path_aliases() -> io::Result<()> {
        for path in [b"dir\\file.rs".as_slice(), b"file.rs:stream", b"CON.txt"] {
            verify(
                validate_git_path(path).is_err(),
                "Windows path alias was accepted",
            )?;
        }
        verify(
            native_path_identity(b"Source/File.rs")? == native_path_identity(b"source/file.rs")?,
            "Windows case-folded path identity drifted",
        )
    }
}
