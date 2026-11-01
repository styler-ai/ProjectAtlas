//! Selected external policy inputs and their non-recursive native watch coverage.

use super::{CliError, IndexWorkControl, IndexWorkStage};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Bound native symlink expansion before installing policy watch aliases.
const MAX_POLICY_SYMLINKS: usize = 64;
/// Per-binding native external-directory capacity; unavailable observation retains exact/poll fallback.
const MAX_POLICY_WATCH_DIRECTORIES: usize = 256;
/// Bound selection convergence while establishing cold native observation.
const MAX_POLICY_WATCH_ATTEMPTS: usize = 3;
/// Git's directory whose children change the registered worktree inventory.
const REGISTRATIONS_DIRECTORY: &str = "worktrees";

/// An immutable selection snapshot, independent of indexed source node identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PolicyWatchInputs {
    /// Selected file/control paths and their native aliases, including absent leaves.
    paths: BTreeSet<PathBuf>,
    /// Existing external directories watched only for immediate child events.
    directories: BTreeSet<PathBuf>,
    /// Git registration directories whose immediate children affect policy discovery.
    registrations: BTreeSet<PathBuf>,
    /// Proven immediate names for each external parent; None preserves unknown/all-child scope.
    event_children: BTreeMap<PathBuf, Option<BTreeSet<OsString>>>,
}

impl PolicyWatchInputs {
    /// Capture exactly the inputs used by source policy, without observing outside contents.
    pub(super) fn discover(
        root: &Path,
        config: Option<&Path>,
        control: &IndexWorkControl,
    ) -> Result<Self, CliError> {
        let mut selected = projectatlas_fs::source_selection_policy_paths_controlled(root, control)
            .map_err(|error| super::source_inspection_error(root, error))?;
        if let Some(config) = config {
            selected.push(config.to_path_buf());
        } else {
            selected.push(root.join(".projectatlas/config.toml"));
            selected.push(root.join("projectatlas.toml"));
        }
        let mut result = Self::default();
        for path in selected {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            result.add_path(root, &path, control)?;
        }
        result.retain_alias_children(control)?;
        Ok(result)
    }

    /// Preserve both a link's parent and the underlying regular/absent target's parent.
    fn add_path(
        &mut self,
        root: &Path,
        path: &Path,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        let mut pending = vec![path.to_path_buf()];
        let mut expanded = BTreeSet::new();
        while let Some(path) = pending.pop() {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            if !expanded.insert(path.clone()) {
                continue;
            }
            if expanded.len() > MAX_POLICY_SYMLINKS {
                return Err(CliError::InvalidInput(
                    "external policy symlink expansion exceeds the native watch bound".to_string(),
                ));
            }
            self.paths.insert(path.clone());
            match path.canonicalize() {
                Ok(canonical) => {
                    self.paths.insert(canonical.clone());
                    if canonical.is_dir()
                        && path
                            .file_name()
                            .is_some_and(|name| name == REGISTRATIONS_DIRECTORY)
                    {
                        self.registrations.insert(canonical.clone());
                        if !canonical.starts_with(root) {
                            self.record_parent(root, canonical, None);
                        }
                    }
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(CliError::Io { path, source }),
            }
            let mut nearest_directory_found = false;
            // Continue above the nearest existing directory: a symlink in an
            // earlier component can be replaced independently of its target.
            for ancestor in path.ancestors() {
                control.check(IndexWorkStage::RepositoryTraversal)?;
                match fs::symlink_metadata(ancestor) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        let target = fs::read_link(ancestor).map_err(|source| CliError::Io {
                            path: ancestor.to_path_buf(),
                            source,
                        })?;
                        let target = if target.is_absolute() {
                            target
                        } else {
                            ancestor.parent().unwrap_or(root).join(target)
                        };
                        if let Ok(suffix) = path.strip_prefix(ancestor) {
                            // Joining an empty suffix appends a separator and
                            // turns a regular-file target into a directory lookup.
                            pending.push(if suffix.as_os_str().is_empty() {
                                target
                            } else {
                                target.join(suffix)
                            });
                        }
                        if let Some(parent) = ancestor.parent() {
                            let canonical =
                                parent.canonicalize().map_err(|source| CliError::Io {
                                    path: parent.to_path_buf(),
                                    source,
                                })?;
                            if !canonical.starts_with(root) {
                                self.record_parent(root, canonical, ancestor.file_name());
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(CliError::Io {
                            path: ancestor.to_path_buf(),
                            source,
                        });
                    }
                }
                if !nearest_directory_found && ancestor.is_dir() {
                    let canonical = ancestor.canonicalize().map_err(|source| CliError::Io {
                        path: ancestor.to_path_buf(),
                        source,
                    })?;
                    if let Ok(suffix) = path.strip_prefix(ancestor) {
                        self.paths.insert(canonical.join(suffix));
                    }
                    if !canonical.starts_with(root) {
                        let child = path
                            .strip_prefix(ancestor)
                            .ok()
                            .and_then(|relative| relative.components().next())
                            .map(std::path::Component::as_os_str);
                        self.record_parent(root, canonical, child);
                    }
                    nearest_directory_found = true;
                }
            }
        }
        Ok(())
    }

    /// Record a native namespace while its selected child identity is still explicit.
    fn record_parent(&mut self, root: &Path, parent: PathBuf, child: Option<&OsStr>) {
        let root_child = root
            .strip_prefix(&parent)
            .ok()
            .and_then(|relative| relative.components().next())
            .map(std::path::Component::as_os_str);
        self.directories.insert(parent.clone());
        let children = self
            .event_children
            .entry(parent)
            .or_insert_with(|| Some(BTreeSet::new()));
        let Some(child) = child else {
            *children = None;
            return;
        };
        if let Some(children) = children {
            children.insert(child.to_os_string());
            // Ancestor watches must also preserve creation/removal of this root.
            if let Some(root_child) = root_child {
                children.insert(root_child.to_os_string());
            }
        }
    }

    /// Keep lexical path components as well as resolved targets (including meaningful `..` prefixes).
    fn retain_alias_children(&mut self, control: &IndexWorkControl) -> Result<(), CliError> {
        if self.directories.len() > MAX_POLICY_WATCH_DIRECTORIES {
            return Err(CliError::Watcher(format!(
                "external policy observation exceeds its {MAX_POLICY_WATCH_DIRECTORIES} directory capacity"
            )));
        }
        for (parent, children) in &mut self.event_children {
            let Some(children) = children else {
                continue;
            };
            for input in &self.paths {
                control.check(IndexWorkStage::RepositoryTraversal)?;
                if let Ok(relative) = input.strip_prefix(parent)
                    && let Some(component) = relative.components().next()
                {
                    children.insert(component.as_os_str().to_os_string());
                }
            }
        }
        Ok(())
    }

    /// Reject only proven unrelated external children, with no filesystem I/O.
    /// Unknown aliases, Unicode case relationships, directory controls and rescan stay visible.
    fn may_affect(&self, root: &Path, event: &Event) -> bool {
        if event.need_rescan() {
            return true;
        }
        if !super::event_kind_affects_index(event.kind) {
            return false;
        }
        if event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|candidate| {
            if candidate.starts_with(root) || root.starts_with(candidate) {
                return true;
            }
            let Some(parent) = candidate.parent() else {
                return true;
            };
            let Some(Some(children)) = self.event_children.get(parent) else {
                return true;
            };
            let Some(name) = candidate.file_name() else {
                return true;
            };
            let name = name.as_encoded_bytes();
            children.iter().any(|child| {
                let child = child.as_encoded_bytes();
                !name.is_ascii() || !child.is_ascii() || name.eq_ignore_ascii_case(child)
            })
        })
    }

    /// Identify selected-input edits and missing-parent creation without admitting outside nodes.
    pub(super) fn affects(&self, event: &Event) -> bool {
        if !super::event_kind_affects_index(event.kind) {
            return false;
        }
        event.paths.iter().any(|candidate| {
            self.paths.contains(candidate)
                || self.paths.iter().any(|input| input.starts_with(candidate))
                || self
                    .registrations
                    .iter()
                    .any(|directory| candidate.parent() == Some(directory.as_path()))
                || {
                    #[cfg(windows)]
                    {
                        self.paths.iter().any(|input| {
                            input.ancestors().any(|ancestor| {
                                super::source_observation::same_native_path(ancestor, candidate)
                            })
                        }) || self.registrations.iter().any(|directory| {
                            candidate.parent().is_some_and(|parent| {
                                super::source_observation::same_native_path(directory, parent)
                            })
                        })
                    }
                    #[cfg(not(windows))]
                    {
                        false
                    }
                }
        })
    }

    /// Establish a stable selected namespace before source sampling starts.
    pub(super) fn install_selected(
        root: &Path,
        config: Option<&Path>,
        selection: &PolicyWatchSelection,
        watcher: &mut RecommendedWatcher,
        registered: &mut BTreeSet<PathBuf>,
        control: &IndexWorkControl,
    ) -> Result<Arc<Self>, CliError> {
        Self::install_selected_with_discovery(selection, watcher, registered, control, || {
            Self::discover(root, config, control)
        })
    }

    /// Keep discovery at the native registration boundary for deterministic handoff tests.
    fn install_selected_with_discovery(
        selection: &PolicyWatchSelection,
        watcher: &mut RecommendedWatcher,
        registered: &mut BTreeSet<PathBuf>,
        control: &IndexWorkControl,
        mut discover: impl FnMut() -> Result<Self, CliError>,
    ) -> Result<Arc<Self>, CliError> {
        let mut inputs = Arc::new(discover()?);
        for _attempt in 0..MAX_POLICY_WATCH_ATTEMPTS {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            selection.publish(Arc::clone(&inputs))?;
            inputs.install(watcher, registered, control)?;
            let sampled = Arc::new(discover()?);
            if sampled == inputs {
                return Ok(inputs);
            }
            inputs = sampled;
        }
        Err(CliError::Watcher(
            "external policy selection changed throughout native watch installation".to_string(),
        ))
    }

    /// Rebind cold coverage before sampling, retaining acknowledged registrations on failure.
    /// This is never called on warm witnessed reads. Rebinding closes same-path directory
    /// replacement without adding platform identity requirements to legitimate source paths.
    pub(super) fn install(
        &self,
        watcher: &mut RecommendedWatcher,
        registered: &mut BTreeSet<PathBuf>,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        if self
            .directories
            .union(registered)
            .take(MAX_POLICY_WATCH_DIRECTORIES + 1)
            .count()
            > MAX_POLICY_WATCH_DIRECTORIES
        {
            return Err(CliError::Watcher(format!(
                "external policy observation exceeds its {MAX_POLICY_WATCH_DIRECTORIES} directory capacity"
            )));
        }
        for directory in &self.directories {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            if registered.contains(directory) {
                if let Err(error) = watcher.unwatch(directory)
                    && !matches!(error.kind, notify::ErrorKind::WatchNotFound)
                {
                    return Err(CliError::Watcher(format!(
                        "cannot rebind external policy parent {}: {error}",
                        directory.display()
                    )));
                }
                registered.remove(directory);
            }
            watcher
                .watch(directory, RecursiveMode::NonRecursive)
                .map_err(|error| {
                    CliError::Watcher(format!(
                        "cannot watch external policy parent {}: {error}",
                        directory.display()
                    ))
                })?;
            registered.insert(directory.clone());
        }
        let retired: Vec<_> = registered.difference(&self.directories).cloned().collect();
        for directory in retired {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            if let Err(error) = watcher.unwatch(&directory)
                && !matches!(error.kind, notify::ErrorKind::WatchNotFound)
            {
                return Err(CliError::Watcher(format!(
                    "cannot retire external policy parent {}: {error}",
                    directory.display()
                )));
            }
            registered.remove(&directory);
        }
        Ok(())
    }

    /// Release native watches before a Windows observer lifetime acknowledgement.
    #[cfg(windows)]
    pub(super) fn unwatch(watcher: &mut RecommendedWatcher, registered: &BTreeSet<PathBuf>) {
        for directory in registered {
            drop(watcher.unwatch(directory));
        }
    }
}

/// Current policy namespace shared with native callback ingress; updates never hold a watch lock.
#[derive(Clone, Debug)]
pub(super) struct PolicyWatchSelection {
    /// Exact source root, whose recursive events always remain eligible.
    root: Arc<PathBuf>,
    /// Immutable selection pointer; this lock is released before callback queue publication.
    inputs: Arc<Mutex<Arc<PolicyWatchInputs>>>,
    /// Structural native events require rebuilding recursive source coverage before reuse.
    recursive_rebind_required: Arc<AtomicBool>,
}

impl PolicyWatchSelection {
    /// Start conservatively before the first external coverage installation.
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            inputs: Arc::new(Mutex::new(Arc::new(PolicyWatchInputs::default()))),
            recursive_rebind_required: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Publish the next namespace before adding watches and sampling its source policy.
    pub(super) fn publish(&self, inputs: Arc<PolicyWatchInputs>) -> Result<(), CliError> {
        *self.inputs.lock().map_err(|_poisoned| {
            CliError::Watcher("external policy selection lock is unavailable".to_string())
        })? = inputs;
        Ok(())
    }

    /// Capture one cheap immutable batch snapshot without holding a native watch lock.
    pub(super) fn snapshot(&self) -> Result<Arc<PolicyWatchInputs>, CliError> {
        self.inputs
            .lock()
            .map(|inputs| Arc::clone(&inputs))
            .map_err(|_poisoned| {
                CliError::Watcher("external policy selection lock is unavailable".to_string())
            })
    }

    /// Filter only proven unrelated children; uncertainty retains the original bounded callback path.
    pub(super) fn may_affect(&self, event: &Event) -> bool {
        if event.need_rescan() {
            return true;
        }
        if !super::event_kind_affects_index(event.kind) {
            return false;
        }
        // The snapshot guard is dropped before any receiver lock is acquired.
        self.snapshot()
            .map_or(true, |inputs| inputs.may_affect(&self.root, event))
    }

    /// Retain structural coverage uncertainty independently of queue draining.
    /// This callback path performs no filesystem I/O and does not classify source bytes.
    pub(super) fn observe(&self, event: &Event) -> bool {
        if !self.may_affect(event) {
            return false;
        }
        let root_depth = self.root.components().count();
        let structural = matches!(
            event.kind,
            notify::EventKind::Any
                | notify::EventKind::Other
                | notify::EventKind::Create(
                    notify::event::CreateKind::Any | notify::event::CreateKind::Folder
                )
                | notify::EventKind::Remove(
                    notify::event::RemoveKind::Any | notify::event::RemoveKind::Folder
                )
                | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
        if event.need_rescan()
            || event.paths.is_empty()
            || event.paths.iter().any(|path| {
                let path = if path.is_absolute() {
                    Cow::Borrowed(path.as_path())
                } else {
                    Cow::Owned(self.root.join(path))
                };
                // Native aliases can differ in Windows prefix/case spelling.
                // A deeper normal child cannot name this root or an ancestor;
                // its source/runtime classification remains with the drain owner.
                self.root.starts_with(&path)
                    || structural && path.components().count() <= root_depth
                    || structural
                        && path
                            .components()
                            .any(|component| matches!(component, std::path::Component::ParentDir))
            })
        {
            self.require_recursive_rebind();
        }
        true
    }

    /// Preserve unknown native coverage until the cold owner successfully re-registers it.
    pub(super) fn require_recursive_rebind(&self) {
        self.recursive_rebind_required
            .store(true, Ordering::Release);
    }

    /// A structural event must not certify a warm witness before recursive resynchronization.
    pub(super) fn recursive_rebind_pending(&self) -> bool {
        self.recursive_rebind_required.load(Ordering::Acquire)
    }

    /// Re-register only after structural uncertainty, never on ordinary file data edits.
    /// Sampling follows this gap under the caller's original cooperative budget.
    pub(super) fn rebind_recursive_if_required(
        &self,
        watcher: &mut RecommendedWatcher,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        if !self.recursive_rebind_required.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let result = (|| {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            if let Err(error) = watcher.unwatch(&self.root)
                && !matches!(error.kind, notify::ErrorKind::WatchNotFound)
            {
                return Err(CliError::Watcher(format!(
                    "cannot retire recursive source observation for {}: {error}",
                    self.root.display()
                )));
            }
            control.check(IndexWorkStage::RepositoryTraversal)?;
            watcher
                .watch(&self.root, RecursiveMode::Recursive)
                .map_err(|error| {
                    CliError::Watcher(format!(
                        "cannot restore recursive source observation for {}: {error}",
                        self.root.display()
                    ))
                })?;
            control.check(IndexWorkStage::RepositoryTraversal)?;
            Ok(())
        })();
        if result.is_err() {
            self.require_recursive_rebind();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind};
    use projectatlas_core::IndexCancellation;
    use std::error::Error;
    use std::time::Duration;

    /// Report causal failures without panicking inside fallible tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    /// Select fixture paths without mutating process-wide Git environment.
    fn selected(root: &Path, paths: &[PathBuf]) -> Result<PolicyWatchInputs, CliError> {
        let control = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(5)));
        let mut inputs = PolicyWatchInputs::default();
        for path in paths {
            inputs.add_path(root, path, &control)?;
        }
        inputs.retain_alias_children(&control)?;
        Ok(inputs)
    }

    #[test]
    fn absent_policy_parents_are_routed_without_watching_unrelated_trees()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let path = base.join("settings/missing/rules");
        let inputs = selected(&root, &[path.clone(), root.join("local.ignore")])?;
        require(
            inputs.directories == BTreeSet::from([base.clone()]),
            "missing input did not watch its nearest external ancestor",
        )?;
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Create(CreateKind::Folder))
                    .add_path(base.join("settings")),
            ),
            "parent creation was not routed",
        )?;
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Modify(ModifyKind::Data(
                    notify::event::DataChange::Any,
                )))
                .add_path(path),
            ),
            "selected leaf edit was not routed",
        )?;
        require(
            !inputs.affects(
                &Event::new(notify::EventKind::Create(CreateKind::File))
                    .add_path(base.join("unrelated")),
            ),
            "unrelated external content invalidated policy",
        )?;
        require(
            !inputs.affects(
                &Event::new(notify::EventKind::Access(AccessKind::Any))
                    .add_path(base.join("settings")),
            ),
            "read access invalidated policy",
        )?;
        fs::create_dir_all(base.join("settings/missing"))?;
        let updated = selected(&root, &[base.join("settings/missing/rules")])?;
        require(
            updated.directories == BTreeSet::from([base.join("settings/missing")]),
            "coverage did not advance to the created parent",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestor_replacement_keeps_alias_and_target_coverage() -> Result<(), Box<dyn Error>>
    {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let aliases = base.join("aliases");
        let first = base.join("first/child");
        let second = base.join("second/child");
        for directory in [&root, &aliases, &first, &second] {
            fs::create_dir_all(directory)?;
        }
        let link = aliases.join("selected");
        symlink(base.join("first"), &link)?;
        let path = link.join("child/rules");
        let inputs = selected(&root, std::slice::from_ref(&path))?;
        require(
            inputs.directories == BTreeSet::from([aliases.clone(), first.clone()]),
            "an earlier symlink component or its target was not watched",
        )?;
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any))
                    .add_path(link.clone()),
            ),
            "ancestor replacement was not routed",
        )?;
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Create(CreateKind::File))
                    .add_path(first.join("rules")),
            ),
            "resolved absent leaf was not routed",
        )?;
        fs::remove_file(&link)?;
        symlink(base.join("second"), &link)?;
        let updated = selected(&root, std::slice::from_ref(&path))?;
        require(
            updated.directories == BTreeSet::from([aliases, second]),
            "replacement target coverage was not refreshed",
        )?;
        fs::write(base.join("regular-rules"), "ignored.rs\n")?;
        let leaf = base.join("leaf-link");
        symlink(base.join("regular-rules"), &leaf)?;
        require(
            selected(&root, &[leaf])?
                .paths
                .contains(&base.join("regular-rules")),
            "regular-file symlink target was not retained",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cyclic_policy_symlinks_refuse_without_an_unbounded_expansion() -> Result<(), Box<dyn Error>>
    {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        symlink(root.join("second"), root.join("first"))?;
        symlink(root.join("first"), root.join("second"))?;
        require(
            selected(&root, &[root.join("first")]).is_err(),
            "cyclic policy aliases were accepted",
        )
    }

    #[test]
    fn ingress_filter_preserves_alias_controls_and_retargets_before_registration()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let selection = PolicyWatchSelection::new(root.clone());
        selection.publish(Arc::new(selected(&root, &[base.join("first-rules")])?))?;
        let event = |path| {
            Event::new(notify::EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(path)
        };
        require(
            !selection.may_affect(&event(base.join("unrelated"))),
            "known unrelated sibling was admitted",
        )?;
        require(
            selection.may_affect(&event(root.join("source.rs"))),
            "source-root event was filtered",
        )?;
        require(
            selection.may_affect(&event(base.join("FIRST-RULES"))),
            "possible case alias was filtered",
        )?;
        require(
            selection.may_affect(&event(base.join("π-unknown"))),
            "uncertain Unicode identity was filtered",
        )?;
        require(
            selection.may_affect(&event(base.join("unknown-parent/unrelated"))),
            "unproven native namespace was filtered",
        )?;
        selection.publish(Arc::new(selected(&root, &[base.join("second-rules")])?))?;
        require(
            selection.may_affect(&event(base.join("second-rules"))),
            "new selection was invisible before native registration",
        )?;
        require(
            !selection.may_affect(&event(base.join("first-rules"))),
            "retired sibling stayed selected",
        )?;
        fs::create_dir(base.join("intermediate"))?;
        fs::write(base.join("rules"), "")?;
        #[cfg(windows)]
        {
            let normalized = base.join("intermediate/../rules");
            require(
                !normalized
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
                "verbatim join no longer normalizes the fixture control",
            )?;
            selection.publish(Arc::new(selected(&root, &[normalized])?))?;
            require(
                selection.may_affect(&event(base.join("rules")))
                    && !selection.may_affect(&event(base.join("intermediate"))),
                "normalized selection retained an unrelated lexical component",
            )?;
            let mut unsupported = base.as_os_str().to_os_string();
            unsupported.push(std::path::MAIN_SEPARATOR_STR);
            unsupported.push(["intermediate", "..", "rules"].join(std::path::MAIN_SEPARATOR_STR));
            require(
                selected(&root, &[PathBuf::from(unsupported)]).is_err(),
                "verbatim parent syntax unexpectedly became a supported native input",
            )?;
        }
        // Win32 paths process parent components; verbatim paths do not.
        #[cfg(windows)]
        let ordinary_base = PathBuf::from(projectatlas_core::lossless_native_path_display(&base)?);
        #[cfg(not(windows))]
        let ordinary_base = base.clone();
        #[cfg(windows)]
        require(
            ordinary_base.is_absolute()
                && matches!(ordinary_base.components().next(), Some(std::path::Component::Prefix(prefix))
                    if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(..))),
            "parent fixture requires an ordinary Win32 base",
        )?;
        let mut spelling = ordinary_base.as_os_str().to_os_string();
        spelling.push(std::path::MAIN_SEPARATOR_STR);
        spelling.push(["intermediate", "..", "rules"].join(std::path::MAIN_SEPARATOR_STR));
        let parent_path = PathBuf::from(spelling);
        require(
            parent_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)),
            "fixture normalized away the selected parent component",
        )?;
        selection.publish(Arc::new(selected(&root, &[parent_path])?))?;
        require(
            selection.may_affect(&event(base.join("intermediate"))),
            "structural parent component was filtered",
        )?;
        require(
            selection.may_affect(
                &Event::new(notify::EventKind::Any).set_flag(notify::event::Flag::Rescan),
            ),
            "native rescan uncertainty was filtered",
        )?;
        Ok(())
    }

    #[test]
    fn watch_capacity_and_cancellation_refuse_before_native_mutation() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let inputs = PolicyWatchInputs {
            directories: (0..=MAX_POLICY_WATCH_DIRECTORIES)
                .map(|i| temp.path().join(format!("parent-{i}")))
                .collect(),
            ..PolicyWatchInputs::default()
        };
        let mut watcher = notify::recommended_watcher(|_event| {})?;
        let mut registered = BTreeSet::new();
        require(
            matches!(
                inputs.install(
                    &mut watcher,
                    &mut registered,
                    &super::super::standalone_index_work_control()
                ),
                Err(CliError::Watcher(_))
            ),
            "native registration count was not bounded",
        )?;
        require(
            registered.is_empty(),
            "over-capacity proposal partially mutated native coverage",
        )?;
        let cancelled = super::super::standalone_index_work_control();
        cancelled.cancel();
        require(
            matches!(
                PolicyWatchInputs::default().install(&mut watcher, &mut registered, &cancelled),
                Err(CliError::IndexWork(_))
            ),
            "native update ignored caller cancellation",
        )
    }

    #[cfg(unix)]
    #[test]
    fn newly_selected_alias_retarget_before_registration_is_reconciled()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let first = base.join("first");
        let second = base.join("second");
        let aliases = base.join("aliases");
        for directory in [&root, &first, &second, &aliases] {
            fs::create_dir(directory)?;
        }
        let alias = aliases.join("selected");
        symlink(&first, &alias)?;
        let input = alias.join("rules");
        let selection = PolicyWatchSelection::new(root.clone());
        let mut watcher = notify::recommended_watcher(|_event| {})?;
        let mut registered = BTreeSet::new();
        let control = super::super::standalone_index_work_control();
        let mut discoveries = 0;
        let installed = PolicyWatchInputs::install_selected_with_discovery(
            &selection,
            &mut watcher,
            &mut registered,
            &control,
            || {
                let captured = selected(&root, std::slice::from_ref(&input))?;
                discoveries += 1;
                if discoveries == 1 {
                    fs::remove_file(&alias)
                        .and_then(|()| symlink(&second, &alias))
                        .map_err(|source| CliError::Io {
                            path: alias.clone(),
                            source,
                        })?;
                }
                Ok(captured)
            },
        )?;
        require(
            discoveries == 3,
            "newly selected alias was not resampled to convergence",
        )?;
        require(
            registered.contains(&second)
                && !registered.contains(&first)
                && registered == installed.directories,
            "new target was not watched before accepting the selected namespace",
        )?;
        require(
            *selection.snapshot()? == *installed,
            "callback selection differs from acknowledged coverage",
        )
    }

    #[test]
    fn continuously_changing_selection_refuses_with_bounded_native_work()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir(&first)?;
        fs::create_dir(&second)?;
        let selection = PolicyWatchSelection::new(root);
        let mut watcher = notify::recommended_watcher(|_event| {})?;
        let mut registered = BTreeSet::new();
        let mut discoveries = 0;
        let result = PolicyWatchInputs::install_selected_with_discovery(
            &selection,
            &mut watcher,
            &mut registered,
            &super::super::standalone_index_work_control(),
            || {
                discoveries += 1;
                Ok(PolicyWatchInputs {
                    directories: BTreeSet::from([if discoveries % 2 == 0 {
                        second.clone()
                    } else {
                        first.clone()
                    }]),
                    ..PolicyWatchInputs::default()
                })
            },
        );
        require(
            matches!(result, Err(CliError::Watcher(_))),
            "unstable selection was accepted",
        )?;
        require(
            discoveries == MAX_POLICY_WATCH_ATTEMPTS + 1,
            "unstable selection exceeded its native work bound",
        )
    }

    #[test]
    fn ordinary_access_does_not_request_recursive_rebinding() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let selection = PolicyWatchSelection::new(root.clone());
        for path in [root.clone(), root.join("source.rs")] {
            let access = Event::new(notify::EventKind::Access(AccessKind::Any)).add_path(path);
            require(
                !selection.observe(&access) && !selection.recursive_rebind_pending(),
                "ordinary access requested recursive recovery",
            )?;
        }
        let rescan = Event::new(notify::EventKind::Access(AccessKind::Any))
            .add_path(root)
            .set_flag(notify::event::Flag::Rescan);
        require(
            selection.observe(&rescan) && selection.recursive_rebind_pending(),
            "access filtering discarded a native rescan request",
        )
    }

    #[test]
    fn recursive_rebind_preserves_runtime_filtering_and_original_work_control()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let selection = PolicyWatchSelection::new(root.clone());
        for event in [
            Event::new(notify::EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(root.join("source.rs")),
            Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("source.rs")),
            Event::new(notify::EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(
                root.parent()
                    .ok_or_else(|| std::io::Error::other("fixture parent missing"))?
                    .join("状态.db"),
            ),
            Event::new(notify::EventKind::Create(CreateKind::Folder))
                .add_path(root.join(".projectatlas/staging")),
            Event::new(notify::EventKind::Modify(ModifyKind::Name(
                notify::event::RenameMode::Both,
            )))
            .add_path(root.join(".projectatlas/staging")),
        ] {
            require(
                selection.observe(&event),
                "source child disappeared from normal classification",
            )?;
            require(
                !selection.recursive_rebind_pending(),
                "ordinary source/runtime children rebuilt recursive coverage before classification",
            )?;
        }
        let mut watcher = notify::recommended_watcher(|_event| {})?;
        watcher.watch(&root, RecursiveMode::Recursive)?;
        let renamed = Event::new(notify::EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::From,
        )))
        .add_path(root.clone());
        require(
            selection.observe(&renamed) && selection.recursive_rebind_pending(),
            "source root replacement was not retained",
        )?;
        let cancelled = super::super::standalone_index_work_control();
        cancelled.cancel();
        require(
            matches!(
                selection.rebind_recursive_if_required(&mut watcher, &cancelled),
                Err(CliError::IndexWork(_))
            ),
            "recursive recovery ignored caller cancellation",
        )?;
        require(
            selection.recursive_rebind_pending(),
            "failed root recovery lost its retry state",
        )?;
        selection.rebind_recursive_if_required(
            &mut watcher,
            &super::super::standalone_index_work_control(),
        )?;
        require(
            !selection.recursive_rebind_pending(),
            "successful root recovery retained stale uncertainty",
        )?;
        require(
            selection.observe(&Event::new(notify::EventKind::Create(CreateKind::File)))
                && selection.recursive_rebind_pending(),
            "an unknown source location was acknowledged as current",
        )
    }

    #[test]
    fn failed_watch_update_retains_acknowledged_coverage_for_retry_and_cleanup()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let previous = base.join("previous");
        let admitted = base.join("a-admitted");
        let missing = base.join("z-missing");
        fs::create_dir(&previous)?;
        fs::create_dir(&admitted)?;
        let mut watcher = notify::recommended_watcher(|_event| {})?;
        let mut registered = BTreeSet::new();
        let inputs = PolicyWatchInputs {
            directories: BTreeSet::from([previous.clone()]),
            ..PolicyWatchInputs::default()
        };
        inputs.install(
            &mut watcher,
            &mut registered,
            &super::super::standalone_index_work_control(),
        )?;
        let next = PolicyWatchInputs {
            directories: BTreeSet::from([admitted.clone(), missing.clone()]),
            ..PolicyWatchInputs::default()
        };
        require(
            next.install(
                &mut watcher,
                &mut registered,
                &super::super::standalone_index_work_control(),
            )
            .is_err(),
            "missing native parent was accepted",
        )?;
        require(
            registered == BTreeSet::from([previous, admitted]),
            "failed update lost its actual acknowledged coverage",
        )?;
        fs::create_dir(&missing)?;
        next.install(
            &mut watcher,
            &mut registered,
            &super::super::standalone_index_work_control(),
        )?;
        require(
            registered == next.directories,
            "retry did not retire and replace old coverage",
        )?;
        for directory in &registered {
            watcher.unwatch(directory)?;
        }
        #[cfg(windows)]
        watcher.configure(notify::Config::default())?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn missing_parent_events_match_proven_windows_case_aliases() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        fs::create_dir(base.join("MixedParent"))?;
        let inputs = selected(&root, &[base.join("MixedParent/missing/rules")])?;
        fs::create_dir(base.join("MixedParent/missing"))?;
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Create(CreateKind::Folder))
                    .add_path(base.join("MIXEDPARENT/missing")),
            ),
            "native missing-parent case alias was not routed",
        )
    }
}
