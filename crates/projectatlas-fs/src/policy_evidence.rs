//! Transient evidence of the policy bytes and selections consumed by one scan.

use super::{
    FsError, FsResult, HASH_BUFFER_BYTES, RootScanPolicy, ScanLimits, ScanOptions, claim_resource,
    git_config::ConfigEnvironment, linked_worktree_excluded_prefixes, open_regular_file,
};
use blake3::{Hash, Hasher};
use ignore::{IgnoreFileKind, IgnoreFileSource};
use projectatlas_core::{
    IndexWorkControl, IndexWorkFailure, IndexWorkResource, IndexWorkStage,
    MAX_GIT_WORKTREE_REGISTRATIONS,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// File roles are distinct even when an operator selects the same path twice.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum InputKind {
    /// Directly parsed system/global Git configuration.
    Config,
    /// Canonical ignore engine input and its parsing role.
    Ignore(IgnoreFileKind),
}

/// Bytes actually returned to one parser, or its observed absent candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
enum InputState {
    /// Directory enumeration or an attempted open found no input.
    Absent,
    /// Exact streamed prefix, with EOF distinguishing a complete input.
    Bytes {
        /// Digest of precisely the bytes returned to the parser.
        digest: Hash,
        /// Length of the observed prefix.
        bytes: u64,
        /// Whether the reader observed the end of the selected file.
        complete: bool,
    },
}

/// Canonical matcher policy together with its operation-owned input observation.
///
/// Matchers borrow [`Self::policy`]; consume the observation only after those
/// borrows have ended. No mutable process-wide policy or retained cache is used.
#[derive(Debug)]
pub struct ObservedRootScanPolicy {
    /// Canonical policy and matcher configuration.
    pub(super) policy: RootScanPolicy,
    /// Input capture shared only with this policy's readers.
    pub(super) capture: Arc<PolicyCapture>,
    /// Repository-derived prefixes consumed while constructing this policy.
    pub(super) worktree_prefixes: Vec<String>,
}

impl ObservedRootScanPolicy {
    /// Discover canonical policy with bounded evidence of its consumed inputs.
    ///
    /// # Errors
    /// Returns the existing policy discovery, resource or cooperative-work error.
    pub fn discover(
        root: &Path,
        options: &ScanOptions,
        limits: ScanLimits,
        control: &IndexWorkControl,
    ) -> FsResult<Self> {
        let capture = PolicyCapture::new(root, limits, control);
        let policy = RootScanPolicy::discover_recorded(root, options, control, Some(&capture));
        capture.check_failure()?;
        let policy = policy?;
        let worktree_prefixes =
            policy.options.exclude_path_prefixes[options.exclude_path_prefixes.len()..].to_vec();
        Ok(Self {
            policy,
            capture,
            worktree_prefixes,
        })
    }

    /// Borrow the canonical matcher; all existing precedence and alias rules apply.
    #[must_use]
    pub fn policy(&self) -> &RootScanPolicy {
        &self.policy
    }

    /// Close all policy readers and move their observations into immutable evidence.
    ///
    /// # Errors
    /// Returns an error if reader ownership or captured state is inconsistent.
    pub fn finish(self) -> FsResult<ScanPolicyEvidence> {
        let root = self.policy.root.clone();
        drop(self.policy);
        PolicyCapture::finish(self.capture, root, self.worktree_prefixes)
    }
}

/// Mutable capture shared only with this walk's canonical matcher readers.
#[derive(Debug)]
pub(super) struct PolicyCapture {
    /// Original cooperative work owner; no new timeout clock is introduced.
    control: IndexWorkControl,
    /// Bound evidence entries by policy families per admitted directory/registration.
    max_inputs: u64,
    /// Independent policy-stream byte bound, retaining nested files above 16 MiB.
    max_bytes: u64,
    /// Actual parser bytes, including repeated uncached loads.
    bytes: AtomicU64,
    /// An ignored I/O error or inconsistent observation cannot certify freshness.
    failed: AtomicBool,
    /// One record per input role/path in the selected scan context.
    inputs: Mutex<BTreeMap<(InputKind, PathBuf), InputState>>,
    /// Actual environment used for direct configuration parsing.
    environment: Mutex<Option<ConfigEnvironment>>,
    /// Retain typed work failures even if a dependency wraps or ignores its I/O error.
    work_failure: Mutex<Option<IndexWorkFailure>>,
}

impl PolicyCapture {
    /// Allocate a bounded capture without reading any input.
    pub(super) fn new(root: &Path, limits: ScanLimits, control: &IndexWorkControl) -> Arc<Self> {
        // Three local rule candidates and two linked-checkout pointer families.
        const INPUT_FAMILIES: [&str; 5] =
            [".ignore", ".gitignore", "info/exclude", ".git", "commondir"];
        let contexts = limits
            .max_entries()
            .saturating_add(u64::try_from(root.ancestors().count()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(MAX_GIT_WORKTREE_REGISTRATIONS).unwrap_or(u64::MAX));
        Arc::new(Self {
            control: control.clone(),
            max_inputs: contexts.saturating_mul(INPUT_FAMILIES.len() as u64),
            max_bytes: limits.max_source_bytes(),
            bytes: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            inputs: Mutex::new(BTreeMap::new()),
            environment: Mutex::new(None),
            work_failure: Mutex::new(None),
        })
    }

    /// Restore the original typed cooperative failure at the filesystem boundary.
    pub(super) fn check_failure(&self) -> FsResult<()> {
        let failure = self.work_failure.lock().map_err(|error| FsError::Io {
            path: PathBuf::new(),
            source: io::Error::other(error.to_string()),
        })?;
        if let Some(failure) = *failure {
            return Err(failure.into());
        }
        Ok(())
    }

    /// Preserve the first work failure without replacing a later error's cause.
    fn retain_work_failure(&self, failure: IndexWorkFailure) -> io::Error {
        self.failed.store(true, Ordering::Release);
        if let Ok(mut previous) = self.work_failure.lock() {
            previous.get_or_insert(failure);
        }
        io::Error::other(failure)
    }

    /// Bind the environment supplied to the parser, rather than sampling it separately.
    pub(super) fn environment(&self, value: &ConfigEnvironment) -> io::Result<()> {
        let mut selected = self
            .environment
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        if selected.as_ref().is_some_and(|previous| previous != value) {
            self.failed.store(true, Ordering::Release);
        }
        *selected = Some(value.clone());
        Ok(())
    }

    /// Freeze the completed reader inventory without cloning it or holding I/O locks.
    pub(super) fn finish(
        capture: Arc<Self>,
        root: PathBuf,
        worktree_prefixes: Vec<String>,
    ) -> FsResult<ScanPolicyEvidence> {
        let error = |message| FsError::Io {
            path: root.clone(),
            source: io::Error::other(message),
        };
        let capture = Arc::try_unwrap(capture)
            .map_err(|_remaining| error("policy readers remain after traversal"))?;
        capture.check_failure()?;
        let inputs = capture
            .inputs
            .into_inner()
            .map_err(|_poisoned| error("policy input capture lock poisoned"))?;
        let environment = capture
            .environment
            .into_inner()
            .map_err(|_poisoned| error("policy environment capture lock poisoned"))?
            .ok_or_else(|| error("policy environment was not captured"))?;
        Ok(ScanPolicyEvidence {
            root,
            environment,
            worktree_prefixes,
            inputs,
            failed: capture.failed.into_inner(),
            max_bytes: capture.max_bytes,
        })
    }

    /// Record the exact complete buffer already supplied to the Git config parser.
    pub(super) fn config(&self, path: &Path, bytes: Option<&[u8]>) -> io::Result<()> {
        let state = if let Some(bytes) = bytes {
            self.charge(bytes.len())?;
            InputState::Bytes {
                digest: blake3::hash(bytes),
                bytes: bytes.len() as u64,
                complete: true,
            }
        } else {
            InputState::Absent
        };
        self.record(InputKind::Config, path, state)
    }

    /// Preserve one actual observation without holding a lock during filesystem I/O.
    fn record(&self, kind: InputKind, path: &Path, state: InputState) -> io::Result<()> {
        self.charge(0)?;
        let path = std::path::absolute(path)?;
        let mut inputs = self
            .inputs
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let key = (kind, path);
        if let Some(previous) = inputs.get(&key) {
            if previous != &state {
                self.failed.store(true, Ordering::Release);
            }
        } else {
            let observed = inputs.len().saturating_add(1) as u64;
            if observed > self.max_inputs {
                return Err(self.retain_work_failure(IndexWorkFailure::resource_limit(
                    IndexWorkStage::RepositoryTraversal,
                    IndexWorkResource::Entries,
                    self.max_inputs,
                    observed,
                )));
            }
            inputs.insert(key, state);
        }
        Ok(())
    }

    /// Meter each streamed parser read against the original cancellation/deadline.
    fn charge(&self, bytes: usize) -> io::Result<()> {
        self.control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(|failure| self.retain_work_failure(failure))?;
        claim_resource(
            &self.bytes,
            bytes as u64,
            self.max_bytes,
            IndexWorkStage::RepositoryTraversal,
            IndexWorkResource::SourceBytes,
        )
        .map_err(|failure| self.retain_work_failure(failure))
    }
}

/// Canonical parser I/O adapter retaining this scan's shared capture.
#[derive(Debug)]
pub(super) struct PolicyFileSource(pub(super) Arc<PolicyCapture>);

impl IgnoreFileSource for PolicyFileSource {
    fn open(&self, path: &Path, kind: IgnoreFileKind) -> io::Result<Box<dyn Read + Send>> {
        match open_regular_file(path) {
            Ok(file) => Ok(Box::new(CapturedReader {
                file,
                capture: Arc::clone(&self.0),
                path: path.to_path_buf(),
                kind,
                hasher: Hasher::new(),
                bytes: 0,
                complete: false,
            })),
            Err(error) => {
                if error.kind() == io::ErrorKind::NotFound {
                    self.0
                        .record(InputKind::Ignore(kind), path, InputState::Absent)?;
                } else {
                    self.0.failed.store(true, Ordering::Release);
                }
                Err(error)
            }
        }
    }

    fn absent(&self, path: &Path, kind: IgnoreFileKind) -> io::Result<()> {
        self.0
            .record(InputKind::Ignore(kind), path, InputState::Absent)
    }
}

/// Stream the exact bytes returned to the dependency's existing parser.
struct CapturedReader {
    /// Selected regular file, preserving intentional operator aliases.
    file: File,
    /// This scan's bounded capture.
    capture: Arc<PolicyCapture>,
    /// Actual selected path, before any later namespace restoration.
    path: PathBuf,
    /// Canonical parser role.
    kind: IgnoreFileKind,
    /// Digest of bytes returned by this reader.
    hasher: Hasher,
    /// Number of bytes returned, even when the parser consumes only one line.
    bytes: u64,
    /// Whether the parser observed EOF.
    complete: bool,
}

impl Read for CapturedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.capture.charge(0)?;
        let read = self
            .file
            .read(buffer)
            .inspect_err(|_error| self.capture.failed.store(true, Ordering::Release))?;
        self.capture
            .charge(read)
            .inspect_err(|_error| self.capture.failed.store(true, Ordering::Release))?;
        self.hasher.update(&buffer[..read]);
        self.bytes = self.bytes.saturating_add(read as u64);
        self.complete |= read == 0 && !buffer.is_empty();
        Ok(read)
    }
}

impl Drop for CapturedReader {
    fn drop(&mut self) {
        if self
            .capture
            .record(
                InputKind::Ignore(self.kind),
                &self.path,
                InputState::Bytes {
                    digest: self.hasher.finalize(),
                    bytes: self.bytes,
                    complete: self.complete,
                },
            )
            .is_err()
        {
            self.capture.failed.store(true, Ordering::Release);
        }
    }
}

/// Policy inputs and root selection actually consumed by a completed scan.
///
/// This is operation-owned evidence, not an authority for reuse across requests.
/// It does not claim a globally atomic filesystem snapshot.
#[derive(Debug)]
pub struct ScanPolicyEvidence {
    /// Canonical source context for every recorded input.
    root: PathBuf,
    /// Environment selection used by the direct Git config parser.
    environment: ConfigEnvironment,
    /// Repository-derived selection actually used by the walker.
    worktree_prefixes: Vec<String>,
    /// Reader observations, no whole policy file retention.
    inputs: BTreeMap<(InputKind, PathBuf), InputState>,
    /// Whether any parser observation was incomplete or inconsistent.
    failed: bool,
    /// Independent bound for one fresh validation pass over consumed inputs.
    max_bytes: u64,
}

impl ScanPolicyEvidence {
    /// Check consumed input bytes and selections against current local state.
    ///
    /// # Errors
    /// Returns cooperative-work failures or filesystem inspection errors. A
    /// changed consumed input returns `Ok(false)` and must not authorize a read.
    pub fn is_current(&self, control: &IndexWorkControl) -> FsResult<bool> {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        if self.failed
            || self.environment != ConfigEnvironment::capture()
            || self.worktree_prefixes != linked_worktree_excluded_prefixes(&self.root, control)?
        {
            return Ok(false);
        }
        let validated_bytes = AtomicU64::new(0);
        for ((_kind, path), expected) in &self.inputs {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            let current = match open_regular_file(path) {
                Ok(file) => Some(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(source) => {
                    return Err(FsError::Io {
                        path: path.clone(),
                        source,
                    });
                }
            };
            match (expected, current) {
                (InputState::Absent, None) => {}
                (
                    InputState::Bytes {
                        digest,
                        bytes,
                        complete,
                    },
                    Some(mut file),
                ) => {
                    let mut hasher = Hasher::new();
                    let mut remaining = *bytes;
                    let mut buffer = [0; HASH_BUFFER_BYTES];
                    while remaining > 0 {
                        control.check(IndexWorkStage::RepositoryTraversal)?;
                        let length = buffer.len().min(remaining as usize);
                        let read =
                            file.read(&mut buffer[..length])
                                .map_err(|source| FsError::Io {
                                    path: path.clone(),
                                    source,
                                })?;
                        if read == 0 {
                            return Ok(false);
                        }
                        claim_resource(
                            &validated_bytes,
                            read as u64,
                            self.max_bytes,
                            IndexWorkStage::RepositoryTraversal,
                            IndexWorkResource::SourceBytes,
                        )?;
                        hasher.update(&buffer[..read]);
                        remaining -= read as u64;
                    }
                    if hasher.finalize() != *digest
                        || (*complete
                            && file.read(&mut buffer[..1]).map_err(|source| FsError::Io {
                                path: path.clone(),
                                source,
                            })? != 0)
                    {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scan_repo_controlled_with_policy_evidence, scan_repo_controlled_with_work};
    use projectatlas_core::IndexCancellation;
    use std::error::Error;
    use std::fs;
    use std::io::Write;
    use std::time::Duration;

    /// Keep all fixture work under one bounded cooperative owner.
    fn control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(30)))
    }

    #[test]
    fn observed_policy_preserves_selection_precedence_and_selected_aliases()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("nested"))?;
        fs::write(root.join(".gitignore"), "*.tmp\n!keep.tmp\n.gitignore\n")?;
        fs::write(root.join("nested/.ignore"), "blocked.rs\n.ignore\n")?;
        fs::write(root.join("keep.tmp"), "kept\n")?;
        fs::write(root.join("discard.tmp"), "excluded\n")?;
        fs::write(root.join("nested/keep.rs"), "fn kept() {}\n")?;
        fs::write(root.join("nested/blocked.rs"), "fn excluded() {}\n")?;
        let options = ScanOptions::default();
        let work = control();
        let expected =
            scan_repo_controlled_with_work(&root, &options, ScanLimits::default(), &work)?;
        let alias = temp.path().join("selected-root");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, &alias)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&root, &alias)?;
        for selected in [root.clone(), alias] {
            let (actual, evidence) = scan_repo_controlled_with_policy_evidence(
                &selected,
                &options,
                ScanLimits::default(),
                &work,
            )?;
            if actual != expected || !evidence.is_current(&work)? {
                return Err(
                    "observed policy changed canonical selection/work or rejected a stable alias"
                        .into(),
                );
            }
            if !actual.nodes.iter().any(|node| node.path == "keep.tmp")
                || actual
                    .nodes
                    .iter()
                    .any(|node| node.path == "discard.tmp" || node.path == "nested/blocked.rs")
            {
                return Err("canonical local negation or exclusion controls failed".into());
            }
            fs::write(root.join("nested/.ignore"), ".ignore\nkeep.rs\n")?;
            if evidence.is_current(&work)? {
                return Err("consumed nested rule change retained current evidence".into());
            }
            fs::write(root.join("nested/.ignore"), "blocked.rs\n.ignore\n")?;
        }
        Ok(())
    }

    #[test]
    fn outside_root_worktree_registration_preserves_equal_selection_evidence()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let run_git = |command: &mut std::process::Command| -> Result<(), Box<dyn Error>> {
            let output = command
                .env("GIT_CONFIG_GLOBAL", "")
                .env("GIT_CONFIG_SYSTEM", "")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()?;
            if !output.status.success() {
                return Err(
                    io::Error::other(String::from_utf8_lossy(&output.stderr).into_owned()).into(),
                );
            }
            Ok(())
        };
        run_git(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .arg(&root),
        )?;
        run_git(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args([
                    "-c",
                    "user.name=Policy Fixture",
                    "-c",
                    "user.email=policy-fixture@example.invalid",
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "-m",
                    "fixture",
                ]),
        )?;
        fs::write(root.join("source.rs"), "fn kept() {}\n")?;
        let work = control();
        let (_scan, evidence) = scan_repo_controlled_with_policy_evidence(
            &root,
            &ScanOptions::default(),
            ScanLimits::default(),
            &work,
        )?;
        run_git(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["worktree", "add", "--quiet", "--detach"])
                .arg(temp.path().join("outside-checkout"))
                .arg("HEAD"),
        )?;
        if !evidence.is_current(&work)? {
            return Err("an outside-root registration changed no exclusion but invalidated semantic selection evidence".into());
        }
        Ok(())
    }

    #[test]
    fn observed_nested_policy_retains_inputs_above_external_file_cap() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("nested"))?;
        let rules = root.join("nested/.ignore");
        let mut file = File::create(&rules)?;
        file.write_all(b".ignore\n")?;
        let mut comment = [b'x'; HASH_BUFFER_BYTES];
        comment[0] = b'#';
        comment[HASH_BUFFER_BYTES - 1] = b'\n';
        for _ in 0..(17 * 1_024 * 1_024 / HASH_BUFFER_BYTES) {
            file.write_all(&comment)?;
        }
        drop(file);
        fs::write(root.join("nested/source.rs"), "fn kept() {}\n")?;
        let work = control();
        let (scan, evidence) = scan_repo_controlled_with_policy_evidence(
            root,
            &ScanOptions::default(),
            ScanLimits::default(),
            &work,
        )?;
        if !evidence.is_current(&work)?
            || !scan
                .nodes
                .iter()
                .any(|node| node.path == "nested/source.rs")
            || scan.nodes.iter().any(|node| node.path == "nested/.ignore")
        {
            return Err("large self-ignored local policy lost canonical selection or inherited the external cap".into());
        }
        Ok(())
    }

    #[test]
    fn parser_work_failures_survive_dependency_io_wrapping() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let rules = temp.path().join("rules");
        fs::write(&rules, "a rule that exceeds the selected byte allowance\n")?;
        let work = control();
        let capture = PolicyCapture::new(temp.path(), ScanLimits::new(10, 4, 1), &work);
        let mut reader =
            PolicyFileSource(Arc::clone(&capture)).open(&rules, IgnoreFileKind::Rules)?;
        let mut buffer = [0; HASH_BUFFER_BYTES];
        if reader.read(&mut buffer).is_ok() {
            return Err("parser input exceeded its bounded stream allowance".into());
        }
        drop(reader);
        if !matches!(
            capture.check_failure(),
            Err(FsError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::SourceBytes,
                    ..
                }
            ))
        ) {
            return Err("dependency I/O wrapping erased the typed source-byte failure".into());
        }
        let cancellation = IndexCancellation::new();
        let work = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(30)));
        let capture = PolicyCapture::new(temp.path(), ScanLimits::default(), &work);
        let mut reader =
            PolicyFileSource(Arc::clone(&capture)).open(&rules, IgnoreFileKind::Rules)?;
        cancellation.cancel();
        if reader.read(&mut buffer).is_ok() {
            return Err("parser reader ignored operation cancellation".into());
        }
        drop(reader);
        if !matches!(
            capture.check_failure(),
            Err(FsError::IndexWork(IndexWorkFailure::Cancelled { .. }))
        ) {
            return Err("dependency I/O wrapping erased the typed cancellation".into());
        }
        Ok(())
    }

    #[test]
    fn absent_policy_candidates_and_inconsistent_reads_cannot_certify_freshness()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("source.rs"), "fn kept() {}\n")?;
        let work = control();
        let observed = ObservedRootScanPolicy::discover(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::default(),
            &work,
        )?;
        let rules = temp.path().join(".ignore");
        let reader = PolicyFileSource(Arc::clone(&observed.capture));
        reader.absent(&rules, IgnoreFileKind::Rules)?;
        drop(reader);
        let evidence = observed.finish()?;
        if !evidence.is_current(&work)? {
            return Err("an unchanged absent policy input was rejected".into());
        }
        fs::write(&rules, "source.rs\n")?;
        if evidence.is_current(&work)? {
            return Err("creation of an omitted policy candidate retained current evidence".into());
        }
        let observed = ObservedRootScanPolicy::discover(
            temp.path(),
            &ScanOptions::default(),
            ScanLimits::default(),
            &work,
        )?;
        let reader = PolicyFileSource(Arc::clone(&observed.capture));
        let mut first = reader.open(&rules, IgnoreFileKind::Rules)?;
        first.read_to_end(&mut Vec::new())?;
        drop(first);
        fs::write(&rules, "different.rs\n")?;
        let mut second = reader.open(&rules, IgnoreFileKind::Rules)?;
        second.read_to_end(&mut Vec::new())?;
        drop(second);
        drop(reader);
        let evidence = observed.finish()?;
        if evidence.is_current(&work)? {
            return Err(
                "inconsistent repeated policy loads were deduplicated into a current result".into(),
            );
        }
        Ok(())
    }
}
