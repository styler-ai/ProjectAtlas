//! Validate repository-owned Rust test-quality policy and retained evidence.

use globset::{Glob, GlobSet, GlobSetBuilder};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Supported test-quality policy schema version.
const POLICY_SCHEMA_VERSION: u32 = 1;
/// Supported retained-evidence schema version.
const EVIDENCE_SCHEMA_VERSION: u32 = 1;
/// Required cargo-nextest version.
const EXPECTED_NEXTEST_VERSION: &str = "0.9.140";
/// Required cargo-llvm-cov version.
const EXPECTED_LLVM_COV_VERSION: &str = "0.8.7";
/// Required cargo-mutants version.
const EXPECTED_MUTANTS_VERSION: &str = "27.1.0";
/// Exact shard count required for complete mutation evidence.
const REQUIRED_MUTATION_SHARDS: u8 = 16;
/// Process exit code for invalid validator usage.
const EXIT_USAGE: u8 = 2;
/// Validation-summary identity for the retained coverage enforcement contract.
const COVERAGE_ENFORCEMENT_IDENTITY: &str = "coverage_enforcement";
/// Pinned historical mutant count used to verify observed drift.
const HISTORICAL_MUTATION_BASELINE: u64 = 4_911;

/// Run a fixed test-quality validation subcommand.
pub(crate) fn run(args: &[String], current_dir: &Path) -> Result<(), QualityError> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return write_help();
    }
    let command = args
        .first()
        .ok_or_else(|| QualityError::Usage("missing test-quality command".to_string()))?;
    let parsed = FixedArgs::parse(&args[1..])?;
    let result = execute(command, &parsed, current_dir);
    match result {
        Ok(summary) => write_summary(&summary, parsed.json),
        Err(error) => {
            let summary = ValidationSummary::failure(command, error.status(), error.to_string());
            write_summary(&summary, parsed.json)?;
            Err(error)
        }
    }
}

/// Execute one validated test-quality subcommand.
fn execute(
    command: &str,
    args: &FixedArgs,
    current_dir: &Path,
) -> Result<ValidationSummary, QualityError> {
    let root = RepositoryRoot::open(args.required_one("--root")?, current_dir)?;
    let policy_path = root.input(args.required_one("--policy")?)?;
    let policy: QualityPolicy = read_toml(&policy_path)?;
    validate_policy(&root, &policy)?;

    let mut summary = ValidationSummary::passed(command);
    summary
        .identities
        .insert("policy_sha256".to_string(), digest_file(&policy_path)?);
    summary.identities.insert(
        "source_scope_sha256".to_string(),
        digest_strings(policy.scope.include_globs.iter().map(String::as_str)),
    );
    let commit = if command == "tasks" {
        root.task_commit(args.required_one("--expected-commit")?)?
    } else {
        root.head_commit()?
    };
    summary
        .identities
        .insert("commit".to_string(), commit.clone());

    match command {
        "policy" => {
            args.require_only(&["--root", "--policy", "--base-policy"])?;
            if let Some(base) = args.optional_one("--base-policy")? {
                let base: QualityPolicy = read_toml(&root.input(base)?)?;
                validate_policy_ratchet(&base, &policy)?;
            }
        }
        "configs" => {
            args.require_only(&["--root", "--policy", "--nextest", "--mutants"])?;
            validate_nextest_config(&read_toml_value(
                &root.input(args.required_one("--nextest")?)?,
            )?)?;
            validate_mutants_config(&read_toml_value(
                &root.input(args.required_one("--mutants")?)?,
            )?)?;
        }
        "nextest" => {
            args.require_only(&["--root", "--policy", "--inventory", "--junit"])?;
            let inventory: NativeNextestInventory =
                read_json(&root.input(args.required_one("--inventory")?)?)?;
            let junit_path = root.input(args.required_one("--junit")?)?;
            let counts = validate_nextest_evidence(&inventory, &junit_path)?;
            counts.insert_summary(&mut summary.counts);
        }
        "doctest" => {
            args.require_only(&["--root", "--policy", "--log", "--exit-code"])?;
            let log_path = root.input(args.required_one("--log")?)?;
            let exit_code = args
                .required_one("--exit-code")?
                .parse::<i32>()
                .map_err(|source| QualityError::Usage(format!("invalid --exit-code: {source}")))?;
            let counts = validate_doctest_log(&read_text(&log_path)?, exit_code)?;
            counts.insert_summary(&mut summary.counts);
            summary
                .identities
                .insert("doctest_log_sha256".to_string(), digest_file(&log_path)?);
        }
        "tasks" => {
            args.require_only(&[
                "--root",
                "--policy",
                "--tasks",
                "--plan",
                "--evidence",
                "--expected-commit",
            ])?;
            let tasks_path = root.input(args.required_one("--tasks")?)?;
            let plan_path = root.input(args.required_one("--plan")?)?;
            let evidence_path = root.input(args.required_one("--evidence")?)?;
            let tasks = parse_openspec_tasks(&read_text(&tasks_path)?)?;
            let plan: VerificationPlan = read_json(&plan_path)?;
            let evidence: TaskEvidenceLedger = read_json(&evidence_path)?;
            validate_task_evidence(&root, &policy, &commit, &tasks, &plan, &evidence)?;
            summary
                .counts
                .insert("tasks".to_string(), usize_to_u64(tasks.len())?);
        }
        "coverage" => {
            args.require_only(&[
                "--root",
                "--policy",
                "--platform",
                "--llvm-json",
                "--enforcement",
            ])?;
            let platform = args.required_one("--platform")?;
            let enforcement = CoverageEnforcement::from_cli(args.optional_one("--enforcement")?)?;
            let export: LlvmCoverageExport =
                read_json(&root.input(args.required_one("--llvm-json")?)?)?;
            let counts = validate_coverage(&root, &policy, platform, &export, enforcement)?;
            counts.insert_summary(&mut summary.counts);
            summary.identities.insert(
                COVERAGE_ENFORCEMENT_IDENTITY.to_string(),
                enforcement.manifest_name().to_string(),
            );
        }
        "mutation-inventory" => {
            args.require_only(&["--root", "--policy", "--inventory"])?;
            let path = root.input(args.required_one("--inventory")?)?;
            let native: Vec<NativeMutant> = read_json(&path)?;
            let inventory = MutationInventory::from_native(&root, native)?;
            validate_inventory_baseline(&root, &policy, &inventory)?;
            summary.counts.insert(
                "mutants".to_string(),
                usize_to_u64(inventory.mutants.len())?,
            );
            summary
                .identities
                .insert("inventory_sha256".to_string(), digest_file(&path)?);
            summary.mutation_plan = Some(inventory.mutation_plan(&policy, &digest_file(&path)?)?);
        }
        "mutation-changed" => {
            args.require_only(&[
                "--root",
                "--policy",
                "--inventory",
                "--outcomes",
                "--merge-base",
            ])?;
            let inventory_path = root.input(args.required_one("--inventory")?)?;
            let inventory = MutationInventory::from_native(
                &root,
                read_json::<Vec<NativeMutant>>(&inventory_path)?,
            )?;
            let outcomes: NativeLabOutcome =
                read_json(&root.input(args.required_one("--outcomes")?)?)?;
            let counts = validate_changed_mutation(
                &root,
                &policy,
                &inventory,
                &outcomes,
                args.required_one("--merge-base")?,
            )?;
            counts.insert_summary(&mut summary.counts);
            summary.counts.insert("baseline_passed".to_string(), 1);
        }
        "mutation-aggregate" => {
            args.require_only(&["--root", "--policy", "--inventory", "--shard"])?;
            let inventory_path = root.input(args.required_one("--inventory")?)?;
            let inventory = MutationInventory::from_native(
                &root,
                read_json::<Vec<NativeMutant>>(&inventory_path)?,
            )?;
            let shard_paths = args.required_many("--shard")?;
            let counts = validate_mutation_aggregate(
                &root,
                &policy,
                &digest_file(&policy_path)?,
                &inventory,
                &digest_file(&inventory_path)?,
                shard_paths,
            )?;
            counts.insert_summary(&mut summary.counts);
            summary.counts.insert("baseline_passed".to_string(), 1);
            summary
                .counts
                .insert("shards".to_string(), u64::from(REQUIRED_MUTATION_SHARDS));
        }
        "evidence" => {
            args.require_only(&["--root", "--policy", "--manifest", "--release-commit"])?;
            let manifest_paths = args.required_many("--manifest")?;
            let release_commit = args.optional_one("--release-commit")?;
            let manifests = manifest_paths
                .iter()
                .map(|path| {
                    let manifest_path = root.input(path)?;
                    let manifest = read_json(&manifest_path)?;
                    Ok((manifest_path, manifest))
                })
                .collect::<Result<Vec<(PathBuf, EvidenceManifest)>, QualityError>>()?;
            validate_evidence_manifests(
                &root,
                &policy,
                &digest_file(&policy_path)?,
                &manifests,
                release_commit,
            )?;
            summary
                .counts
                .insert("manifests".to_string(), usize_to_u64(manifests.len())?);
        }
        other => {
            return Err(QualityError::Usage(format!(
                "unknown test-quality command {other:?}"
            )));
        }
    }
    Ok(summary)
}

/// Write test-quality command help to standard output.
fn write_help() -> Result<(), QualityError> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Usage: cargo projectatlas-lints test-quality <COMMAND> [OPTIONS]\n\nCommands:\n  policy             Validate policy and optional merge-base ratchet.\n  configs            Validate pinned nextest and cargo-mutants configuration.\n  nextest            Reconcile native nextest inventory with JUnit evidence.\n  doctest            Normalize stable cargo test --doc evidence.\n  tasks              Validate OpenSpec task, verification-plan, and evidence binding.\n  coverage           Validate one platform LLVM coverage export; --enforcement defaults to release-quality.\n  mutation-inventory Validate one raw cargo-mutants inventory.\n  mutation-changed   Validate changed-source mutation outcomes.\n  mutation-aggregate Reconcile exactly 16 full-mutation shards.\n  evidence           Validate normalized gate or release evidence.\n\nEvery command requires --root and --policy. Add --json for JSON output."
    )
    .map_err(QualityError::Output)
}

#[derive(Debug, Default)]
/// Parsed fixed-shape command-line options grouped by flag.
struct FixedArgs {
    /// Option values grouped by command-line flag.
    values: BTreeMap<String, Vec<String>>,
    /// Whether validation output uses the JSON envelope.
    json: bool,
}

impl FixedArgs {
    /// Parse fixed command-line flags and reject malformed option shapes.
    fn parse(args: &[String]) -> Result<Self, QualityError> {
        let mut parsed = Self::default();
        let mut index = 0;
        while index < args.len() {
            let key = &args[index];
            if key == "--json" {
                parsed.json = true;
                index += 1;
                continue;
            }
            if !key.starts_with("--") {
                return Err(QualityError::Usage(format!(
                    "unexpected positional argument {key:?}"
                )));
            }
            let value = args.get(index + 1).ok_or_else(|| {
                QualityError::Usage(format!("option {key} requires one argument"))
            })?;
            if value.starts_with("--") {
                return Err(QualityError::Usage(format!(
                    "option {key} requires one argument"
                )));
            }
            parsed
                .values
                .entry(key.clone())
                .or_default()
                .push(value.clone());
            index += 2;
        }
        Ok(parsed)
    }

    /// Return the single value required for an option.
    fn required_one(&self, key: &str) -> Result<&str, QualityError> {
        self.optional_one(key)?
            .ok_or_else(|| QualityError::Usage(format!("missing required option {key}")))
    }

    /// Return the optional single value for an option.
    fn optional_one(&self, key: &str) -> Result<Option<&str>, QualityError> {
        let Some(values) = self.values.get(key) else {
            return Ok(None);
        };
        if values.len() != 1 {
            return Err(QualityError::Usage(format!(
                "option {key} must appear exactly once"
            )));
        }
        Ok(values.first().map(String::as_str))
    }

    /// Return every value for a required repeatable option.
    fn required_many(&self, key: &str) -> Result<&[String], QualityError> {
        self.values
            .get(key)
            .map(Vec::as_slice)
            .filter(|values| !values.is_empty())
            .ok_or_else(|| QualityError::Usage(format!("missing required option {key}")))
    }

    /// Reject options that the selected subcommand does not accept.
    fn require_only(&self, allowed: &[&str]) -> Result<(), QualityError> {
        for key in self.values.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(QualityError::Usage(format!("unknown option {key}")));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
/// Canonical repository boundary used to confine validator I/O.
struct RepositoryRoot(PathBuf);

impl RepositoryRoot {
    /// Canonicalize and validate a `ProjectAtlas` repository root.
    fn open(value: &str, current_dir: &Path) -> Result<Self, QualityError> {
        let supplied = Path::new(value);
        let path = if supplied.is_absolute() {
            supplied.to_path_buf()
        } else {
            current_dir.join(supplied)
        };
        let canonical = path.canonicalize().map_err(|source| QualityError::Io {
            operation: "canonicalize repository root",
            path: path.clone(),
            source,
        })?;
        if !canonical.is_dir() {
            return Err(QualityError::WrongRoot(format!(
                "repository root is not a directory: {}",
                canonical.display()
            )));
        }
        for required in ["Cargo.toml", "Cargo.lock"] {
            if !canonical.join(required).is_file() {
                return Err(QualityError::WrongRoot(format!(
                    "repository root is missing {required}: {}",
                    canonical.display()
                )));
            }
        }
        Ok(Self(canonical))
    }

    /// Resolve one regular-file input inside the repository boundary.
    fn input(&self, relative: &str) -> Result<PathBuf, QualityError> {
        self.confined(relative, true)
    }

    /// Resolve one directory input inside the repository boundary.
    fn tree(&self, relative: &str) -> Result<PathBuf, QualityError> {
        self.confined(relative, false)
    }

    /// Resolve a path while rejecting escapes, links, and the wrong file kind.
    fn confined(&self, relative: &str, file: bool) -> Result<PathBuf, QualityError> {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(QualityError::PathEscape(relative.to_string()));
        }
        let joined = self.0.join(path);
        let mut checked = self.0.clone();
        for component in path.components() {
            let Component::Normal(component) = component else {
                return Err(QualityError::PathEscape(relative.to_string()));
            };
            checked.push(component);
            let metadata = fs::symlink_metadata(&checked).map_err(|source| QualityError::Io {
                operation: "inspect input path",
                path: checked.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(QualityError::PathEscape(relative.to_string()));
            }
        }
        let canonical = joined.canonicalize().map_err(|source| QualityError::Io {
            operation: "canonicalize input",
            path: joined.clone(),
            source,
        })?;
        if !canonical.starts_with(&self.0)
            || (file && !canonical.is_file())
            || (!file && !canonical.is_dir())
        {
            return Err(QualityError::PathEscape(relative.to_string()));
        }
        Ok(canonical)
    }

    /// Convert a confined path into a normalized repository-relative key.
    fn relative_key(&self, path: &Path) -> Result<String, QualityError> {
        let relative = path
            .strip_prefix(&self.0)
            .map_err(|_source| QualityError::PathEscape(path.to_string_lossy().into_owned()))?;
        let mut key = String::new();
        for component in relative.components() {
            let Component::Normal(value) = component else {
                return Err(QualityError::PathEscape(
                    relative.to_string_lossy().into_owned(),
                ));
            };
            if !key.is_empty() {
                key.push('/');
            }
            key.push_str(&value.to_string_lossy());
        }
        Ok(key)
    }

    /// Resolve an evidence-relative input without leaving the repository.
    fn input_from(&self, manifest: &Path, relative: &str) -> Result<PathBuf, QualityError> {
        validate_relative_path(relative)?;
        let parent = manifest
            .parent()
            .ok_or_else(|| QualityError::PathEscape(manifest.to_string_lossy().into_owned()))?;
        let joined = parent.join(relative);
        let canonical = joined.canonicalize().map_err(|source| QualityError::Io {
            operation: "canonicalize manifest-relative input",
            path: joined.clone(),
            source,
        })?;
        if !canonical.starts_with(parent) || !canonical.starts_with(&self.0) {
            return Err(QualityError::PathEscape(relative.to_string()));
        }
        let key = self.relative_key(&canonical)?;
        self.input(&key)
    }

    /// Read and validate the repository HEAD commit.
    fn head_commit(&self) -> Result<String, QualityError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["rev-parse", "--verify", "HEAD"])
            .output()
            .map_err(|source| QualityError::Io {
                operation: "run git rev-parse",
                path: self.0.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(QualityError::Git {
                operation: "rev-parse HEAD",
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        validate_commit(&commit)?;
        Ok(commit)
    }

    /// Accept HEAD or a narrowly evidence-only descendant of the tested commit.
    fn task_commit(&self, expected: &str) -> Result<String, QualityError> {
        validate_commit(expected)?;
        if !self.0.join(".git").exists() {
            return Ok(expected.to_ascii_lowercase());
        }
        let actual = self.head_commit()?;
        if actual.eq_ignore_ascii_case(expected) {
            return Ok(actual);
        }
        let ancestry = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["merge-base", "--is-ancestor", expected, &actual])
            .status()
            .map_err(|source| QualityError::Io {
                operation: "check task evidence ancestry",
                path: self.0.clone(),
                source,
            })?;
        let changed = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["diff", "--name-only", expected, &actual, "--"])
            .output()
            .map_err(|source| QualityError::Io {
                operation: "list task evidence closure paths",
                path: self.0.clone(),
                source,
            })?;
        let metadata_only = ancestry.success()
            && changed.status.success()
            && String::from_utf8_lossy(&changed.stdout)
                .lines()
                .all(task_evidence_metadata_path);
        if !metadata_only {
            return Err(QualityError::Status {
                status: QualityStatus::StaleEvidence,
                message: format!(
                    "task snapshot expected commit {expected}, live repository is {actual}"
                ),
            });
        }
        Ok(expected.to_ascii_lowercase())
    }

    /// List repository paths changed from a validated merge base.
    fn changed_paths(&self, merge_base: &str) -> Result<Vec<String>, QualityError> {
        validate_commit(merge_base)?;
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "diff",
                "--name-only",
                "--diff-filter=ACMRD",
                merge_base,
                "HEAD",
            ])
            .output()
            .map_err(|source| QualityError::Io {
                operation: "run git diff",
                path: self.0.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(QualityError::Git {
                operation: "diff merge base",
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.replace('\\', "/"))
            .collect())
    }
}

/// Return whether a path is narrowly owned by task evidence closure metadata.
fn task_evidence_metadata_path(path: &str) -> bool {
    if path == "openspec/task-evidence.json" {
        return true;
    }
    if let Some(change) = path
        .strip_prefix("openspec/changes/")
        .and_then(|path| path.strip_suffix("/tasks.md"))
    {
        let mut bytes = change.bytes();
        return bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    }
    let Some(name) = path.strip_prefix("docs/benchmarks/results/") else {
        return false;
    };
    name.rsplit('/').next().is_some_and(|file| {
        file.starts_with("task-verification-")
            && Path::new(file)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    })
}

/// Read a UTF-8 text artifact with path-aware errors.
fn read_text(path: &Path) -> Result<String, QualityError> {
    fs::read_to_string(path).map_err(|source| QualityError::Io {
        operation: "read UTF-8 input",
        path: path.to_path_buf(),
        source,
    })
}

/// Deserialize a JSON artifact with path-aware errors.
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, QualityError> {
    let bytes = fs::read(path).map_err(|source| QualityError::Io {
        operation: "read JSON input",
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| QualityError::Json {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Deserialize a TOML artifact with path-aware errors.
fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T, QualityError> {
    let text = read_text(path)?;
    toml::from_str(&text).map_err(|source| QualityError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Read an untyped TOML value for typed sub-schema validation.
fn read_toml_value(path: &Path) -> Result<toml::Value, QualityError> {
    read_toml(path)
}

/// Hash an artifact with SHA-256.
fn digest_file(path: &Path) -> Result<String, QualityError> {
    let bytes = fs::read(path).map_err(|source| QualityError::Io {
        operation: "read digest input",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(hex_digest(&bytes))
}

/// Hash bytes and encode the digest as lowercase hexadecimal.
fn hex_digest(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

/// Encode bytes as canonical lowercase hexadecimal.
fn encode_hex(bytes: &[u8]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

/// Hash sorted covered inputs after normalizing task checkboxes.
fn normalized_covered_inputs_digest(
    root: &RepositoryRoot,
    task: &VerificationTask,
) -> Result<String, QualityError> {
    let inputs = &task.covered_inputs;
    if inputs.is_empty() {
        return Err(QualityError::Policy(vec![
            "covered_inputs must not be empty".to_string(),
        ]));
    }
    let mut records = BTreeMap::new();
    for input in inputs {
        match input.kind {
            CoveredInputKind::File => {
                add_covered_file(root, &root.input(&input.path)?, &mut records)?;
            }
            CoveredInputKind::Tree => {
                let tree = root.tree(&input.path)?;
                let mut pending = vec![tree];
                while let Some(directory) = pending.pop() {
                    let mut entries = fs::read_dir(&directory)
                        .map_err(|source| QualityError::Io {
                            operation: "read covered input tree",
                            path: directory.clone(),
                            source,
                        })?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|source| QualityError::Io {
                            operation: "read covered input tree entry",
                            path: directory.clone(),
                            source,
                        })?;
                    entries.sort_by_key(fs::DirEntry::file_name);
                    for entry in entries {
                        let kind = entry.file_type().map_err(|source| QualityError::Io {
                            operation: "inspect covered input tree entry",
                            path: entry.path(),
                            source,
                        })?;
                        if kind.is_symlink() {
                            return Err(QualityError::PathEscape(
                                entry.path().to_string_lossy().into_owned(),
                            ));
                        }
                        if kind.is_dir() {
                            pending.push(entry.path());
                        } else if kind.is_file() {
                            add_covered_file(root, &entry.path(), &mut records)?;
                        }
                    }
                }
            }
        }
    }
    let projection = VerificationTaskDigest::from(task);
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(&projection)?);
    hasher.update([0]);
    for (key, bytes) in records {
        hasher.update(key.as_bytes());
        hasher.update([0]);
        hasher.update(Sha256::digest(bytes));
        hasher.update(b"\n");
    }
    Ok(encode_hex(&hasher.finalize()))
}

/// Add one confined file to the covered-input digest set.
fn add_covered_file(
    root: &RepositoryRoot,
    path: &Path,
    records: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), QualityError> {
    let key = root.relative_key(path)?;
    let mut bytes = fs::read(path).map_err(|source| QualityError::Io {
        operation: "read covered input",
        path: path.to_path_buf(),
        source,
    })?;
    bytes = normalize_covered_value(&key, bytes);
    if records.insert(key.clone(), bytes).is_some() {
        return Err(QualityError::Policy(vec![format!(
            "duplicate covered input {key}"
        )]));
    }
    Ok(())
}

/// Feed a length-prefixed byte sequence into a digest.
fn hash_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(bytes.len().to_le_bytes());
    hasher.update(bytes);
}

/// Normalize checked `OpenSpec` boxes so closure-only edits do not stale evidence.
fn normalize_task_checkboxes(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.starts_with("- [x] ") || line.starts_with("- [X] ") {
                line.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// Normalize covered metadata and text identically to the `IssueOps` adapter.
fn normalize_covered_value(key: &str, bytes: Vec<u8>) -> Vec<u8> {
    if key == "openspec/task-verification.json" {
        return b"task-plan-entry-normalized-by-covered-input-digest-v1".to_vec();
    }
    if key == "openspec/task-evidence.json" {
        return b"task-evidence-metadata-normalized-by-issueops-v1".to_vec();
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => return error.into_bytes(),
    };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if key.ends_with("/tasks.md") || key == "tasks.md" {
        normalize_task_checkboxes(&text).into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Serialize a digest projection like Python's sorted, ASCII-only compact JSON.
fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, QualityError> {
    let json = serde_json::to_string(value).map_err(|source| QualityError::Json {
        path: PathBuf::from("task verification digest projection"),
        source: Box::new(source),
    })?;
    let mut output = String::with_capacity(json.len());
    for character in json.chars() {
        if character.is_ascii() {
            output.push(character);
            continue;
        }
        let code = u32::from(character);
        if code <= 0xffff {
            push_json_code_unit(&mut output, code);
        } else {
            let supplementary = code - 0x1_0000;
            let high = 0xd800 + (supplementary >> 10);
            let low = 0xdc00 + (supplementary & 0x03ff);
            push_json_code_unit(&mut output, high);
            push_json_code_unit(&mut output, low);
        }
    }
    Ok(output.into_bytes())
}

/// Append one lowercase JSON Unicode escape without allocation.
fn push_json_code_unit(output: &mut String, code: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push('\\');
    output.push('u');
    for shift in [12, 8, 4, 0] {
        let index = usize::try_from((code >> shift) & 0x0f).unwrap_or_default();
        output.push(char::from(HEX[index]));
    }
}

/// Require a lowercase 40-character Git commit identity.
fn validate_commit(value: &str) -> Result<(), QualityError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(QualityError::Evidence(format!(
            "invalid 40-character commit identity {value:?}"
        )));
    }
    Ok(())
}

/// Require a canonical lowercase SHA-256 identity.
fn validate_digest(value: &str, label: &str) -> Result<(), QualityError> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(QualityError::Evidence(format!(
            "{label} must be a SHA-256 digest"
        )));
    }
    Ok(())
}

/// Convert an in-memory count without truncation.
fn usize_to_u64(value: usize) -> Result<u64, QualityError> {
    u64::try_from(value).map_err(|source| QualityError::Evidence(source.to_string()))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Closed set of quality status values accepted by the quality validator.
pub(crate) enum QualityStatus {
    /// Passed validation status.
    Passed,
    /// Missing tool validation status.
    MissingTool,
    /// No tests validation status.
    NoTests,
    /// No mutants validation status.
    NoMutants,
    /// Test failure validation status.
    TestFailure,
    /// Baseline failure validation status.
    BaselineFailure,
    /// Missed mutant validation status.
    MissedMutant,
    /// Mutant timeout validation status.
    MutantTimeout,
    /// Command timeout validation status.
    CommandTimeout,
    /// Job timeout validation status.
    JobTimeout,
    /// Cancelled validation status.
    Cancelled,
    /// Corrupt evidence validation status.
    CorruptEvidence,
    /// Incomplete evidence validation status.
    IncompleteEvidence,
    /// Stale evidence validation status.
    StaleEvidence,
    /// Policy failure validation status.
    PolicyFailure,
    /// Infrastructure failure validation status.
    InfrastructureFailure,
}

impl QualityStatus {
    /// Return the stable process exit code for this status or error.
    fn exit_code(self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::MissingTool => 10,
            Self::NoTests => 11,
            Self::NoMutants => 12,
            Self::TestFailure => 13,
            Self::BaselineFailure => 14,
            Self::MissedMutant => 15,
            Self::MutantTimeout => 16,
            Self::CommandTimeout => 17,
            Self::JobTimeout => 18,
            Self::Cancelled => 19,
            Self::CorruptEvidence => 20,
            Self::IncompleteEvidence => 21,
            Self::StaleEvidence => 22,
            Self::PolicyFailure => 23,
            Self::InfrastructureFailure => 24,
        }
    }
}

#[derive(Debug, Error)]
/// Closed set of quality error values accepted by the quality validator.
pub(crate) enum QualityError {
    #[error("{0}")]
    /// Usage failure reported by the validator.
    Usage(String),
    #[error("{operation} failed for {path}: {source}", path = .path.display())]
    /// Io failure reported by the validator.
    Io {
        /// Filesystem or Git operation that failed.
        operation: &'static str,
        /// Repository-relative path confined by the owning record.
        path: PathBuf,
        #[source]
        /// Underlying error that caused this failure.
        source: io::Error,
    },
    #[error("failed to parse JSON {path}: {source}", path = .path.display())]
    /// Json failure reported by the validator.
    Json {
        /// Repository-relative path confined by the owning record.
        path: PathBuf,
        #[source]
        /// Underlying error that caused this failure.
        source: Box<serde_json::Error>,
    },
    #[error("failed to parse TOML {path}: {source}", path = .path.display())]
    /// Toml failure reported by the validator.
    Toml {
        /// Repository-relative path confined by the owning record.
        path: PathBuf,
        #[source]
        /// Underlying error that caused this failure.
        source: Box<toml::de::Error>,
    },
    #[error("output failed: {0}")]
    /// Output failure reported by the validator.
    Output(#[source] io::Error),
    #[error("wrong repository root: {0}")]
    /// Wrong root failure reported by the validator.
    WrongRoot(String),
    #[error("path escapes the selected repository root: {0}")]
    /// Path escape failure reported by the validator.
    PathEscape(String),
    #[error("git {operation} failed with status {status:?}: {stderr}")]
    /// Git failure reported by the validator.
    Git {
        /// Filesystem or Git operation that failed.
        operation: &'static str,
        /// Exit status returned by the failed Git process.
        status: Option<i32>,
        /// Bounded standard-error output from the failed Git command.
        stderr: String,
    },
    #[error("quality policy failed: {reasons}", reasons = .0.join("; "))]
    /// Policy failure reported by the validator.
    Policy(Vec<String>),
    #[error("evidence failed: {0}")]
    /// Evidence failure reported by the validator.
    Evidence(String),
    #[error("{status:?}: {message}")]
    /// Status failure reported by the validator.
    Status {
        /// Stable quality classification carried by this error.
        status: QualityStatus,
        /// Human-readable failure diagnostic.
        message: String,
    },
}

impl QualityError {
    /// Return the stable process exit code for this status or error.
    pub(crate) fn exit_code(&self) -> u8 {
        if matches!(self, Self::Usage(_)) {
            EXIT_USAGE
        } else {
            self.status().exit_code()
        }
    }

    /// Classify an error into its stable quality status.
    fn status(&self) -> QualityStatus {
        match self {
            Self::Usage(_)
            | Self::Io { .. }
            | Self::Output(_)
            | Self::WrongRoot(_)
            | Self::PathEscape(_)
            | Self::Git { .. } => QualityStatus::InfrastructureFailure,
            Self::Json { .. } | Self::Toml { .. } => QualityStatus::CorruptEvidence,
            Self::Policy(_) => QualityStatus::PolicyFailure,
            Self::Evidence(_) => QualityStatus::IncompleteEvidence,
            Self::Status { status, .. } => *status,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Stable machine-readable summary emitted by every validation command.
struct ValidationSummary {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Command contract or gate command.
    command: String,
    /// Stable result status emitted by the command.
    status: QualityStatus,
    /// Human-readable validation diagnostics.
    diagnostics: Vec<String>,
    /// Stable named counters emitted by validation.
    counts: BTreeMap<String, u64>,
    /// Commit, policy, scope, and artifact identities emitted by validation.
    identities: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional deterministic mutation-shard plan emitted by validation.
    mutation_plan: Option<MutationPlan>,
}

impl ValidationSummary {
    /// Create a successful validation summary.
    fn passed(command: &str) -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            command: command.to_string(),
            status: QualityStatus::Passed,
            diagnostics: Vec::new(),
            counts: BTreeMap::new(),
            identities: BTreeMap::new(),
            mutation_plan: None,
        }
    }

    /// Create a failed validation summary with one diagnostic.
    fn failure(command: &str, status: QualityStatus, message: String) -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            command: command.to_string(),
            status,
            diagnostics: vec![message],
            counts: BTreeMap::new(),
            identities: BTreeMap::new(),
            mutation_plan: None,
        }
    }
}

/// Write a human or JSON validation summary.
fn write_summary(summary: &ValidationSummary, json: bool) -> Result<(), QualityError> {
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, summary).map_err(|source| {
            QualityError::Json {
                path: PathBuf::from("<stdout>"),
                source: Box::new(source),
            }
        })?;
        writeln!(stdout).map_err(QualityError::Output)
    } else {
        writeln!(
            stdout,
            "projectatlas-lints: test-quality {}: {:?}",
            summary.command, summary.status
        )
        .map_err(QualityError::Output)?;
        for diagnostic in &summary.diagnostics {
            writeln!(stdout, "  {diagnostic}").map_err(QualityError::Output)?;
        }
        for (name, value) in &summary.counts {
            writeln!(stdout, "  {name}: {value}").map_err(QualityError::Output)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Complete repository-owned Rust test-quality policy.
struct QualityPolicy {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Stable identifier for the governing quality policy.
    policy_id: String,
    /// Repository identity governed by the policy.
    repository: String,
    /// Release milestone that must satisfy the policy.
    release_milestone: String,
    /// `OpenSpec` changes governed by the policy.
    required_changes: Vec<String>,
    /// Pinned quality-tool versions.
    tools: ToolPins,
    /// Reference Rust, LLVM, and host identity.
    reference_toolchain: ReferenceToolchain,
    /// Owned-source inclusion and exclusion rules.
    scope: ScopePolicy,
    /// Timeouts enforced for this gate or policy.
    timeouts: TimeoutPolicy,
    /// Evidence retention rules.
    retention: RetentionPolicy,
    /// Coverage and mutation release thresholds.
    targets: QualityTargets,
    /// Rules governing threshold claims.
    target_policy: TargetPolicy,
    /// Evidence storage and trust rules.
    evidence: EvidencePolicy,
    /// `IssueOps` trust and write-back rules.
    issueops: IssueOpsPolicy,
    /// Schema constraints for quality exceptions.
    exception_schema: ExceptionSchemaPolicy,
    /// Approved quality exceptions.
    exceptions: ExceptionPolicy,
    /// Pinned historical measurements.
    historical: HistoricalPolicy,
    /// Current non-floor measurements.
    observed: ObservedPolicy,
    /// Supported runner and target policies.
    platforms: Vec<PlatformPolicy>,
    /// Established viable-mutant floor.
    mutation_floor: MutationFloor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "field names intentionally mirror the flat external tool-pin schema"
)]
/// Exact versions required for the quality toolchain.
struct ToolPins {
    /// cargo-nextest version bound to this record.
    cargo_nextest: String,
    /// cargo-llvm-cov version bound to this record.
    cargo_llvm_cov: String,
    /// cargo-mutants version bound to this record.
    cargo_mutants: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Rust compiler, LLVM, and host identity used as the reference environment.
struct ReferenceToolchain {
    /// Rust compiler or toolchain identity.
    rust: String,
    /// LLVM toolchain identity.
    llvm: String,
    /// Host triple or machine identity.
    host: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Owned-source inclusion and exclusion policy.
struct ScopePolicy {
    /// Glob patterns selecting owned Rust source.
    include_globs: Vec<String>,
    /// Explicit source exclusions; required to remain empty by policy.
    exclude_globs: Vec<String>,
    /// Named exclusion categories authorized by policy.
    exclude_categories: Vec<String>,
    /// Whether broad source exclusions are permitted.
    blanket_exclusions_allowed: bool,
    /// Whether raw tool rows must be retained for audit.
    raw_rows_retained: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "the seconds suffix is part of the external timeout schema and preserves units"
)]
/// Command, job, build, and test time limits, all expressed in seconds.
struct TimeoutPolicy {
    /// Nextest test timeout in seconds.
    nextest_test_seconds: u64,
    /// Nextest command timeout in seconds.
    nextest_command_seconds: u64,
    /// Nextest job timeout in seconds.
    nextest_job_seconds: u64,
    /// Doctest command timeout in seconds.
    doctest_command_seconds: u64,
    /// Doctest job timeout in seconds.
    doctest_job_seconds: u64,
    /// Coverage command timeout in seconds.
    coverage_command_seconds: u64,
    /// Coverage job timeout in seconds.
    coverage_job_seconds: u64,
    /// Changed mutant test timeout in seconds.
    changed_mutant_test_seconds: u64,
    /// Changed mutant build timeout in seconds.
    changed_mutant_build_seconds: u64,
    /// Changed mutation command timeout in seconds.
    changed_mutation_command_seconds: u64,
    /// Changed mutation job timeout in seconds.
    changed_mutation_job_seconds: u64,
    /// Inventory command timeout in seconds.
    inventory_command_seconds: u64,
    /// Inventory job timeout in seconds.
    inventory_job_seconds: u64,
    /// Mutation shard test timeout in seconds.
    mutation_shard_test_seconds: u64,
    /// Mutation shard build timeout in seconds.
    mutation_shard_build_seconds: u64,
    /// Mutation shard command timeout in seconds.
    mutation_shard_command_seconds: u64,
    /// Mutation shard job timeout in seconds.
    mutation_shard_job_seconds: u64,
    /// Mutation aggregate command timeout in seconds.
    mutation_aggregate_command_seconds: u64,
    /// Mutation aggregate job timeout in seconds.
    mutation_aggregate_job_seconds: u64,
    /// Release consumption command timeout in seconds.
    release_consumption_command_seconds: u64,
    /// Release consumption job timeout in seconds.
    release_consumption_job_seconds: u64,
}

impl TimeoutPolicy {
    /// Return every timeout name and value for common validation.
    fn values(&self) -> [(&'static str, u64); 21] {
        [
            ("nextest_test_seconds", self.nextest_test_seconds),
            ("nextest_command_seconds", self.nextest_command_seconds),
            ("nextest_job_seconds", self.nextest_job_seconds),
            ("doctest_command_seconds", self.doctest_command_seconds),
            ("doctest_job_seconds", self.doctest_job_seconds),
            ("coverage_command_seconds", self.coverage_command_seconds),
            ("coverage_job_seconds", self.coverage_job_seconds),
            (
                "changed_mutant_test_seconds",
                self.changed_mutant_test_seconds,
            ),
            (
                "changed_mutant_build_seconds",
                self.changed_mutant_build_seconds,
            ),
            (
                "changed_mutation_command_seconds",
                self.changed_mutation_command_seconds,
            ),
            (
                "changed_mutation_job_seconds",
                self.changed_mutation_job_seconds,
            ),
            ("inventory_command_seconds", self.inventory_command_seconds),
            ("inventory_job_seconds", self.inventory_job_seconds),
            (
                "mutation_shard_test_seconds",
                self.mutation_shard_test_seconds,
            ),
            (
                "mutation_shard_build_seconds",
                self.mutation_shard_build_seconds,
            ),
            (
                "mutation_shard_command_seconds",
                self.mutation_shard_command_seconds,
            ),
            (
                "mutation_shard_job_seconds",
                self.mutation_shard_job_seconds,
            ),
            (
                "mutation_aggregate_command_seconds",
                self.mutation_aggregate_command_seconds,
            ),
            (
                "mutation_aggregate_job_seconds",
                self.mutation_aggregate_job_seconds,
            ),
            (
                "release_consumption_command_seconds",
                self.release_consumption_command_seconds,
            ),
            (
                "release_consumption_job_seconds",
                self.release_consumption_job_seconds,
            ),
        ]
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Evidence retention windows used for release decisions.
struct RetentionPolicy {
    /// Number of days ordinary evidence artifacts are retained.
    artifact_days: u64,
    /// Minimum days required to cover the release-decision window.
    release_decision_window_days: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Coverage and mutation thresholds enforced for the release.
struct QualityTargets {
    /// Coverage policy or measurements.
    coverage: CoverageTargets,
    /// Mutation policy or measurements.
    mutation: MutationTargets,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Line, region, and function coverage thresholds.
struct CoverageTargets {
    /// Line coverage threshold or counts.
    lines: CoverageTarget,
    /// Region coverage threshold or counts.
    regions: CoverageTarget,
    /// Function coverage threshold or counts.
    functions: CoverageTarget,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Raw and exception-adjusted threshold for one coverage metric.
struct CoverageTarget {
    /// Raw ratio expressed in basis points.
    raw_basis_points: u16,
    /// Adjusted ratio expressed in basis points.
    adjusted_basis_points: u16,
    /// GitHub issue that owns progress toward this threshold or exception.
    tracking_issue: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Raw and exception-adjusted viable-mutant kill thresholds.
struct MutationTargets {
    /// Raw viable kill ratio expressed in basis points.
    raw_viable_kill_basis_points: u16,
    /// Adjusted viable kill ratio expressed in basis points.
    adjusted_viable_kill_basis_points: u16,
    /// GitHub issue that owns progress toward this threshold or exception.
    tracking_issue: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Rules governing threshold claims and gap waivers.
struct TargetPolicy {
    /// Whether threshold-gap waivers are permitted.
    target_gap_waivers_allowed: bool,
    /// Whether a public completion claim requires complete counts.
    public_complete_claim_requires_complete_counts: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Locations, trusted kinds, and closure rules for retained evidence.
struct EvidencePolicy {
    /// Version identity reported for manifest schema.
    manifest_schema_version: u32,
    /// Repository-relative path to the task verification plan.
    verification_plan: String,
    /// Repository-relative path to the task evidence ledger.
    task_ledger: String,
    /// Repository-relative root for retained quality evidence.
    output_root: String,
    /// Whether task closure may change metadata without invalidating covered-input evidence.
    metadata_only_closure: bool,
    /// Task-file mutations allowed during metadata-only closure.
    closure_task_mutations: Vec<String>,
    /// Evidence-ledger mutations allowed during metadata-only closure.
    closure_ledger_mutations: Vec<String>,
    /// Issue-map mutations allowed during metadata-only closure.
    closure_issue_map_mutations: Vec<String>,
    /// Whether aggregate evidence must be bound to the pull-request HEAD.
    require_pr_head_aggregate: bool,
    /// Retained result kinds eligible to prove task completion.
    trusted_result_kinds: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent trust decisions intentionally mirror the flat IssueOps policy schema"
)]
/// Trust and write-back constraints for `IssueOps` automation.
struct IssueOpsPolicy {
    /// Maximum managed GitHub issue-body size.
    body_character_limit: u64,
    /// Prefix identifying IssueOps-managed body sections.
    managed_marker_prefix: String,
    /// Issue comment scope reserved for retained evidence links.
    evidence_comment_scope: String,
    /// Issue-form field that binds a pull request.
    pull_request_issue_field: String,
    /// Issue-form field that records pull-request scope.
    pull_request_scope_field: String,
    /// Whether `IssueOps` may target a pull request instead of an issue.
    allow_pull_request_target: bool,
    /// Whether `IssueOps` may execute commands supplied by artifacts.
    execute_artifact_commands: bool,
    /// Status reported for trust caller urls or.
    trust_caller_urls_or_status: bool,
    /// Whether `IssueOps` may write evidence back from a fork.
    fork_writeback_allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent exception invariants intentionally mirror the flat policy schema"
)]
/// Required selectors, approvals, expiry, and overlap rules for exceptions.
struct ExceptionSchemaPolicy {
    /// Fields required for every quality exception.
    common_required_fields: Vec<String>,
    /// Fields that identify an exact coverage exception range.
    coverage_selector_fields: Vec<String>,
    /// Fields that identify an exact mutant exception.
    mutation_selector_fields: Vec<String>,
    /// Supported date or release expiry selectors.
    expiry_fields: Vec<String>,
    /// Whether every exception must select an exact source or mutant.
    exact_selector_required: bool,
    /// Whether every exception must expire in the future.
    future_expiry_required: bool,
    /// Whether exactly one date or release expiry is required.
    exactly_one_expiry_required: bool,
    /// Whether coverage exception ranges may overlap.
    overlap_allowed: bool,
    /// Whether an approved exception may remain unused.
    unused_allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Approved quality exceptions carried by the policy.
struct ExceptionPolicy {
    /// Approved exception records.
    records: Vec<QualityException>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
/// Closed set of quality exception values accepted by the quality validator.
enum QualityException {
    /// Coverage case accepted by the quality exception contract.
    Coverage {
        /// Stable identifier for this policy or evidence record.
        id: String,
        /// Repository-relative path confined by the owning record.
        path: String,
        /// Inclusive first line selected by the coverage exception.
        start_line: u64,
        /// Inclusive final line selected by the coverage exception.
        end_line: u64,
        /// Reason category authorizing the exception.
        category: ExceptionCategory,
        /// Human rationale supporting the exception.
        rationale: String,
        /// Maintainer accountable for the exception.
        owner: String,
        /// GitHub issue that owns progress toward this threshold or exception.
        tracking_issue: String,
        /// Release authority that approved the exception.
        approved_by: String,
        /// UTC date on which the exception was approved.
        approved_on: String,
        /// Canonical SHA-256 identity of the selected source file.
        source_sha256: String,
        /// Optional UTC expiry date.
        expires_on: Option<String>,
        /// Optional release milestone expiry.
        expires_release: Option<String>,
    },
    /// Mutation case accepted by the quality exception contract.
    Mutation {
        /// Stable identifier for this policy or evidence record.
        id: String,
        /// Stable identity of the excepted mutant.
        mutant_id: String,
        /// Repository-relative path confined by the owning record.
        path: String,
        /// Reason category authorizing the exception.
        category: ExceptionCategory,
        /// Human rationale supporting the exception.
        rationale: String,
        /// Maintainer accountable for the exception.
        owner: String,
        /// GitHub issue that owns progress toward this threshold or exception.
        tracking_issue: String,
        /// Release authority that approved the exception.
        approved_by: String,
        /// UTC date on which the exception was approved.
        approved_on: String,
        /// Canonical SHA-256 identity of the selected source file.
        source_sha256: String,
        /// Optional UTC expiry date.
        expires_on: Option<String>,
        /// Optional release milestone expiry.
        expires_release: Option<String>,
    },
}

impl QualityException {
    /// Return the stable exception identifier.
    fn id(&self) -> &str {
        match self {
            Self::Coverage { id, .. } | Self::Mutation { id, .. } => id,
        }
    }

    /// Return the repository-relative source selected by an exception.
    fn source_path(&self) -> &str {
        match self {
            Self::Coverage { path, .. } | Self::Mutation { path, .. } => path,
        }
    }

    /// Return the source digest bound to an exception.
    fn source_sha256(&self) -> &str {
        match self {
            Self::Coverage { source_sha256, .. } | Self::Mutation { source_sha256, .. } => {
                source_sha256
            }
        }
    }

    /// Return the optional date and release expiry selectors.
    fn expiry(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::Coverage {
                expires_on,
                expires_release,
                ..
            }
            | Self::Mutation {
                expires_on,
                expires_release,
                ..
            } => (expires_on.as_deref(), expires_release.as_deref()),
        }
    }

    /// Return the approval and rationale fields shared by exception variants.
    fn descriptive_fields(&self) -> (&str, &str, &str, &str, &str) {
        match self {
            Self::Coverage {
                rationale,
                owner,
                tracking_issue,
                approved_by,
                approved_on,
                ..
            }
            | Self::Mutation {
                rationale,
                owner,
                tracking_issue,
                approved_by,
                approved_on,
                ..
            } => (rationale, owner, tracking_issue, approved_by, approved_on),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Closed set of exception category values accepted by the quality validator.
enum ExceptionCategory {
    /// Generated reason for a bounded exception.
    Generated,
    /// Platform unreachable reason for a bounded exception.
    PlatformUnreachable,
    /// Tool limitation reason for a bounded exception.
    ToolLimitation,
    /// Defensive impossible state reason for a bounded exception.
    DefensiveImpossibleState,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Pinned historical evidence that cannot establish a release floor.
struct HistoricalPolicy {
    /// Nextest snapshot for this policy section.
    nextest: HistoricalNextest,
    /// Coverage policy or measurements.
    coverage: HistoricalCoverage,
    /// Mutation inventory snapshot for this policy section.
    mutation_inventory: HistoricalMutationInventory,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Pinned historical nextest inventory metadata.
struct HistoricalNextest {
    /// Git commit bound to the measurement.
    commit: String,
    /// Runner platform bound to the record.
    platform: String,
    /// Number of test records.
    test_count: u64,
    /// Number of suite records.
    suite_count: u64,
    /// Number of ignored records.
    ignored_count: u64,
    /// Whether the measurement may establish a release floor.
    eligible_floor_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Pinned historical coverage metadata and percentages.
struct HistoricalCoverage {
    /// Git commit bound to the measurement.
    commit: String,
    /// Runner platform bound to the record.
    platform: String,
    /// Rust compiler or toolchain identity.
    rust: String,
    /// LLVM toolchain identity.
    llvm: String,
    /// cargo-llvm-cov version bound to this record.
    cargo_llvm_cov: String,
    /// Line percentage reported by the source tool.
    line_percent: f64,
    /// Region percentage reported by the source tool.
    region_percent: f64,
    /// Function percentage reported by the source tool.
    function_percent: f64,
    /// Number of source lines not covered.
    missed_lines: u64,
    /// Whether the measurement may establish a release floor.
    eligible_floor_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Normalized inventory of historical mutation.
struct HistoricalMutationInventory {
    /// Git commit bound to the measurement.
    commit: String,
    /// cargo-mutants version bound to this record.
    cargo_mutants: String,
    /// Total number of records.
    total: u64,
    /// Whether the measurement may establish a release floor.
    eligible_floor_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
    /// Per-package mutant counts.
    packages: MutationPackageCounts,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Current measurements retained for drift visibility but not release claims.
struct ObservedPolicy {
    /// Nextest snapshot for this policy section.
    nextest: ObservedNextest,
    /// Coverage policy or measurements.
    coverage: ObservedCoverage,
    /// Mutation inventory snapshot for this policy section.
    mutation_inventory: ObservedMutationInventory,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Current nextest inventory provenance and counts.
struct ObservedNextest {
    /// Git commit bound to the measurement.
    commit: String,
    /// Runner platform bound to the record.
    platform: String,
    /// UTC timestamp when the measurement was captured.
    observed_at_utc: String,
    /// Number of test records.
    test_count: u64,
    /// Number of suite records.
    suite_count: u64,
    /// Number of ignored records.
    ignored_count: u64,
    /// Executable used to produce the measurement.
    command_executable: String,
    /// Ordered arguments used to produce the measurement.
    command_arguments: Vec<String>,
    /// Repository-relative native nextest inventory artifact.
    inventory_artifact: String,
    /// Canonical SHA-256 identity of the retained artifact.
    artifact_sha256: String,
    /// Whether the measurement qualifies as release evidence.
    eligible_release_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Current LLVM coverage provenance and exact counts.
struct ObservedCoverage {
    /// Git commit bound to the measurement.
    commit: String,
    /// Runner platform bound to the record.
    platform: String,
    /// UTC timestamp when the measurement was captured.
    observed_at_utc: String,
    /// Rust compiler or toolchain identity.
    rust: String,
    /// LLVM toolchain identity.
    llvm: String,
    /// cargo-llvm-cov version bound to this record.
    cargo_llvm_cov: String,
    /// Number of covered lines.
    lines_covered: u64,
    /// Total number of lines.
    lines_total: u64,
    /// Number of covered regions.
    regions_covered: u64,
    /// Total number of regions.
    regions_total: u64,
    /// Number of covered functions.
    functions_covered: u64,
    /// Total number of functions.
    functions_total: u64,
    /// Number of source lines not covered.
    missed_lines: u64,
    /// Line ratio expressed in basis points.
    line_basis_points: u64,
    /// Region ratio expressed in basis points.
    region_basis_points: u64,
    /// Function ratio expressed in basis points.
    function_basis_points: u64,
    /// Executable used to produce the measurement.
    command_executable: String,
    /// Ordered arguments used to produce the measurement.
    command_arguments: Vec<String>,
    /// Repository-relative retained artifact path.
    artifact: String,
    /// Canonical SHA-256 identity of the retained artifact.
    artifact_sha256: String,
    /// Whether the measurement may establish a release floor.
    eligible_floor_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Normalized inventory of observed mutation.
struct ObservedMutationInventory {
    /// Git commit bound to the measurement.
    commit: String,
    /// cargo-mutants version bound to this record.
    cargo_mutants: String,
    /// Runner platform bound to the record.
    platform: String,
    /// UTC timestamp when the measurement was captured.
    observed_at_utc: String,
    /// Total number of records.
    total: u64,
    /// Whether the inventory was captured without filtering.
    raw_unfiltered: bool,
    /// Whether cargo-mutants default call-skipping is enabled.
    skip_calls_defaults: bool,
    /// Executable used to produce the measurement.
    command_executable: String,
    /// Ordered arguments used to produce the measurement.
    command_arguments: Vec<String>,
    /// Repository-relative retained artifact path.
    artifact: String,
    /// Canonical SHA-256 identity of the retained artifact.
    artifact_sha256: String,
    /// Canonical SHA-256 identity of the consumed configuration.
    config_sha256: String,
    /// Whether the measurement may establish a release floor.
    eligible_floor_evidence: bool,
    /// Reason this evidence is or is not eligible for a release floor.
    eligibility_note: String,
    /// Signed difference from the pinned historical mutant count.
    historical_drift: i64,
    /// Reason the observed inventory differs from the historical snapshot.
    drift_reason: String,
    /// Per-package mutant counts.
    packages: MutationPackageCounts,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "package keys intentionally mirror the external per-package inventory schema"
)]
/// Validated counters for mutation package.
struct MutationPackageCounts {
    /// Mutant count for the `projectatlas-cli` crate.
    projectatlas_cli: u64,
    /// Mutant count for the `projectatlas-db` crate.
    projectatlas_db: u64,
    /// Mutant count for the `projectatlas-service` crate.
    projectatlas_service: u64,
    /// Mutant count for the `projectatlas-symbols` crate.
    projectatlas_symbols: u64,
    /// Mutant count for the `projectatlas-core` crate.
    projectatlas_core: u64,
    /// Mutant count for the `projectatlas-fs` crate.
    projectatlas_fs: u64,
    /// Mutant count for the `projectatlas-lints` crate.
    projectatlas_lints: u64,
}

impl MutationPackageCounts {
    /// Return the checked total across per-package mutant counts.
    fn total(&self) -> Option<u64> {
        [
            self.projectatlas_cli,
            self.projectatlas_db,
            self.projectatlas_service,
            self.projectatlas_symbols,
            self.projectatlas_core,
            self.projectatlas_fs,
            self.projectatlas_lints,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Per-platform runner identity and established coverage floors.
struct PlatformPolicy {
    /// Stable identifier for this policy or evidence record.
    id: String,
    /// Runner label used for this platform or task evidence.
    runner: String,
    /// Runner target triple or configured target name.
    target: String,
    /// Whether all coverage metrics have established platform floors.
    coverage_floor_established: bool,
    /// Established minimum for lines covered.
    lines_covered_floor: Option<u64>,
    /// Total number of lines.
    lines_total: Option<u64>,
    /// Established minimum for regions covered.
    regions_covered_floor: Option<u64>,
    /// Total number of regions.
    regions_total: Option<u64>,
    /// Established minimum for functions covered.
    functions_covered_floor: Option<u64>,
    /// Total number of functions.
    functions_total: Option<u64>,
    /// Git commit identity bound to evidence.
    evidence_commit: Option<String>,
    /// Number of covered observed lines.
    observed_lines_covered: Option<u64>,
    /// Total number of observed lines.
    observed_lines_total: Option<u64>,
    /// Number of covered observed regions.
    observed_regions_covered: Option<u64>,
    /// Total number of observed regions.
    observed_regions_total: Option<u64>,
    /// Number of covered observed functions.
    observed_functions_covered: Option<u64>,
    /// Total number of observed functions.
    observed_functions_total: Option<u64>,
    /// Git commit identity bound to observed evidence.
    observed_evidence_commit: Option<String>,
}

impl PlatformPolicy {
    /// Return established coverage floors only when all metrics are present.
    fn coverage_floor(&self) -> Option<CoverageCounts> {
        Some(CoverageCounts {
            lines: MetricCounts::new(self.lines_covered_floor?, self.lines_total?),
            regions: MetricCounts::new(self.regions_covered_floor?, self.regions_total?),
            functions: MetricCounts::new(self.functions_covered_floor?, self.functions_total?),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Established viable-mutant floor and its supporting issue.
struct MutationFloor {
    /// Whether the platform metric has an established floor.
    established: bool,
    /// Raw viable kill ratio expressed in basis points.
    raw_viable_kill_basis_points: u16,
    /// Adjusted viable kill ratio expressed in basis points.
    adjusted_viable_kill_basis_points: u16,
    /// Exact shard count required by the mutation plan.
    required_shards: u8,
    /// Reason supplied by the source tool or retained evidence.
    reason: String,
}

/// Validate policy against its repository contract.
fn validate_policy(root: &RepositoryRoot, policy: &QualityPolicy) -> Result<(), QualityError> {
    let mut errors = Vec::new();
    if policy.schema_version != POLICY_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {POLICY_SCHEMA_VERSION}, found {}",
            policy.schema_version
        ));
    }
    require_nonempty(&mut errors, "policy_id", &policy.policy_id);
    if policy.repository != "styler-ai/ProjectAtlas" || policy.repository.split('/').count() != 2 {
        errors.push("repository must be the canonical styler-ai/ProjectAtlas identity".to_string());
    }
    require_nonempty(&mut errors, "release_milestone", &policy.release_milestone);
    if policy.required_changes.is_empty() {
        errors.push("required_changes must not be empty".to_string());
    }
    for (field, actual, expected) in [
        (
            "tools.cargo_nextest",
            policy.tools.cargo_nextest.as_str(),
            EXPECTED_NEXTEST_VERSION,
        ),
        (
            "tools.cargo_llvm_cov",
            policy.tools.cargo_llvm_cov.as_str(),
            EXPECTED_LLVM_COV_VERSION,
        ),
        (
            "tools.cargo_mutants",
            policy.tools.cargo_mutants.as_str(),
            EXPECTED_MUTANTS_VERSION,
        ),
    ] {
        if actual != expected {
            errors.push(format!("{field} must be {expected}, found {actual}"));
        }
    }
    for (field, value) in [
        ("reference_toolchain.rust", &policy.reference_toolchain.rust),
        ("reference_toolchain.llvm", &policy.reference_toolchain.llvm),
        ("reference_toolchain.host", &policy.reference_toolchain.host),
    ] {
        require_nonempty(&mut errors, field, value);
    }
    if policy.scope.include_globs.is_empty() {
        errors.push("scope.include_globs must not be empty".to_string());
    }
    if !policy.scope.exclude_globs.is_empty() {
        errors
            .push("scope.exclude_globs must remain empty; use exact typed exceptions".to_string());
    }
    if policy.scope.blanket_exclusions_allowed {
        errors.push("scope.blanket_exclusions_allowed must be false".to_string());
    }
    if !policy.scope.raw_rows_retained {
        errors.push("scope.raw_rows_retained must be true".to_string());
    }
    if let Err(error) = build_scope_globs(&policy.scope.include_globs) {
        errors.push(error.to_string());
    }
    if let Err(error) = build_scope_globs(&policy.scope.exclude_globs) {
        errors.push(error.to_string());
    }
    let mut categories = BTreeSet::new();
    for category in &policy.scope.exclude_categories {
        if !categories.insert(category) {
            errors.push(format!("duplicate scope exclusion category {category}"));
        }
        if !["test-only", "generated", "proven-unreachable"].contains(&category.as_str()) {
            errors.push(format!("unsupported scope exclusion category {category}"));
        }
    }
    for (name, seconds) in policy.timeouts.values() {
        if seconds == 0 {
            errors.push(format!("timeouts.{name} must be greater than zero"));
        }
    }
    for (command, job, label) in [
        (
            policy.timeouts.nextest_command_seconds,
            policy.timeouts.nextest_job_seconds,
            "nextest",
        ),
        (
            policy.timeouts.doctest_command_seconds,
            policy.timeouts.doctest_job_seconds,
            "doctest",
        ),
        (
            policy.timeouts.coverage_command_seconds,
            policy.timeouts.coverage_job_seconds,
            "coverage",
        ),
        (
            policy.timeouts.changed_mutation_command_seconds,
            policy.timeouts.changed_mutation_job_seconds,
            "changed mutation",
        ),
        (
            policy.timeouts.inventory_command_seconds,
            policy.timeouts.inventory_job_seconds,
            "inventory",
        ),
        (
            policy.timeouts.mutation_shard_command_seconds,
            policy.timeouts.mutation_shard_job_seconds,
            "mutation shard",
        ),
        (
            policy.timeouts.mutation_aggregate_command_seconds,
            policy.timeouts.mutation_aggregate_job_seconds,
            "mutation aggregate",
        ),
        (
            policy.timeouts.release_consumption_command_seconds,
            policy.timeouts.release_consumption_job_seconds,
            "release consumption",
        ),
    ] {
        if command > job {
            errors.push(format!("{label} command timeout exceeds job timeout"));
        }
    }
    if policy.retention.artifact_days < policy.retention.release_decision_window_days
        || policy.retention.release_decision_window_days == 0
    {
        errors.push("artifact retention must cover a nonzero release decision window".to_string());
    }
    for (name, target) in [
        ("coverage.lines", &policy.targets.coverage.lines),
        ("coverage.regions", &policy.targets.coverage.regions),
        ("coverage.functions", &policy.targets.coverage.functions),
    ] {
        validate_target(&mut errors, name, target);
    }
    for (name, value) in [
        (
            "targets.mutation.raw_viable_kill_basis_points",
            policy.targets.mutation.raw_viable_kill_basis_points,
        ),
        (
            "targets.mutation.adjusted_viable_kill_basis_points",
            policy.targets.mutation.adjusted_viable_kill_basis_points,
        ),
    ] {
        validate_basis_points(&mut errors, name, value, false);
    }
    if policy.targets.mutation.tracking_issue == 0 {
        errors.push("targets.mutation.tracking_issue must be nonzero".to_string());
    }
    if policy.target_policy.target_gap_waivers_allowed
        || !policy
            .target_policy
            .public_complete_claim_requires_complete_counts
    {
        errors.push(
            "target_policy must prohibit gap waivers and require complete counts".to_string(),
        );
    }
    validate_evidence_policy(&mut errors, &policy.evidence);
    validate_issueops_policy(&mut errors, &policy.issueops);
    validate_exception_schema(&mut errors, &policy.exception_schema);
    validate_historical_policy(&mut errors, &policy.historical);
    validate_observed_policy(&mut errors, &policy.observed);
    validate_platforms(&mut errors, &policy.platforms);
    if policy.mutation_floor.required_shards != REQUIRED_MUTATION_SHARDS {
        errors.push(format!(
            "mutation_floor.required_shards must be {REQUIRED_MUTATION_SHARDS}"
        ));
    }
    validate_basis_points(
        &mut errors,
        "mutation_floor.raw_viable_kill_basis_points",
        policy.mutation_floor.raw_viable_kill_basis_points,
        !policy.mutation_floor.established,
    );
    validate_basis_points(
        &mut errors,
        "mutation_floor.adjusted_viable_kill_basis_points",
        policy.mutation_floor.adjusted_viable_kill_basis_points,
        !policy.mutation_floor.established,
    );
    require_nonempty(
        &mut errors,
        "mutation_floor.reason",
        &policy.mutation_floor.reason,
    );
    validate_exceptions(
        root,
        &policy.release_milestone,
        &policy.exceptions.records,
        &mut errors,
    )?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(QualityError::Policy(errors))
    }
}

/// Require nonempty and record a policy diagnostic when absent.
fn require_nonempty(errors: &mut Vec<String>, field: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

/// Validate target against its repository contract.
fn validate_target(errors: &mut Vec<String>, name: &str, target: &CoverageTarget) {
    validate_basis_points(
        errors,
        &format!("targets.{name}.raw_basis_points"),
        target.raw_basis_points,
        false,
    );
    validate_basis_points(
        errors,
        &format!("targets.{name}.adjusted_basis_points"),
        target.adjusted_basis_points,
        false,
    );
    if target.tracking_issue == 0 {
        errors.push(format!("targets.{name}.tracking_issue must be nonzero"));
    }
}

/// Validate basis points against its repository contract.
fn validate_basis_points(errors: &mut Vec<String>, name: &str, value: u16, zero_ok: bool) {
    if value > 10_000 || (!zero_ok && value == 0) {
        errors.push(format!("{name} must be within 1..=10000 basis points"));
    }
}

/// Validate evidence policy against its repository contract.
fn validate_evidence_policy(errors: &mut Vec<String>, policy: &EvidencePolicy) {
    if policy.manifest_schema_version != EVIDENCE_SCHEMA_VERSION {
        errors.push(format!(
            "evidence.manifest_schema_version must be {EVIDENCE_SCHEMA_VERSION}"
        ));
    }
    for (field, value) in [
        ("verification_plan", policy.verification_plan.as_str()),
        ("task_ledger", policy.task_ledger.as_str()),
        ("output_root", policy.output_root.as_str()),
    ] {
        require_nonempty(errors, &format!("evidence.{field}"), value);
        if Path::new(value).is_absolute() || value.contains("..") {
            errors.push(format!("evidence.{field} must be repository-relative"));
        }
    }
    if !policy.metadata_only_closure || !policy.require_pr_head_aggregate {
        errors.push(
            "evidence must require metadata-only closure and PR-head aggregate gates".to_string(),
        );
    }
    if policy.closure_task_mutations != ["checkbox-state-only"]
        || policy.closure_ledger_mutations != ["current-evidence-pointer-only"]
        || !policy.closure_issue_map_mutations.is_empty()
    {
        errors.push(
            "evidence metadata-only closure mutations are not the exact safe set".to_string(),
        );
    }
    let trusted = policy
        .trusted_result_kinds
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if trusted != BTreeSet::from(["github-actions", "repository-retained-local"]) {
        errors.push(
            "evidence.trusted_result_kinds must contain only the two trusted kinds".to_string(),
        );
    }
}

/// Validate issueops policy against its repository contract.
fn validate_issueops_policy(errors: &mut Vec<String>, policy: &IssueOpsPolicy) {
    if policy.body_character_limit != 65_536 {
        errors.push("issueops.body_character_limit must be 65536".to_string());
    }
    for (field, value) in [
        (
            "managed_marker_prefix",
            policy.managed_marker_prefix.as_str(),
        ),
        (
            "evidence_comment_scope",
            policy.evidence_comment_scope.as_str(),
        ),
        (
            "pull_request_issue_field",
            policy.pull_request_issue_field.as_str(),
        ),
        (
            "pull_request_scope_field",
            policy.pull_request_scope_field.as_str(),
        ),
    ] {
        require_nonempty(errors, &format!("issueops.{field}"), value);
    }
    if policy.allow_pull_request_target
        || policy.execute_artifact_commands
        || policy.trust_caller_urls_or_status
        || policy.fork_writeback_allowed
    {
        errors.push("IssueOps trust-boundary flags must all be false".to_string());
    }
}

/// Validate exception schema against its repository contract.
fn validate_exception_schema(errors: &mut Vec<String>, policy: &ExceptionSchemaPolicy) {
    let exact = |actual: &[String], expected: &[&str]| {
        actual.iter().map(String::as_str).collect::<BTreeSet<_>>()
            == expected.iter().copied().collect::<BTreeSet<_>>()
            && actual.len() == expected.len()
    };
    if !exact(
        &policy.common_required_fields,
        &[
            "id",
            "category",
            "rationale",
            "owner",
            "tracking_issue",
            "approved_by",
            "approved_on",
            "source_sha256",
        ],
    ) || !exact(
        &policy.coverage_selector_fields,
        &["path", "start_line", "end_line"],
    ) || !exact(&policy.mutation_selector_fields, &["mutant_id", "path"])
        || !exact(&policy.expiry_fields, &["expires_on", "expires_release"])
    {
        errors.push(
            "exception_schema field declarations do not match the typed variants".to_string(),
        );
    }
    if !policy.exact_selector_required
        || !policy.future_expiry_required
        || !policy.exactly_one_expiry_required
        || policy.overlap_allowed
        || policy.unused_allowed
    {
        errors.push("exception_schema safety flags are invalid".to_string());
    }
}

/// Validate historical policy against its repository contract.
fn validate_historical_policy(errors: &mut Vec<String>, policy: &HistoricalPolicy) {
    for commit in [
        &policy.nextest.commit,
        &policy.coverage.commit,
        &policy.mutation_inventory.commit,
    ] {
        if validate_commit(commit).is_err() {
            errors.push(format!("invalid historical commit {commit:?}"));
        }
    }
    if policy.nextest.test_count != 286
        || policy.nextest.suite_count != 9
        || policy.nextest.ignored_count != 0
        || policy.nextest.eligible_floor_evidence
    {
        errors.push("historical nextest snapshot must remain 286/9/0 and ineligible".to_string());
    }
    require_nonempty(
        errors,
        "historical.nextest.platform",
        &policy.nextest.platform,
    );
    require_nonempty(
        errors,
        "historical.nextest.eligibility_note",
        &policy.nextest.eligibility_note,
    );
    let coverage = &policy.coverage;
    if !coverage.line_percent.is_finite()
        || !coverage.region_percent.is_finite()
        || !coverage.function_percent.is_finite()
        || coverage.line_percent.to_bits() != 87.75_f64.to_bits()
        || coverage.region_percent.to_bits() != 84.90_f64.to_bits()
        || coverage.function_percent.to_bits() != 86.28_f64.to_bits()
        || coverage.missed_lines != 3_369
        || coverage.eligible_floor_evidence
    {
        errors.push("historical coverage snapshot or non-eligibility changed".to_string());
    }
    for (field, value) in [
        ("platform", coverage.platform.as_str()),
        ("rust", coverage.rust.as_str()),
        ("llvm", coverage.llvm.as_str()),
        ("cargo_llvm_cov", coverage.cargo_llvm_cov.as_str()),
        ("eligibility_note", coverage.eligibility_note.as_str()),
    ] {
        require_nonempty(errors, &format!("historical.coverage.{field}"), value);
    }
    let mutation = &policy.mutation_inventory;
    if mutation.total != 4_911
        || mutation.packages.total() != Some(mutation.total)
        || mutation.eligible_floor_evidence
        || mutation.cargo_mutants != EXPECTED_MUTANTS_VERSION
    {
        errors.push("historical mutation snapshot is inconsistent".to_string());
    }
    require_nonempty(
        errors,
        "historical.mutation_inventory.eligibility_note",
        &mutation.eligibility_note,
    );
}

/// Validate observed policy against its repository contract.
fn validate_observed_policy(errors: &mut Vec<String>, policy: &ObservedPolicy) {
    for commit in [
        &policy.nextest.commit,
        &policy.coverage.commit,
        &policy.mutation_inventory.commit,
    ] {
        if validate_commit(commit).is_err() {
            errors.push(format!("invalid observed commit {commit:?}"));
        }
    }
    if policy.nextest.test_count == 0 || policy.nextest.suite_count == 0 {
        errors.push("observed nextest inventory must not be empty".to_string());
    }
    for (field, value) in [
        ("platform", policy.nextest.platform.as_str()),
        ("observed_at_utc", policy.nextest.observed_at_utc.as_str()),
        (
            "command_executable",
            policy.nextest.command_executable.as_str(),
        ),
        (
            "inventory_artifact",
            policy.nextest.inventory_artifact.as_str(),
        ),
        ("eligibility_note", policy.nextest.eligibility_note.as_str()),
    ] {
        require_nonempty(errors, &format!("observed.nextest.{field}"), value);
    }
    if policy.nextest.ignored_count != 0
        || policy.nextest.command_arguments.is_empty()
        || policy.nextest.eligible_release_evidence
    {
        errors.push("observed nextest inventory metadata is invalid".to_string());
    }
    if validate_digest(&policy.nextest.artifact_sha256, "observed nextest artifact").is_err() {
        errors.push("observed nextest artifact digest is invalid".to_string());
    }
    let coverage = &policy.coverage;
    let coverage_counts = CoverageCounts {
        lines: MetricCounts::new(coverage.lines_covered, coverage.lines_total),
        regions: MetricCounts::new(coverage.regions_covered, coverage.regions_total),
        functions: MetricCounts::new(coverage.functions_covered, coverage.functions_total),
    };
    for metric in [
        coverage_counts.lines,
        coverage_counts.regions,
        coverage_counts.functions,
    ] {
        if metric.validate().is_err() {
            errors.push("observed coverage counts are invalid".to_string());
        }
    }
    for (field, value) in [
        ("platform", coverage.platform.as_str()),
        ("observed_at_utc", coverage.observed_at_utc.as_str()),
        ("rust", coverage.rust.as_str()),
        ("llvm", coverage.llvm.as_str()),
        ("cargo_llvm_cov", coverage.cargo_llvm_cov.as_str()),
        ("command_executable", coverage.command_executable.as_str()),
        ("artifact", coverage.artifact.as_str()),
        ("eligibility_note", coverage.eligibility_note.as_str()),
    ] {
        require_nonempty(errors, &format!("observed.coverage.{field}"), value);
    }
    if coverage.command_arguments.is_empty()
        || coverage.eligible_floor_evidence
        || coverage.lines_total.saturating_sub(coverage.lines_covered) != coverage.missed_lines
        || coverage_counts.lines.basis_points().ok() != Some(coverage.line_basis_points)
        || coverage_counts.regions.basis_points().ok() != Some(coverage.region_basis_points)
        || coverage_counts.functions.basis_points().ok() != Some(coverage.function_basis_points)
    {
        errors.push("observed coverage derived counts or provenance are inconsistent".to_string());
    }
    if validate_digest(&coverage.artifact_sha256, "observed coverage artifact").is_err() {
        errors.push("observed coverage artifact digest is invalid".to_string());
    }
    let mutation = &policy.mutation_inventory;
    if mutation.total == 0
        || mutation.packages.total() != Some(mutation.total)
        || !mutation.raw_unfiltered
        || mutation.skip_calls_defaults
        || mutation.cargo_mutants != EXPECTED_MUTANTS_VERSION
        || mutation.command_arguments.is_empty()
        || mutation.eligible_floor_evidence
    {
        errors.push("observed mutation inventory is not raw or internally consistent".to_string());
    }
    if i128::from(mutation.total) - i128::from(HISTORICAL_MUTATION_BASELINE)
        != i128::from(mutation.historical_drift)
    {
        errors.push("observed mutation drift arithmetic is inconsistent".to_string());
    }
    require_nonempty(
        errors,
        "observed.mutation_inventory.artifact",
        &mutation.artifact,
    );
    for (field, value) in [
        ("platform", mutation.platform.as_str()),
        ("observed_at_utc", mutation.observed_at_utc.as_str()),
        ("command_executable", mutation.command_executable.as_str()),
        ("eligibility_note", mutation.eligibility_note.as_str()),
    ] {
        require_nonempty(
            errors,
            &format!("observed.mutation_inventory.{field}"),
            value,
        );
    }
    require_nonempty(
        errors,
        "observed.mutation_inventory.drift_reason",
        &mutation.drift_reason,
    );
    if validate_digest(&mutation.artifact_sha256, "observed mutation artifact").is_err() {
        errors.push("observed mutation artifact digest is invalid".to_string());
    }
    if validate_digest(&mutation.config_sha256, "observed mutation config").is_err() {
        errors.push("observed mutation config digest is invalid".to_string());
    }
}

/// Validate platforms against its repository contract.
fn validate_platforms(errors: &mut Vec<String>, platforms: &[PlatformPolicy]) {
    if platforms.is_empty() {
        errors.push("platforms must not be empty".to_string());
        return;
    }
    let mut ids = BTreeSet::new();
    for platform in platforms {
        if !ids.insert(&platform.id) {
            errors.push(format!("duplicate platform id {}", platform.id));
        }
        require_nonempty(errors, "platform.runner", &platform.runner);
        require_nonempty(errors, "platform.target", &platform.target);
        if platform.coverage_floor_established {
            match platform.coverage_floor() {
                Some(floor) => {
                    if floor.validate().is_err() {
                        errors.push(format!(
                            "platform {} has invalid coverage floor",
                            platform.id
                        ));
                    }
                }
                None => errors.push(format!(
                    "platform {} has an incomplete established coverage floor",
                    platform.id
                )),
            }
            if platform
                .evidence_commit
                .as_deref()
                .is_none_or(|commit| validate_commit(commit).is_err())
            {
                errors.push(format!(
                    "platform {} lacks a valid floor evidence commit",
                    platform.id
                ));
            }
        } else if platform.coverage_floor().is_some() || platform.evidence_commit.is_some() {
            errors.push(format!(
                "platform {} has placeholder floor data without established evidence",
                platform.id
            ));
        }
        let observed = match (
            platform.observed_lines_covered,
            platform.observed_lines_total,
            platform.observed_regions_covered,
            platform.observed_regions_total,
            platform.observed_functions_covered,
            platform.observed_functions_total,
            platform.observed_evidence_commit.as_deref(),
        ) {
            (
                Some(lines_covered),
                Some(lines_total),
                Some(regions_covered),
                Some(regions_total),
                Some(functions_covered),
                Some(functions_total),
                Some(commit),
            ) => {
                if validate_commit(commit).is_err()
                    || (CoverageCounts {
                        lines: MetricCounts::new(lines_covered, lines_total),
                        regions: MetricCounts::new(regions_covered, regions_total),
                        functions: MetricCounts::new(functions_covered, functions_total),
                    })
                    .validate()
                    .is_err()
                {
                    errors.push(format!(
                        "platform {} has invalid observed coverage",
                        platform.id
                    ));
                }
                true
            }
            (None, None, None, None, None, None, None) => false,
            _ => {
                errors.push(format!(
                    "platform {} has incomplete observed coverage",
                    platform.id
                ));
                false
            }
        };
        if observed && platform.coverage_floor_established {
            errors.push(format!(
                "platform {} must distinguish observations from established floors",
                platform.id
            ));
        }
    }
}

/// Validate exceptions against its repository contract.
fn validate_exceptions(
    root: &RepositoryRoot,
    release_milestone: &str,
    exceptions: &[QualityException],
    errors: &mut Vec<String>,
) -> Result<(), QualityError> {
    let today = utc_date_from_system_time()?;
    let mut ids = BTreeSet::new();
    let mut coverage_ranges: BTreeMap<&str, Vec<(u64, u64)>> = BTreeMap::new();
    for exception in exceptions {
        if !ids.insert(exception.id()) {
            errors.push(format!("duplicate exception id {}", exception.id()));
        }
        let (rationale, owner, issue, approved_by, approved_on) = exception.descriptive_fields();
        for (field, value) in [
            ("rationale", rationale),
            ("owner", owner),
            ("tracking_issue", issue),
            ("approved_by", approved_by),
            ("approved_on", approved_on),
        ] {
            require_nonempty(errors, &format!("exception.{field}"), value);
        }
        if parse_date(approved_on).is_none() {
            errors.push(format!(
                "exception {} has invalid approval date",
                exception.id()
            ));
        }
        match exception.expiry() {
            (Some(expires_on), None) => match parse_date(expires_on) {
                Some(expiry) if expiry > today => {}
                Some(_) => errors.push(format!("exception {} is expired", exception.id())),
                None => errors.push(format!(
                    "exception {} has invalid expiry date",
                    exception.id()
                )),
            },
            (None, Some(expires_release)) => {
                require_nonempty(errors, "exception.expires_release", expires_release);
                if expires_release <= release_milestone {
                    errors.push(format!(
                        "exception {} does not expire after the current release",
                        exception.id()
                    ));
                }
            }
            _ => errors.push(format!(
                "exception {} must declare exactly one expiry",
                exception.id()
            )),
        }
        let path = match root.input(exception.source_path()) {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!("exception {} source: {error}", exception.id()));
                continue;
            }
        };
        if digest_file(&path)? != exception.source_sha256().to_ascii_lowercase() {
            errors.push(format!(
                "exception {} source identity is stale",
                exception.id()
            ));
        }
        match exception {
            QualityException::Coverage {
                path,
                start_line,
                end_line,
                ..
            } => {
                if *start_line == 0 || start_line > end_line {
                    errors.push(format!(
                        "exception {} has an invalid line range",
                        exception.id()
                    ));
                }
                coverage_ranges
                    .entry(path)
                    .or_default()
                    .push((*start_line, *end_line));
            }
            QualityException::Mutation { mutant_id, .. } => {
                if validate_digest(mutant_id, "mutation exception selector").is_err() {
                    errors.push(format!(
                        "exception {} has an invalid mutant selector",
                        exception.id()
                    ));
                }
            }
        }
    }
    for (path, mut ranges) in coverage_ranges {
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if pair[0].1 >= pair[1].0 {
                errors.push(format!("coverage exceptions overlap for {path}"));
            }
        }
    }
    Ok(())
}

/// Validate policy ratchet against its repository contract.
fn validate_policy_ratchet(
    base: &QualityPolicy,
    current: &QualityPolicy,
) -> Result<(), QualityError> {
    let mut errors = Vec::new();
    if current.repository != base.repository {
        errors.push("repository identity changed".to_string());
    }
    for (name, base_target, current_target) in [
        (
            "lines",
            &base.targets.coverage.lines,
            &current.targets.coverage.lines,
        ),
        (
            "regions",
            &base.targets.coverage.regions,
            &current.targets.coverage.regions,
        ),
        (
            "functions",
            &base.targets.coverage.functions,
            &current.targets.coverage.functions,
        ),
    ] {
        if current_target.raw_basis_points < base_target.raw_basis_points
            || current_target.adjusted_basis_points < base_target.adjusted_basis_points
        {
            errors.push(format!("coverage target {name} was lowered"));
        }
    }
    if current.targets.mutation.raw_viable_kill_basis_points
        < base.targets.mutation.raw_viable_kill_basis_points
        || current.targets.mutation.adjusted_viable_kill_basis_points
            < base.targets.mutation.adjusted_viable_kill_basis_points
    {
        errors.push("mutation target was lowered".to_string());
    }
    if current.mutation_floor.raw_viable_kill_basis_points
        < base.mutation_floor.raw_viable_kill_basis_points
        || current.mutation_floor.adjusted_viable_kill_basis_points
            < base.mutation_floor.adjusted_viable_kill_basis_points
    {
        errors.push("mutation floor was lowered".to_string());
    }
    let current_scope = current.scope.include_globs.iter().collect::<BTreeSet<_>>();
    for pattern in &base.scope.include_globs {
        if !current_scope.contains(pattern) {
            errors.push(format!("owned source scope lost include glob {pattern}"));
        }
    }
    let base_exceptions = base
        .exceptions
        .records
        .iter()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    for record in &current.exceptions.records {
        if let Some(base_record) = base_exceptions.get(record.id())
            && *base_record != record
        {
            errors.push(format!(
                "existing exception {} changed reach or identity",
                record.id()
            ));
        }
    }
    let base_platforms = base
        .platforms
        .iter()
        .map(|platform| (platform.id.as_str(), platform))
        .collect::<BTreeMap<_, _>>();
    for platform in &current.platforms {
        let Some(base_platform) = base_platforms.get(platform.id.as_str()) else {
            continue;
        };
        if base_platform.coverage_floor_established && !platform.coverage_floor_established {
            errors.push(format!(
                "platform {} removed its established floor",
                platform.id
            ));
            continue;
        }
        if let (Some(base_floor), Some(current_floor)) =
            (base_platform.coverage_floor(), platform.coverage_floor())
            && !current_floor.at_least(&base_floor)
        {
            errors.push(format!("platform {} lowered a coverage floor", platform.id));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(QualityError::Policy(errors))
    }
}

/// Build scope globs from validated inputs.
fn build_scope_globs(patterns: &[String]) -> Result<GlobSet, QualityError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|source| {
            QualityError::Policy(vec![format!(
                "invalid owned source glob {pattern:?}: {source}"
            )])
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|source| QualityError::Policy(vec![format!("invalid scope globs: {source}")]))
}

/// Return whether a repository path is included by the owned-source policy.
fn path_is_applicable(policy: &QualityPolicy, path: &str) -> Result<bool, QualityError> {
    Ok(build_scope_globs(&policy.scope.include_globs)?.is_match(path))
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Covered and total counts for one coverage metric.
struct MetricCounts {
    /// Covered item count.
    covered: u64,
    /// Total eligible item count.
    total: u64,
}

impl MetricCounts {
    /// Create metric counts from covered and total values.
    const fn new(covered: u64, total: u64) -> Self {
        Self { covered, total }
    }

    /// Reject impossible covered and total metric counts.
    fn validate(self) -> Result<(), QualityError> {
        if self.total == 0 || self.covered > self.total {
            return Err(QualityError::Evidence(format!(
                "invalid metric counts {}/{}",
                self.covered, self.total
            )));
        }
        Ok(())
    }

    /// Return whether the metric meets a basis-point threshold.
    fn meets(self, basis_points: u16) -> bool {
        u128::from(self.covered) * 10_000 >= u128::from(self.total) * u128::from(basis_points)
    }

    /// Return the component-wise maximum of two metric-count records.
    fn at_least(self, floor: Self) -> bool {
        u128::from(self.covered) * u128::from(floor.total)
            >= u128::from(floor.covered) * u128::from(self.total)
    }

    /// Compute the covered ratio in basis points without floating-point drift.
    fn basis_points(self) -> Result<u64, QualityError> {
        self.validate()?;
        let numerator = u128::from(self.covered) * 10_000 + u128::from(self.total) / 2;
        u64::try_from(numerator / u128::from(self.total))
            .map_err(|source| QualityError::Evidence(source.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Line, region, and function coverage counts.
struct CoverageCounts {
    /// Line coverage threshold or counts.
    lines: MetricCounts,
    /// Region coverage threshold or counts.
    regions: MetricCounts,
    /// Function coverage threshold or counts.
    functions: MetricCounts,
}

impl CoverageCounts {
    /// Add two coverage records without overflowing any counter.
    fn checked_add(&mut self, other: Self) -> Result<(), QualityError> {
        self.lines.covered = self
            .lines
            .covered
            .checked_add(other.lines.covered)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        self.lines.total = self
            .lines
            .total
            .checked_add(other.lines.total)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        self.regions.covered = self
            .regions
            .covered
            .checked_add(other.regions.covered)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        self.regions.total = self
            .regions
            .total
            .checked_add(other.regions.total)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        self.functions.covered = self
            .functions
            .covered
            .checked_add(other.functions.covered)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        self.functions.total = self
            .functions
            .total
            .checked_add(other.functions.total)
            .ok_or_else(|| QualityError::Evidence("coverage count overflow".to_string()))?;
        Ok(())
    }

    /// Reject impossible covered and total metric counts.
    fn validate(self) -> Result<(), QualityError> {
        self.lines.validate()?;
        self.regions.validate()?;
        self.functions.validate()
    }

    /// Return the component-wise maximum of two metric-count records.
    fn at_least(self, floor: &Self) -> bool {
        self.lines.at_least(floor.lines)
            && self.regions.at_least(floor.regions)
            && self.functions.at_least(floor.functions)
    }

    /// Add these counters to the stable validation summary.
    fn insert_summary(self, counts: &mut BTreeMap<String, u64>) {
        for (name, metric) in [
            ("lines", self.lines),
            ("regions", self.regions),
            ("functions", self.functions),
        ] {
            counts.insert(format!("{name}_covered"), metric.covered);
            counts.insert(format!("{name}_total"), metric.total);
            if let Ok(value) = metric.basis_points() {
                counts.insert(format!("{name}_basis_points"), value);
            }
        }
    }
}

/// Return the current UTC civil date from the system clock.
fn utc_date_from_system_time() -> Result<(i32, u8, u8), QualityError> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|source| QualityError::Evidence(source.to_string()))?
        .as_secs();
    let days = i64::try_from(seconds / 86_400)
        .map_err(|source| QualityError::Evidence(source.to_string()))?;
    Ok(civil_from_days(days))
}

/// Convert days since the Unix epoch into a Gregorian civil date.
fn civil_from_days(days_since_epoch: i64) -> (i32, u8, u8) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u8::try_from(month).unwrap_or(u8::MAX),
        u8::try_from(day).unwrap_or(u8::MAX),
    )
}

/// Parse date from retained evidence.
fn parse_date(value: &str) -> Option<(i32, u8, u8)> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() || year < 1970 || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=max_day).contains(&day).then_some((year, month, day))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Typed subset of nextest configuration enforced by the validator.
struct NextestConfig {
    /// Version identity reported for nextest.
    nextest_version: NextestVersion,
    /// Native nextest metadata-storage policy.
    store: NextestStore,
    /// Named nextest or mutation profile.
    profile: NextestProfiles,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Required nextest metadata version.
struct NextestVersion {
    /// Required nextest action retained for diagnostics.
    required: String,
    /// Recommended nextest action retained for diagnostics.
    recommended: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Nextest on-disk metadata policy.
struct NextestStore {
    /// Directory used by nextest for persisted metadata.
    dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Named nextest profiles enforced by the validator.
struct NextestProfiles {
    /// Default nextest profile.
    default: NextestDefaultProfile,
    /// CI nextest profile.
    ci: NextestCiProfile,
    /// nextest profile used by cargo-mutants.
    mutants: NextestMutantsProfile,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Retry, failure, and timeout rules for the default nextest profile.
struct NextestDefaultProfile {
    /// Number of retries allowed by the nextest profile.
    retries: u64,
    /// Flaky-test result reported by nextest.
    flaky_result: String,
    /// Whether the profile stops after the first failure.
    fail_fast: bool,
    /// Nextest suite status severity.
    status_level: String,
    /// Final nextest status severity retained for reconciliation.
    final_status_level: String,
    /// Slow timeout configuration.
    slow_timeout: NextestSlowTimeout,
    /// Global timeout configuration.
    global_timeout: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Slow-test threshold and termination policy.
struct NextestSlowTimeout {
    /// Slow-test observation period.
    period: String,
    /// Number of slow periods before nextest terminates a test.
    terminate_after: u64,
    /// Grace period before nextest terminates a slow test.
    grace_period: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// CI nextest profile and `JUnit` output policy.
struct NextestCiProfile {
    /// Parent nextest profile inherited by this profile.
    inherits: String,
    /// Whether the profile stops after the first failure.
    fail_fast: bool,
    /// Global timeout configuration.
    global_timeout: String,
    /// `JUnit` output configuration for the CI profile.
    junit: NextestJunitConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Deserialized configuration contract for nextest junit.
struct NextestJunitConfig {
    /// Repository-relative path confined by the owning record.
    path: String,
    /// Stable name embedded in the nextest `JUnit` report.
    report_name: String,
    /// Whether nextest stores successful test output.
    store_success_output: bool,
    /// Whether nextest stores failed test output.
    store_failure_output: bool,
    /// Status reported for flaky fail.
    flaky_fail_status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Nextest profile used by cargo-mutants.
struct NextestMutantsProfile {
    /// Parent nextest profile inherited by this profile.
    inherits: String,
    /// Whether the profile stops after the first failure.
    fail_fast: bool,
    /// Global timeout configuration.
    global_timeout: String,
}

/// Validate nextest config against its repository contract.
fn validate_nextest_config(value: &toml::Value) -> Result<(), QualityError> {
    let config: NextestConfig = value.clone().try_into().map_err(|source| {
        QualityError::Policy(vec![format!("invalid nextest configuration: {source}")])
    })?;
    let valid = config.nextest_version.required == EXPECTED_NEXTEST_VERSION
        && config.nextest_version.recommended == EXPECTED_NEXTEST_VERSION
        && config.store.dir == "target/nextest"
        && config.profile.default.retries == 0
        && config.profile.default.flaky_result == "fail"
        && config.profile.default.fail_fast
        && config.profile.default.status_level == "fail"
        && config.profile.default.final_status_level == "pass"
        && config.profile.default.slow_timeout.period == "120s"
        && config.profile.default.slow_timeout.terminate_after == 1
        && !config.profile.default.slow_timeout.grace_period.is_empty()
        && !config.profile.default.global_timeout.is_empty()
        && config.profile.ci.inherits == "default"
        && !config.profile.ci.fail_fast
        && !config.profile.ci.global_timeout.is_empty()
        && config.profile.ci.junit.path == "junit.xml"
        && !config.profile.ci.junit.report_name.is_empty()
        && !config.profile.ci.junit.store_success_output
        && config.profile.ci.junit.store_failure_output
        && config.profile.ci.junit.flaky_fail_status == "failure"
        && config.profile.mutants.inherits == "default"
        && config.profile.mutants.fail_fast
        && !config.profile.mutants.global_timeout.is_empty();
    if valid {
        Ok(())
    } else {
        Err(QualityError::Policy(vec![
            "nextest config weakens deterministic retry, timeout, or JUnit policy".to_string(),
        ]))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the fields intentionally mirror cargo-mutants' independent boolean settings"
)]
/// Typed subset of cargo-mutants configuration enforced by the validator.
struct MutantsConfig {
    /// Whether cargo-mutants tests every Cargo feature.
    all_features: bool,
    /// Whether cargo-mutants tests the complete workspace.
    test_workspace: bool,
    /// Cargo test runner selected by cargo-mutants.
    test_tool: String,
    /// Additional Cargo arguments required by cargo-mutants.
    additional_cargo_args: Vec<String>,
    /// Additional Cargo test arguments required by cargo-mutants.
    additional_cargo_test_args: Vec<String>,
    /// Minimum test timeout configuration.
    minimum_test_timeout: f64,
    /// Multiplier applied to cargo-mutants test timeouts.
    timeout_multiplier: f64,
    /// Multiplier applied to cargo-mutants build timeouts.
    build_timeout_multiplier: f64,
    /// cargo-mutants sharding strategy.
    sharding: String,
    /// Whether cargo-mutants copies version-control metadata.
    copy_vcs: bool,
    /// Whether cargo-mutants honors gitignore rules.
    gitignore: bool,
    /// Whether cargo-mutants default call-skipping is enabled.
    skip_calls_defaults: bool,
    /// Whether cargo-mutants caps compiler lints.
    cap_lints: bool,
}

/// Validate mutants config against its repository contract.
fn validate_mutants_config(value: &toml::Value) -> Result<(), QualityError> {
    let config: MutantsConfig = value.clone().try_into().map_err(|source| {
        QualityError::Policy(vec![format!(
            "invalid cargo-mutants configuration: {source}"
        )])
    })?;
    let positive = |value: f64| value.is_finite() && value > 0.0;
    let valid = config.all_features
        && config.test_workspace
        && config.test_tool == "nextest"
        && config.additional_cargo_args == ["--locked"]
        && config.additional_cargo_test_args == ["--profile", "mutants", "--no-tests=fail"]
        && positive(config.minimum_test_timeout)
        && positive(config.timeout_multiplier)
        && positive(config.build_timeout_multiplier)
        && config.sharding == "slice"
        && config.copy_vcs
        && config.gitignore
        && !config.skip_calls_defaults
        && !config.cap_lints;
    if valid {
        Ok(())
    } else {
        Err(QualityError::Policy(vec![
            "cargo-mutants config is broad, unbounded, nondeterministic, or hides candidates"
                .to_string(),
        ]))
    }
}

#[derive(Debug, Deserialize)]
/// Native nextest inventory used to prove runnable test identity.
struct NativeNextestInventory {
    #[serde(rename = "test-count")]
    /// Number of test records.
    test_count: u64,
    #[serde(rename = "rust-suites")]
    /// Native nextest suites keyed by suite identity.
    rust_suites: BTreeMap<String, NativeNextestSuite>,
}

#[derive(Debug, Deserialize)]
/// One native nextest suite and its test cases.
struct NativeNextestSuite {
    /// Suite status reported by nextest.
    status: String,
    /// Native nextest cases keyed by test identity.
    testcases: BTreeMap<String, NativeNextestCase>,
}

#[derive(Debug, Deserialize)]
/// One native nextest test case and ignored state.
struct NativeNextestCase {
    /// Schema discriminator for this record.
    kind: String,
    /// Whether the native nextest case is ignored.
    ignored: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Reconciled nextest suite, test, and result counts.
struct NextestCounts {
    /// Number of runnable tests.
    tests: u64,
    /// Number of discovered test suites.
    suites: u64,
    /// Number of ignored tests.
    ignored: u64,
    /// Number of failed tests.
    failed: u64,
    /// Number of test execution errors.
    errors: u64,
    /// Number of timed-out tests.
    timed_out: u64,
}

impl NextestCounts {
    /// Add these counters to the stable validation summary.
    fn insert_summary(self, counts: &mut BTreeMap<String, u64>) {
        for (name, value) in [
            ("tests", self.tests),
            ("suites", self.suites),
            ("ignored", self.ignored),
            ("failed", self.failed),
            ("errors", self.errors),
            ("timed_out", self.timed_out),
        ] {
            counts.insert(name.to_string(), value);
        }
    }
}

/// Derive the runnable test count without allowing inconsistent inventory counts.
fn nextest_runnable_test_count(listed: u64, ignored: u64) -> Result<u64, QualityError> {
    listed.checked_sub(ignored).ok_or_else(|| {
        QualityError::Evidence(format!(
            "nextest ignored count {ignored} exceeds listed count {listed}"
        ))
    })
}

/// Validate nextest evidence against its repository contract.
fn validate_nextest_evidence(
    inventory: &NativeNextestInventory,
    junit_path: &Path,
) -> Result<NextestCounts, QualityError> {
    if inventory.test_count == 0 || inventory.rust_suites.is_empty() {
        return Err(QualityError::Status {
            status: QualityStatus::NoTests,
            message: "nextest inventory contains no runnable tests".to_string(),
        });
    }
    let mut listed = 0_u64;
    let mut ignored = 0_u64;
    for suite in inventory.rust_suites.values() {
        if suite.status != "listed" {
            return Err(QualityError::Evidence(format!(
                "nextest suite has incomplete status {:?}",
                suite.status
            )));
        }
        for case in suite.testcases.values() {
            if case.kind != "test" {
                return Err(QualityError::Evidence(format!(
                    "unsupported nextest testcase kind {:?}",
                    case.kind
                )));
            }
            listed = listed.checked_add(1).ok_or_else(|| {
                QualityError::Evidence("nextest testcase count overflow".to_string())
            })?;
            ignored += u64::from(case.ignored);
        }
    }
    if listed != inventory.test_count {
        return Err(QualityError::Evidence(format!(
            "nextest inventory count mismatch: declared {}, listed {listed}",
            inventory.test_count
        )));
    }
    let runnable = nextest_runnable_test_count(listed, ignored)?;
    if runnable == 0 {
        return Err(QualityError::Status {
            status: QualityStatus::NoTests,
            message: "nextest inventory contains no runnable tests".to_string(),
        });
    }
    let junit = parse_junit(junit_path)?;
    let listed_suites = usize_to_u64(inventory.rust_suites.len())?;
    let runnable_suites = usize_to_u64(
        inventory
            .rust_suites
            .values()
            .filter(|suite| suite.testcases.values().any(|case| !case.ignored))
            .count(),
    )?;
    if junit.tests != runnable
        || junit.suites != runnable_suites
        || junit.ignored != 0
        || junit.failed != 0
        || junit.errors != 0
        || junit.timed_out != 0
    {
        return Err(QualityError::Status {
            status: QualityStatus::TestFailure,
            message: format!(
                "nextest/JUnit reconciliation failed: inventory {listed} listed/{runnable} runnable/{listed_suites} listed suites/{runnable_suites} runnable suites/{ignored} ignored, JUnit {}/{}/{} with {} failures, {} errors, {} timeouts",
                junit.tests,
                junit.suites,
                junit.ignored,
                junit.failed,
                junit.errors,
                junit.timed_out
            ),
        });
    }
    Ok(NextestCounts {
        tests: runnable,
        suites: listed_suites,
        ignored,
        ..junit
    })
}

/// Parse junit from retained evidence.
fn parse_junit(path: &Path) -> Result<NextestCounts, QualityError> {
    let file = fs::File::open(path).map_err(|source| QualityError::Io {
        operation: "open JUnit input",
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut counts = NextestCounts::default();
    let mut testcase_open = false;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => match event.name().as_ref() {
                b"testsuite" => counts.suites += 1,
                b"testcase" => {
                    counts.tests += 1;
                    testcase_open = true;
                }
                b"failure" if testcase_open => counts.failed += 1,
                b"error" if testcase_open => counts.errors += 1,
                b"skipped" if testcase_open => counts.ignored += 1,
                b"system-err" if testcase_open => {}
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.name().as_ref() {
                b"testcase" => counts.tests += 1,
                b"failure" if testcase_open => counts.failed += 1,
                b"error" if testcase_open => counts.errors += 1,
                b"skipped" if testcase_open => counts.ignored += 1,
                _ => {}
            },
            Ok(Event::End(event)) if event.name().as_ref() == b"testcase" => {
                testcase_open = false;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(source) => {
                return Err(QualityError::Evidence(format!(
                    "failed to parse JUnit {}: {source}",
                    path.display()
                )));
            }
        }
        buffer.clear();
    }
    Ok(counts)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Normalized cargo doctest counts parsed from the retained log.
struct DoctestCounts {
    /// Number of passed doctests.
    passed: u64,
    /// Number of failed doctests.
    failed: u64,
    /// Number of ignored doctests.
    ignored: u64,
    /// Number of doctests measured by Cargo.
    measured: u64,
    /// Number of doctests filtered out.
    filtered: u64,
    /// Number of parsed Cargo doctest summaries.
    summaries: u64,
}

impl DoctestCounts {
    /// Add these counters to the stable validation summary.
    fn insert_summary(self, counts: &mut BTreeMap<String, u64>) {
        for (name, value) in [
            ("passed", self.passed),
            ("failed", self.failed),
            ("ignored", self.ignored),
            ("measured", self.measured),
            ("filtered", self.filtered),
            ("summaries", self.summaries),
        ] {
            counts.insert(name.to_string(), value);
        }
    }
}

/// Validate doctest log against its repository contract.
fn validate_doctest_log(log: &str, exit_code: i32) -> Result<DoctestCounts, QualityError> {
    let mut counts = DoctestCounts::default();
    for line in log.lines().filter(|line| line.contains("test result:")) {
        let fields = line
            .split_whitespace()
            .map(|word| word.trim_matches(|character: char| !character.is_ascii_alphanumeric()))
            .collect::<Vec<_>>();
        let read = |label: &str| -> Option<u64> {
            fields
                .windows(2)
                .find(|pair| pair[1] == label)
                .and_then(|pair| pair[0].parse().ok())
        };
        let required = |label| {
            read(label).ok_or_else(|| {
                QualityError::Evidence(format!(
                    "doctest summary is missing the {label} count: {line:?}"
                ))
            })
        };
        counts.passed += required("passed")?;
        counts.failed += required("failed")?;
        counts.ignored += required("ignored")?;
        counts.measured += required("measured")?;
        counts.filtered += required("filtered")?;
        counts.summaries += 1;
    }
    if exit_code != 0 || counts.failed != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::TestFailure,
            message: format!("stable doctest command failed with exit code {exit_code}"),
        });
    }
    if counts.summaries == 0 || counts.passed == 0 {
        return Err(QualityError::Status {
            status: QualityStatus::NoTests,
            message: "stable doctest log contains no complete runnable summary".to_string(),
        });
    }
    Ok(counts)
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// `OpenSpec` task identity and checked state parsed from Markdown.
struct ParsedTask {
    /// `OpenSpec` task identifier.
    task_id: String,
    /// Whether the `OpenSpec` task checkbox is checked.
    checked: bool,
    /// Runnable test identities reconciled from inventory and `JUnit`.
    test_ids: BTreeSet<String>,
}

/// Parse openspec tasks from retained evidence.
fn parse_openspec_tasks(source: &str) -> Result<Vec<ParsedTask>, QualityError> {
    let mut tasks = Vec::new();
    let mut task_ids = BTreeSet::new();
    let mut test_ids = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let checked = if trimmed.starts_with("- [ ] ") {
            false
        } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
            true
        } else {
            continue;
        };
        let remainder = &trimmed[6..];
        let task_id = remainder
            .split_whitespace()
            .next()
            .filter(|value| valid_task_id(value))
            .ok_or_else(|| QualityError::Policy(vec![format!("malformed task line {line:?}")]))?
            .to_string();
        if !task_ids.insert(task_id.clone()) {
            return Err(QualityError::Policy(vec![format!(
                "duplicate task id {task_id}"
            )]));
        }
        let declared = task_test_ids(remainder, &task_id)?;
        for test_id in &declared {
            if !test_ids.insert(test_id.clone()) {
                return Err(QualityError::Policy(vec![format!(
                    "duplicate unit-test id {test_id}"
                )]));
            }
        }
        if declared.is_empty() {
            return Err(QualityError::Policy(vec![format!(
                "task {task_id} has no task-specific unit-test identifier"
            )]));
        }
        tasks.push(ParsedTask {
            task_id,
            checked,
            test_ids: declared,
        });
    }
    if tasks.is_empty() {
        return Err(QualityError::Policy(vec![
            "OpenSpec checklist contains no tasks".to_string(),
        ]));
    }
    Ok(tasks)
}

/// Extract the backticked and tagged task-test identifier forms used by `OpenSpec` changes.
fn task_test_ids(remainder: &str, task_id: &str) -> Result<BTreeSet<String>, QualityError> {
    let mut declared = BTreeSet::new();
    for (index, part) in remainder.split('`').enumerate() {
        if index % 2 == 1 && part.contains("-UT-") {
            insert_task_test_id(&mut declared, part, task_id)?;
        }
    }

    let mut unscanned = remainder;
    while let Some(start) = unscanned.find("[UT:") {
        let tagged = &unscanned[start + 1..];
        let end = tagged.find(']').ok_or_else(|| {
            QualityError::Policy(vec![format!(
                "task {task_id} has an unterminated tagged unit-test id"
            )])
        })?;
        insert_task_test_id(&mut declared, &tagged[..end], task_id)?;
        unscanned = &tagged[end + 1..];
    }
    Ok(declared)
}

/// Validate and insert one task-specific test identifier.
fn insert_task_test_id(
    declared: &mut BTreeSet<String>,
    test_id: &str,
    task_id: &str,
) -> Result<(), QualityError> {
    let backticked = test_id
        .strip_suffix(&format!("-UT-{task_id}"))
        .is_some_and(valid_upper_identifier);
    let tagged = test_id
        .strip_prefix("UT:")
        .and_then(|value| value.strip_suffix(&format!("-{task_id}")))
        .is_some_and(valid_upper_identifier);
    if !backticked && !tagged {
        return Err(QualityError::Policy(vec![format!(
            "task {task_id} has malformed or reused test id {test_id}"
        )]));
    }
    if !declared.insert(test_id.to_string()) {
        return Err(QualityError::Policy(vec![format!(
            "duplicate unit-test id {test_id}"
        )]));
    }
    Ok(())
}

/// Return whether a test-identifier owner uses the accepted uppercase token syntax.
fn valid_upper_identifier(value: &str) -> bool {
    value.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    }) && value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

/// Return whether task id satisfies the accepted syntax.
fn valid_task_id(value: &str) -> bool {
    let mut parts = value.split('.');
    matches!((parts.next(), parts.next(), parts.next()), (Some(first), Some(second), None)
        if !first.is_empty()
            && !second.is_empty()
            && first.bytes().all(|byte| byte.is_ascii_digit())
            && second.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Task-to-command and covered-input verification plan.
struct VerificationPlan {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Verification records keyed by `OpenSpec` change identifier.
    changes: BTreeMap<String, VerificationChange>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Tasks declared for one `OpenSpec` change.
struct VerificationChange {
    /// Verification tasks declared for the change.
    tasks: Vec<VerificationTask>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// One task-specific verification command and its covered inputs.
struct VerificationTask {
    /// `OpenSpec` task identifier.
    task_id: String,
    /// Runnable test identities reconciled from inventory and `JUnit`.
    test_ids: Vec<String>,
    /// Acceptance assertion proved by a task.
    assertion: String,
    /// Command contract or gate command.
    command: VerificationCommand,
    /// Maximum task command duration in seconds.
    timeout_seconds: u64,
    /// Files and trees bound into task evidence.
    covered_inputs: Vec<CoveredInput>,
    /// Exact test-definition sources used to derive final commit permalinks.
    #[serde(default)]
    test_sources: Vec<VerificationTestSource>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// One task-specific test definition owned by a repository source file.
struct VerificationTestSource {
    /// Test identifier declared by the owning `OpenSpec` task.
    test_id: String,
    /// Repository-relative test source path.
    path: String,
    /// Function or command definition name expected in the source.
    anchor: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Executable, arguments, and exact test identity for one task.
struct VerificationCommand {
    /// Executable identity used by the command.
    executable: String,
    /// Ordered command arguments.
    arguments: Vec<String>,
}

#[derive(Serialize)]
/// Canonical task projection shared with `IssueOps` covered-input digests.
struct VerificationTaskDigest<'a> {
    /// Acceptance assertion proved by a task.
    assertion: &'a str,
    /// Command contract or gate command.
    command: VerificationCommandDigest<'a>,
    /// Files and trees bound into task evidence.
    covered_inputs: &'a [CoveredInput],
    /// `OpenSpec` task identifier.
    task_id: &'a str,
    /// Runnable test identities reconciled from inventory and `JUnit`.
    test_ids: &'a [String],
    /// Maximum task command duration in seconds.
    timeout_seconds: u64,
}

impl<'a> From<&'a VerificationTask> for VerificationTaskDigest<'a> {
    fn from(task: &'a VerificationTask) -> Self {
        Self {
            assertion: &task.assertion,
            command: VerificationCommandDigest {
                arguments: &task.command.arguments,
                executable: &task.command.executable,
            },
            covered_inputs: &task.covered_inputs,
            task_id: &task.task_id,
            test_ids: &task.test_ids,
            timeout_seconds: task.timeout_seconds,
        }
    }
}

#[derive(Serialize)]
/// Canonical command projection shared with `IssueOps` covered-input digests.
struct VerificationCommandDigest<'a> {
    /// Ordered command arguments.
    arguments: &'a [String],
    /// Executable identity used by the command.
    executable: &'a str,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// File or tree whose normalized digest binds task evidence.
struct CoveredInput {
    /// Schema discriminator for this record.
    kind: CoveredInputKind,
    /// Repository-relative path confined by the owning record.
    path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
/// Closed set of covered input kind values accepted by the quality validator.
enum CoveredInputKind {
    /// One repository file.
    File,
    /// One recursively enumerated repository tree.
    Tree,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Retained task results grouped by `OpenSpec` change.
struct TaskEvidenceLedger {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Task evidence records retained by the ledger.
    results: Vec<TaskEvidenceResult>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Commit-bound evidence retained for one `OpenSpec` task.
struct TaskEvidenceResult {
    /// `OpenSpec` change identifier owning the task result.
    change: String,
    /// `OpenSpec` task identifier.
    task_id: String,
    /// Exact unit-test filter bound to a task.
    test_id: String,
    /// Pass or fail outcome retained for the task.
    outcome: TaskOutcome,
    /// Git commit exercised by retained task evidence.
    tested_commit: String,
    /// Canonical digest of normalized covered inputs.
    covered_input_digest: String,
    /// Runner platform bound to the record.
    platform: Option<EvidencePlatform>,
    /// Shard execution start timestamp.
    started_at: String,
    /// Completion timestamp reported by cargo-mutants.
    completed_at: String,
    /// Typed retained result bound to the task.
    retained_result: RetainedResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
/// Closed set of task outcome values accepted by the quality validator.
enum TaskOutcome {
    /// Passed case accepted by the task outcome contract.
    Passed,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Runner and target identity attached to retained task evidence.
struct EvidencePlatform {
    /// Operating-system identity.
    os: String,
    /// Processor architecture identity.
    arch: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Closed set of retained result values accepted by the quality validator.
enum RetainedResult {
    /// Github actions case accepted by the retained result contract.
    GithubActions {
        /// Repository identity governed by the policy.
        repository: String,
        /// Workflow or local run identity.
        run_id: u64,
        /// Hosted workflow attempt number.
        run_attempt: u64,
        /// Hosted workflow job identity.
        job_id: u64,
        /// Hosted workflow job name.
        job_name: String,
        /// Hosted artifact identifier.
        artifact_id: u64,
        /// Hosted artifact name containing this evidence.
        artifact_name: String,
        /// Canonical SHA-256 identity of the hosted artifact.
        artifact_digest: String,
        /// Repository-relative retained result path.
        result_path: String,
        /// Canonical SHA-256 identity of the retained result.
        result_digest: String,
    },
    /// Repository case accepted by the retained result contract.
    Repository {
        /// Repository-relative retained result path.
        result_path: String,
        /// Canonical SHA-256 identity of the retained result.
        result_digest: String,
    },
}

/// Validate task evidence against its repository contract.
fn validate_task_evidence(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    expected_commit: &str,
    tasks: &[ParsedTask],
    plan: &VerificationPlan,
    ledger: &TaskEvidenceLedger,
) -> Result<(), QualityError> {
    if plan.schema_version != EVIDENCE_SCHEMA_VERSION
        || ledger.schema_version != EVIDENCE_SCHEMA_VERSION
    {
        return Err(QualityError::Policy(vec![
            "task verification schemas must be version 1".to_string(),
        ]));
    }
    let parsed_ids = tasks
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    let matching = plan
        .changes
        .iter()
        .filter(|(_, change)| {
            change
                .tasks
                .iter()
                .map(|task| task.task_id.as_str())
                .collect::<BTreeSet<_>>()
                == parsed_ids
        })
        .collect::<Vec<_>>();
    let [(change_name, change)] = matching.as_slice() else {
        return Err(QualityError::Policy(vec![
            "verification plan must contain exactly one change matching the checklist".to_string(),
        ]));
    };
    if !policy.required_changes.contains(change_name) {
        return Err(QualityError::Policy(vec![format!(
            "verification change {change_name} is not release-required"
        )]));
    }
    for result in &ledger.results {
        if !plan.changes.contains_key(&result.change)
            || !policy.required_changes.contains(&result.change)
        {
            return Err(QualityError::Policy(vec![format!(
                "orphan task evidence for change {}",
                result.change
            )]));
        }
    }
    let task_map = change
        .tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect::<BTreeMap<_, _>>();
    if task_map.len() != change.tasks.len() {
        return Err(QualityError::Policy(vec![
            "verification plan contains duplicate task ids".to_string(),
        ]));
    }
    let mut required = BTreeMap::new();
    for parsed in tasks {
        let planned = task_map.get(parsed.task_id.as_str()).ok_or_else(|| {
            QualityError::Policy(vec![format!("missing plan task {}", parsed.task_id)])
        })?;
        validate_verification_task(root, parsed, planned)?;
        for test_id in &parsed.test_ids {
            required.insert((parsed.task_id.as_str(), test_id.as_str()), parsed.checked);
        }
    }
    let mut seen = BTreeSet::new();
    for result in ledger
        .results
        .iter()
        .filter(|result| &result.change == *change_name)
    {
        let key = (result.task_id.as_str(), result.test_id.as_str());
        if !required.contains_key(&key) || !seen.insert(key) {
            return Err(QualityError::Policy(vec![format!(
                "duplicate or orphan evidence for {}/{}",
                result.task_id, result.test_id
            )]));
        }
        let planned = task_map[result.task_id.as_str()];
        validate_task_result(root, policy, expected_commit, planned, result)?;
    }
    for ((task_id, test_id), checked) in required {
        if checked && !seen.contains(&(task_id, test_id)) {
            return Err(QualityError::Status {
                status: QualityStatus::IncompleteEvidence,
                message: format!("completed task {task_id} lacks current evidence for {test_id}"),
            });
        }
    }
    Ok(())
}

/// Validate verification task against its repository contract.
fn validate_verification_task(
    root: &RepositoryRoot,
    parsed: &ParsedTask,
    planned: &VerificationTask,
) -> Result<(), QualityError> {
    let planned_ids = planned.test_ids.iter().cloned().collect::<BTreeSet<_>>();
    if planned_ids != parsed.test_ids
        || planned.test_ids.len() != planned_ids.len()
        || planned.assertion.trim().is_empty()
        || planned.timeout_seconds == 0
        || planned.command.arguments.is_empty()
        || planned.covered_inputs.is_empty()
    {
        return Err(QualityError::Policy(vec![format!(
            "verification plan task {} is incomplete or does not match its checklist",
            parsed.task_id
        )]));
    }
    if !planned.test_sources.is_empty() {
        let source_ids = planned
            .test_sources
            .iter()
            .map(|source| source.test_id.clone())
            .collect::<BTreeSet<_>>();
        if source_ids != planned_ids || source_ids.len() != planned.test_sources.len() {
            return Err(QualityError::Policy(vec![format!(
                "verification plan task {} has incomplete or duplicate test sources",
                parsed.task_id
            )]));
        }
        for source in &planned.test_sources {
            validate_relative_path(&source.path)?;
            root.input(&source.path)?;
            let supported = [".rs", ".py", ".js", ".jsx", ".ts", ".tsx", ".ps1", ".sh"]
                .iter()
                .any(|suffix| source.path.ends_with(suffix));
            let valid_anchor = source.anchor.bytes().enumerate().all(|(index, byte)| {
                byte == b'_'
                    || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
            });
            if !supported || source.anchor.is_empty() || !valid_anchor {
                return Err(QualityError::Policy(vec![format!(
                    "verification plan task {} has an invalid test source",
                    parsed.task_id
                )]));
            }
        }
    }
    let declared_rust_filter = !planned.test_sources.is_empty()
        && planned.test_sources.iter().all(|source| {
            planned.command.arguments.iter().any(|argument| {
                argument == &source.anchor || argument.ends_with(&format!("::{}", source.anchor))
            })
        });
    let generated_rust_filter = planned.test_sources.is_empty()
        && planned.test_ids.iter().all(|test_id| {
            let suffix = test_id
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            planned
                .command
                .arguments
                .iter()
                .any(|argument| argument.starts_with("task_") && argument.ends_with(&suffix))
        });
    let rust_command =
        planned.command.executable == "cargo" && (declared_rust_filter || generated_rust_filter);
    let python_command = ["python", "python3"].contains(&planned.command.executable.as_str())
        && planned
            .command
            .arguments
            .iter()
            .any(|argument| argument == ".github/scripts/issue-checklists.py")
        && planned.test_ids.iter().all(|test_id| {
            planned
                .command
                .arguments
                .windows(2)
                .any(|pair| pair[0] == "--test-id" && pair[1] == test_id.as_str())
        });
    if (!rust_command && !python_command)
        || planned
            .command
            .arguments
            .iter()
            .any(|argument| argument.contains('\0'))
    {
        return Err(QualityError::Policy(vec![format!(
            "verification plan task {} does not name its exact test filter",
            parsed.task_id
        )]));
    }
    for input in &planned.covered_inputs {
        validate_relative_path(&input.path)?;
        match input.kind {
            CoveredInputKind::File => {
                root.input(&input.path)?;
            }
            CoveredInputKind::Tree => {
                root.tree(&input.path)?;
            }
        }
    }
    normalized_covered_inputs_digest(root, planned)?;
    Ok(())
}

/// Validate task result against its repository contract.
fn validate_task_result(
    root: &RepositoryRoot,
    _policy: &QualityPolicy,
    expected_commit: &str,
    planned: &VerificationTask,
    result: &TaskEvidenceResult,
) -> Result<(), QualityError> {
    validate_commit(&result.tested_commit)?;
    let digest = normalized_covered_inputs_digest(root, planned)?;
    if result.covered_input_digest != format!("sha256:{digest}") {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: format!("covered inputs changed for {}", result.test_id),
        });
    }
    if !result.tested_commit.eq_ignore_ascii_case(expected_commit) && !root.0.join(".git").exists()
    {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: "archive evidence commit does not match expected commit".to_string(),
        });
    }
    if !valid_timestamp(&result.started_at)
        || !valid_timestamp(&result.completed_at)
        || result.completed_at < result.started_at
    {
        return Err(QualityError::Evidence(format!(
            "invalid task evidence timestamps for {}",
            result.test_id
        )));
    }
    if let Some(platform) = &result.platform
        && (platform.os.trim().is_empty() || platform.arch.trim().is_empty())
    {
        return Err(QualityError::Evidence(
            "empty evidence platform".to_string(),
        ));
    }
    validate_retained_result(root, &result.retained_result)
}

/// Validate retained result against its repository contract.
fn validate_retained_result(
    root: &RepositoryRoot,
    result: &RetainedResult,
) -> Result<(), QualityError> {
    let (path, digest) = match result {
        RetainedResult::GithubActions {
            repository,
            run_id,
            run_attempt,
            job_id,
            job_name,
            artifact_id,
            artifact_name,
            artifact_digest,
            result_path,
            result_digest,
        } => {
            if repository.trim().is_empty()
                || *run_id == 0
                || *run_attempt == 0
                || *job_id == 0
                || job_name.trim().is_empty()
                || *artifact_id == 0
                || artifact_name.trim().is_empty()
            {
                return Err(QualityError::Evidence(
                    "incomplete GitHub Actions retained-result identity".to_string(),
                ));
            }
            validate_digest(artifact_digest, "artifact digest")?;
            (result_path, result_digest)
        }
        RetainedResult::Repository {
            result_path,
            result_digest,
        } => (result_path, result_digest),
    };
    validate_digest(digest, "retained result digest")?;
    let actual = digest_file(&root.input(path)?)?;
    if digest.strip_prefix("sha256:").unwrap_or(digest) != actual {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: format!("retained result digest changed for {path}"),
        });
    }
    Ok(())
}

/// Validate relative path against its repository contract.
fn validate_relative_path(value: &str) -> Result<(), QualityError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(QualityError::PathEscape(value.to_string()));
    }
    Ok(())
}

/// Return whether timestamp satisfies the accepted syntax.
fn valid_timestamp(value: &str) -> bool {
    value.len() >= 20
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && value.contains('T')
        && (value.ends_with('Z')
            || value
                .rfind(['+', '-'])
                .is_some_and(|index| index > value.find('T').unwrap_or(usize::MAX)))
}

#[derive(Debug, Deserialize)]
/// LLVM coverage export plus cargo-llvm-cov provenance.
struct LlvmCoverageExport {
    /// Coverage data records from the LLVM export.
    data: Vec<LlvmCoverageData>,
    #[serde(rename = "type")]
    /// LLVM coverage export type identifier.
    export_type: String,
    /// Schema or tool version reported by the producer.
    version: String,
    /// cargo-llvm-cov version bound to this record.
    cargo_llvm_cov: LlvmCovTool,
}

#[derive(Debug, Deserialize)]
/// cargo-llvm-cov version and manifest provenance.
struct LlvmCovTool {
    /// Schema or tool version reported by the producer.
    version: String,
    /// Repository-relative path to manifest.
    manifest_path: String,
}

#[derive(Debug, Deserialize)]
/// One data record in an LLVM coverage export.
struct LlvmCoverageData {
    /// Source-file coverage records.
    files: Vec<LlvmCoverageFile>,
    /// Function coverage threshold or counts.
    functions: Vec<LlvmCoverageFunction>,
    /// Aggregate LLVM coverage summary.
    totals: LlvmCoverageSummary,
}

#[derive(Debug, Deserialize)]
/// One source file and its LLVM coverage summary.
struct LlvmCoverageFile {
    /// Source filename reported by LLVM.
    filename: String,
    /// Raw LLVM coverage segments for the source file.
    segments: Vec<(u64, u64, u64, bool, bool, bool)>,
    /// Coverage or mutation summary reported by the source tool.
    summary: LlvmCoverageSummary,
}

#[derive(Debug, Deserialize)]
/// One function record in an LLVM coverage export.
struct LlvmCoverageFunction {
    /// Function execution count reported by LLVM.
    count: u64,
    /// Source filenames associated with an LLVM function.
    filenames: Vec<String>,
    /// Tool, test, artifact, or mutant name reported by the producer.
    name: String,
    /// Region coverage threshold or counts.
    regions: Vec<(u64, u64, u64, u64, u64, u64, u64, u64)>,
}

#[derive(Debug, Deserialize)]
/// Line, region, and function totals from LLVM.
struct LlvmCoverageSummary {
    /// Line coverage threshold or counts.
    lines: LlvmMetric,
    /// Region coverage threshold or counts.
    regions: LlvmMetric,
    /// Function coverage threshold or counts.
    functions: LlvmMetric,
}

#[derive(Clone, Copy, Debug, Deserialize)]
/// Covered, total, and percent values for one LLVM metric.
struct LlvmMetric {
    /// Total item count reported by LLVM.
    count: u64,
    /// Covered item count reported by LLVM.
    covered: u64,
    /// Optional uncovered count reported by LLVM.
    notcovered: Option<u64>,
}

impl LlvmMetric {
    /// Convert an LLVM metric into validated covered and total counts.
    fn counts(self, label: &str) -> Result<MetricCounts, QualityError> {
        if self.covered > self.count
            || self
                .notcovered
                .is_some_and(|missed| missed != self.count - self.covered)
        {
            return Err(QualityError::Evidence(format!(
                "LLVM {label} counts are inconsistent"
            )));
        }
        Ok(MetricCounts::new(self.covered, self.count))
    }
}

/// Validate coverage against its repository contract.
fn validate_coverage(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    platform_id: &str,
    export: &LlvmCoverageExport,
    enforcement: CoverageEnforcement,
) -> Result<CoverageCounts, QualityError> {
    if export.export_type != "llvm.coverage.json.export"
        || export.version.trim().is_empty()
        || export.cargo_llvm_cov.version != EXPECTED_LLVM_COV_VERSION
        || export.data.len() != 1
    {
        return Err(QualityError::Evidence(
            "LLVM export type, version, tool pin, or data cardinality is invalid".to_string(),
        ));
    }
    let manifest = Path::new(&export.cargo_llvm_cov.manifest_path)
        .canonicalize()
        .map_err(|source| QualityError::Io {
            operation: "canonicalize LLVM manifest path",
            path: PathBuf::from(&export.cargo_llvm_cov.manifest_path),
            source,
        })?;
    if manifest != root.0.join("Cargo.toml") {
        return Err(QualityError::WrongRoot(
            "LLVM export manifest does not belong to the selected root".to_string(),
        ));
    }
    let platform = policy
        .platforms
        .iter()
        .find(|platform| platform.id == platform_id)
        .ok_or_else(|| {
            QualityError::Policy(vec![format!("unsupported coverage platform {platform_id}")])
        })?;
    let data = &export.data[0];
    if data.files.is_empty() || data.functions.is_empty() {
        return Err(QualityError::Evidence(
            "LLVM export has no file or function rows".to_string(),
        ));
    }
    for function in &data.functions {
        if function.name.is_empty()
            || function.filenames.is_empty()
            || function.regions.is_empty()
            || function
                .regions
                .iter()
                .any(|region| region.0 == 0 || region.2 < region.0)
        {
            return Err(QualityError::Evidence(
                "LLVM function evidence is truncated".to_string(),
            ));
        }
        let _ = function.count;
    }
    let mut filenames = BTreeSet::new();
    let mut all = CoverageCounts::default();
    let mut applicable = CoverageCounts::default();
    for file in &data.files {
        if file.segments.is_empty() {
            return Err(QualityError::Evidence(format!(
                "LLVM file {} has no segments",
                file.filename
            )));
        }
        let canonical =
            Path::new(&file.filename)
                .canonicalize()
                .map_err(|source| QualityError::Io {
                    operation: "canonicalize LLVM source path",
                    path: PathBuf::from(&file.filename),
                    source,
                })?;
        if !canonical.starts_with(&root.0) {
            return Err(QualityError::PathEscape(file.filename.clone()));
        }
        let key = root.relative_key(&canonical)?;
        if !filenames.insert(key.clone()) {
            return Err(QualityError::Evidence(format!(
                "duplicate LLVM file row {key}"
            )));
        }
        let counts = coverage_summary_counts(&file.summary, &key)?;
        all.checked_add(counts)?;
        if path_is_applicable(policy, &key)? {
            applicable.checked_add(counts)?;
        }
    }
    let totals = coverage_summary_counts(&data.totals, "totals")?;
    if all != totals {
        return Err(QualityError::Evidence(format!(
            "LLVM per-file totals {all:?} do not reconcile with aggregate {totals:?}"
        )));
    }
    applicable.validate()?;
    if policy
        .exceptions
        .records
        .iter()
        .any(|record| matches!(record, QualityException::Coverage { .. }))
    {
        return Err(QualityError::Evidence(
            "coverage exception records require exact normalized range evidence".to_string(),
        ));
    }
    if let Some(floor) = platform.coverage_floor()
        && !applicable.at_least(&floor)
    {
        return Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            message: format!("coverage regressed below the {platform_id} floor"),
        });
    }
    if enforcement.enforces_targets() && !coverage_meets_targets(policy, &applicable, &applicable) {
        return Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            message: format!("coverage misses an agreed v0.4 target on {platform_id}"),
        });
    }
    Ok(applicable)
}

/// Return whether raw and adjusted coverage counts meet every agreed target.
fn coverage_meets_targets(
    policy: &QualityPolicy,
    raw: &CoverageCounts,
    adjusted: &CoverageCounts,
) -> bool {
    let targets = &policy.targets.coverage;
    raw.lines.meets(targets.lines.raw_basis_points)
        && raw.regions.meets(targets.regions.raw_basis_points)
        && raw.functions.meets(targets.functions.raw_basis_points)
        && adjusted.lines.meets(targets.lines.adjusted_basis_points)
        && adjusted
            .regions
            .meets(targets.regions.adjusted_basis_points)
        && adjusted
            .functions
            .meets(targets.functions.adjusted_basis_points)
}

/// Extract validated line, region, and function counts from an LLVM summary.
fn coverage_summary_counts(
    summary: &LlvmCoverageSummary,
    label: &str,
) -> Result<CoverageCounts, QualityError> {
    Ok(CoverageCounts {
        lines: summary.lines.counts(&format!("{label} lines"))?,
        regions: summary.regions.counts(&format!("{label} regions"))?,
        functions: summary.functions.counts(&format!("{label} functions"))?,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// One cargo-mutants candidate before normalization.
struct NativeMutant {
    /// Tool, test, artifact, or mutant name reported by the producer.
    name: String,
    /// Cargo package that owns the source.
    package: String,
    /// Repository-relative source file.
    file: String,
    /// Optional function metadata reported for the mutant.
    function: Option<NativeFunction>,
    /// Source span selected by the mutant or exception.
    span: NativeSpan,
    /// Replacement expression proposed by cargo-mutants.
    replacement: String,
    /// Mutation operation reported by cargo-mutants.
    genre: NativeMutantGenre,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Function name and return type reported for a mutant.
struct NativeFunction {
    /// Function name reported by LLVM.
    function_name: String,
    /// Function return type reported for the mutant.
    return_type: String,
    /// Source span selected by the mutant or exception.
    span: NativeSpan,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Source range reported for a mutant.
struct NativeSpan {
    /// Inclusive start coordinate.
    start: NativePosition,
    /// Exclusive end coordinate.
    end: NativePosition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// One line-and-column source coordinate.
struct NativePosition {
    /// One-based source line.
    line: u64,
    /// One-based source column.
    column: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Closed set of native mutant genre values accepted by the quality validator.
enum NativeMutantGenre {
    /// Mutation that changes a fn value.
    FnValue,
    /// Mutation that changes a binary operator.
    BinaryOperator,
    /// Mutation that changes a unary operator.
    UnaryOperator,
    /// Mutation that changes a match arm.
    MatchArm,
    /// Mutation that changes a match arm guard.
    MatchArmGuard,
    /// Mutation that changes a struct field.
    StructField,
}

#[derive(Debug)]
/// Canonical mutant map keyed by stable mutant identity.
struct MutationInventory {
    /// Deterministically ordered mutants keyed for mutation inventory.
    mutants: BTreeMap<String, NativeMutant>,
}

impl MutationInventory {
    /// Normalize native cargo-mutants records into a stable inventory.
    fn from_native(root: &RepositoryRoot, native: Vec<NativeMutant>) -> Result<Self, QualityError> {
        let mut mutants = BTreeMap::new();
        for mutant in native {
            validate_relative_path(&mutant.file)?;
            let source = root.input(&mutant.file)?;
            if root.relative_key(&source)? != mutant.file {
                return Err(QualityError::Evidence(format!(
                    "mutant path is not normalized: {}",
                    mutant.file
                )));
            }
            validate_native_span(mutant.span)?;
            if let Some(function) = &mutant.function {
                validate_native_span(function.span)?;
                if function.function_name.trim().is_empty() {
                    return Err(QualityError::Evidence(
                        "mutant function name is empty".to_string(),
                    ));
                }
            }
            if mutant.name.trim().is_empty()
                || mutant.package.trim().is_empty()
                || mutant.replacement.contains('\0')
            {
                return Err(QualityError::Evidence(
                    "mutant identity fields are incomplete".to_string(),
                ));
            }
            let id = native_mutant_id(&mutant)?;
            if mutants.insert(id.clone(), mutant).is_some() {
                return Err(QualityError::Evidence(format!(
                    "duplicate native mutant identity {id}"
                )));
            }
        }
        Ok(Self { mutants })
    }

    /// Assign every mutant to one deterministic shard.
    fn mutation_plan(
        &self,
        policy: &QualityPolicy,
        raw_digest: &str,
    ) -> Result<MutationPlan, QualityError> {
        validate_digest(raw_digest, "raw mutation inventory")?;
        let mut excluded_ids = Vec::new();
        let mut exclude_argv = Vec::new();
        for exception in &policy.exceptions.records {
            let QualityException::Mutation {
                mutant_id, path, ..
            } = exception
            else {
                continue;
            };
            let mutant = self.mutants.get(mutant_id).ok_or_else(|| {
                QualityError::Policy(vec![format!(
                    "unused mutation exception for unknown mutant {mutant_id}"
                )])
            })?;
            if &mutant.file != path {
                return Err(QualityError::Policy(vec![format!(
                    "mutation exception {mutant_id} path does not match the native mutant"
                )]));
            }
            excluded_ids.push(mutant_id.clone());
            exclude_argv.push("--exclude-re".to_string());
            exclude_argv.push(format!("^{}$", regex::escape(&mutant.name)));
        }
        excluded_ids.sort();
        let excluded = excluded_ids.iter().cloned().collect::<BTreeSet<_>>();
        let filtered = self
            .mutants
            .keys()
            .filter(|id| !excluded.contains(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        Ok(MutationPlan {
            raw_digest: raw_digest.to_ascii_lowercase(),
            filtered_digest: digest_strings(filtered.iter().map(String::as_str)),
            excluded_digest: digest_strings(excluded_ids.iter().map(String::as_str)),
            excluded_ids,
            exclude_argv,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Deterministic shard assignment for a mutation inventory.
struct MutationPlan {
    /// Canonical SHA-256 identity of the raw mutant inventory.
    raw_digest: String,
    /// Canonical SHA-256 identity of the included mutant identities.
    filtered_digest: String,
    /// Canonical SHA-256 identity of the excluded mutant identities.
    excluded_digest: String,
    /// Stable mutant identities excluded by approved exceptions.
    excluded_ids: Vec<String>,
    /// Exact cargo-mutants exclusion arguments derived from approved exceptions.
    exclude_argv: Vec<String>,
}

/// Validate native span against its repository contract.
fn validate_native_span(span: NativeSpan) -> Result<(), QualityError> {
    if span.start.line == 0
        || span.end.line == 0
        || (span.end.line, span.end.column) < (span.start.line, span.start.column)
    {
        return Err(QualityError::Evidence(
            "native mutant has an invalid source span".to_string(),
        ));
    }
    Ok(())
}

/// Compute the stable identity of one normalized mutant.
fn native_mutant_id(mutant: &NativeMutant) -> Result<String, QualityError> {
    #[derive(Serialize)]
    struct Identity<'a> {
        package: &'a str,
        file: &'a str,
        span: NativeSpan,
        function: &'a Option<NativeFunction>,
        genre: NativeMutantGenre,
        replacement: &'a str,
    }
    let bytes = serde_json::to_vec(&Identity {
        package: &mutant.package,
        file: &mutant.file,
        span: mutant.span,
        function: &mutant.function,
        genre: mutant.genre,
        replacement: &mutant.replacement,
    })
    .map_err(|source| QualityError::Json {
        path: PathBuf::from("<mutant-identity>"),
        source: Box::new(source),
    })?;
    Ok(hex_digest(&bytes))
}

/// Compute the canonical SHA-256 identity for strings.
fn digest_strings<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hash_length_prefixed(&mut hasher, value.as_bytes());
    }
    encode_hex(&hasher.finalize())
}

/// Validate inventory baseline against its repository contract.
fn validate_inventory_baseline(
    _root: &RepositoryRoot,
    policy: &QualityPolicy,
    inventory: &MutationInventory,
) -> Result<(), QualityError> {
    if inventory.mutants.is_empty() {
        return Err(QualityError::Status {
            status: QualityStatus::NoMutants,
            message: "raw cargo-mutants inventory is empty".to_string(),
        });
    }
    let mut packages = BTreeMap::<&str, u64>::new();
    for mutant in inventory.mutants.values() {
        if !path_is_applicable(policy, &mutant.file)? {
            return Err(QualityError::Evidence(format!(
                "raw inventory contains out-of-scope source {}",
                mutant.file
            )));
        }
        *packages.entry(mutant.package.as_str()).or_default() += 1;
    }
    let observed = &policy.observed.mutation_inventory;
    let expected = BTreeMap::from([
        ("projectatlas-cli", observed.packages.projectatlas_cli),
        ("projectatlas-db", observed.packages.projectatlas_db),
        (
            "projectatlas-service",
            observed.packages.projectatlas_service,
        ),
        (
            "projectatlas-symbols",
            observed.packages.projectatlas_symbols,
        ),
        ("projectatlas-core", observed.packages.projectatlas_core),
        ("projectatlas-fs", observed.packages.projectatlas_fs),
        ("projectatlas-lints", observed.packages.projectatlas_lints),
    ]);
    if usize_to_u64(inventory.mutants.len())? != observed.total || packages != expected {
        return Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            message: "raw mutation inventory drift lacks matching policy provenance".to_string(),
        });
    }
    inventory.mutation_plan(policy, &observed.artifact_sha256)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
/// Native cargo-mutants lab outcomes plus tool provenance.
struct NativeLabOutcome {
    /// Native mutation scenarios reported by cargo-mutants.
    outcomes: Vec<NativeScenarioOutcome>,
    /// Total mutant count assigned by the deterministic plan.
    total_mutants: u64,
    /// Number of missed outcomes in this native lab outcome record.
    missed: u64,
    /// Number of caught outcomes in this native lab outcome record.
    caught: u64,
    /// Number of mutant scenarios that timed out.
    timeout: u64,
    /// Number of unviable outcomes in this native lab outcome record.
    unviable: u64,
    /// Number of success outcomes in this native lab outcome record.
    success: u64,
    /// Process start timestamp reported by cargo-mutants.
    start_time: String,
    /// Optional process completion timestamp.
    end_time: Option<String>,
    /// Version identity reported for cargo mutants.
    cargo_mutants_version: String,
}

#[derive(Debug, Deserialize)]
/// One mutant scenario and its build/test phase results.
struct NativeScenarioOutcome {
    /// Mutation scenario represented by this outcome.
    scenario: NativeScenario,
    /// Coverage or mutation summary reported by the source tool.
    summary: NativeSummaryOutcome,
    /// Repository-relative path to log.
    log_path: String,
    /// Repository-relative path to diff.
    diff_path: Option<String>,
    /// Ordered build and test phase results.
    phase_results: Vec<NativePhaseResult>,
}

#[derive(Debug, Deserialize)]
/// Closed set of native scenario values accepted by the quality validator.
enum NativeScenario {
    /// Baseline case accepted by the native scenario contract.
    Baseline,
    /// Mutant case accepted by the native scenario contract.
    Mutant(Box<NativeMutant>),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
/// Closed set of native summary outcome values accepted by the quality validator.
enum NativeSummaryOutcome {
    /// Success case accepted by the native summary outcome contract.
    Success,
    /// Caught mutant case accepted by the native summary outcome contract.
    CaughtMutant,
    /// Missed mutant case accepted by the native summary outcome contract.
    MissedMutant,
    /// Unviable case accepted by the native summary outcome contract.
    Unviable,
    /// Failure case accepted by the native summary outcome contract.
    Failure,
    /// Timeout case accepted by the native summary outcome contract.
    Timeout,
}

#[derive(Debug, Deserialize)]
/// One cargo-mutants build or test process result.
struct NativePhaseResult {
    /// Build or test phase represented by this result.
    phase: NativePhase,
    /// Execution duration reported by the source tool.
    duration: f64,
    /// Status reported for process.
    process_status: NativeExit,
    /// Exact process arguments reported by cargo-mutants.
    argv: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
/// Closed set of native phase values accepted by the quality validator.
enum NativePhase {
    /// Check case accepted by the native phase contract.
    Check,
    /// Build case accepted by the native phase contract.
    Build,
    /// Test case accepted by the native phase contract.
    Test,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
/// Closed set of native exit values accepted by the quality validator.
enum NativeExit {
    /// Success case accepted by the native exit contract.
    Success,
    /// Failure case accepted by the native exit contract.
    Failure(i32),
    /// Timeout case accepted by the native exit contract.
    Timeout,
    /// Signalled case accepted by the native exit contract.
    Signalled(i32),
    /// Other case accepted by the native exit contract.
    Other,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Reconciled mutation outcome counters.
struct MutationCounts {
    /// Total number of raw mutants before exclusions.
    raw_total: u64,
    /// Number of caught outcomes in this mutation counts record.
    caught: u64,
    /// Number of missed outcomes in this mutation counts record.
    missed: u64,
    /// Number of timed out outcomes in this mutation counts record.
    timed_out: u64,
    /// Number of unviable outcomes in this mutation counts record.
    unviable: u64,
    /// Number of excluded outcomes in this mutation counts record.
    excluded: u64,
    /// Number of unresolved outcomes in this mutation counts record.
    unresolved: u64,
}

impl MutationCounts {
    /// Add these counters to the stable validation summary.
    fn insert_summary(self, counts: &mut BTreeMap<String, u64>) {
        for (name, value) in [
            ("raw_total", self.raw_total),
            ("caught", self.caught),
            ("missed", self.missed),
            ("timed_out", self.timed_out),
            ("unviable", self.unviable),
            ("excluded", self.excluded),
            ("unresolved", self.unresolved),
        ] {
            counts.insert(name.to_string(), value);
        }
        if let Some(raw_kill_basis_points) = self
            .caught
            .saturating_mul(10_000)
            .checked_div(self.raw_total)
        {
            counts.insert("raw_kill_basis_points".to_string(), raw_kill_basis_points);
        }
        let viable = self
            .caught
            .saturating_add(self.missed)
            .saturating_add(self.timed_out)
            .saturating_add(self.unresolved);
        if let Some(adjusted_viable_kill_basis_points) =
            self.caught.saturating_mul(10_000).checked_div(viable)
        {
            counts.insert(
                "adjusted_viable_kill_basis_points".to_string(),
                adjusted_viable_kill_basis_points,
            );
        }
    }
}

/// Validate changed mutation against its repository contract.
fn validate_changed_mutation(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    inventory: &MutationInventory,
    outcomes: &NativeLabOutcome,
    merge_base: &str,
) -> Result<MutationCounts, QualityError> {
    let changed = root.changed_paths(merge_base)?;
    let applicable = changed
        .iter()
        .filter(|path| path_is_applicable(policy, path).unwrap_or(false))
        .cloned()
        .collect::<BTreeSet<_>>();
    if inventory.mutants.is_empty() {
        if applicable.is_empty() && outcomes.outcomes.is_empty() {
            return Ok(MutationCounts::default());
        }
        return Err(QualityError::Status {
            status: QualityStatus::NoMutants,
            message: "empty changed inventory is not proven by an empty applicable diff"
                .to_string(),
        });
    }
    if inventory
        .mutants
        .values()
        .any(|mutant| !applicable.contains(&mutant.file))
    {
        return Err(QualityError::Evidence(
            "changed mutation inventory contains a foreign source path".to_string(),
        ));
    }
    let plan = inventory.mutation_plan(
        policy,
        &digest_strings(inventory.mutants.keys().map(String::as_str)),
    )?;
    let excluded = plan.excluded_ids.iter().cloned().collect::<BTreeSet<_>>();
    let expected = inventory
        .mutants
        .keys()
        .filter(|id| !excluded.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut counts = validate_lab_outcome(inventory, outcomes, &expected)?;
    counts.raw_total = usize_to_u64(inventory.mutants.len())?;
    counts.excluded = usize_to_u64(excluded.len())?;
    if counts.missed != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::MissedMutant,
            message: format!("{} changed viable mutants were missed", counts.missed),
        });
    }
    if counts.timed_out != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::MutantTimeout,
            message: format!("{} changed mutants timed out", counts.timed_out),
        });
    }
    if counts.unresolved != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: format!("{} changed mutants are unresolved", counts.unresolved),
        });
    }
    Ok(counts)
}

/// Validate lab outcome against its repository contract.
fn validate_lab_outcome(
    inventory: &MutationInventory,
    lab: &NativeLabOutcome,
    expected: &BTreeSet<String>,
) -> Result<MutationCounts, QualityError> {
    if lab.cargo_mutants_version != EXPECTED_MUTANTS_VERSION
        || lab
            .end_time
            .as_deref()
            .is_none_or(|value| !valid_timestamp(value))
        || !valid_timestamp(&lab.start_time)
    {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: "cargo-mutants lab is truncated or uses the wrong tool version".to_string(),
        });
    }
    let mut baseline = 0_u64;
    let mut seen = BTreeSet::new();
    let mut counts = MutationCounts::default();
    let mut native_success = 0_u64;
    for outcome in &lab.outcomes {
        validate_native_phases(outcome)?;
        let derived = derive_native_summary(outcome)?;
        if outcome.summary != derived {
            return Err(QualityError::Evidence(
                "cargo-mutants scenario summary disagrees with native phases".to_string(),
            ));
        }
        match &outcome.scenario {
            NativeScenario::Baseline => {
                baseline += 1;
                if derived != NativeSummaryOutcome::Success {
                    return Err(QualityError::Status {
                        status: QualityStatus::BaselineFailure,
                        message: "cargo-mutants unmutated baseline failed".to_string(),
                    });
                }
            }
            NativeScenario::Mutant(mutant) => {
                let id = native_mutant_id(mutant)?;
                if !inventory.mutants.contains_key(&id)
                    || !expected.contains(&id)
                    || !seen.insert(id)
                {
                    return Err(QualityError::Evidence(
                        "cargo-mutants outcome is duplicate, excluded, or foreign".to_string(),
                    ));
                }
                match derived {
                    NativeSummaryOutcome::CaughtMutant => counts.caught += 1,
                    NativeSummaryOutcome::MissedMutant => counts.missed += 1,
                    NativeSummaryOutcome::Timeout => counts.timed_out += 1,
                    NativeSummaryOutcome::Unviable => counts.unviable += 1,
                    NativeSummaryOutcome::Failure => counts.unresolved += 1,
                    NativeSummaryOutcome::Success => native_success += 1,
                }
            }
        }
    }
    if baseline != 1 {
        return Err(QualityError::Status {
            status: QualityStatus::BaselineFailure,
            message: format!("expected one successful baseline, found {baseline}"),
        });
    }
    counts.unresolved += usize_to_u64(expected.len().saturating_sub(seen.len()))?;
    let native_total = counts
        .caught
        .saturating_add(counts.missed)
        .saturating_add(counts.timed_out)
        .saturating_add(counts.unviable)
        .saturating_add(counts.unresolved)
        .saturating_add(native_success);
    if lab.total_mutants != native_total
        || lab.caught != counts.caught
        || lab.missed != counts.missed
        || lab.timeout != counts.timed_out
        || lab.unviable != counts.unviable
        || lab.success != native_success
    {
        return Err(QualityError::Evidence(
            "cargo-mutants lab summary counts do not reconcile with scenarios".to_string(),
        ));
    }
    counts.unresolved += native_success;
    Ok(counts)
}

/// Validate native phases against its repository contract.
fn validate_native_phases(outcome: &NativeScenarioOutcome) -> Result<(), QualityError> {
    if outcome.log_path.trim().is_empty() || outcome.phase_results.is_empty() {
        return Err(QualityError::Evidence(
            "cargo-mutants scenario lacks log or phase evidence".to_string(),
        ));
    }
    if matches!(outcome.scenario, NativeScenario::Mutant(_)) && outcome.diff_path.is_none() {
        return Err(QualityError::Evidence(
            "mutant scenario lacks a native diff path".to_string(),
        ));
    }
    let mut prior = None;
    for phase in &outcome.phase_results {
        if !phase.duration.is_finite()
            || phase.duration < 0.0
            || phase.argv.is_empty()
            || phase.argv.iter().any(|argument| argument.contains('\0'))
            || prior.is_some_and(|previous| phase.phase <= previous)
        {
            return Err(QualityError::Evidence(
                "cargo-mutants phase evidence is malformed or out of order".to_string(),
            ));
        }
        prior = Some(phase.phase);
    }
    Ok(())
}

/// Derive the native scenario summary from build and test phases.
fn derive_native_summary(
    outcome: &NativeScenarioOutcome,
) -> Result<NativeSummaryOutcome, QualityError> {
    let last = outcome
        .phase_results
        .last()
        .ok_or_else(|| QualityError::Evidence("missing mutation phase".to_string()))?;
    let timeout = outcome
        .phase_results
        .iter()
        .any(|phase| phase.process_status == NativeExit::Timeout);
    let build_failed = outcome.phase_results.iter().any(|phase| {
        phase.phase != NativePhase::Test && matches!(phase.process_status, NativeExit::Failure(_))
    });
    Ok(match &outcome.scenario {
        NativeScenario::Baseline => {
            if timeout {
                NativeSummaryOutcome::Timeout
            } else if last.process_status == NativeExit::Success {
                NativeSummaryOutcome::Success
            } else {
                NativeSummaryOutcome::Failure
            }
        }
        NativeScenario::Mutant(_) => {
            if build_failed {
                NativeSummaryOutcome::Unviable
            } else if timeout {
                NativeSummaryOutcome::Timeout
            } else if last.phase == NativePhase::Test
                && matches!(last.process_status, NativeExit::Failure(_))
            {
                NativeSummaryOutcome::CaughtMutant
            } else if last.phase == NativePhase::Test && last.process_status == NativeExit::Success
            {
                NativeSummaryOutcome::MissedMutant
            } else if last.process_status == NativeExit::Success {
                NativeSummaryOutcome::Success
            } else {
                NativeSummaryOutcome::Failure
            }
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Commit-bound result manifest for one deterministic mutation shard.
struct ShardManifest {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Schema discriminator for this record.
    kind: String,
    /// Repository identity governed by the policy.
    repository: String,
    /// Git commit exercised by this shard.
    commit_sha: String,
    /// Stable platform-policy identifier.
    platform_id: String,
    /// Rust compilation target triple.
    target_triple: String,
    /// Zero-based deterministic mutation shard index.
    shard_index: u8,
    /// Number of shard records.
    shard_count: u8,
    /// `rustc` version used by this shard.
    rustc_version: String,
    /// LLVM version paired with `rustc`.
    llvm_version: String,
    /// Version identity reported for cargo mutants.
    cargo_mutants_version: String,
    /// Canonical SHA-256 identity for cargo lock.
    cargo_lock_sha256: String,
    /// Canonical SHA-256 identity of the quality policy.
    policy_sha256: String,
    /// Canonical SHA-256 identity for base config.
    base_config_sha256: String,
    /// Canonical SHA-256 identity for mutation plan.
    mutation_plan_sha256: String,
    /// Canonical SHA-256 identity for raw inventory.
    raw_inventory_sha256: String,
    /// Canonical SHA-256 identity for filtered inventory.
    filtered_inventory_sha256: String,
    /// Canonical SHA-256 identity for excluded inventory.
    excluded_inventory_sha256: String,
    /// Cargo feature set exercised by the evidence.
    feature_set: String,
    /// Nextest profile used by cargo-mutants.
    test_profile: String,
    /// Repository-relative path to native mutants.
    native_mutants_path: String,
    /// Canonical SHA-256 identity for native mutants.
    native_mutants_sha256: String,
    /// Repository-relative path to native outcomes.
    native_outcomes_path: String,
    /// Canonical SHA-256 identity for native outcomes.
    native_outcomes_sha256: String,
    /// UTC timestamp at which execution began.
    started_at_utc: String,
    /// UTC completion timestamp for task evidence.
    completed_at_utc: String,
    /// Workflow or local run identity.
    run_id: u64,
    /// Hosted workflow attempt number.
    run_attempt: u64,
    /// Job identifier reported by nextest.
    job: String,
    /// Hosted artifact name containing this evidence.
    artifact_name: String,
}

/// Validate mutation aggregate against its repository contract.
fn validate_mutation_aggregate(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    policy_digest: &str,
    inventory: &MutationInventory,
    inventory_digest: &str,
    shard_paths: &[String],
) -> Result<MutationCounts, QualityError> {
    if shard_paths.len() != usize::from(REQUIRED_MUTATION_SHARDS) {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: format!(
                "full mutation aggregate requires exactly {REQUIRED_MUTATION_SHARDS} shard manifests"
            ),
        });
    }
    let plan = inventory.mutation_plan(policy, inventory_digest)?;
    let plan_bytes = serde_json::to_vec(&plan).map_err(|source| QualityError::Json {
        path: PathBuf::from("<mutation-plan>"),
        source: Box::new(source),
    })?;
    let plan_digest = hex_digest(&plan_bytes);
    let lock_digest = digest_file(&root.input("Cargo.lock")?)?;
    let config_digest = digest_file(&root.input(".cargo/mutants.toml")?)?;
    let commit = root.head_commit()?;
    let bindings = ShardEvidenceBindings {
        commit: &commit,
        policy_digest,
        inventory_digest,
        plan: &plan,
        plan_digest: &plan_digest,
        lock_digest: &lock_digest,
        config_digest: &config_digest,
    };
    let excluded = plan.excluded_ids.iter().cloned().collect::<BTreeSet<_>>();
    let filtered = inventory
        .mutants
        .keys()
        .filter(|id| !excluded.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if filtered.is_empty() {
        return Err(QualityError::Status {
            status: QualityStatus::NoMutants,
            message: "full mutation policy excludes every raw candidate".to_string(),
        });
    }
    let mut indices = BTreeSet::new();
    let mut executed = BTreeSet::new();
    let mut aggregate = MutationCounts {
        raw_total: usize_to_u64(inventory.mutants.len())?,
        excluded: usize_to_u64(excluded.len())?,
        ..MutationCounts::default()
    };
    let mut common_repository = None::<String>;
    for path in shard_paths {
        let manifest_path = root.input(path)?;
        let manifest: ShardManifest = read_json(&manifest_path)?;
        validate_shard_manifest(root, policy, &manifest, &manifest_path, &bindings)?;
        if !indices.insert(manifest.shard_index) {
            return Err(QualityError::Evidence(format!(
                "duplicate mutation shard {}",
                manifest.shard_index
            )));
        }
        if let Some(repository) = &common_repository {
            if repository != &manifest.repository {
                return Err(QualityError::Evidence(
                    "mutation shards name different repositories".to_string(),
                ));
            }
        } else {
            common_repository = Some(manifest.repository.clone());
        }
        let native_inventory_path =
            root.input_from(&manifest_path, &manifest.native_mutants_path)?;
        if digest_file(&native_inventory_path)? != manifest.native_mutants_sha256 {
            return Err(QualityError::Status {
                status: QualityStatus::StaleEvidence,
                message: format!(
                    "native mutant digest changed for shard {}",
                    manifest.shard_index
                ),
            });
        }
        let shard_inventory = MutationInventory::from_native(
            root,
            read_json::<Vec<NativeMutant>>(&native_inventory_path)?,
        )?;
        let shard_ids = shard_inventory
            .mutants
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if shard_ids.is_empty()
            || !shard_ids.is_subset(&filtered)
            || shard_ids.iter().any(|id| !executed.insert(id.clone()))
        {
            return Err(QualityError::Evidence(format!(
                "shard {} has empty, foreign, or duplicate candidates",
                manifest.shard_index
            )));
        }
        let outcome_path = root.input_from(&manifest_path, &manifest.native_outcomes_path)?;
        if digest_file(&outcome_path)? != manifest.native_outcomes_sha256 {
            return Err(QualityError::Status {
                status: QualityStatus::StaleEvidence,
                message: format!(
                    "native outcome digest changed for shard {}",
                    manifest.shard_index
                ),
            });
        }
        let lab: NativeLabOutcome = read_json(&outcome_path)?;
        let counts = validate_lab_outcome(&shard_inventory, &lab, &shard_ids)?;
        aggregate.caught += counts.caught;
        aggregate.missed += counts.missed;
        aggregate.timed_out += counts.timed_out;
        aggregate.unviable += counts.unviable;
        aggregate.unresolved += counts.unresolved;
    }
    let expected_indices = (1..=REQUIRED_MUTATION_SHARDS).collect::<BTreeSet<_>>();
    if indices != expected_indices || executed != filtered || !executed.is_disjoint(&excluded) {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: "full mutation shards do not exactly reconcile the filtered master"
                .to_string(),
        });
    }
    if aggregate.timed_out != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::MutantTimeout,
            message: format!("{} full-inventory mutants timed out", aggregate.timed_out),
        });
    }
    if aggregate.unresolved != 0 {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: format!(
                "{} full-inventory mutants are unresolved",
                aggregate.unresolved
            ),
        });
    }
    let raw_met = ratio_meets(
        aggregate.caught,
        aggregate.raw_total,
        policy.targets.mutation.raw_viable_kill_basis_points,
    );
    let viable_total = aggregate.caught.saturating_add(aggregate.missed);
    let adjusted_met = ratio_meets(
        aggregate.caught,
        viable_total,
        policy.targets.mutation.adjusted_viable_kill_basis_points,
    );
    let floor_met = !policy.mutation_floor.established
        || (ratio_meets(
            aggregate.caught,
            aggregate.raw_total,
            policy.mutation_floor.raw_viable_kill_basis_points,
        ) && ratio_meets(
            aggregate.caught,
            viable_total,
            policy.mutation_floor.adjusted_viable_kill_basis_points,
        ));
    if !raw_met || !adjusted_met || !floor_met {
        return Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            message: "full mutation strength misses an agreed target or monotonic floor"
                .to_string(),
        });
    }
    Ok(aggregate)
}

/// Expected commit and digest identities shared by every mutation shard.
struct ShardEvidenceBindings<'a> {
    /// Git commit exercised by every shard.
    commit: &'a str,
    /// Canonical SHA-256 identity of the quality policy.
    policy_digest: &'a str,
    /// Canonical SHA-256 identity of the raw mutation inventory.
    inventory_digest: &'a str,
    /// Deterministic mutation plan shared by every shard.
    plan: &'a MutationPlan,
    /// Canonical SHA-256 identity of the mutation plan.
    plan_digest: &'a str,
    /// Canonical SHA-256 identity of `Cargo.lock`.
    lock_digest: &'a str,
    /// Canonical SHA-256 identity of the cargo-mutants configuration.
    config_digest: &'a str,
}

/// Validate shard manifest against its repository contract.
fn validate_shard_manifest(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    manifest: &ShardManifest,
    manifest_path: &Path,
    bindings: &ShardEvidenceBindings<'_>,
) -> Result<(), QualityError> {
    let platform = policy
        .platforms
        .iter()
        .find(|platform| platform.id == manifest.platform_id)
        .ok_or_else(|| {
            QualityError::Evidence(format!(
                "shard {} has unsupported platform {}",
                manifest.shard_index, manifest.platform_id
            ))
        })?;
    for (digest, label) in [
        (&manifest.cargo_lock_sha256, "shard Cargo.lock"),
        (&manifest.policy_sha256, "shard policy"),
        (&manifest.base_config_sha256, "shard base config"),
        (&manifest.mutation_plan_sha256, "shard mutation plan"),
        (&manifest.raw_inventory_sha256, "shard raw inventory"),
        (
            &manifest.filtered_inventory_sha256,
            "shard filtered inventory",
        ),
        (
            &manifest.excluded_inventory_sha256,
            "shard excluded inventory",
        ),
        (&manifest.native_mutants_sha256, "shard native mutants"),
        (&manifest.native_outcomes_sha256, "shard native outcomes"),
    ] {
        validate_digest(digest, label)?;
    }
    let valid = manifest.schema_version == EVIDENCE_SCHEMA_VERSION
        && manifest.kind == "full-mutation-shard"
        && !manifest.repository.trim().is_empty()
        && manifest.commit_sha == bindings.commit
        && manifest.target_triple == platform.target
        && (1..=REQUIRED_MUTATION_SHARDS).contains(&manifest.shard_index)
        && manifest.shard_count == REQUIRED_MUTATION_SHARDS
        && !manifest.rustc_version.trim().is_empty()
        && !manifest.llvm_version.trim().is_empty()
        && manifest.cargo_mutants_version == EXPECTED_MUTANTS_VERSION
        && manifest.cargo_lock_sha256 == bindings.lock_digest
        && manifest.policy_sha256 == bindings.policy_digest
        && manifest.base_config_sha256 == bindings.config_digest
        && manifest.mutation_plan_sha256 == bindings.plan_digest
        && manifest.raw_inventory_sha256 == bindings.inventory_digest
        && manifest.filtered_inventory_sha256 == bindings.plan.filtered_digest
        && manifest.excluded_inventory_sha256 == bindings.plan.excluded_digest
        && manifest.feature_set == "all-features"
        && manifest.test_profile == "mutants"
        && valid_timestamp(&manifest.started_at_utc)
        && valid_timestamp(&manifest.completed_at_utc)
        && manifest.completed_at_utc >= manifest.started_at_utc
        && manifest.run_id > 0
        && manifest.run_attempt > 0
        && manifest.job == "mutation-shard"
        && !manifest.artifact_name.trim().is_empty();
    if !valid {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: format!(
                "shard {} identity is stale or incomplete",
                manifest.shard_index
            ),
        });
    }
    for path in [
        &manifest.native_mutants_path,
        &manifest.native_outcomes_path,
    ] {
        validate_relative_path(path)?;
        root.input_from(manifest_path, path)?;
    }
    Ok(())
}

/// Return whether a covered-to-total ratio meets a basis-point threshold.
fn ratio_meets(covered: u64, total: u64, basis_points: u16) -> bool {
    total != 0 && u128::from(covered) * 10_000 >= u128::from(total) * u128::from(basis_points)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Normalized quality-gate manifest used for merge and release decisions.
struct EvidenceManifest {
    /// Schema version used to decode and emit this record.
    schema_version: u32,
    /// Repository identity governed by the policy.
    repository: String,
    /// Quality gate represented by this manifest.
    gate: GateKind,
    /// Stable pass or failure status for the gate.
    status: QualityStatus,
    /// Git commit exercised by the gate.
    commit_sha: String,
    /// Runner platform bound to the record.
    platform: GatePlatform,
    /// Rust and LLVM identity used by the gate.
    toolchain: GateToolchain,
    /// Pinned quality tool used by the gate.
    tool: GateTool,
    /// Commit and digest identities consumed by the gate.
    inputs: GateInputs,
    /// Command contract or gate command.
    command: GateCommand,
    /// Timeouts enforced for this gate or policy.
    timeouts: GateTimeouts,
    /// UTC timestamp at which execution began.
    started_at_utc: String,
    /// UTC completion timestamp for task evidence.
    completed_at_utc: String,
    /// Hosted or repository-retained run identity.
    run: GateRun,
    /// Retained artifacts produced by the gate.
    artifacts: Vec<GateArtifact>,
    /// Normalized gate result.
    result: GateResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
/// Closed set of gate kind values accepted by the quality validator.
enum GateKind {
    /// Nextest quality gate.
    Nextest,
    /// Doctest quality gate.
    Doctest,
    /// Coverage quality gate.
    Coverage,
    /// Changed mutation quality gate.
    ChangedMutation,
    /// Full mutation quality gate.
    FullMutation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Closed coverage-enforcement modes carried by validation and retained evidence.
enum CoverageEnforcement {
    /// Validate complete coverage structure and platform floors for an implementation checkpoint.
    ImplementationCheckpoint,
    /// Enforce every structural, floor, and final v0.4 coverage target requirement.
    #[default]
    ReleaseQuality,
}

impl CoverageEnforcement {
    /// Parse the CLI spelling while preserving strict release-quality behavior when omitted.
    fn from_cli(value: Option<&str>) -> Result<Self, QualityError> {
        match value {
            None | Some("release-quality") => Ok(Self::ReleaseQuality),
            Some("implementation-checkpoint") => Ok(Self::ImplementationCheckpoint),
            Some(other) => Err(QualityError::Usage(format!(
                "unsupported coverage enforcement {other:?}"
            ))),
        }
    }

    /// Return the stable retained-manifest spelling.
    const fn manifest_name(self) -> &'static str {
        match self {
            Self::ImplementationCheckpoint => "implementation_checkpoint",
            Self::ReleaseQuality => "release_quality",
        }
    }

    /// Require a retained summary to carry this exact enforcement identity.
    fn validate_manifest_name(self, value: Option<&str>) -> Result<(), QualityError> {
        if value.is_some_and(|value| value == self.manifest_name()) {
            Ok(())
        } else {
            Err(QualityError::Evidence(
                "coverage enforcement does not reconcile with its validator summary".to_string(),
            ))
        }
    }

    /// Return whether the final v0.4 target percentages are enforced.
    const fn enforces_targets(self) -> bool {
        matches!(self, Self::ReleaseQuality)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Operating system, architecture, target, and runner identity for a gate.
struct GatePlatform {
    /// Stable identifier for this policy or evidence record.
    id: String,
    /// Operating-system identity.
    os: String,
    /// Processor architecture identity.
    arch: String,
    /// Runner target triple or configured target name.
    target: String,
    /// Hosted runner image identity.
    runner_image: String,
    /// Hosted runner image version.
    runner_image_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Rust and LLVM versions used by a gate.
struct GateToolchain {
    /// `rustc` version used by the gate.
    rustc_version: String,
    /// LLVM version paired with `rustc`.
    llvm_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Pinned quality tool and version used by a gate.
struct GateTool {
    /// Tool, test, artifact, or mutant name reported by the producer.
    name: String,
    /// Schema or tool version reported by the producer.
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Commit and digest identities consumed by a gate.
struct GateInputs {
    /// Canonical SHA-256 identity for cargo lock.
    cargo_lock_sha256: String,
    /// Canonical SHA-256 identity of the quality policy.
    policy_sha256: String,
    /// Canonical SHA-256 identity for source scope.
    source_scope_sha256: String,
    /// Configuration artifacts consumed by the gate.
    configs: Vec<GateConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Configuration artifact and digest consumed by a gate.
struct GateConfig {
    /// Semantic role of the retained artifact or configuration.
    role: ConfigRole,
    /// Repository-relative path confined by the owning record.
    path: String,
    /// Canonical SHA-256 identity of the referenced artifact.
    sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
/// Closed set of config role values accepted by the quality validator.
enum ConfigRole {
    /// Nextest configuration artifact.
    Nextest,
    /// Mutants configuration artifact.
    Mutants,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Executable, arguments, and profile used by a gate.
struct GateCommand {
    /// Executable identity used by the command.
    executable: String,
    /// Ordered command arguments.
    arguments: Vec<String>,
    /// Named nextest or mutation profile.
    profile: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "the seconds suffix is part of the retained gate-evidence schema and preserves units"
)]
/// Command, job, test, and build limits retained in seconds.
struct GateTimeouts {
    /// Command timeout in seconds.
    command_seconds: u64,
    /// Job timeout in seconds.
    job_seconds: u64,
    /// Test timeout in seconds.
    test_seconds: Option<u64>,
    /// Build timeout in seconds.
    build_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Closed set of gate run values accepted by the quality validator.
enum GateRun {
    /// Github actions case accepted by the gate run contract.
    GithubActions {
        /// Workflow or local run identity.
        run_id: u64,
        /// Hosted workflow attempt number.
        run_attempt: u64,
        /// Hosted workflow job identity.
        job_id: u64,
        /// Hosted workflow job name.
        job_name: String,
        /// Immutable workflow reference that produced the evidence.
        workflow_ref: String,
    },
    /// Repository retained local case accepted by the gate run contract.
    RepositoryRetainedLocal {
        /// Workflow or local run identity.
        run_id: String,
        /// Host triple or machine identity.
        host: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Retained artifact identity and semantic role produced by a gate.
struct GateArtifact {
    /// Semantic role of the retained artifact or configuration.
    role: ArtifactRole,
    /// Repository-relative path confined by the owning record.
    path: String,
    /// Canonical SHA-256 identity of the referenced artifact.
    sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
/// Closed set of artifact role values accepted by the quality validator.
enum ArtifactRole {
    /// Nextest inventory retained evidence artifact.
    NextestInventory,
    /// Junit retained evidence artifact.
    Junit,
    /// Doctest log retained evidence artifact.
    DoctestLog,
    /// Llvm json retained evidence artifact.
    LlvmJson,
    /// Coverage report retained evidence artifact.
    CoverageReport,
    /// Mutation inventory retained evidence artifact.
    MutationInventory,
    /// Mutation outcomes retained evidence artifact.
    MutationOutcomes,
    /// Mutation plan retained evidence artifact.
    MutationPlan,
    /// Shard manifest retained evidence artifact.
    ShardManifest,
    /// Validation summary retained evidence artifact.
    ValidationSummary,
    /// Diagnostics retained evidence artifact.
    Diagnostics,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Closed set of gate result values accepted by the quality validator.
enum GateResult {
    /// Nextest case accepted by the gate result contract.
    Nextest {
        /// Number of runnable tests.
        tests: u64,
        /// Number of discovered test suites.
        suites: u64,
        /// Number of ignored tests.
        ignored: u64,
        /// Number of failed tests.
        failed: u64,
        /// Number of test execution errors.
        errors: u64,
        /// Number of timed-out tests.
        timed_out: u64,
    },
    /// Doctest case accepted by the gate result contract.
    Doctest {
        /// Number of passed doctests.
        passed: u64,
        /// Number of failed doctests.
        failed: u64,
        /// Number of ignored doctests.
        ignored: u64,
        /// Number of doctests measured by Cargo.
        measured: u64,
        /// Number of doctests filtered out.
        filtered: u64,
        /// Number of parsed Cargo doctest summaries.
        summaries: u64,
    },
    /// Coverage case accepted by the gate result contract.
    Coverage {
        #[serde(default)]
        /// Enforcement contract used to validate this coverage record.
        enforcement: CoverageEnforcement,
        /// Unadjusted coverage counts from the source tool.
        raw: CoverageCounts,
        /// Coverage counts after approved exceptions are applied.
        adjusted: CoverageCounts,
        /// Number of approved exceptions consumed by validation.
        exceptions_used: u64,
    },
    /// Changed mutation case accepted by the gate result contract.
    ChangedMutation {
        /// Total number of raw mutants before exclusions.
        raw_total: u64,
        /// Number of caught outcomes in this changed mutation record.
        caught: u64,
        /// Number of missed outcomes in this changed mutation record.
        missed: u64,
        /// Number of timed out outcomes in this changed mutation record.
        timed_out: u64,
        /// Number of unviable outcomes in this changed mutation record.
        unviable: u64,
        /// Number of excluded outcomes in this changed mutation record.
        excluded: u64,
        /// Number of unresolved outcomes in this changed mutation record.
        unresolved: u64,
        /// Whether the unmutated baseline test run passed.
        baseline_passed: bool,
    },
    /// Full mutation case accepted by the gate result contract.
    FullMutation {
        /// Total number of raw mutants before exclusions.
        raw_total: u64,
        /// Number of caught outcomes in this full mutation record.
        caught: u64,
        /// Number of missed outcomes in this full mutation record.
        missed: u64,
        /// Number of timed out outcomes in this full mutation record.
        timed_out: u64,
        /// Number of unviable outcomes in this full mutation record.
        unviable: u64,
        /// Number of excluded outcomes in this full mutation record.
        excluded: u64,
        /// Number of unresolved outcomes in this full mutation record.
        unresolved: u64,
        /// Whether the unmutated baseline test run passed.
        baseline_passed: bool,
        /// Number of shards assigned by the plan.
        shards: u8,
        /// Raw kill ratio expressed in basis points.
        raw_kill_basis_points: u64,
        /// Adjusted viable kill ratio expressed in basis points.
        adjusted_viable_kill_basis_points: u64,
    },
}

impl GateResult {
    /// Return the gate kind encoded by this result variant.
    fn gate(&self) -> GateKind {
        match self {
            Self::Nextest { .. } => GateKind::Nextest,
            Self::Doctest { .. } => GateKind::Doctest,
            Self::Coverage { .. } => GateKind::Coverage,
            Self::ChangedMutation { .. } => GateKind::ChangedMutation,
            Self::FullMutation { .. } => GateKind::FullMutation,
        }
    }

    /// Add gate-result counters to the validation summary.
    fn summary_counts(&self) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();
        match self {
            Self::Nextest {
                tests,
                suites,
                ignored,
                failed,
                errors,
                timed_out,
            } => NextestCounts {
                tests: *tests,
                suites: *suites,
                ignored: *ignored,
                failed: *failed,
                errors: *errors,
                timed_out: *timed_out,
            }
            .insert_summary(&mut counts),
            Self::Doctest {
                passed,
                failed,
                ignored,
                measured,
                filtered,
                summaries,
            } => DoctestCounts {
                passed: *passed,
                failed: *failed,
                ignored: *ignored,
                measured: *measured,
                filtered: *filtered,
                summaries: *summaries,
            }
            .insert_summary(&mut counts),
            Self::Coverage { raw, .. } => raw.insert_summary(&mut counts),
            Self::ChangedMutation {
                raw_total,
                caught,
                missed,
                timed_out,
                unviable,
                excluded,
                unresolved,
                ..
            }
            | Self::FullMutation {
                raw_total,
                caught,
                missed,
                timed_out,
                unviable,
                excluded,
                unresolved,
                ..
            } => MutationCounts {
                raw_total: *raw_total,
                caught: *caught,
                missed: *missed,
                timed_out: *timed_out,
                unviable: *unviable,
                excluded: *excluded,
                unresolved: *unresolved,
            }
            .insert_summary(&mut counts),
        }
        match self {
            Self::ChangedMutation {
                baseline_passed, ..
            } => {
                counts.insert("baseline_passed".to_string(), u64::from(*baseline_passed));
            }
            Self::FullMutation {
                baseline_passed,
                shards,
                raw_kill_basis_points,
                adjusted_viable_kill_basis_points,
                ..
            } => {
                counts.insert("baseline_passed".to_string(), u64::from(*baseline_passed));
                counts.insert("shards".to_string(), u64::from(*shards));
                counts.insert("raw_kill_basis_points".to_string(), *raw_kill_basis_points);
                counts.insert(
                    "adjusted_viable_kill_basis_points".to_string(),
                    *adjusted_viable_kill_basis_points,
                );
            }
            _ => {}
        }
        counts
    }
}

/// Validate evidence manifests against its repository contract.
fn validate_evidence_manifests(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    policy_digest: &str,
    manifests: &[(PathBuf, EvidenceManifest)],
    release_commit: Option<&str>,
) -> Result<(), QualityError> {
    if manifests.is_empty() {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: "no gate manifests were provided".to_string(),
        });
    }
    let expected_commit = release_commit.unwrap_or(&root.head_commit()?).to_string();
    validate_commit(&expected_commit)?;
    if release_commit.is_some() && root.head_commit()? != expected_commit {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: "release evidence commit differs from the checkout".to_string(),
        });
    }
    let mut keys = BTreeSet::new();
    for (path, manifest) in manifests {
        validate_gate_manifest(
            root,
            policy,
            policy_digest,
            path,
            manifest,
            &expected_commit,
            release_commit.is_some(),
        )?;
        let platform = if manifest.gate == GateKind::Coverage {
            manifest.platform.id.as_str()
        } else {
            ""
        };
        if !keys.insert((manifest.gate, platform)) {
            return Err(QualityError::Evidence(format!(
                "duplicate {:?} manifest for {platform}",
                manifest.gate
            )));
        }
    }
    if release_commit.is_some() {
        for gate in [
            GateKind::Nextest,
            GateKind::Doctest,
            GateKind::ChangedMutation,
            GateKind::FullMutation,
        ] {
            if !keys.contains(&(gate, "")) {
                return Err(QualityError::Status {
                    status: QualityStatus::IncompleteEvidence,
                    message: format!("release lacks {gate:?} evidence"),
                });
            }
        }
        for platform in &policy.platforms {
            if !keys.contains(&(GateKind::Coverage, platform.id.as_str())) {
                return Err(QualityError::Status {
                    status: QualityStatus::IncompleteEvidence,
                    message: format!("release lacks coverage for {}", platform.id),
                });
            }
        }
    }
    Ok(())
}

/// Validate gate manifest against its repository contract.
fn validate_gate_manifest(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    policy_digest: &str,
    manifest_path: &Path,
    manifest: &EvidenceManifest,
    commit: &str,
    release: bool,
) -> Result<(), QualityError> {
    if manifest.schema_version != EVIDENCE_SCHEMA_VERSION
        || manifest.repository != policy.repository
        || manifest.gate != manifest.result.gate()
        || manifest.status != QualityStatus::Passed
        || manifest.commit_sha != commit
    {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: "manifest schema, repository, gate, status, or commit is ineligible"
                .to_string(),
        });
    }
    validate_gate_platform(policy, &manifest.platform)?;
    if manifest.toolchain.rustc_version != policy.reference_toolchain.rust
        || manifest.toolchain.llvm_version != policy.reference_toolchain.llvm
    {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: "manifest toolchain differs from policy".to_string(),
        });
    }
    validate_gate_tool(policy, manifest.gate, &manifest.tool)?;
    validate_gate_inputs(root, policy, policy_digest, manifest.gate, &manifest.inputs)?;
    validate_gate_command(manifest.gate, &manifest.command)?;
    validate_gate_timeouts(policy, manifest.gate, &manifest.timeouts)?;
    if !valid_timestamp(&manifest.started_at_utc)
        || !valid_timestamp(&manifest.completed_at_utc)
        || manifest.completed_at_utc < manifest.started_at_utc
    {
        return Err(QualityError::Evidence(
            "manifest timestamps are invalid or reversed".to_string(),
        ));
    }
    validate_gate_run(&manifest.run, release)?;
    let summary = validate_gate_artifacts(root, manifest_path, manifest.gate, &manifest.artifacts)?;
    if summary.status != manifest.status
        || summary.command != gate_command_name(manifest.gate)
        || summary.counts != manifest.result.summary_counts()
        || summary
            .identities
            .get("commit")
            .is_none_or(|value| value != &manifest.commit_sha)
        || summary
            .identities
            .get("policy_sha256")
            .is_none_or(|value| value != &manifest.inputs.policy_sha256)
        || summary
            .identities
            .get("source_scope_sha256")
            .is_none_or(|value| value != &manifest.inputs.source_scope_sha256)
    {
        return Err(QualityError::Evidence(
            "typed result does not reconcile with its validator summary".to_string(),
        ));
    }
    if let GateResult::Coverage { enforcement, .. } = &manifest.result {
        enforcement.validate_manifest_name(
            summary
                .identities
                .get(COVERAGE_ENFORCEMENT_IDENTITY)
                .map(String::as_str),
        )?;
    }
    validate_gate_result(policy, &manifest.platform.id, &manifest.result, release)
}

/// Validate gate platform against its repository contract.
fn validate_gate_platform(
    policy: &QualityPolicy,
    platform: &GatePlatform,
) -> Result<(), QualityError> {
    let expected = policy
        .platforms
        .iter()
        .find(|candidate| candidate.id == platform.id)
        .ok_or_else(|| QualityError::Evidence("unsupported gate platform".to_string()))?;
    let identity = match platform.id.as_str() {
        "linux-x86_64-gnu" => ("linux", "x86_64"),
        "windows-x86_64-msvc" => ("windows", "x86_64"),
        "macos-x86_64" => ("macos", "x86_64"),
        "macos-aarch64" => ("macos", "aarch64"),
        _ => {
            return Err(QualityError::Evidence(
                "unknown platform identity".to_string(),
            ));
        }
    };
    if (platform.os.as_str(), platform.arch.as_str()) != identity
        || platform.target != expected.target
        || platform.runner_image != expected.runner
        || platform.runner_image_version.trim().is_empty()
    {
        return Err(QualityError::Evidence(
            "manifest platform does not match policy".to_string(),
        ));
    }
    Ok(())
}

/// Validate gate tool against its repository contract.
fn validate_gate_tool(
    policy: &QualityPolicy,
    gate: GateKind,
    tool: &GateTool,
) -> Result<(), QualityError> {
    let expected = match gate {
        GateKind::Nextest => ("cargo-nextest", policy.tools.cargo_nextest.as_str()),
        GateKind::Doctest => ("rustc", policy.reference_toolchain.rust.as_str()),
        GateKind::Coverage => ("cargo-llvm-cov", policy.tools.cargo_llvm_cov.as_str()),
        GateKind::ChangedMutation | GateKind::FullMutation => {
            ("cargo-mutants", policy.tools.cargo_mutants.as_str())
        }
    };
    if (tool.name.as_str(), tool.version.as_str()) != expected {
        return Err(QualityError::Status {
            status: QualityStatus::MissingTool,
            message: format!("{gate:?} tool identity differs from policy"),
        });
    }
    Ok(())
}

/// Validate gate inputs against its repository contract.
fn validate_gate_inputs(
    root: &RepositoryRoot,
    policy: &QualityPolicy,
    policy_digest: &str,
    gate: GateKind,
    inputs: &GateInputs,
) -> Result<(), QualityError> {
    if inputs.cargo_lock_sha256 != digest_file(&root.input("Cargo.lock")?)?
        || inputs.policy_sha256 != policy_digest
        || inputs.source_scope_sha256
            != digest_strings(policy.scope.include_globs.iter().map(String::as_str))
    {
        return Err(QualityError::Status {
            status: QualityStatus::StaleEvidence,
            message: "manifest input digest changed".to_string(),
        });
    }
    let required = match gate {
        GateKind::Nextest | GateKind::Coverage => BTreeSet::from([ConfigRole::Nextest]),
        GateKind::Doctest => BTreeSet::new(),
        GateKind::ChangedMutation | GateKind::FullMutation => {
            BTreeSet::from([ConfigRole::Nextest, ConfigRole::Mutants])
        }
    };
    let roles = inputs
        .configs
        .iter()
        .map(|value| value.role)
        .collect::<BTreeSet<_>>();
    if roles != required || roles.len() != inputs.configs.len() {
        return Err(QualityError::Evidence(
            "manifest config roles are incomplete or duplicated".to_string(),
        ));
    }
    for config in &inputs.configs {
        let path = match config.role {
            ConfigRole::Nextest => ".config/nextest.toml",
            ConfigRole::Mutants => ".cargo/mutants.toml",
        };
        if config.path != path || config.sha256 != digest_file(&root.input(path)?)? {
            return Err(QualityError::Status {
                status: QualityStatus::StaleEvidence,
                message: format!("manifest {:?} config changed", config.role),
            });
        }
    }
    Ok(())
}

/// Validate gate command against its repository contract.
fn validate_gate_command(gate: GateKind, command: &GateCommand) -> Result<(), QualityError> {
    let profile = match gate {
        GateKind::Nextest | GateKind::Coverage => "ci",
        GateKind::Doctest => "doc",
        GateKind::ChangedMutation | GateKind::FullMutation => "mutants",
    };
    if command.executable != "cargo"
        || command.profile != profile
        || command.arguments.is_empty()
        || command.arguments.iter().any(|value| value.contains('\0'))
    {
        return Err(QualityError::Evidence(
            "manifest command is not a fixed bounded Cargo argv".to_string(),
        ));
    }
    Ok(())
}

/// Validate gate timeouts against its repository contract.
fn validate_gate_timeouts(
    policy: &QualityPolicy,
    gate: GateKind,
    value: &GateTimeouts,
) -> Result<(), QualityError> {
    let expected = match gate {
        GateKind::Nextest => (
            policy.timeouts.nextest_command_seconds,
            policy.timeouts.nextest_job_seconds,
            Some(policy.timeouts.nextest_test_seconds),
            None,
        ),
        GateKind::Doctest => (
            policy.timeouts.doctest_command_seconds,
            policy.timeouts.doctest_job_seconds,
            None,
            None,
        ),
        GateKind::Coverage => (
            policy.timeouts.coverage_command_seconds,
            policy.timeouts.coverage_job_seconds,
            None,
            None,
        ),
        GateKind::ChangedMutation => (
            policy.timeouts.changed_mutation_command_seconds,
            policy.timeouts.changed_mutation_job_seconds,
            Some(policy.timeouts.changed_mutant_test_seconds),
            Some(policy.timeouts.changed_mutant_build_seconds),
        ),
        GateKind::FullMutation => (
            policy.timeouts.mutation_aggregate_command_seconds,
            policy.timeouts.mutation_aggregate_job_seconds,
            Some(policy.timeouts.mutation_shard_test_seconds),
            Some(policy.timeouts.mutation_shard_build_seconds),
        ),
    };
    if (
        value.command_seconds,
        value.job_seconds,
        value.test_seconds,
        value.build_seconds,
    ) != expected
    {
        return Err(QualityError::Evidence(
            "manifest timeouts differ from policy".to_string(),
        ));
    }
    Ok(())
}

/// Validate gate run against its repository contract.
fn validate_gate_run(run: &GateRun, release: bool) -> Result<(), QualityError> {
    match run {
        GateRun::GithubActions {
            run_id,
            run_attempt,
            job_id,
            job_name,
            workflow_ref,
        } if *run_id > 0
            && *run_attempt > 0
            && *job_id > 0
            && !job_name.trim().is_empty()
            && !workflow_ref.trim().is_empty() =>
        {
            Ok(())
        }
        GateRun::RepositoryRetainedLocal { run_id, host }
            if !release && !run_id.trim().is_empty() && !host.trim().is_empty() =>
        {
            Ok(())
        }
        _ => Err(QualityError::Evidence(
            "manifest run identity is incomplete or not release-eligible".to_string(),
        )),
    }
}

/// Validate gate artifacts against its repository contract.
fn validate_gate_artifacts(
    root: &RepositoryRoot,
    manifest_path: &Path,
    gate: GateKind,
    artifacts: &[GateArtifact],
) -> Result<ValidationSummary, QualityError> {
    let required = match gate {
        GateKind::Nextest => BTreeSet::from([
            ArtifactRole::NextestInventory,
            ArtifactRole::Junit,
            ArtifactRole::ValidationSummary,
        ]),
        GateKind::Doctest => {
            BTreeSet::from([ArtifactRole::DoctestLog, ArtifactRole::ValidationSummary])
        }
        GateKind::Coverage => BTreeSet::from([
            ArtifactRole::LlvmJson,
            ArtifactRole::CoverageReport,
            ArtifactRole::ValidationSummary,
        ]),
        GateKind::ChangedMutation => BTreeSet::from([
            ArtifactRole::MutationInventory,
            ArtifactRole::MutationOutcomes,
            ArtifactRole::ValidationSummary,
        ]),
        GateKind::FullMutation => BTreeSet::from([
            ArtifactRole::MutationInventory,
            ArtifactRole::MutationOutcomes,
            ArtifactRole::MutationPlan,
            ArtifactRole::ShardManifest,
            ArtifactRole::ValidationSummary,
        ]),
    };
    let roles = artifacts
        .iter()
        .map(|value| value.role)
        .collect::<BTreeSet<_>>();
    if !required.is_subset(&roles) {
        return Err(QualityError::Status {
            status: QualityStatus::IncompleteEvidence,
            message: format!("{gate:?} manifest lacks a required artifact"),
        });
    }
    let mut paths = BTreeSet::new();
    let mut summary = None;
    for artifact in artifacts {
        validate_relative_path(&artifact.path)?;
        validate_digest(&artifact.sha256, "gate artifact")?;
        if !paths.insert(artifact.path.as_str()) {
            return Err(QualityError::Evidence(format!(
                "duplicate artifact path {}",
                artifact.path
            )));
        }
        let path = root.input_from(manifest_path, &artifact.path)?;
        if digest_file(&path)? != artifact.sha256 {
            return Err(QualityError::Status {
                status: QualityStatus::StaleEvidence,
                message: format!("artifact changed: {}", artifact.path),
            });
        }
        if artifact.role == ArtifactRole::ValidationSummary
            && summary.replace(read_json(&path)?).is_some()
        {
            return Err(QualityError::Evidence(
                "manifest has duplicate validator summaries".to_string(),
            ));
        }
    }
    summary.ok_or_else(|| QualityError::Status {
        status: QualityStatus::IncompleteEvidence,
        message: "manifest lacks its validator summary".to_string(),
    })
}

/// Return the canonical command name for a gate kind.
fn gate_command_name(gate: GateKind) -> &'static str {
    match gate {
        GateKind::Nextest => "nextest",
        GateKind::Doctest => "doctest",
        GateKind::Coverage => "coverage",
        GateKind::ChangedMutation => "mutation-changed",
        GateKind::FullMutation => "mutation-aggregate",
    }
}

/// Validate gate result against its repository contract.
fn validate_gate_result(
    policy: &QualityPolicy,
    platform_id: &str,
    result: &GateResult,
    release: bool,
) -> Result<(), QualityError> {
    let valid = match result {
        GateResult::Nextest {
            tests,
            suites,
            failed,
            errors,
            timed_out,
            ..
        } => *tests > 0 && *suites > 0 && *failed == 0 && *errors == 0 && *timed_out == 0,
        GateResult::Doctest {
            passed,
            failed,
            summaries,
            ..
        } => *passed > 0 && *failed == 0 && *summaries > 0,
        GateResult::Coverage {
            enforcement,
            raw,
            adjusted,
            exceptions_used,
        } => {
            let meets_floor = policy
                .platforms
                .iter()
                .find(|platform| platform.id == platform_id)
                .and_then(PlatformPolicy::coverage_floor)
                .is_none_or(|floor| raw.at_least(&floor));
            raw.validate().is_ok()
                && adjusted.validate().is_ok()
                && (!enforcement.enforces_targets()
                    || coverage_meets_targets(policy, raw, adjusted))
                && meets_floor
                && (!release || *enforcement == CoverageEnforcement::ReleaseQuality)
                && *exceptions_used
                    == usize_to_u64(
                        policy
                            .exceptions
                            .records
                            .iter()
                            .filter(|value| matches!(value, QualityException::Coverage { .. }))
                            .count(),
                    )?
        }
        GateResult::ChangedMutation {
            missed,
            timed_out,
            unresolved,
            baseline_passed,
            ..
        } => *baseline_passed && *missed == 0 && *timed_out == 0 && *unresolved == 0,
        GateResult::FullMutation {
            raw_total,
            caught,
            missed,
            timed_out,
            unresolved,
            baseline_passed,
            shards,
            raw_kill_basis_points,
            adjusted_viable_kill_basis_points,
            ..
        } => {
            let viable = caught.saturating_add(*missed);
            *baseline_passed
                && *raw_total > 0
                && *timed_out == 0
                && *unresolved == 0
                && *shards == REQUIRED_MUTATION_SHARDS
                && *raw_kill_basis_points == caught.saturating_mul(10_000) / raw_total
                && *adjusted_viable_kill_basis_points
                    == caught.saturating_mul(10_000) / viable.max(1)
                && ratio_meets(
                    *caught,
                    *raw_total,
                    policy.targets.mutation.raw_viable_kill_basis_points,
                )
                && ratio_meets(
                    *caught,
                    viable,
                    policy.targets.mutation.adjusted_viable_kill_basis_points,
                )
        }
    };
    if valid {
        Ok(())
    } else {
        Err(QualityError::Status {
            status: QualityStatus::PolicyFailure,
            message: "typed gate result is failed, incomplete, or below agreement".to_string(),
        })
    }
}

#[cfg(test)]
#[path = "test_quality_tests.rs"]
mod tests;
