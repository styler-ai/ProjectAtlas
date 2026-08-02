//! Purpose: Enforce `ProjectAtlas` source-code policy beyond built-in Clippy lints.
//! Cargo-adjacent lint gate for `ProjectAtlas`-specific Rust contracts.

use proc_macro2::{TokenStream, TokenTree};
use regex::Regex;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Expr, ExprLit, ExprMethodCall, ImplItemFn, ItemConst, ItemFn, ItemMod, ItemStatic, Lit, LitStr,
    Macro, Meta, Token,
};

/// Subcommand that runs strict string-contract linting.
const COMMAND_STRICT_STRINGS: &str = "strict-strings";
/// Subcommand that scans the changed blobs described by standard pre-push rows.
const COMMAND_PRIVATE_PATH_UPDATES: &str = "private-path-updates";
/// Subcommand that scans newly reachable history relative to one published base.
const COMMAND_PRIVATE_PATH_RANGE: &str = "private-path-range";
/// Successful process exit.
const EXIT_OK: u8 = 0;
/// Lint failure process exit.
const EXIT_FAILURE: u8 = 1;
/// Incorrect command usage process exit.
const EXIT_USAGE: u8 = 2;

/// Stable identifier for the repository-wide private absolute-path rule.
const PRIVATE_PATH_RULE_ID: &str = "private-absolute-path";

/// Private absolute-path shapes forbidden in every Git-visible text file.
const PRIVATE_PATH_RULES: &[(&str, &str)] = &[
    (
        "windows-drive-root",
        r"(?i)(?:^|[^A-Za-z0-9_.+\-])(?P<path>[A-Z]:[\\/])",
    ),
    (
        "verbatim-network-root",
        r"(?i)(?:^|[^\\])(?P<path>\\{2,}\?\\+UNC\\+[A-Za-z0-9][A-Za-z0-9_.-]+\\+[A-Za-z0-9_$.-]+)",
    ),
    (
        "network-root",
        r#"(?:^|^["']|[\s(=\[]|[^\\]["'])(?P<path>(?:\\{2,}|/{2})[A-Za-z0-9][A-Za-z0-9_.-]+[\\/]+[A-Za-z0-9_$.-]+)"#,
    ),
    (
        "file-network-root",
        r#"(?i)file:(?P<path>/{2}[^\s\\/:"'`<>]+/)"#,
    ),
    (
        "file-user-home-root",
        r#"(?i)file:(?P<path>/{2,3}(?:home|Users)/[^/\s"'`<>]+(?:/|$))"#,
    ),
    (
        "user-home-root",
        r#"(?:^|[\s"'`(=:\[])(?P<path>/(?:home|Users)/[^/\s"'`<>]+(?:/|$))"#,
    ),
    (
        "wsl-drive-root",
        r#"(?:^|[\s"'`(=:\[])(?P<path>/mnt/[A-Za-z](?:/|$))"#,
    ),
];

/// MCP project-selection response strings owned by the MCP adapter contract.
const MCP_PROJECT_SCHEMA_LITERALS: &[&str] =
    &["project", "root", "db", "config", "status", "active"];

/// Cross-module domain contract strings that must be centralized before reuse.
const DOMAIN_CONTRACT_LITERALS: &[&str] = &[
    "missing-purpose",
    "suggested-purpose-review",
    "stale-purpose",
    "purpose-agent-review-required",
    "duplicate-purpose",
    "repeated-temporary-folder",
    "repository-intelligence",
    "approved",
    "agent",
    "pass",
    "fail",
    "info",
    "warning",
    "error",
];

/// Reused e2e fixture path segments that should stay centralized.
const E2E_FIXTURE_PATH_LITERALS: &[&str] = &[
    "repo",
    "src",
    ".projectatlas",
    ".codex",
    "fake-codex.log",
    "fake-path",
    "isolated-home",
];

/// Existing repeated e2e path joins reviewed as ordinary fixture structure.
const E2E_ALLOWED_REPEATED_PATH_JOIN_LITERALS: &[&str] = &[
    ".cargo",
    ".github",
    ".gitignore",
    ".purpose",
    "AppData",
    "Cargo.toml",
    "Local",
    "README.md",
    "Roaming",
    "a.rs",
    "api",
    "app",
    "assets",
    "atlas_core",
    "b.rs",
    "bin",
    "build.gradle.kts",
    "ci.yml",
    "codex.cmd",
    "config.toml",
    "crates",
    "customers",
    "data",
    "detail.rs",
    "docs",
    "empty.rs",
    "engine.rs",
    "feature",
    "fixtures",
    "function_alias",
    "generated",
    "install-runtime.ps1",
    "install-runtime.sh",
    "kept-state.txt",
    "languages",
    "lib.rs",
    "live.rs",
    "local-cache",
    "logo.svg",
    "main.rs",
    "metadata.egg-info",
    "module_alias",
    "named_alias",
    "nested",
    "no_alias",
    "node_modules",
    "noise.rs",
    "outside",
    "package",
    "package_entry",
    "pkg",
    "plugin.json",
    "plugins",
    "projectatlas",
    "projectatlas-nonsource-files.toon",
    "projectatlas.claude.mcp.json",
    "projectatlas.cmd",
    "projectatlas.db",
    "projectatlas.exe",
    "projectatlas.mcp.json",
    "projectatlas.opencode.json",
    "projectatlas.toml",
    "projectatlas.toon",
    "public",
    "py",
    "python",
    "release.yml",
    "rogue",
    "runtimes",
    "rust",
    "scripts",
    "service.rs",
    "service.ts",
    "settings",
    "styles",
    "target",
    "tmp",
    "ts",
    "unrelated",
    "workflows",
    "x86_64-pc-windows-msvc",
];

/// Reviewed diagnostic format templates that must stay inline for Rust format macros.
const MCP_ALLOWED_INLINE_LITERALS: &[&str] = &[
    "absolute path '{}' has no existing ancestor",
    "ProjectAtlas index '{}' is missing for selected project root '{}'; {MISSING_INDEX_GUIDANCE}",
    "project path '{}' is not a directory",
    "MCP path '{original}' resolves to '{resolved_display}', not the selected project root '{project_root_display}'; {SELECTED_ROOT_ASSERTION_GUIDANCE}",
    "MCP path '{original}' resolves to '{resolved_display}', outside the selected project root '{project_root_display}'; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}",
    "path {node_key:?} is not indexed in the MCP-bound project",
    "{message}; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}",
    "invalid health severity '{trimmed}'; expected {expected}",
    "unsupported token trend window {window:?}; {TOKEN_TREND_WINDOW_ERROR_SUFFIX}",
    "CARGO_PKG_VERSION",
];

/// Strict string literal rules enabled for this repository.
const STRICT_STRING_RULES: &[StringLiteralRule] = &[
    StringLiteralRule {
        id: "mcp-production-inline-strings",
        description: "MCP production strings must be represented by typed structs, enums, constants, or a reviewed inline-format allowlist.",
        paths: &["crates/projectatlas-cli/src/mcp.rs"],
        ban_unlisted: true,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: MCP_ALLOWED_INLINE_LITERALS,
    },
    StringLiteralRule {
        id: "domain-contract-inline-strings",
        description: "Domain contract strings must live in the owning enum, typed schema, or constants instead of being repeated inline.",
        paths: &[
            "crates/projectatlas-core/src/lib.rs",
            "crates/projectatlas-core/src/health.rs",
            "crates/projectatlas-core/src/toon.rs",
            "crates/projectatlas-db/src/lib.rs",
            "crates/projectatlas-cli/src/main.rs",
            "crates/projectatlas-cli/src/runtime.rs",
        ],
        ban_unlisted: false,
        literals: DOMAIN_CONTRACT_LITERALS,
        allowed_literals: &[],
    },
    StringLiteralRule {
        id: "e2e-fixture-path-inline-strings",
        description: "Repeated e2e fixture path segments must live in local constants instead of being repeated inline.",
        paths: &["crates/projectatlas-cli/tests/e2e.rs"],
        ban_unlisted: false,
        literals: E2E_FIXTURE_PATH_LITERALS,
        allowed_literals: &[],
    },
];

/// Repeated path-join rules enabled for this repository.
const REPEATED_PATH_JOIN_RULES: &[PathJoinLiteralRule] = &[PathJoinLiteralRule {
    id: "repeated-path-join-inline-strings",
    description: "Repeated path join string literals must be centralized as constants or reviewed into the fixture allowlist.",
    paths: &["crates/projectatlas-cli/tests/e2e.rs"],
    allowed_repeated_literals: E2E_ALLOWED_REPEATED_PATH_JOIN_LITERALS,
}];

/// Run the cargo-adjacent `ProjectAtlas` lint command.
fn main() -> ExitCode {
    match run(env::args_os().skip(1), &current_dir()) {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(LintError::Violations {
            string_literals,
            private_paths,
        }) => {
            let mut stderr = io::stderr().lock();
            if write_violations(&mut stderr, &string_literals, &private_paths).is_err() {
                return ExitCode::from(EXIT_FAILURE);
            }
            ExitCode::from(EXIT_FAILURE)
        }
        Err(error) => {
            let mut stderr = io::stderr().lock();
            if writeln!(stderr, "{error}").is_err() {
                return ExitCode::from(EXIT_FAILURE);
            }
            if matches!(error, LintError::Usage(_)) {
                ExitCode::from(EXIT_USAGE)
            } else {
                ExitCode::from(EXIT_FAILURE)
            }
        }
    }
}

/// Return the current process directory or the relative default when unavailable.
fn current_dir() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Dispatch one lint command from normalized command-line arguments.
fn run(args: impl IntoIterator<Item = OsString>, current_dir: &Path) -> Result<(), LintError> {
    let args = normalized_args(args);
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return help();
    }
    if args.is_empty() {
        return help();
    }
    match args.as_slice() {
        [command] if command == COMMAND_STRICT_STRINGS => run_strict_strings(current_dir),
        [command, remote_name] if command == COMMAND_PRIVATE_PATH_UPDATES => {
            let stdin = io::stdin();
            run_private_path_updates(current_dir, remote_name, stdin.lock())
        }
        [command, base, head] if command == COMMAND_PRIVATE_PATH_RANGE => {
            run_private_path_range(current_dir, base, head)
        }
        [other, ..]
            if other != COMMAND_STRICT_STRINGS
                && other != COMMAND_PRIVATE_PATH_UPDATES
                && other != COMMAND_PRIVATE_PATH_RANGE =>
        {
            Err(LintError::Usage(format!(
                "unknown projectatlas lint command {other:?}"
            )))
        }
        _ => Err(LintError::Usage("unexpected lint arguments".to_string())),
    }
}

/// Accept the full SHA-1 and SHA-256 object identifiers supplied by Git hooks.
fn is_full_git_object_id(revision: &str) -> bool {
    matches!(revision.len(), 40 | 64) && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Normalize direct binary and Cargo external-subcommand invocation shapes.
fn normalized_args(args: impl IntoIterator<Item = OsString>) -> Vec<String> {
    let mut args = args
        .into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "projectatlas-lints") {
        args.remove(0);
    }
    args
}

/// Print command help.
fn help() -> Result<(), LintError> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Usage: cargo projectatlas-lints <command>\n\nCommands:\n  {COMMAND_STRICT_STRINGS}  Enforce ProjectAtlas string contracts and repository-wide private-path policy.\n  {COMMAND_PRIVATE_PATH_UPDATES} <remote>  Scan changed blobs from standard pre-push rows on stdin.\n  {COMMAND_PRIVATE_PATH_RANGE} <base> <head>  Scan newly reachable history while allowing unchanged private-path source occurrences already present at the published base."
    )
    .map_err(LintError::Io)
}

/// Run all strict string-contract rules from a workspace root.
fn run_strict_strings(root: &Path) -> Result<(), LintError> {
    let mut string_violations = Vec::new();
    for rule in STRICT_STRING_RULES {
        for relative_path in rule.paths {
            let path = root.join(relative_path);
            let source = fs::read_to_string(&path).map_err(|source| LintError::ReadFile {
                path: (*relative_path).to_string(),
                source,
            })?;
            string_violations.extend(lint_source(relative_path, rule, &source)?);
        }
    }
    for rule in REPEATED_PATH_JOIN_RULES {
        for relative_path in rule.paths {
            let path = root.join(relative_path);
            let source = fs::read_to_string(&path).map_err(|source| LintError::ReadFile {
                path: (*relative_path).to_string(),
                source,
            })?;
            string_violations.extend(lint_repeated_path_join_literals(
                relative_path,
                rule,
                &source,
            )?);
        }
    }
    let private_path_violations = lint_repository_private_paths(root)?;
    finish_lint(string_violations, private_path_violations)
}

/// Scan one pre-push update batch without repeating unchanged tree blobs.
fn run_private_path_updates(
    root: &Path,
    remote_name: &str,
    reader: impl BufRead,
) -> Result<(), LintError> {
    let revisions = outgoing_revisions(root, remote_name, reader)?;
    if revisions.is_empty() {
        return finish_lint(Vec::new(), Vec::new());
    }
    let private_path_violations = lint_git_revisions_private_paths(root, &revisions)?;
    finish_lint(Vec::new(), private_path_violations)
}

/// Scan newly reachable history without re-reporting unchanged private-path source at the base.
fn run_private_path_range(root: &Path, base: &str, head: &str) -> Result<(), LintError> {
    if !is_full_git_object_id(base)
        || !is_full_git_object_id(head)
        || base.len() != head.len()
        || head.bytes().all(|byte| byte == b'0')
    {
        return Err(LintError::InvalidPrePushUpdate);
    }
    let updates = format!("refs/heads/ci {head} refs/heads/ci {base}\n");
    let revisions = outgoing_revisions(root, "", io::Cursor::new(updates))?;
    let baseline = if base.bytes().all(|byte| byte == b'0') {
        Vec::new()
    } else {
        lint_git_tree_private_paths(root, base)?
    };
    let outgoing = lint_git_revisions_private_paths(root, &revisions)?;
    finish_lint(
        Vec::new(),
        private_paths_not_in_baseline(outgoing, &baseline),
    )
}

/// Resolve every newly reachable revision from standard pre-push update rows.
fn outgoing_revisions(
    root: &Path,
    remote_name: &str,
    reader: impl BufRead,
) -> Result<Vec<String>, LintError> {
    let mut revisions = BTreeSet::new();
    for line in reader.lines() {
        let line = line.map_err(LintError::ReadGitRevision)?;
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let [_, local_object, _, remote_object] = fields.as_slice() else {
            return Err(LintError::InvalidPrePushUpdate);
        };
        if !is_full_git_object_id(local_object) || !is_full_git_object_id(remote_object) {
            return Err(LintError::InvalidPrePushUpdate);
        }
        if local_object.bytes().all(|byte| byte == b'0') {
            continue;
        }
        let mut command = Command::new("git");
        command.arg("rev-list");
        if remote_object.bytes().any(|byte| byte != b'0') {
            command.arg(format!("{remote_object}..{local_object}"));
        } else {
            command.arg(local_object);
            if !remote_name.is_empty() {
                command.args(["--not", &format!("--remotes={remote_name}")]);
            }
        }
        let output = command
            .current_dir(root)
            .stderr(Stdio::null())
            .output()
            .map_err(LintError::ReadGitRevision)?;
        if !output.status.success() {
            return Err(LintError::GitRevisionCommandFailed {
                operation: "outgoing revision listing",
                code: output.status.code(),
            });
        }
        let listed = String::from_utf8(output.stdout).map_err(LintError::NonUtf8GitPath)?;
        let mut found = false;
        for revision in listed.lines() {
            if !is_full_git_object_id(revision) {
                return Err(LintError::InvalidPrePushUpdate);
            }
            found = true;
            revisions.insert(revision.to_string());
        }
        if !found {
            revisions.insert((*local_object).to_string());
        }
    }
    Ok(revisions.into_iter().collect())
}

/// Return success output or the collected source-policy failure.
fn finish_lint(
    string_violations: Vec<StringLiteralViolation>,
    private_path_violations: Vec<PrivatePathViolation>,
) -> Result<(), LintError> {
    if string_violations.is_empty() && private_path_violations.is_empty() {
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "projectatlas-lints: repository source policies passed"
        )
        .map_err(LintError::Io)
    } else {
        Err(LintError::Violations {
            string_literals: string_violations,
            private_paths: private_path_violations,
        })
    }
}

/// Compile the closed private-path rule set.
fn private_path_rules() -> Result<Vec<PrivatePathRule>, LintError> {
    PRIVATE_PATH_RULES
        .iter()
        .map(|(kind, pattern)| {
            Regex::new(pattern)
                .map(|regex| PrivatePathRule { kind, regex })
                .map_err(LintError::InvalidPrivatePathRule)
        })
        .collect()
}

/// Scan every Git-visible supported text file without emitting matched private text.
fn lint_repository_private_paths(root: &Path) -> Result<Vec<PrivatePathViolation>, LintError> {
    let rules = private_path_rules()?;
    let mut violations = Vec::new();
    for relative_path in git_visible_paths(root)? {
        let path = root.join(&relative_path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(LintError::ReadFile {
                    path: relative_path,
                    source,
                });
            }
        };
        if metadata.is_dir() {
            continue;
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|source| LintError::ReadFile {
                path: relative_path.clone(),
                source,
            })?;
            let target = target
                .to_str()
                .ok_or_else(|| LintError::NonUtf8Text(relative_path.clone()))?;
            violations.extend(lint_private_paths(&relative_path, target, &rules));
            continue;
        }
        let bytes = fs::read(&path).map_err(|source| LintError::ReadFile {
            path: relative_path.clone(),
            source,
        })?;
        if let Some(source) = decode_repository_text(&relative_path, &bytes)? {
            violations.extend(lint_private_paths(&relative_path, &source, &rules));
        }
    }
    Ok(violations)
}

/// Scan each blob changed by an outgoing revision once, independent of export policy.
fn lint_git_revisions_private_paths(
    root: &Path,
    revisions: &[String],
) -> Result<Vec<PrivatePathViolation>, LintError> {
    let mut diff = Command::new("git")
        .args([
            "diff-tree",
            "--stdin",
            "--root",
            "-r",
            "-m",
            "--no-commit-id",
            "--no-renames",
            "--no-abbrev",
            "--raw",
            "-z",
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(LintError::ReadGitRevision)?;
    {
        let stdin = diff.stdin.take().ok_or(LintError::MissingGitObjectStream)?;
        let mut requests = BufWriter::new(stdin);
        for revision in revisions {
            writeln!(requests, "{revision}").map_err(LintError::ReadGitRevision)?;
        }
    }
    let changed = diff
        .wait_with_output()
        .map_err(LintError::ReadGitRevision)?;
    if !changed.status.success() {
        return Err(LintError::GitRevisionCommandFailed {
            operation: "changed blob listing",
            code: changed.status.code(),
        });
    }
    let mut blobs = BTreeSet::new();
    let mut records = changed.stdout.split(|byte| *byte == 0);
    loop {
        let Some(metadata) = records.next() else {
            break;
        };
        if metadata.is_empty() {
            break;
        }
        let path = records.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let Ok(metadata) = std::str::from_utf8(metadata) else {
            return Err(LintError::InvalidGitTreeEntry);
        };
        let mut fields = metadata.split_whitespace();
        let old_mode = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let new_mode = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let old_object = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let new_object = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let status = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        if !old_mode.starts_with(':')
            || fields.next().is_some()
            || old_object.len() != new_object.len()
            || !is_full_git_object_id(new_object)
        {
            return Err(LintError::InvalidGitTreeEntry);
        }
        if status == "D" || new_mode == "160000" || new_object.bytes().all(|byte| byte == b'0') {
            continue;
        }
        let path = String::from_utf8(path.to_vec()).map_err(LintError::NonUtf8GitPath)?;
        blobs.insert((new_object.to_string(), path));
    }
    lint_git_blobs(root, &blobs)
}

/// Scan every text blob in one exact Git tree.
fn lint_git_tree_private_paths(
    root: &Path,
    revision: &str,
) -> Result<Vec<PrivatePathViolation>, LintError> {
    let output = Command::new("git")
        .args(["ls-tree", "-r", "-z", "--full-tree", revision])
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .map_err(LintError::ReadGitRevision)?;
    if !output.status.success() {
        return Err(LintError::GitRevisionCommandFailed {
            operation: "baseline tree listing",
            code: output.status.code(),
        });
    }
    let mut blobs = BTreeSet::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(LintError::InvalidGitTreeEntry)?;
        let metadata =
            String::from_utf8(record[..separator].to_vec()).map_err(LintError::NonUtf8GitPath)?;
        let path = String::from_utf8(record[separator + 1..].to_vec())
            .map_err(LintError::NonUtf8GitPath)?;
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let kind = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        let object = fields.next().ok_or(LintError::InvalidGitTreeEntry)?;
        if fields.next().is_some() || !is_full_git_object_id(object) {
            return Err(LintError::InvalidGitTreeEntry);
        }
        if kind == "blob" {
            blobs.insert((object.to_string(), path));
        } else if kind != "commit" || mode != "160000" {
            return Err(LintError::InvalidGitTreeEntry);
        }
    }
    lint_git_blobs(root, &blobs)
}

/// Keep only private path occurrences that exceed the unchanged published-base multiset.
fn private_paths_not_in_baseline(
    outgoing: Vec<PrivatePathViolation>,
    baseline: &[PrivatePathViolation],
) -> Vec<PrivatePathViolation> {
    let mut baseline_counts = BTreeMap::new();
    for violation in baseline {
        *baseline_counts
            .entry((
                violation.path.clone(),
                violation.kind,
                violation.source_identity.clone(),
            ))
            .or_insert(0usize) += 1;
    }
    let mut admitted_by_blob = BTreeMap::new();
    outgoing
        .into_iter()
        .filter(|violation| {
            let Some(git_object) = violation.git_object.as_ref() else {
                return true;
            };
            let baseline_count = baseline_counts
                .get(&(
                    violation.path.clone(),
                    violation.kind,
                    violation.source_identity.clone(),
                ))
                .copied()
                .unwrap_or_default();
            let admitted = admitted_by_blob
                .entry((
                    git_object.clone(),
                    violation.path.clone(),
                    violation.kind,
                    violation.source_identity.clone(),
                ))
                .or_insert(0usize);
            *admitted += 1;
            *admitted > baseline_count
        })
        .collect()
}

/// Read a deduplicated set of Git blobs through one validated batch process.
fn lint_git_blobs(
    root: &Path,
    blobs: &BTreeSet<(String, String)>,
) -> Result<Vec<PrivatePathViolation>, LintError> {
    let rules = private_path_rules()?;
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(LintError::ReadGitRevision)?;
    let stdin = child
        .stdin
        .take()
        .ok_or(LintError::MissingGitObjectStream)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(LintError::MissingGitObjectStream)?;
    let mut requests = BufWriter::new(stdin);
    let mut responses = BufReader::new(stdout);
    let mut violations = Vec::new();
    for (object, relative_path) in blobs {
        writeln!(requests, "{object}").map_err(LintError::ReadGitRevision)?;
        requests.flush().map_err(LintError::ReadGitRevision)?;
        let mut header = String::new();
        if responses
            .read_line(&mut header)
            .map_err(LintError::ReadGitRevision)?
            == 0
        {
            return Err(LintError::InvalidGitObjectResponse);
        }
        let mut response_fields = header.split_whitespace();
        let returned_object = response_fields
            .next()
            .ok_or(LintError::InvalidGitObjectResponse)?;
        let returned_kind = response_fields
            .next()
            .ok_or(LintError::InvalidGitObjectResponse)?;
        let size = response_fields
            .next()
            .ok_or(LintError::InvalidGitObjectResponse)?
            .parse::<usize>()
            .map_err(LintError::InvalidGitObjectSize)?;
        if returned_object != object || returned_kind != "blob" || response_fields.next().is_some()
        {
            return Err(LintError::InvalidGitObjectResponse);
        }
        let mut bytes = vec![0; size];
        responses
            .read_exact(&mut bytes)
            .map_err(LintError::ReadGitRevision)?;
        let mut terminator = [0];
        responses
            .read_exact(&mut terminator)
            .map_err(LintError::ReadGitRevision)?;
        if terminator[0] != b'\n' {
            return Err(LintError::InvalidGitObjectResponse);
        }
        if let Some(source) = decode_repository_text(relative_path, &bytes)? {
            let mut blob_violations = lint_private_paths(relative_path, &source, &rules);
            for violation in &mut blob_violations {
                violation.git_object = Some(object.clone());
            }
            violations.extend(blob_violations);
        }
    }
    drop(requests);
    drop(responses);
    let status = child.wait().map_err(LintError::ReadGitRevision)?;
    if !status.success() {
        return Err(LintError::GitRevisionCommandFailed {
            operation: "blob reading",
            code: status.code(),
        });
    }
    Ok(violations)
}

/// Return tracked and non-ignored untracked paths relative to the workspace root.
fn git_visible_paths(root: &Path) -> Result<Vec<String>, LintError> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .map_err(LintError::ListGitFiles)?;
    if !output.status.success() {
        return Err(LintError::GitFileListFailed(output.status.code()));
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(LintError::NonUtf8GitPath))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Decode Git-visible text while leaving binary files outside the source-policy surface.
fn decode_repository_text<'a>(
    relative_path: &str,
    bytes: &'a [u8],
) -> Result<Option<Cow<'a, str>>, LintError> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if let Some(encoded) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(relative_path, encoded, u16::from_le_bytes).map(Some);
    }
    if let Some(encoded) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(relative_path, encoded, u16::from_be_bytes).map(Some);
    }
    match std::str::from_utf8(bytes) {
        Ok(source) => Ok(Some(Cow::Borrowed(source))),
        Err(_) if bytes.contains(&0) => Ok(None),
        Err(_) => Err(LintError::NonUtf8Text(relative_path.to_string())),
    }
}

/// Decode one BOM-selected UTF-16 byte stream without guessing a host encoding.
fn decode_utf16(
    relative_path: &str,
    encoded: &[u8],
    decode_unit: fn([u8; 2]) -> u16,
) -> Result<Cow<'static, str>, LintError> {
    if !encoded.len().is_multiple_of(2) {
        return Err(LintError::NonUtf8Text(relative_path.to_string()));
    }
    let units = encoded
        .chunks_exact(2)
        .map(|chunk| decode_unit([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units)
        .map(Cow::Owned)
        .map_err(|source| LintError::InvalidUtf16Text {
            path: relative_path.to_string(),
            source,
        })
}

/// Find private absolute-path shapes in one repository-relative text source.
fn lint_private_paths(
    relative_path: &str,
    source: &str,
    rules: &[PrivatePathRule],
) -> Vec<PrivatePathViolation> {
    let mut violations = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let mut source_identity = None;
        for rule in rules {
            for captures in rule.regex.captures_iter(line) {
                let Some(path_match) = captures.name("path") else {
                    continue;
                };
                let source_identity = source_identity
                    .get_or_insert_with(|| PrivatePathIdentity(Rc::from(line)))
                    .clone();
                violations.push(PrivatePathViolation {
                    path: relative_path.to_string(),
                    line: line_index + 1,
                    column: line[..path_match.start()].chars().count() + 1,
                    kind: rule.kind,
                    source_identity,
                    git_object: None,
                });
            }
        }
    }
    violations
}

/// Parse one Rust source file and return strict string-contract violations.
fn lint_source(
    relative_path: &str,
    rule: &'static StringLiteralRule,
    source: &str,
) -> Result<Vec<StringLiteralViolation>, LintError> {
    let file = syn::parse_file(source).map_err(|source| LintError::Parse {
        path: relative_path.to_string(),
        source,
    })?;
    let mut visitor = StringLiteralVisitor {
        relative_path,
        rule,
        centralized_depth: 0,
        violations: Vec::new(),
    };
    visitor.visit_file(&file);
    Ok(visitor.violations)
}

/// Parse one Rust source file and return repeated path-join literal violations.
fn lint_repeated_path_join_literals(
    relative_path: &str,
    rule: &'static PathJoinLiteralRule,
    source: &str,
) -> Result<Vec<StringLiteralViolation>, LintError> {
    let file = syn::parse_file(source).map_err(|source| LintError::Parse {
        path: relative_path.to_string(),
        source,
    })?;
    let mut visitor = PathJoinLiteralVisitor {
        occurrences: Vec::new(),
    };
    visitor.visit_file(&file);

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for occurrence in &visitor.occurrences {
        *counts.entry(occurrence.literal.clone()).or_default() += 1;
    }

    let violations = visitor
        .occurrences
        .into_iter()
        .filter(|occurrence| {
            counts
                .get(occurrence.literal.as_str())
                .is_some_and(|count| *count > 1)
                && !rule
                    .allowed_repeated_literals
                    .iter()
                    .any(|allowed| *allowed == occurrence.literal)
        })
        .map(|occurrence| StringLiteralViolation {
            path: relative_path.to_string(),
            line: occurrence.line,
            column: occurrence.column,
            rule_id: rule.id,
            description: rule.description,
        })
        .collect();
    Ok(violations)
}

/// Path-scoped rule for protected exact string literals.
#[derive(Clone, Copy, Debug)]
struct StringLiteralRule {
    /// Stable rule identifier printed in diagnostics.
    id: &'static str,
    /// Human-readable rule description printed in diagnostics.
    description: &'static str,
    /// Repository-relative Rust source files protected by this rule.
    paths: &'static [&'static str],
    /// Exact string literal values protected by this rule.
    literals: &'static [&'static str],
    /// Whether every unallowed production string is prohibited.
    ban_unlisted: bool,
    /// Exact string literal values explicitly allowed inline.
    allowed_literals: &'static [&'static str],
}

/// Path-scoped rule for repeated `.join("...")` string literals.
#[derive(Clone, Copy, Debug)]
struct PathJoinLiteralRule {
    /// Stable rule identifier printed in diagnostics.
    id: &'static str,
    /// Human-readable rule description printed in diagnostics.
    description: &'static str,
    /// Repository-relative Rust source files protected by this rule.
    paths: &'static [&'static str],
    /// Repeated path-join literals reviewed as acceptable inline fixtures.
    allowed_repeated_literals: &'static [&'static str],
}

/// Compiled repository-wide private absolute-path rule.
#[derive(Debug)]
struct PrivatePathRule {
    /// Stable path-shape classification used in redacted diagnostics.
    kind: &'static str,
    /// Compiled structural matcher.
    regex: Regex,
}

/// One redacted private absolute-path violation.
struct PrivatePathViolation {
    /// Repository-relative source path.
    path: String,
    /// One-based source line.
    line: usize,
    /// One-based source column.
    column: usize,
    /// Stable path-shape classification; never contains matched source text.
    kind: &'static str,
    /// Non-formattable identity for the complete source line containing the private path.
    source_identity: PrivatePathIdentity,
    /// Git blob identity used to apply the baseline allowance independently per historical file.
    git_object: Option<String>,
}

/// Exact identity that cannot expose the private source text through formatting.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct PrivatePathIdentity(Rc<str>);

impl fmt::Debug for PrivatePathViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivatePathViolation")
            .field("path", &self.path)
            .field("line", &self.line)
            .field("column", &self.column)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Display for PrivatePathViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?}:{}:{}: {PRIVATE_PATH_RULE_ID} ({}): machine-specific absolute path is forbidden; derive paths at runtime",
            self.path, self.line, self.column, self.kind
        )
    }
}

/// One source-aware string-contract violation.
#[derive(Debug)]
struct StringLiteralViolation {
    /// Repository-relative source path.
    path: String,
    /// One-based source line.
    line: usize,
    /// One-based source column.
    column: usize,
    /// Stable rule identifier.
    rule_id: &'static str,
    /// Human-readable rule description.
    description: &'static str,
}

impl Display for StringLiteralViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?}:{}:{}: {}: inline string literal violates repository contract; {}",
            self.path, self.line, self.column, self.rule_id, self.description
        )
    }
}

/// Syntax visitor that records protected inline string literals.
struct StringLiteralVisitor<'a> {
    /// Repository-relative source path.
    relative_path: &'a str,
    /// Rule used by this visitor.
    rule: &'static StringLiteralRule,
    /// Nesting depth for allowed centralization declarations.
    centralized_depth: usize,
    /// Collected violations.
    violations: Vec<StringLiteralViolation>,
}

/// One `.join("literal")` occurrence found by the path-join rule.
#[derive(Debug)]
struct PathJoinLiteralOccurrence {
    /// Exact path segment literal.
    literal: String,
    /// One-based source line.
    line: usize,
    /// One-based source column.
    column: usize,
}

/// Syntax visitor that records direct `.join("...")` calls.
struct PathJoinLiteralVisitor {
    /// Collected path-join literal occurrences.
    occurrences: Vec<PathJoinLiteralOccurrence>,
}

impl PathJoinLiteralVisitor {
    /// Record a direct path join literal.
    fn record_join_literal(&mut self, literal: &LitStr) {
        if !is_path_join_literal(&literal.value()) {
            return;
        }
        let location = literal.span().start();
        self.occurrences.push(PathJoinLiteralOccurrence {
            literal: literal.value(),
            line: location.line,
            column: location.column + 1,
        });
    }
}

/// Return whether a method-call `join` literal looks like a path segment, not a collection separator.
fn is_path_join_literal(literal: &str) -> bool {
    !matches!(
        literal,
        "" | " " | "\n" | "\r\n" | "\t" | "," | ", " | "; " | ": "
    )
}

impl<'ast> Visit<'ast> for PathJoinLiteralVisitor {
    /// Record only direct `.join("...")` calls; constants are the desired form.
    fn visit_expr_method_call(&mut self, expr: &'ast ExprMethodCall) {
        if expr.method == "join"
            && let Some(Expr::Lit(ExprLit {
                lit: Lit::Str(literal),
                ..
            })) = expr.args.first()
        {
            self.record_join_literal(literal);
        }
        visit::visit_expr_method_call(self, expr);
    }
}

impl StringLiteralVisitor<'_> {
    /// Record one string literal unless it is allowed by the current context.
    fn record_literal(&mut self, literal: &LitStr) {
        if self.centralized_depth > 0 {
            return;
        }
        let value = literal.value();
        if self
            .rule
            .allowed_literals
            .iter()
            .any(|allowed| *allowed == value)
        {
            return;
        }
        let protected = self
            .rule
            .literals
            .iter()
            .any(|protected| *protected == value);
        if !self.rule.ban_unlisted && !protected {
            return;
        }
        let location = literal.span().start();
        self.violations.push(StringLiteralViolation {
            path: self.relative_path.to_string(),
            line: location.line,
            column: location.column + 1,
            rule_id: self.rule.id,
            description: self.rule.description,
        });
    }

    /// Visit a constant/static item as an allowed centralization declaration.
    fn visit_centralized_item(&mut self, visit: impl FnOnce(&mut Self)) {
        self.centralized_depth += 1;
        visit(self);
        self.centralized_depth -= 1;
    }

    /// Return whether a function is an explicit string centralization method.
    fn is_centralized_function_name(name: &syn::Ident) -> bool {
        name == "as_str"
    }

    /// Scan macro token trees because `syn` does not parse macro bodies as expressions.
    fn scan_macro_tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => self.scan_macro_tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Ok(literal) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        self.record_literal(&literal);
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }

    /// Return whether an attribute marks test-only code.
    fn is_test_attribute(attribute: &syn::Attribute) -> bool {
        match &attribute.meta {
            Meta::Path(path) => path.is_ident("test"),
            Meta::List(list) => {
                if !list.path.is_ident("cfg") {
                    return false;
                }
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                let Ok(cfg_items) = parser.parse2(list.tokens.clone()) else {
                    return false;
                };
                cfg_items
                    .iter()
                    .any(StringLiteralVisitor::cfg_meta_is_test_only)
            }
            Meta::NameValue(_) => false,
        }
    }

    /// Return whether one cfg expression can only be active for tests.
    fn cfg_meta_is_test_only(meta: &Meta) -> bool {
        match meta {
            Meta::Path(path) => path.is_ident("test"),
            Meta::List(list) if list.path.is_ident("all") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                parser.parse2(list.tokens.clone()).is_ok_and(|items| {
                    items
                        .iter()
                        .any(StringLiteralVisitor::cfg_meta_is_test_only)
                })
            }
            Meta::List(list) if list.path.is_ident("any") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                parser.parse2(list.tokens.clone()).is_ok_and(|items| {
                    !items.is_empty()
                        && items
                            .iter()
                            .all(StringLiteralVisitor::cfg_meta_is_test_only)
                })
            }
            Meta::List(_) | Meta::NameValue(_) => false,
        }
    }
}

impl<'ast> Visit<'ast> for StringLiteralVisitor<'_> {
    /// Ignore attributes so serialization metadata on local enums stays centralized.
    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    /// Allow constants as local centralization points.
    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        self.visit_centralized_item(|visitor| visit::visit_item_const(visitor, item));
    }

    /// Allow dedicated string-conversion functions as centralization points.
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if StringLiteralVisitor::is_centralized_function_name(&item.sig.ident) {
            self.visit_centralized_item(|visitor| visit::visit_item_fn(visitor, item));
            return;
        }
        visit::visit_item_fn(self, item);
    }

    /// Allow dedicated string-conversion methods as centralization points.
    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if StringLiteralVisitor::is_centralized_function_name(&item.sig.ident) {
            self.visit_centralized_item(|visitor| visit::visit_impl_item_fn(visitor, item));
            return;
        }
        visit::visit_impl_item_fn(self, item);
    }

    /// Allow statics as local centralization points.
    fn visit_item_static(&mut self, item: &'ast ItemStatic) {
        self.visit_centralized_item(|visitor| visit::visit_item_static(visitor, item));
    }

    /// Ignore test-only modules so fixtures and assertion text do not create lint noise.
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if item
            .attrs
            .iter()
            .any(StringLiteralVisitor::is_test_attribute)
        {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    /// Record ordinary Rust string literals.
    fn visit_expr_lit(&mut self, expr: &'ast ExprLit) {
        if let Lit::Str(literal) = &expr.lit {
            self.record_literal(literal);
        }
        visit::visit_expr_lit(self, expr);
    }

    /// Record macro-contained string literals such as `json!` schema keys.
    fn visit_macro(&mut self, macro_call: &'ast Macro) {
        if self.centralized_depth == 0 {
            self.scan_macro_tokens(macro_call.tokens.clone());
        }
        visit::visit_macro(self, macro_call);
    }
}

/// Errors returned by the cargo-adjacent lint command.
#[derive(Debug)]
enum LintError {
    /// Invalid command-line usage.
    Usage(String),
    /// Generic stream IO failure.
    Io(io::Error),
    /// File read failure.
    ReadFile {
        /// Repository-relative file path that could not be read.
        path: String,
        /// Source IO error.
        source: io::Error,
    },
    /// Rust parser failure.
    Parse {
        /// Repository-relative file path that could not be parsed.
        path: String,
        /// Source parser error.
        source: syn::Error,
    },
    /// Failed to start Git-backed source discovery.
    ListGitFiles(io::Error),
    /// Git-backed source discovery returned an unsuccessful status.
    GitFileListFailed(Option<i32>),
    /// Failed to stream or read the requested Git revision.
    ReadGitRevision(io::Error),
    /// Git object reading did not expose its requested input or output stream.
    MissingGitObjectStream,
    /// One exact-revision Git operation returned an unsuccessful status.
    GitRevisionCommandFailed {
        /// Stable operation category without command output or host paths.
        operation: &'static str,
        /// Process status code when available.
        code: Option<i32>,
    },
    /// Git returned a malformed recursive tree record.
    InvalidGitTreeEntry,
    /// Pre-push input did not contain four valid ref/object fields.
    InvalidPrePushUpdate,
    /// Git returned a malformed blob protocol response.
    InvalidGitObjectResponse,
    /// Git returned a blob size that could not be parsed.
    InvalidGitObjectSize(std::num::ParseIntError),
    /// Git returned a repository path that was not UTF-8.
    NonUtf8GitPath(std::string::FromUtf8Error),
    /// Git-visible source text had no supported deterministic encoding.
    NonUtf8Text(String),
    /// A BOM-marked UTF-16 source file contained invalid code units.
    InvalidUtf16Text {
        /// Repository-relative source path.
        path: String,
        /// Original UTF-16 decoding failure.
        source: std::string::FromUtf16Error,
    },
    /// A built-in private-path rule did not compile.
    InvalidPrivatePathRule(regex::Error),
    /// Repository source-policy violations.
    Violations {
        /// Strict string-contract violations.
        string_literals: Vec<StringLiteralViolation>,
        /// Redacted private absolute-path violations.
        private_paths: Vec<PrivatePathViolation>,
    },
}

impl Display for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "{message}"),
            Self::Io(source) => write!(formatter, "io error: {source}"),
            Self::ReadFile { path, source } => {
                write!(formatter, "failed to read {path:?}: {source}")
            }
            Self::Parse { path, source } => {
                write!(formatter, "failed to parse {path:?}: {source}")
            }
            Self::ListGitFiles(source) => {
                write!(formatter, "failed to list repository source: {source}")
            }
            Self::GitFileListFailed(code) => {
                write!(
                    formatter,
                    "repository source listing failed with status {code:?}"
                )
            }
            Self::ReadGitRevision(_) => {
                write!(formatter, "failed to read exact Git revision source")
            }
            Self::MissingGitObjectStream => {
                write!(
                    formatter,
                    "exact Git revision source stream was unavailable"
                )
            }
            Self::GitRevisionCommandFailed { operation, code } => {
                write!(
                    formatter,
                    "exact Git revision {operation} failed with status {code:?}"
                )
            }
            Self::InvalidGitTreeEntry => {
                write!(
                    formatter,
                    "exact Git revision returned a malformed tree entry"
                )
            }
            Self::InvalidPrePushUpdate => {
                write!(formatter, "pre-push update input is malformed")
            }
            Self::InvalidGitObjectResponse | Self::InvalidGitObjectSize(_) => {
                write!(
                    formatter,
                    "exact Git revision returned a malformed blob response"
                )
            }
            Self::NonUtf8GitPath(_) => {
                write!(
                    formatter,
                    "repository source listing returned a non-UTF-8 path"
                )
            }
            Self::NonUtf8Text(path) => {
                write!(
                    formatter,
                    "Git-visible text {path:?} is not valid UTF-8 or BOM-marked UTF-16"
                )
            }
            Self::InvalidUtf16Text { path, .. } => {
                write!(formatter, "Git-visible UTF-16 text {path:?} is malformed")
            }
            Self::InvalidPrivatePathRule(_) => {
                write!(formatter, "built-in private absolute-path rule is invalid")
            }
            Self::Violations {
                string_literals,
                private_paths,
            } => write!(
                formatter,
                "{} repository source-policy violations",
                string_literals.len() + private_paths.len()
            ),
        }
    }
}

impl std::error::Error for LintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(source)
            | Self::ListGitFiles(source)
            | Self::ReadGitRevision(source)
            | Self::ReadFile { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::NonUtf8GitPath(source) => Some(source),
            Self::InvalidGitObjectSize(source) => Some(source),
            Self::InvalidUtf16Text { source, .. } => Some(source),
            Self::InvalidPrivatePathRule(source) => Some(source),
            Self::Usage(_)
            | Self::GitFileListFailed(_)
            | Self::MissingGitObjectStream
            | Self::GitRevisionCommandFailed { .. }
            | Self::InvalidGitTreeEntry
            | Self::InvalidPrePushUpdate
            | Self::InvalidGitObjectResponse
            | Self::NonUtf8Text(_)
            | Self::Violations { .. } => None,
        }
    }
}

/// Write all repository source-policy diagnostics without private matched text.
fn write_violations(
    writer: &mut impl Write,
    string_literals: &[StringLiteralViolation],
    private_paths: &[PrivatePathViolation],
) -> io::Result<()> {
    writeln!(
        writer,
        "projectatlas-lints: {} repository source-policy violation(s)",
        string_literals.len() + private_paths.len()
    )?;
    for violation in string_literals {
        writeln!(writer, "{violation}")?;
    }
    for violation in private_paths {
        writeln!(writer, "{violation}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        E2E_FIXTURE_PATH_LITERALS, LintError, MCP_PROJECT_SCHEMA_LITERALS, PathJoinLiteralRule,
        StringLiteralRule, StringLiteralViolation, decode_repository_text,
        lint_git_revisions_private_paths, lint_git_tree_private_paths, lint_private_paths,
        lint_repeated_path_join_literals, lint_repository_private_paths, lint_source,
        outgoing_revisions, private_path_rules, private_paths_not_in_baseline,
        run_private_path_range, run_strict_strings, write_violations,
    };
    use std::fs;
    use std::io::{self, Write};
    use std::path::Path;
    use std::process::Command;

    /// Rule used by parser-focused unit tests.
    const TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-rule",
        description: "test protected string rule",
        paths: &["demo.rs"],
        ban_unlisted: false,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: &[],
    };

    /// Broad rule used by production-string tests.
    const BROAD_TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-broad-rule",
        description: "test broad string rule",
        paths: &["demo.rs"],
        ban_unlisted: true,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: &["allowed inline format {value}"],
    };

    /// Rule used by e2e fixture path centralization tests.
    const E2E_FIXTURE_TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-e2e-fixture-path-rule",
        description: "test e2e fixture path string rule",
        paths: &["demo.rs"],
        ban_unlisted: false,
        literals: E2E_FIXTURE_PATH_LITERALS,
        allowed_literals: &[],
    };

    /// Rule used by repeated path-join tests.
    const PATH_JOIN_TEST_RULE: PathJoinLiteralRule = PathJoinLiteralRule {
        id: "test-path-join-rule",
        description: "test repeated path join rule",
        paths: &["demo.rs"],
        allowed_repeated_literals: &["reviewed-existing"],
    };

    /// Return a test error instead of panicking on a failed condition.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    /// Ordinary workspace tests must enforce the same repository source contracts as CI.
    #[test]
    fn repository_strict_string_contracts_pass() -> Result<(), Box<dyn std::error::Error>> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| {
                io::Error::other("lint crate must be nested below the workspace root")
            })?;
        run_strict_strings(workspace_root).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(())
    }

    /// Build representative private path shapes without embedding an absolute path literal.
    fn private_path_samples() -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let backslash = char::from_u32(92)
            .ok_or_else(|| io::Error::other("backslash code point must be valid"))?;
        let drive =
            char::from_u32(88).ok_or_else(|| io::Error::other("drive code point must be valid"))?;
        Ok(vec![
            format!(
                "{drive}:{backslash}{}{backslash}{}",
                "private-host", "workspace"
            ),
            format!("{drive}:/{}/{}", "private-host", "workspace"),
            format!(
                "{backslash}{backslash}{}{backslash}{}",
                "private-host", "share"
            ),
            format!(
                "{backslash}{backslash}?{backslash}UNC{backslash}{}{backslash}{}",
                "private-host", "share"
            ),
            format!(
                "{backslash}{backslash}{backslash}{backslash}?{backslash}{backslash}UNC{backslash}{backslash}{}{backslash}{backslash}{}",
                "private-host", "share"
            ),
            ["", "", "private-host", "share"].join("/"),
            ["", "home", "example-user", "workspace"].join("/"),
            ["", "Users", "example-user", "workspace"].join("/"),
            ["", "mnt", "x", "workspace"].join("/"),
            ["file:", "", "private-host", "share"].join("/"),
            ["file:", "", "", "home", "example-user", "workspace"].join("/"),
        ])
    }

    /// Every private path family is detected while diagnostics omit matched source text.
    #[test]
    fn private_path_lint_detects_all_shapes_with_redacted_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let samples = private_path_samples()?;
        let source = samples.join("\n");
        let rules = private_path_rules()?;
        let violations = lint_private_paths("scripts/privacy-check.ps1", &source, &rules);
        require(
            violations.len() == samples.len(),
            "expected every private path shape to be rejected",
        )?;
        let string_violation = StringLiteralViolation {
            path: "scripts/privacy-check.ps1".to_string(),
            line: 1,
            column: 1,
            rule_id: "test-rule",
            description: "test string contract",
        };
        let mut diagnostics = Vec::new();
        write_violations(&mut diagnostics, &[string_violation], &violations)?;
        let diagnostics = String::from_utf8(diagnostics)?;
        require(
            samples.iter().all(|sample| !diagnostics.contains(sample)),
            "private path diagnostic disclosed matched source text",
        )?;
        Ok(())
    }

    /// Relative paths, placeholders, and network URLs remain portable source text.
    #[test]
    fn private_path_lint_allows_portable_path_forms() -> Result<(), Box<dyn std::error::Error>> {
        let backslash = char::from_u32(92)
            .ok_or_else(|| io::Error::other("backslash code point must be valid"))?;
        let source = format!(
            "scripts/install-runtime.ps1\n<project-root>/scripts/install-runtime.sh\n\
             $HOME/.local/bin\n%USERPROFILE%{backslash}AppData{backslash}Local\n\
             https://example.com/downloads/runtime\n\
             \"{backslash}{backslash}Sessions{backslash}{backslash}\"\n\
             \"{backslash}{backslash}r{backslash}{backslash}n\""
        );
        let rules = private_path_rules()?;
        let violations = lint_private_paths("docs/example.md", &source, &rules);
        require(violations.is_empty(), "portable path form was rejected")?;
        Ok(())
    }

    /// Tracked and non-ignored untracked files are scanned while ignored state is excluded.
    #[test]
    fn private_path_lint_scans_untracked_script_files() -> Result<(), Box<dyn std::error::Error>> {
        let repository = tempfile::tempdir()?;
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .status()?;
        require(
            status.success(),
            "temporary Git repository initialization failed",
        )?;
        let samples = private_path_samples()?;
        let sample = samples
            .get(4)
            .ok_or_else(|| io::Error::other("untracked private path sample is missing"))?;
        fs::write(
            repository.path().join("privacy-check.ps1"),
            format!("$runtime = {sample:?}\n"),
        )?;
        let tracked_sample = samples
            .get(1)
            .ok_or_else(|| io::Error::other("tracked private path sample is missing"))?;
        fs::write(
            repository.path().join("tracked.rs"),
            format!("const RUNTIME: &str = {tracked_sample:?};\n"),
        )?;
        fs::write(repository.path().join(".gitignore"), "ignored.txt\n")?;
        fs::write(repository.path().join("ignored.txt"), format!("{sample}\n"))?;
        let status = Command::new("git")
            .args(["add", ".gitignore", "tracked.rs"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "temporary Git files could not be staged")?;
        let violations = lint_repository_private_paths(repository.path())?;
        require(
            violations.len() == 2,
            "tracked or non-ignored untracked private path was not rejected",
        )?;
        require(
            violations
                .iter()
                .any(|violation| violation.path == "privacy-check.ps1")
                && violations
                    .iter()
                    .any(|violation| violation.path == "tracked.rs")
                && violations
                    .iter()
                    .all(|violation| violation.path != "ignored.txt"),
            "Git visibility or repository-relative diagnostics regressed",
        )?;
        Ok(())
    }

    /// BOM-marked source is text, while binary Git files stay excluded.
    #[test]
    fn private_path_lint_scans_text_encodings() -> Result<(), Box<dyn std::error::Error>> {
        let repository = tempfile::tempdir()?;
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(
            status.success(),
            "temporary Git repository initialization failed",
        )?;
        let sample = private_path_samples()?
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("private path sample is missing"))?;
        let mut encoded = vec![0xff, 0xfe];
        let mut encoded_be = vec![0xfe, 0xff];
        for unit in sample.encode_utf16() {
            encoded.extend_from_slice(&unit.to_le_bytes());
            encoded_be.extend_from_slice(&unit.to_be_bytes());
        }
        fs::write(repository.path().join("privacy-check.ps1"), encoded)?;
        fs::write(repository.path().join("privacy-check-be.ps1"), encoded_be)?;
        fs::write(repository.path().join("asset.bin"), [0x89, 0, 0xff, 0])?;
        let status = Command::new("git")
            .args([
                "add",
                "privacy-check.ps1",
                "privacy-check-be.ps1",
                "asset.bin",
            ])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "temporary Git files could not be staged")?;
        let violations = lint_repository_private_paths(repository.path())?;
        require(
            violations.len() == 2
                && violations
                    .iter()
                    .any(|violation| violation.path == "privacy-check.ps1")
                && violations
                    .iter()
                    .any(|violation| violation.path == "privacy-check-be.ps1"),
            "UTF-16 source or binary classification regressed",
        )?;
        require(
            decode_repository_text("malformed.ps1", &[0xff, 0xfe, 0]).is_err(),
            "malformed UTF-16 source was accepted",
        )?;
        let rules = private_path_rules()?;
        for sample in private_path_samples()? {
            let mut encoded = vec![0xef, 0xbb, 0xbf];
            encoded.extend_from_slice(sample.as_bytes());
            let decoded = decode_repository_text("bom.txt", &encoded)?
                .ok_or_else(|| io::Error::other("UTF-8 BOM source was treated as binary"))?;
            require(
                lint_private_paths("bom.txt", &decoded, &rules).len() == 1,
                "UTF-8 BOM hid a private path family at the start of source",
            )?;
        }
        Ok(())
    }

    /// Every outgoing revision is scanned even when its tip and dirty overlay are safe.
    #[test]
    fn private_path_lint_scans_outgoing_revisions() -> Result<(), Box<dyn std::error::Error>> {
        let repository = tempfile::tempdir()?;
        for args in [
            &["init", "--quiet"][..],
            &["config", "user.name", "ProjectAtlas Test"][..],
            &["config", "user.email", "projectatlas@example.invalid"][..],
            &["config", "core.abbrev", "4"][..],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(repository.path())
                .output()?
                .status;
            require(status.success(), "temporary Git setup failed")?;
        }
        let samples = private_path_samples()?;
        let sample = samples
            .first()
            .ok_or_else(|| io::Error::other("private path sample is missing"))?;
        let baseline_sample = samples
            .get(1)
            .ok_or_else(|| io::Error::other("baseline private path sample is missing"))?;
        let source_path = repository.path().join("privacy-check.ps1");
        let legacy_path = repository.path().join("legacy.ps1");
        let same_root_path = repository.path().join("same-root.ps1");
        let same_root_sample = sample.replace("private-host", "private host");
        let replacement_sample = same_root_sample.replace("workspace", "replacement");
        fs::write(&source_path, "$runtime = Join-Path $env:TEMP 'base'\n")?;
        fs::write(&legacy_path, format!("$legacy = {baseline_sample:?}\n"))?;
        fs::write(
            &same_root_path,
            format!("$runtime = {same_root_sample:?}\n"),
        )?;
        let status = Command::new("git")
            .args(["add", "privacy-check.ps1", "legacy.ps1", "same-root.ps1"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "temporary Git base could not be staged")?;
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "clean base"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "temporary Git base commit failed")?;
        let base = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository.path())
            .output()?;
        require(base.status.success(), "temporary Git base lookup failed")?;
        let base = String::from_utf8(base.stdout)?.trim().to_string();
        fs::write(
            &legacy_path,
            format!("$legacy = {baseline_sample:?}\n# retained baseline text\n"),
        )?;
        let status = Command::new("git")
            .args(["add", "legacy.ps1"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "safe baseline edit could not be staged")?;
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "retain published private text"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(status.success(), "safe baseline edit commit failed")?;
        let safe_head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository.path())
            .output()?;
        require(safe_head.status.success(), "safe Git head lookup failed")?;
        let safe_head = String::from_utf8(safe_head.stdout)?.trim().to_string();
        require(
            run_private_path_range(repository.path(), &base, &safe_head).is_ok(),
            "unchanged private text already present at the published base was rejected",
        )?;
        for (content, message, attributes) in [
            (
                format!("$runtime = {sample:?}\n"),
                "private intermediate",
                Some("privacy-check.ps1 export-ignore\n"),
            ),
            (
                "$runtime = Join-Path $env:TEMP 'clean-tip'\n".to_string(),
                "clean tip",
                None,
            ),
        ] {
            fs::write(&source_path, content)?;
            fs::write(
                &same_root_path,
                if message == "private intermediate" {
                    format!("$runtime = {replacement_sample:?}\n")
                } else {
                    "$runtime = Join-Path $env:TEMP 'clean-tip'\n".to_string()
                },
            )?;
            if let Some(attributes) = attributes {
                fs::write(repository.path().join(".gitattributes"), attributes)?;
            }
            for args in [
                &["add", "--all"][..],
                &["commit", "--quiet", "-m", message][..],
            ] {
                let status = Command::new("git")
                    .args(args)
                    .current_dir(repository.path())
                    .output()?
                    .status;
                require(status.success(), "temporary Git commit failed")?;
            }
        }
        fs::write(
            &source_path,
            "$runtime = Join-Path $env:TEMP 'dirty-safe'\n",
        )?;
        let working_tree_violations = lint_repository_private_paths(repository.path())?;
        require(
            working_tree_violations.len() == 1 && working_tree_violations[0].path == "legacy.ps1",
            "safe dirty overlay must retain only the published-base private text",
        )?;
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository.path())
            .output()?;
        require(head.status.success(), "outgoing Git head lookup failed")?;
        let head = String::from_utf8(head.stdout)?.trim().to_string();
        let null_object = "0".repeat(base.len());
        let updates = format!(
            "refs/heads/feature {head} refs/heads/feature {base}\n\
             refs/heads/new {head} refs/heads/new {null_object}\n\
             refs/heads/delete {null_object} refs/heads/delete {base}\n"
        );
        let revisions = outgoing_revisions(
            repository.path(),
            "unconfigured-remote",
            io::Cursor::new(updates),
        )?;
        require(
            revisions.len() == 4,
            "existing, new, duplicate, or deletion update selection regressed",
        )?;
        let violations = lint_git_revisions_private_paths(repository.path(), &revisions)?;
        require(
            violations.len() == 5,
            &format!(
                "raw outgoing scan must include introduced and inherited private text; found {}",
                violations.len()
            ),
        )?;
        let baseline = lint_git_tree_private_paths(repository.path(), &base)?;
        let introduced = private_paths_not_in_baseline(violations, &baseline);
        require(
            introduced.len() == 2
                && introduced
                    .iter()
                    .any(|violation| violation.path == "privacy-check.ps1")
                && introduced
                    .iter()
                    .any(|violation| violation.path == "same-root.ps1"),
            "range scan did not isolate private text introduced after the published base",
        )?;
        require(
            run_private_path_range(repository.path(), &base, &head).is_err(),
            "private intermediate revision was accepted by the hosted range gate",
        )?;
        require(
            matches!(
                run_private_path_range(repository.path(), &null_object, &head),
                Err(LintError::Violations { .. })
            ),
            "a zero-base branch creation did not scan its complete reachable history",
        )?;
        require(
            outgoing_revisions(
                repository.path(),
                "unconfigured-remote",
                io::Cursor::new("malformed\n"),
            )
            .is_err(),
            "malformed pre-push input was accepted",
        )?;
        Ok(())
    }

    /// SHA-256 history keeps distinct paths even when they share one private blob.
    #[test]
    fn private_path_lint_scans_sha256_history() -> Result<(), Box<dyn std::error::Error>> {
        let repository = tempfile::tempdir()?;
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(repository.path())
                .output()
        };
        if !git(&["init", "--quiet", "--object-format=sha256"])?
            .status
            .success()
        {
            writeln!(
                io::stderr().lock(),
                "skipped: installed Git does not support SHA-256 repositories"
            )?;
            return Ok(());
        }
        for args in [
            &["config", "user.name", "ProjectAtlas Test"][..],
            &["config", "user.email", "projectatlas@example.invalid"][..],
        ] {
            require(git(args)?.status.success(), "SHA-256 Git setup failed")?;
        }
        fs::write(repository.path().join("baseline.txt"), "safe\n")?;
        require(
            git(&["add", "baseline.txt"])?.status.success()
                && git(&["commit", "--quiet", "-m", "clean base"])?
                    .status
                    .success(),
            "SHA-256 Git base commit failed",
        )?;
        let base = git(&["rev-parse", "HEAD"])?;
        require(base.status.success(), "SHA-256 Git base lookup failed")?;
        let base = String::from_utf8(base.stdout)?.trim().to_string();
        require(base.len() == 64, "Git did not create SHA-256 object ids")?;

        let sample = private_path_samples()?
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("private path sample is missing"))?;
        let private_source = format!("$runtime = {sample:?}\n");
        for path in ["first.ps1", "second.ps1"] {
            fs::write(repository.path().join(path), &private_source)?;
        }
        require(
            git(&["add", "first.ps1", "second.ps1"])?.status.success()
                && git(&["commit", "--quiet", "-m", "private intermediate"])?
                    .status
                    .success(),
            "SHA-256 private intermediate commit failed",
        )?;
        for path in ["first.ps1", "second.ps1"] {
            fs::write(repository.path().join(path), "$runtime = $env:TEMP\n")?;
        }
        require(
            git(&["add", "first.ps1", "second.ps1"])?.status.success()
                && git(&["commit", "--quiet", "-m", "clean tip"])?
                    .status
                    .success(),
            "SHA-256 clean tip commit failed",
        )?;
        let head = git(&["rev-parse", "HEAD"])?;
        require(head.status.success(), "SHA-256 Git head lookup failed")?;
        let head = String::from_utf8(head.stdout)?.trim().to_string();
        require(head.len() == 64, "SHA-256 Git head has the wrong width")?;

        let updates = format!("refs/heads/feature {head} refs/heads/feature {base}\n");
        let revisions = outgoing_revisions(repository.path(), "", io::Cursor::new(updates))?;
        let baseline = lint_git_tree_private_paths(repository.path(), &base)?;
        let introduced = private_paths_not_in_baseline(
            lint_git_revisions_private_paths(repository.path(), &revisions)?,
            &baseline,
        );
        require(
            introduced.len() == 2
                && introduced.iter().any(|item| item.path == "first.ps1")
                && introduced.iter().any(|item| item.path == "second.ps1"),
            "SHA-256 range scan deduplicated distinct paths sharing one private blob",
        )?;
        require(
            matches!(
                run_private_path_range(repository.path(), &base, &head),
                Err(LintError::Violations { .. })
            ),
            "SHA-256 intermediate private revision passed the range gate",
        )?;
        let null_object = "0".repeat(64);
        require(
            matches!(
                run_private_path_range(repository.path(), &null_object, &head),
                Err(LintError::Violations { .. })
            ),
            "SHA-256 zero-base history passed the range gate",
        )?;
        Ok(())
    }

    /// Hostile Git-relative filenames stay escaped in source-policy diagnostics.
    #[test]
    fn private_path_diagnostics_escape_control_characters() -> Result<(), Box<dyn std::error::Error>>
    {
        let path = "scripts/line\n::error::forged.ps1";
        let private_path = super::PrivatePathViolation {
            path: path.to_string(),
            line: 1,
            column: 1,
            kind: "windows-drive-root",
            source_identity: super::PrivatePathIdentity(std::rc::Rc::from("")),
            git_object: None,
        };
        let mut diagnostics = Vec::new();
        write_violations(&mut diagnostics, &[], &[private_path])?;
        let diagnostics = String::from_utf8(diagnostics)?;
        require(
            !diagnostics.contains("\n::error::") && diagnostics.contains("\\n::error::"),
            "control-bearing repository path was not escaped",
        )?;
        Ok(())
    }

    /// Unix Git filenames containing controls cannot inject hosted log records.
    #[cfg(unix)]
    #[test]
    fn private_path_lint_escapes_hostile_git_filename() -> Result<(), Box<dyn std::error::Error>> {
        let repository = tempfile::tempdir()?;
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .output()?
            .status;
        require(
            status.success(),
            "temporary Git repository initialization failed",
        )?;
        let path = "line\n::error::forged.ps1";
        let sample = private_path_samples()?
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("private path sample is missing"))?;
        fs::write(repository.path().join(path), sample)?;
        let violations = lint_repository_private_paths(repository.path())?;
        require(
            violations.len() == 1,
            "hostile Git filename was not scanned",
        )?;
        let mut diagnostics = Vec::new();
        write_violations(&mut diagnostics, &[], &violations)?;
        let diagnostics = String::from_utf8(diagnostics)?;
        require(
            !diagnostics.contains("\n::error::") && diagnostics.contains("\\n::error::"),
            "hostile Git filename injected a diagnostic record",
        )?;
        Ok(())
    }

    /// Macro literals are parsed and reported as protected schema strings.
    #[test]
    fn strict_string_lint_flags_macro_schema_literals() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn payload() { let _ = serde_json::json!({ "status": "active" }); }"#,
        )?;
        require(violations.len() == 2, "expected two macro violations")?;
        require(
            violations
                .iter()
                .all(|violation| violation.rule_id == TEST_RULE.id),
            "macro violation used the wrong rule",
        )?;
        Ok(())
    }

    /// Constants are accepted as local centralization points.
    #[test]
    fn strict_string_lint_allows_constants() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"const STATUS_FIELD: &str = "status"; fn payload() { let _ = STATUS_FIELD; }"#,
        )?;
        require(violations.is_empty(), "constant declaration was flagged")?;
        Ok(())
    }

    /// E2E fixture path segments must be centralized just like production contracts.
    #[test]
    fn strict_string_lint_flags_inline_e2e_fixture_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let violations = lint_source(
            "demo.rs",
            &E2E_FIXTURE_TEST_RULE,
            r#"fn fixture(temp: &std::path::Path) { let _ = temp.join("fake-codex.log"); }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one e2e fixture path violation",
        )?;
        let violation = violations
            .first()
            .ok_or_else(|| io::Error::other("missing e2e fixture path violation"))?;
        require(
            violation.rule_id == E2E_FIXTURE_TEST_RULE.id,
            "e2e fixture path violation used the wrong rule",
        )?;
        Ok(())
    }

    /// Constants remain the expected centralization point for e2e fixture paths.
    #[test]
    fn strict_string_lint_allows_e2e_fixture_path_constants()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &E2E_FIXTURE_TEST_RULE,
            r#"const FAKE_CODEX_LOG_FILE: &str = "fake-codex.log"; fn fixture(temp: &std::path::Path) { let _ = temp.join(FAKE_CODEX_LOG_FILE); }"#,
        )?;
        require(
            violations.is_empty(),
            "e2e fixture path constant was flagged",
        )?;
        Ok(())
    }

    /// Repeated unreviewed `.join("...")` path literals are reported.
    #[test]
    fn strict_string_lint_flags_repeated_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn fixture(root: &std::path::Path) {
    let _ = root.join("future-fixture");
    let _ = root.join("future-fixture").join("leaf");
}
"#,
        )?;
        require(
            violations.len() == 2,
            "expected both repeated path joins to be flagged",
        )?;
        require(
            violations
                .iter()
                .all(|violation| violation.rule_id == PATH_JOIN_TEST_RULE.id),
            "repeated path join violation used the wrong rule",
        )?;
        Ok(())
    }

    /// One-off path joins are allowed to avoid fixture noise.
    #[test]
    fn strict_string_lint_allows_one_off_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"fn fixture(root: &std::path::Path) { let _ = root.join("one-off"); }"#,
        )?;
        require(violations.is_empty(), "one-off path join was flagged")?;
        Ok(())
    }

    /// Collection/string join separators are not path fragments.
    #[test]
    fn strict_string_lint_ignores_repeated_join_separators()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn format_messages(messages: &[&str]) -> String {
    let first = messages.join("\n");
    let second = messages.join("\n");
    format!("{first}{second}")
}
"#,
        )?;
        require(
            violations.is_empty(),
            "collection join separator was flagged as a path literal",
        )?;
        Ok(())
    }

    /// Reviewed repeated path joins stay available for existing broad fixtures.
    #[test]
    fn strict_string_lint_allows_reviewed_repeated_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn fixture(root: &std::path::Path) {
    let _ = root.join("reviewed-existing");
    let _ = root.join("reviewed-existing");
}
"#,
        )?;
        require(
            violations.is_empty(),
            "reviewed repeated path joins were flagged",
        )?;
        Ok(())
    }

    /// Enum `as_str` methods are accepted as typed string centralization points.
    #[test]
    fn strict_string_lint_allows_as_str_centralization() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
enum Status {
    Active,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}
"#,
        )?;
        require(violations.is_empty(), "as_str centralization was flagged")?;
        Ok(())
    }

    /// Direct expression literals are reported, not only macro-contained strings.
    #[test]
    fn strict_string_lint_flags_direct_expr_literals() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn status() { let _ = "active"; }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one direct literal violation",
        )?;
        let violation = violations
            .first()
            .ok_or_else(|| io::Error::other("missing direct literal violation"))?;
        require(
            violation.rule_id == TEST_RULE.id,
            "direct literal violation used the wrong rule",
        )?;
        Ok(())
    }

    /// Comments and serialization attributes are ignored to avoid false positives.
    #[test]
    fn strict_string_lint_ignores_comments_and_attributes() -> Result<(), Box<dyn std::error::Error>>
    {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"
// "status" and "active" appear only in a comment.
#[serde(rename_all = "snake_case")]
enum Status {
    Active,
}
"#,
        )?;
        require(
            violations.is_empty(),
            "comments or serde attributes were flagged",
        )?;
        Ok(())
    }

    /// Typed serialization is the preferred schema/status representation.
    #[test]
    fn strict_string_lint_allows_typed_project_state_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"
#[derive(serde::Serialize)]
struct ProjectStateResponse {
    project: ProjectStatePayload,
}

#[derive(serde::Serialize)]
struct ProjectStatePayload {
    root: String,
    db: String,
    config: Option<String>,
    status: ProjectStatus,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ProjectStatus {
    Active,
}
"#,
        )?;
        require(
            violations.is_empty(),
            "typed project-state serialization was flagged",
        )?;
        Ok(())
    }

    /// Human-readable prose with protected words inside a larger sentence is ignored.
    #[test]
    fn strict_string_lint_ignores_non_exact_prose() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn message() -> &'static str { "selected project root is active" }"#,
        )?;
        require(violations.is_empty(), "non-exact prose was flagged")?;
        Ok(())
    }

    /// Broad production mode rejects arbitrary inline strings.
    #[test]
    fn strict_string_lint_flags_unlisted_production_strings()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"fn message() -> &'static str { "selected project root is active" }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one unlisted production string violation",
        )?;
        Ok(())
    }

    /// Reviewed inline format templates are accepted in broad production mode.
    #[test]
    fn strict_string_lint_allows_reviewed_inline_format_templates()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"fn message(value: &str) -> String { format!("allowed inline format {value}") }"#,
        )?;
        require(
            violations.is_empty(),
            "allowlisted inline format was flagged",
        )?;
        Ok(())
    }

    /// Test modules are ignored so fixture strings do not weaken production checks.
    #[test]
    fn strict_string_lint_ignores_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(test)]
mod tests {
    fn fixture() -> &'static str { "arbitrary fixture text" }
}
"#,
        )?;
        require(violations.is_empty(), "test module fixture was flagged")?;
        Ok(())
    }

    /// Test-only `all(...)` cfg modules are ignored.
    #[test]
    fn strict_string_lint_ignores_all_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(all(test, feature = "fixtures"))]
mod tests {
    fn fixture() -> &'static str { "arbitrary fixture text" }
}
"#,
        )?;
        require(violations.is_empty(), "test-only cfg module was flagged")?;
        Ok(())
    }

    /// Production-only cfg modules must still be scanned.
    #[test]
    fn strict_string_lint_scans_not_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(not(test))]
mod prod {
    fn payload() {
        let _ = serde_json::json!({ "status": "active" });
    }
}
"#,
        )?;
        require(
            violations.len() == 2,
            "production-only cfg module was not scanned",
        )?;
        Ok(())
    }

    /// Production branches inside `all(...)` cfg modules must still be scanned.
    #[test]
    fn strict_string_lint_scans_all_not_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(all(not(test), feature = "runtime"))]
mod prod {
    fn payload() {
        let _ = serde_json::json!({ "status": "active" });
    }
}
"#,
        )?;
        require(
            violations.len() == 2,
            "production all-not-test cfg module was not scanned",
        )?;
        Ok(())
    }
}
