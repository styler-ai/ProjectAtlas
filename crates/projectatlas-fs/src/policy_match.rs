//! Bounded, pass-local reuse of the scanner's native hierarchical ignore engine.

use super::{
    FsError, RootScanPolicy, ScanLimits, normalize_repo_path, should_skip_path,
    validate_optional_policy_file, visit_local_ignore_inputs,
};
use ignore::IncrementalIgnore;
use projectatlas_core::{IndexWorkControl, IndexWorkStage};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Default cumulative local policy input admitted by one matcher pass.
const DEFAULT_POLICY_BYTES: u64 = 128 * 1_024 * 1_024;
/// Soft retained child-context count, checked between targets.
const CACHED_CONTEXTS: usize = 256;
/// Soft retained policy-input and native/wrapper key estimate, checked between targets.
const CACHED_INPUT_BYTES: u64 = 4 * 1_024 * 1_024;
/// Estimate for native/wrapper context bookkeeping, excluding input and path bytes.
const CONTEXT_OVERHEAD_BYTES: u64 = 512;

/// Cumulative hierarchical policy work limits for one matcher pass.
///
/// A fresh publication-revalidation pass has its own limits, while borrowing
/// the same whole-operation cancellation/deadline. Compiled external global
/// rules belong to policy discovery and are shared without parsing them again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyMatchLimits {
    /// Cumulative root, ancestor and child context load ceiling.
    directory_contexts: u64,
    /// Cumulative local hierarchical input byte ceiling.
    policy_bytes: u64,
}

impl PolicyMatchLimits {
    /// Set cumulative directory loads and local policy input bytes, including reloads.
    #[must_use]
    pub const fn new(max_directory_contexts: u64, max_policy_bytes: u64) -> Self {
        Self {
            directory_contexts: max_directory_contexts,
            policy_bytes: max_policy_bytes,
        }
    }

    /// Return the maximum cumulative root, ancestor and child context loads.
    #[must_use]
    pub const fn max_directory_contexts(self) -> u64 {
        self.directory_contexts
    }

    /// Return the maximum cumulative local hierarchical policy input bytes.
    #[must_use]
    pub const fn max_policy_bytes(self) -> u64 {
        self.policy_bytes
    }
}

impl Default for PolicyMatchLimits {
    fn default() -> Self {
        Self::new(ScanLimits::default().max_entries(), DEFAULT_POLICY_BYTES)
    }
}

/// Work resource exhausted by a pass-local policy matcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyMatchResource {
    /// Root, ancestor or child directory policy context loads, including reloads.
    DirectoryContexts,
    /// Local hierarchical policy input admitted for parsing, including reloads.
    PolicyBytes,
}

/// Failure from the additive batch matcher API.
///
/// Existing filesystem and cooperative-work errors retain their typed cause;
/// a policy quota is distinct from an incremental scan limit eligible for retry.
#[derive(Debug, Error)]
pub enum PolicyMatchError {
    /// A previous failure made the pass unusable; start a fresh pass to retry.
    #[error("policy matching pass previously failed; create a fresh matcher")]
    Invalidated,
    /// Native policy discovery, validation, matching or cooperative work failed.
    #[error("{0}")]
    Filesystem(#[from] FsError),
    /// Cumulative policy work would exceed this pass's declared limit.
    #[error("policy matching exceeds {resource:?}: observed {observed}, limit {limit}")]
    Limit {
        /// Resource that would be exhausted.
        resource: PolicyMatchResource,
        /// Maximum admitted cumulative work.
        limit: u64,
        /// Work required by the attempted next load.
        observed: u64,
    },
}

/// A bounded native policy matcher owned by one operation or verification pass.
///
/// Loaded directory policy is stable within this pass. Create another matcher
/// for a fresh pass after local hierarchical policy edits; external/global
/// edits require fresh [`RootScanPolicy::discover`] as well. Child contexts may be evicted between
/// targets; reloads are charged again and can observe edits. This is cooperative
/// work accounting, not a filesystem snapshot or kernel-I/O preemption guarantee.
#[derive(Debug)]
pub struct RootScanMatcher<'a> {
    /// Refuse reuse after partial native loading or any other failed admission.
    failed: bool,
    /// Captured root selection and compiled external policy.
    policy: &'a RootScanPolicy,
    /// Original operation deadline and cancellation token.
    control: &'a IndexWorkControl,
    /// Cumulative pass accounting, independent of child-cache eviction.
    limits: PolicyMatchLimits,
    /// Root/ancestor rules loaded once and shared by subsequent cache resets.
    seed: Option<IncrementalIgnore>,
    /// Native hierarchy for the currently retained child contexts.
    matcher: Option<IncrementalIgnore>,
    /// Validated children whose policy the native matcher is allowed to load.
    contexts: BTreeSet<PathBuf>,
    /// Estimated retained child inputs, native keys and wrapper bookkeeping.
    cached_input_bytes: u64,
    /// Root, ancestor and child context admissions, including reloads.
    directory_contexts: u64,
    /// Local hierarchical bytes admitted for parsing, including reloads.
    policy_bytes: u64,
}

impl RootScanPolicy {
    /// Reuse hierarchical matching under default cumulative per-pass limits.
    ///
    /// This borrows the existing operation control and compiled external rules.
    /// Unlike the single-path API, loaded local rules are reused within the pass.
    #[must_use]
    pub fn batch_matcher<'a>(&'a self, control: &'a IndexWorkControl) -> RootScanMatcher<'a> {
        self.batch_matcher_with_limits(control, PolicyMatchLimits::default())
    }

    /// Reuse hierarchical matching with explicit cumulative per-pass limits.
    #[must_use]
    pub fn batch_matcher_with_limits<'a>(
        &'a self,
        control: &'a IndexWorkControl,
        limits: PolicyMatchLimits,
    ) -> RootScanMatcher<'a> {
        RootScanMatcher {
            failed: false,
            policy: self,
            control,
            limits,
            seed: None,
            matcher: None,
            contexts: BTreeSet::new(),
            cached_input_bytes: 0,
            directory_contexts: 0,
            policy_bytes: 0,
        }
    }
}

impl RootScanMatcher<'_> {
    /// Classify one path using bounded reused native hierarchical policy.
    ///
    /// Selected-root aliases, Atlas exclusions and directory semantics match
    /// [`RootScanPolicy::excludes_path`]. Callers admitting derived source paths
    /// must retain their native-prefix guard before invoking this policy reader.
    ///
    /// # Errors
    ///
    /// Returns typed filesystem/cooperative-work errors or a policy quota
    /// refusal before the next disallowed local policy input is parsed. Every
    /// failure invalidates the pass; subsequent calls return
    /// [`PolicyMatchError::Invalidated`] until a fresh matcher is constructed.
    pub fn excludes_path(&mut self, path: &Path) -> Result<bool, PolicyMatchError> {
        if self.failed {
            return Err(PolicyMatchError::Invalidated);
        }
        let result = self.classify_path(path);
        self.failed = result.is_err();
        result
    }

    /// Perform one admission; the public boundary makes any failure terminal.
    fn classify_path(&mut self, path: &Path) -> Result<bool, PolicyMatchError> {
        self.check()?;
        let absolute = self.policy.absolute_target(path);
        if should_skip_path(&self.policy.root, &absolute, &self.policy.options) {
            return Ok(true);
        }
        let relative = normalize_repo_path(&self.policy.root, &absolute).map_err(FsError::from)?;
        let relative = Path::new(&relative);
        if relative == Path::new(".")
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Ok(false);
        }
        self.load_seed(relative)?;
        // A native match can load all ancestors. Admit them one at a time so
        // no pruned directory's own input is inspected, let alone parsed.
        let mut parents = relative
            .ancestors()
            .skip(1)
            .filter(|path| !path.as_os_str().is_empty())
            .collect::<Vec<_>>();
        parents.reverse();
        let new_contexts = parents
            .iter()
            .filter(|parent| !self.contexts.contains(**parent))
            .count();
        let new_key_bytes = parents
            .iter()
            .filter(|parent| !self.contexts.contains(**parent))
            .fold(0_u64, |bytes, parent| {
                bytes.saturating_add(self.context_key_bytes(parent))
            });
        if new_contexts != 0
            && (self.contexts.len().saturating_add(new_contexts) > CACHED_CONTEXTS
                || self.cached_input_bytes.saturating_add(new_key_bytes) > CACHED_INPUT_BYTES)
        {
            self.matcher = self.seed.clone();
            self.contexts.clear();
            self.cached_input_bytes = 0;
        }
        for parent in parents {
            if self.matches(parent, true)? {
                return Ok(true);
            }
            if !self.contexts.contains(parent) {
                let bytes = self.admit_directory(&self.policy.root.join(parent))?;
                self.cached_input_bytes = self
                    .cached_input_bytes
                    .saturating_add(bytes)
                    .saturating_add(self.context_key_bytes(parent));
                self.contexts.insert(parent.to_path_buf());
            }
        }
        self.matches(
            relative,
            absolute.metadata().is_ok_and(|metadata| metadata.is_dir()),
        )
    }

    /// Check the original cooperative control at every admitted policy boundary.
    fn check(&self) -> Result<(), PolicyMatchError> {
        self.control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(FsError::from)?;
        Ok(())
    }

    /// Charge and validate all enabled inputs before native parsing can start.
    fn admit_directory(&mut self, directory: &Path) -> Result<u64, PolicyMatchError> {
        self.check()?;
        let observed = self.directory_contexts.saturating_add(1);
        enforce_limit(
            PolicyMatchResource::DirectoryContexts,
            self.limits.directory_contexts,
            observed,
        )?;
        self.directory_contexts = observed;
        let before = self.policy_bytes;
        visit_local_ignore_inputs(directory, true, true, |path| {
            self.check()?;
            let bytes = validate_optional_policy_file(path)?;
            let observed = self.policy_bytes.saturating_add(bytes);
            enforce_limit(
                PolicyMatchResource::PolicyBytes,
                self.limits.policy_bytes,
                observed,
            )?;
            self.policy_bytes = observed;
            Ok::<_, PolicyMatchError>(())
        })?;
        Ok(self.policy_bytes.saturating_sub(before))
    }

    /// Warm only root/ancestor rules, so resetting children never reparses the seed.
    fn load_seed(&mut self, relative: &Path) -> Result<(), PolicyMatchError> {
        if self.seed.is_some() {
            return Ok(());
        }
        let root = self.policy.root.clone();
        for ancestor in root.ancestors() {
            self.admit_directory(ancestor)?;
        }
        self.check()?;
        let mut matcher = self
            .policy
            .walker
            .build_matchers()
            .into_iter()
            .next()
            .ok_or_else(|| FsError::Io {
                path: root.clone(),
                source: io::Error::other("source policy has no selected ignore root"),
            })?;
        if let Some(first) = relative.components().next() {
            // One component has no child ancestors for the engine to load.
            native_match(&mut matcher, &root, Path::new(first.as_os_str()), false)?;
        }
        self.check()?;
        self.seed = Some(matcher.clone());
        self.matcher = Some(matcher);
        Ok(())
    }

    /// Account for absolute native keys plus native and wrapper relative keys.
    fn context_key_bytes(&self, relative: &Path) -> u64 {
        CONTEXT_OVERHEAD_BYTES
            .saturating_add(self.policy.root.as_os_str().len() as u64)
            .saturating_add((relative.as_os_str().len() as u64).saturating_mul(3))
    }

    /// Use only the native matcher whose parent inputs have already been admitted.
    fn matches(&mut self, relative: &Path, is_dir: bool) -> Result<bool, PolicyMatchError> {
        self.check()?;
        let matcher = self.matcher.as_mut().ok_or_else(|| FsError::Io {
            path: self.policy.root.clone(),
            source: io::Error::other("source policy matcher has no loaded root"),
        })?;
        let matched = native_match(matcher, &self.policy.root, relative, is_dir)?;
        self.check()?;
        Ok(matched)
    }
}

/// Preserve canonical engine failures instead of accepting a partially loaded match.
fn native_match(
    matcher: &mut IncrementalIgnore,
    root: &Path,
    relative: &Path,
    is_dir: bool,
) -> Result<bool, FsError> {
    let (matched, error) = matcher.matched_with_errors(relative, is_dir);
    if let Some(error) = error {
        return Err(FsError::Io {
            path: root.join(relative),
            source: io::Error::other(error),
        });
    }
    Ok(matched.is_ignore())
}

/// Refuse cumulative work before starting the next unadmitted parse.
fn enforce_limit(
    resource: PolicyMatchResource,
    limit: u64,
    observed: u64,
) -> Result<(), PolicyMatchError> {
    if observed > limit {
        return Err(PolicyMatchError::Limit {
            resource,
            limit,
            observed,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanOptions;
    use projectatlas_core::{IndexCancellation, IndexWorkFailure};
    use std::error::Error;
    use std::fs;
    use std::time::Duration;

    /// Report causal fixture failures without panicking.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    #[test]
    fn batch_policy_reuses_loaded_hierarchy_and_refreshes_next_pass() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("nested"))?;
        fs::write(temp.path().join(".ignore"), "root-hidden.md\n")?;
        fs::write(temp.path().join("nested/.gitignore"), "hidden.md\n")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let mut matcher = policy.batch_matcher(&control);
        require(
            matcher.excludes_path(Path::new("nested/hidden.md"))?,
            "nested rule not loaded",
        )?;
        let work = (matcher.directory_contexts, matcher.policy_bytes);
        for target in 0..1024 {
            require(
                !matcher.excludes_path(&PathBuf::from(format!("nested/missing-{target}.md")))?,
                "unmatched target excluded",
            )?;
        }
        require(
            (matcher.directory_contexts, matcher.policy_bytes) == work,
            "each target rebuilt policy",
        )?;
        fs::write(temp.path().join(".ignore"), "root-visible.md\n")?;
        fs::write(temp.path().join("nested/.gitignore"), "visible.md\n")?;
        require(
            matcher.excludes_path(Path::new("root-hidden.md"))?,
            "loaded root rules unexpectedly changed",
        )?;
        require(
            matcher.excludes_path(Path::new("nested/hidden.md"))?,
            "loaded child rules unexpectedly changed",
        )?;
        let mut fresh = policy.batch_matcher(&control);
        require(
            !fresh.excludes_path(Path::new("root-hidden.md"))?,
            "new pass retained old root rules",
        )?;
        require(
            !fresh.excludes_path(Path::new("nested/hidden.md"))?,
            "new pass retained old child rules",
        )?;
        require(
            fresh.excludes_path(Path::new("root-visible.md"))?,
            "new pass missed root edit",
        )?;
        require(
            fresh.excludes_path(Path::new("nested/visible.md"))?,
            "new pass missed child edit",
        )?;
        require(
            !policy.excludes_path(Path::new("nested/hidden.md"))?,
            "single-path API lost fresh semantics",
        )?;
        Ok(())
    }

    #[test]
    fn batch_policy_refuses_work_before_disallowed_parsing() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("nested"))?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let mut matcher =
            policy.batch_matcher_with_limits(&control, PolicyMatchLimits::new(0, u64::MAX));
        require(
            matches!(
                matcher.excludes_path(Path::new("nested/missing.md")),
                Err(PolicyMatchError::Limit {
                    resource: PolicyMatchResource::DirectoryContexts,
                    limit: 0,
                    observed: 1
                })
            ),
            "context limit was not typed",
        )?;
        let mut matcher = policy.batch_matcher(&control);
        matcher.excludes_path(Path::new("missing.md"))?;
        let seed_bytes = matcher.policy_bytes;
        matcher.limits = PolicyMatchLimits::new(u64::MAX, seed_bytes);
        fs::write(temp.path().join("nested/.ignore"), "hidden.md\n")?;
        require(
            matches!(matcher.excludes_path(Path::new("nested/missing.md")),
            Err(PolicyMatchError::Limit { resource: PolicyMatchResource::PolicyBytes, limit, observed })
                if limit == seed_bytes && observed == seed_bytes + 10),
            "child input was not reserved before parsing",
        )?;
        require(
            !matcher.contexts.contains(Path::new("nested")),
            "refused policy entered native context cache",
        )?;
        Ok(())
    }

    #[test]
    fn batch_policy_eviction_charges_reloads_without_reparsing_root() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join(".ignore"), "seed-hidden.md\n")?;
        for i in 0..=CACHED_CONTEXTS {
            let directory = temp.path().join(format!("dir-{i}"));
            fs::create_dir(&directory)?;
            fs::write(directory.join(".ignore"), "child-hidden.md\n")?;
        }
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let mut matcher = policy.batch_matcher(&control);
        matcher.excludes_path(Path::new("seed-hidden.md"))?;
        let seed_work = (matcher.directory_contexts, matcher.policy_bytes);
        fs::write(temp.path().join(".ignore"), "seed-visible.md\n")?;
        for i in 0..=CACHED_CONTEXTS {
            require(
                matcher.excludes_path(&PathBuf::from(format!("dir-{i}/child-hidden.md")))?,
                "child rule lost during eviction",
            )?;
        }
        require(
            matcher.contexts.len() == 1,
            "child contexts were not evicted",
        )?;
        let loads = CACHED_CONTEXTS as u64 + 1;
        require(
            matcher.directory_contexts == seed_work.0 + loads,
            "root context was reloaded during eviction",
        )?;
        require(
            matcher.policy_bytes == seed_work.1 + loads * 16,
            "policy inputs were not charged once per context",
        )?;
        require(
            matcher.excludes_path(Path::new("seed-hidden.md"))?,
            "eviction reparsed the root seed",
        )?;
        let next_bytes = matcher.policy_bytes + 16;
        matcher.excludes_path(Path::new("dir-0/child-hidden.md"))?;
        require(
            matcher.policy_bytes == next_bytes,
            "evicted child reload was not charged",
        )?;
        // Force byte-threshold eviction while count is still small, then refuse
        // a reload at the cumulative work limit instead of resetting its budget.
        matcher.cached_input_bytes = CACHED_INPUT_BYTES;
        matcher.limits = PolicyMatchLimits::new(u64::MAX, matcher.policy_bytes);
        require(
            matches!(
                matcher.excludes_path(Path::new("dir-1/child-hidden.md")),
                Err(PolicyMatchError::Limit {
                    resource: PolicyMatchResource::PolicyBytes,
                    ..
                })
            ),
            "eviction restarted the policy work budget",
        )?;
        require(
            matcher.contexts.is_empty(),
            "byte threshold did not evict child contexts",
        )?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn batch_policy_preserves_alias_admission_and_prunes_nonregular_children()
    -> Result<(), Box<dyn Error>> {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let alias = temp.path().join("selected");
        fs::create_dir_all(root.join("private"))?;
        symlink(&root, &alias)?;
        fs::write(root.join(".gitignore"), "private/\n!private/visible.md\n")?;
        mkfifo(&root.join("private/.ignore"), Mode::S_IRUSR | Mode::S_IWUSR)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&alias, &ScanOptions::default(), &control)?;
        let mut matcher = policy.batch_matcher(&control);
        for target in [
            alias.join("private/visible.md"),
            root.join("private/visible.md"),
            PathBuf::from("private/visible.md"),
        ] {
            require(
                matcher.excludes_path(&target)?,
                "batch reopened a pruned parent or lost selected alias",
            )?;
        }
        require(
            matcher.contexts.is_empty(),
            "pruned directory policy was admitted",
        )?;
        fs::write(root.join(".gitignore"), "")?;
        require(
            matches!(policy.batch_matcher(&control).excludes_path(Path::new("private/missing.md")),
            Err(PolicyMatchError::Filesystem(FsError::Io { source, .. })) if source.kind() == io::ErrorKind::InvalidInput),
            "admitted non-regular child reached the native parser",
        )?;
        fs::remove_file(root.join("private/.ignore"))?;
        fs::write(root.join("policy-rules"), "hidden.md\n")?;
        symlink(root.join("policy-rules"), root.join("private/.ignore"))?;
        require(
            policy
                .batch_matcher(&control)
                .excludes_path(Path::new("private/hidden.md"))?,
            "regular policy symlink was rejected",
        )?;
        Ok(())
    }

    #[test]
    fn failed_policy_pass_cannot_accept_partially_loaded_rules_on_retry()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("nested"))?;
        fs::write(temp.path().join("nested/.ignore"), "hidden.md\n[z-a]\n")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let mut matcher = policy.batch_matcher(&control);
        require(
            matches!(
                matcher.excludes_path(Path::new("nested/hidden.md")),
                Err(PolicyMatchError::Filesystem(FsError::Io { .. }))
            ),
            "malformed child policy did not exercise a native parse error",
        )?;
        fs::write(temp.path().join("nested/.ignore"), "hidden.md\n")?;
        require(
            matches!(
                matcher.excludes_path(Path::new("nested/hidden.md")),
                Err(PolicyMatchError::Invalidated)
            ),
            "failed matcher accepted cached partial native policy",
        )?;
        require(
            policy
                .batch_matcher(&control)
                .excludes_path(Path::new("nested/hidden.md"))?,
            "fresh pass could not recover repaired policy",
        )?;
        Ok(())
    }

    #[test]
    fn batch_policy_preserves_original_cancellation_and_deadline() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let mut matcher = policy.batch_matcher(&control);
        matcher.excludes_path(Path::new("missing.md"))?;
        control.cancel();
        require(
            matches!(
                matcher.excludes_path(Path::new("missing.md")),
                Err(PolicyMatchError::Filesystem(FsError::IndexWork(
                    IndexWorkFailure::Cancelled { .. }
                )))
            ),
            "cached matching replaced cancellation",
        )?;
        let expired = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::ZERO));
        require(
            matches!(
                policy
                    .batch_matcher(&expired)
                    .excludes_path(Path::new("missing.md")),
                Err(PolicyMatchError::Filesystem(FsError::IndexWork(
                    IndexWorkFailure::DeadlineExceeded { .. }
                )))
            ),
            "matcher replaced original deadline",
        )?;
        Ok(())
    }
}
