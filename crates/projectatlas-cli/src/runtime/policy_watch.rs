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

#[cfg(test)]
thread_local! {
    /// Namespace resolver work performed by this test thread, independent of wall-clock noise.
    static POLICY_BINDING_RESOLUTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Replace one empty fixture directory between metadata and namespace checks.
    static POLICY_BINDING_TRANSITION: std::cell::RefCell<Option<(PathBuf, Option<PathBuf>)>> = const { std::cell::RefCell::new(None) };
}

/// Identify events that can replace a directory registration or its pathname.
fn event_changes_structure(kind: notify::EventKind) -> bool {
    matches!(
        kind,
        notify::EventKind::Any
            | notify::EventKind::Other
            | notify::EventKind::Create(
                notify::event::CreateKind::Any | notify::event::CreateKind::Folder
            )
            | notify::EventKind::Remove(
                notify::event::RemoveKind::Any | notify::event::RemoveKind::Folder
            )
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

/// Retain explicitly reported access, ownership, and non-timestamp attribute changes.
fn event_changes_ancestor_policy(kind: notify::EventKind) -> bool {
    event_changes_structure(kind)
        || matches!(kind, notify::EventKind::Modify(notify::event::ModifyKind::Metadata(kind))
            if !matches!(kind, notify::event::MetadataKind::AccessTime | notify::event::MetadataKind::WriteTime))
}

/// Resolve a selected spelling while keeping absence distinct from inspection failure.
fn canonical_policy_binding(path: &Path) -> Result<Option<PathBuf>, CliError> {
    #[cfg(test)]
    POLICY_BINDING_RESOLUTIONS.set(POLICY_BINDING_RESOLUTIONS.get().saturating_add(1));
    #[cfg(test)]
    if let Some((removed, replacement)) = POLICY_BINDING_TRANSITION.with(|slot| {
        slot.borrow_mut()
            .take_if(|(pending, _)| pending.as_path() == path)
    }) {
        fs::remove_dir(&removed).map_err(|source| CliError::Io {
            path: removed.clone(),
            source,
        })?;
        if let Some(target) = replacement {
            tests::create_policy_directory_alias(&target, &removed).map_err(|source| {
                CliError::InvalidInput(format!("policy transition fixture: {source}"))
            })?;
        }
    }
    match path.canonicalize() {
        Ok(canonical) => Ok(Some(canonical)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Resolve an event entry without following a symlink control leaf into its target.
fn policy_event_namespace(path: &Path) -> Result<Option<PathBuf>, CliError> {
    let Some(parent) = path.parent() else {
        return Ok(Some(path.to_path_buf()));
    };
    let Some(parent) = canonical_policy_binding(parent)? else {
        return Ok(None);
    };
    let Some(leaf) = path.file_name() else {
        return Ok(Some(path.to_path_buf()));
    };
    #[cfg(windows)]
    let leaf = crate::project_state::windows_filesystem::entry_name(path)
        .map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .unwrap_or_else(|| leaf.to_os_string());
    Ok(Some(parent.join(leaf)))
}

/// An immutable selection snapshot, independent of indexed source node identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PolicyWatchInputs {
    /// Selected file/control paths and their native aliases, including absent leaves.
    paths: BTreeSet<PathBuf>,
    /// Captured native targets of selected inputs and alias controls, including absent targets.
    bindings: BTreeMap<PathBuf, Option<PathBuf>>,
    /// Native namespace scopes map to their selected spellings for bounded prefix lookup.
    binding_scopes: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
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
            let binding = canonical_policy_binding(&path)?;
            self.bindings.insert(path.clone(), binding.clone());
            if let Some(canonical) = binding {
                self.record_binding_scope(&path, canonical.clone());
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
            let mut nearest_directory_found = false;
            // Continue above the nearest existing directory: a symlink in an
            // earlier component can be replaced independently of its target.
            for ancestor in path.ancestors() {
                control.check(IndexWorkStage::RepositoryTraversal)?;
                match fs::symlink_metadata(ancestor) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        // The alias entry itself is a control. Its target directory's
                        // metadata is not an event on this entry's namespace.
                        self.paths.insert(ancestor.to_path_buf());
                        self.bindings
                            .insert(ancestor.to_path_buf(), canonical_policy_binding(ancestor)?);
                        if let Some(Some(target)) = self.bindings.get(ancestor) {
                            self.record_binding_scope(ancestor, target.clone());
                        }
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
                                self.record_parent(root, canonical.clone(), ancestor.file_name());
                            }
                            let namespace = policy_event_namespace(ancestor)?.ok_or_else(|| CliError::Io {
                                path: ancestor.to_path_buf(),
                                source: std::io::Error::new(std::io::ErrorKind::NotFound,
                                    "external policy control parent disappeared during selection"),
                            })?;
                            self.paths.insert(namespace.clone());
                            self.record_binding_scope(ancestor, namespace);
                        }
                    }
                    Ok(metadata)
                        if ancestor != path && !nearest_directory_found && !metadata.is_dir() =>
                    {
                        // Windows can report a child of a file as NotFound.
                        // A closer resolved directory already proves traversal;
                        // earlier lexical components may be canceled by Win32 `..`.
                        return Err(CliError::Io {
                            path: ancestor.to_path_buf(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::NotADirectory,
                                "external policy parent is not a directory",
                            ),
                        });
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
                        let namespace = canonical.join(suffix);
                        self.paths.insert(namespace.clone());
                        self.record_binding_scope(&path, namespace);
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

    /// Index a selected spelling beneath its canonical target or native namespace spelling.
    fn record_binding_scope(&mut self, input: &Path, namespace: PathBuf) {
        self.binding_scopes
            .entry(namespace)
            .or_default()
            .insert(input.to_path_buf());
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
                || event_changes_ancestor_policy(event.kind)
                    && self.paths.iter().any(|input| input.starts_with(candidate))
                || self
                    .registrations
                    .iter()
                    .any(|directory| candidate.parent() == Some(directory.as_path()))
                || {
                    #[cfg(windows)]
                    {
                        policy_event_namespace(candidate).is_ok_and(|namespace| {
                            namespace.is_some_and(|namespace| self.paths.contains(&namespace))
                        }) || event_changes_ancestor_policy(event.kind)
                            && self.paths.iter().any(|input| {
                                input.ancestors().skip(1).any(|ancestor| {
                                    super::source_observation::same_native_path(ancestor, candidate)
                                })
                            })
                            || candidate.parent().is_some_and(|parent| {
                                canonical_policy_binding(parent)
                                    .ok()
                                    .flatten()
                                    .is_some_and(|parent| self.registrations.contains(&parent))
                            })
                    }
                    #[cfg(not(windows))]
                    {
                        false
                    }
                }
        })
    }

    /// Check ordinary ancestor notifications against the captured input namespace in the drain.
    ///
    /// Callbacks remain free of filesystem I/O. Windows modification events do not identify
    /// which metadata flag changed; this proves namespace stability, not flag-level semantics.
    pub(super) fn ancestor_bindings_changed(
        &self,
        event: &Event,
        control: &IndexWorkControl,
    ) -> Result<bool, CliError> {
        if !super::event_kind_affects_index(event.kind) || event_changes_ancestor_policy(event.kind)
        {
            return Ok(false);
        }
        let mut checked = BTreeSet::new();
        for candidate in &event.paths {
            control.check(IndexWorkStage::RepositoryTraversal)?;
            let (ordinary_directory, observed_present) = match fs::symlink_metadata(candidate) {
                Ok(metadata) => (
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    true,
                ),
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => (false, false),
                Err(source) => {
                    return Err(CliError::Io {
                        path: candidate.clone(),
                        source,
                    });
                }
            };
            // Resolve each event namespace once, rather than walking every selected
            // spelling's ancestors through native filesystem comparisons. A link
            // control must retain its entry identity instead of following its leaf.
            let scope = if ordinary_directory {
                let Some(scope) = canonical_policy_binding(candidate)? else {
                    return Ok(true);
                };
                // Do not use a resolved target scope with a stale ordinary-directory
                // classification when the entry visibly became a reparse control.
                match fs::symlink_metadata(candidate) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                    Ok(_) => return Ok(true),
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(true);
                    }
                    Err(source) => {
                        return Err(CliError::Io {
                            path: candidate.clone(),
                            source,
                        });
                    }
                }
                scope
            } else {
                match policy_event_namespace(candidate)? {
                    Some(scope) => scope,
                    None if observed_present => return Ok(true),
                    None => candidate.clone(),
                }
            };
            for (_, inputs) in self
                .binding_scopes
                .range(scope.clone()..)
                .take_while(|(namespace, _)| namespace.starts_with(&scope))
            {
                for input in inputs {
                    control.check(IndexWorkStage::RepositoryTraversal)?;
                    if !ordinary_directory {
                        // A relevant directory became absent, non-directory, or a
                        // reparse control: the old coverage cannot establish policy.
                        return Ok(true);
                    }
                    if checked.insert(input)
                        && canonical_policy_binding(input)? != self.bindings[input]
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
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
        let structural = event_changes_structure(event.kind);
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
                structural
                    && (self.root.starts_with(&path)
                        || path.components().count() <= root_depth
                        || path
                            .components()
                            .any(|component| matches!(component, std::path::Component::ParentDir)))
            })
        {
            #[cfg(all(test, windows))]
            eprintln!(
                "source observer recursive coverage invalidated: root={:?}, event={event:?}",
                self.root
            );
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
    fn ordinary_parent_modifications_preserve_source_and_policy_observation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let settings = base.join("settings");
        fs::create_dir(&root)?;
        fs::create_dir(&settings)?;
        let input = settings.join("rules");
        fs::write(&input, "ignored.rs\n")?;
        let inputs = Arc::new(selected(&root, std::slice::from_ref(&input))?);
        let selection = PolicyWatchSelection::new(root.clone());
        selection.publish(Arc::clone(&inputs))?;
        for parent in [&base, &settings] {
            let event =
                Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(parent.clone());
            require(
                selection.observe(&event),
                "ancestor notification was dropped at ingress",
            )?;
            require(
                !selection.recursive_rebind_pending(),
                "ordinary parent modification invalidated recursive source coverage",
            )?;
            require(
                !inputs.affects(&event),
                "ordinary parent modification changed policy",
            )?;
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(Ok(event))?;
            let changes = super::super::wait_for_index_event_with_continuity(
                &receiver,
                &root,
                Duration::ZERO,
                &projectatlas_fs::ScanOptions::default(),
                &AtomicBool::new(false),
                &inputs,
                &selection,
            )?;
            require(
                !changes.has_changes(),
                "CLI drain treated parent metadata as source work",
            )?;
        }
        require(
            inputs.affects(&Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(input)),
            "exact selected policy edit was dropped with parent metadata",
        )?;
        for kind in [
            notify::EventKind::Any,
            notify::EventKind::Other,
            notify::EventKind::Create(CreateKind::Folder),
            notify::EventKind::Remove(notify::event::RemoveKind::Folder),
            notify::EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
        ] {
            for path in [&base, &root] {
                let selection = PolicyWatchSelection::new(root.clone());
                selection.publish(Arc::clone(&inputs))?;
                let event = Event::new(kind).add_path(path.clone());
                require(
                    selection.observe(&event),
                    "structural notification was dropped",
                )?;
                require(
                    selection.recursive_rebind_pending(),
                    "structural coverage loss was ignored",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn parent_notifications_recheck_input_bindings_and_preserve_stop_controls()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let settings = base.join("settings");
        fs::create_dir(&root)?;
        fs::create_dir(&settings)?;
        let input = settings.join("rules");
        fs::write(&input, "ignored.rs\n")?;
        let inputs = selected(&root, std::slice::from_ref(&input))?;
        let event =
            Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(settings.clone());
        let control = super::super::standalone_index_work_control();
        require(
            !inputs.ancestor_bindings_changed(&event, &control)?,
            "stable input binding changed",
        )?;
        fs::remove_file(&input)?;
        require(
            inputs.ancestor_bindings_changed(&event, &control)?,
            "disappearing input was hidden",
        )?;
        let absent = selected(&root, std::slice::from_ref(&input))?;
        fs::write(&input, "ignored.rs\n")?;
        require(
            absent.ancestor_bindings_changed(&event, &control)?,
            "appearing input was hidden",
        )?;
        let cancelled = super::super::standalone_index_work_control();
        cancelled.cancel();
        let expired = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::ZERO));
        for stopped in [cancelled, expired] {
            require(
                matches!(
                    inputs.ancestor_bindings_changed(&event, &stopped),
                    Err(CliError::IndexWork(_))
                ),
                "ancestor binding verification ignored its original work control",
            )?;
        }
        require(
            inputs.affects(
                &Event::new(notify::EventKind::Modify(ModifyKind::Metadata(
                    notify::event::MetadataKind::Permissions,
                )))
                .add_path(settings),
            ),
            "explicitly reported ancestor permission change was ignored",
        )
    }

    #[test]
    fn metadata_events_resolve_only_related_bindings_once() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let settings = base.join("settings");
        fs::create_dir(&root)?;
        fs::create_dir(&settings)?;
        let source = root.join("source.rs");
        fs::write(&source, "fn source() {}\n")?;
        // Watch-directory capacity does not bound selected-input cardinality:
        // a single external directory can contain many absent policy inputs.
        let paths: Vec<_> = (0..1024)
            .map(|n| settings.join(format!("rules-{n}")))
            .collect();
        let inputs = selected(&root, &paths)?;
        let control = super::super::standalone_index_work_control();
        require(
            inputs.directories.len() == 1,
            "dense inputs acquired extra watches",
        )?;
        let event = Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(source);
        POLICY_BINDING_RESOLUTIONS.set(0);
        require(
            !inputs.affects(&event),
            "ordinary source edit changed external policy",
        )?;
        require(
            POLICY_BINDING_RESOLUTIONS.get() <= 2,
            "exact event classification resolved selected policy inputs",
        )?;
        POLICY_BINDING_RESOLUTIONS.set(0);
        require(
            !inputs.ancestor_bindings_changed(&event, &control)?,
            "ordinary source edit changed external bindings",
        )?;
        require(
            POLICY_BINDING_RESOLUTIONS.get() <= 1,
            "unrelated source event resolved selected policy inputs",
        )?;
        let event = Event::new(notify::EventKind::Modify(ModifyKind::Any))
            .add_path(settings.clone())
            .add_path(base)
            .add_path(settings);
        POLICY_BINDING_RESOLUTIONS.set(0);
        require(
            !inputs.ancestor_bindings_changed(&event, &control)?,
            "unchanged dense bindings changed policy",
        )?;
        require(
            POLICY_BINDING_RESOLUTIONS.get() == paths.len() as u64 + event.paths.len() as u64,
            "overlapping directory events repeated selected binding resolution",
        )?;
        fs::write(&paths[0], "ignored.rs\n")?;
        require(
            inputs.ancestor_bindings_changed(&event, &control)?,
            "dense binding lookup hid an appearing selected input",
        )
    }

    #[test]
    fn directory_transition_during_namespace_resolution_retains_uncertainty()
    -> Result<(), Box<dyn Error>> {
        for retarget in [false, true] {
            let temp = tempfile::tempdir()?;
            let base = temp.path().canonicalize()?;
            #[cfg(windows)]
            let base = PathBuf::from(projectatlas_core::lossless_native_path_display(&base)?);
            let root = base.join("repo");
            let settings = base.join("settings");
            let intermediate = base.join("intermediate");
            let target = base.join("replacement");
            for path in [&root, &settings, &intermediate, &target] {
                fs::create_dir(path)?;
            }
            let inputs = selected(&root, &[settings.join("missing-rules")])?;
            let candidate = intermediate.join("../settings");
            // Both selected leaves remain absent. A lexical event spelling and
            // an inter-call transition expose raw/new-target scope mistakes.
            POLICY_BINDING_TRANSITION.with(|slot| {
                *slot.borrow_mut() = Some((candidate.clone(), retarget.then_some(target)));
            });
            let event = Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(candidate);
            require(
                inputs.ancestor_bindings_changed(
                    &event,
                    &super::super::standalone_index_work_control(),
                )?,
                "a directory transition was acknowledged through a different namespace scope",
            )?;
            require(
                POLICY_BINDING_TRANSITION.with(|slot| slot.borrow().is_none()),
                "directory transition did not exercise the inter-call boundary",
            )?;
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn create_policy_directory_alias(
        target: &Path,
        alias: &Path,
    ) -> Result<(), Box<dyn Error>> {
        std::os::unix::fs::symlink(target, alias)?;
        Ok(())
    }

    #[cfg(windows)]
    pub(super) fn create_policy_directory_alias(
        target: &Path,
        alias: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(projectatlas_core::lossless_native_path_display(alias)?)
            .arg(projectatlas_core::lossless_native_path_display(target)?)
            .output()?;
        require(
            output.status.success(),
            &format!("junction fixture failed: {output:?}"),
        )
    }

    #[test]
    fn directory_alias_entries_remain_distinct_from_target_metadata() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let aliases = base.join("aliases");
        let first = base.join("first");
        let second = base.join("second");
        for path in [&root, &aliases, &first, &second] {
            fs::create_dir(path)?;
        }
        let alias = aliases.join("selected");
        create_policy_directory_alias(&first, &alias)?;
        require(
            fs::symlink_metadata(&alias)?.file_type().is_symlink(),
            "alias is not a native name-surrogate link",
        )?;
        let input = alias.join("missing-rules");
        let inputs = selected(&root, std::slice::from_ref(&input))?;
        let control = super::super::standalone_index_work_control();
        let modify = |path: &Path| {
            Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(path.to_path_buf())
        };
        require(
            inputs.affects(&modify(&alias)),
            "alias modification lost exact control admission",
        )?;
        require(
            !inputs.affects(&modify(&first))
                && !inputs.ancestor_bindings_changed(&modify(&first), &control)?,
            "target directory metadata impersonated a modification to the alias entry",
        )?;
        #[cfg(windows)]
        require(
            inputs.affects(&modify(&alias.with_file_name("SELECTED"))),
            "native case alias of a control entry was lost",
        )?;
        #[cfg(unix)]
        fs::remove_file(&alias)?;
        #[cfg(windows)]
        fs::remove_dir(&alias)?;
        create_policy_directory_alias(&second, &alias)?;
        require(
            inputs.affects(&modify(&alias)),
            "retargeted alias control was ignored",
        )?;
        require(
            inputs.ancestor_bindings_changed(&modify(&aliases), &control)?,
            "alias retarget between absent leaves was hidden by an ancestor notification",
        )?;
        require(
            selected(&root, &[input])?.directories == BTreeSet::from([aliases, second]),
            "retargeted alias did not select the new native coverage",
        )?;
        let prospective = base.join("prospective");
        fs::create_dir(&prospective)?;
        let prospective_input = prospective.join("missing-rules");
        let before = selected(&root, &[prospective_input])?;
        fs::remove_dir(&prospective)?;
        create_policy_directory_alias(&first, &prospective)?;
        require(
            before.ancestor_bindings_changed(&modify(&prospective), &control)?,
            "a newly installed ancestor link was discarded as ordinary metadata",
        )
    }

    #[cfg(windows)]
    #[test]
    fn ancestor_case_mode_changes_revalidate_selected_input_spelling() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        let settings = base.join("settings");
        fs::create_dir(&root)?;
        fs::create_dir(&settings)?;
        require(
            !crate::project_state::windows_filesystem::case_sensitive(&settings)?,
            "case-mode fixture did not start insensitive",
        )?;
        fs::write(settings.join("Rules"), "ignored.rs\n")?;
        let input = settings.join("rules");
        require(
            fs::read_to_string(&input)? == "ignored.rs\n",
            "case alias fixture was not readable",
        )?;
        let inputs = selected(&root, std::slice::from_ref(&input))?;
        let enabled = std::process::Command::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(projectatlas_core::lossless_native_path_display(&settings)?)
            .arg("enable")
            .status()?;
        require(
            enabled.success(),
            "required native directory case-mode transition failed",
        )?;
        require(
            crate::project_state::windows_filesystem::case_sensitive(&settings)?,
            "case sensitivity was not enabled",
        )?;
        require(
            !input.try_exists()?,
            "selected case alias remained available unexpectedly",
        )?;
        require(
            inputs.ancestor_bindings_changed(
                &Event::new(notify::EventKind::Modify(ModifyKind::Any)).add_path(settings),
                &super::super::standalone_index_work_control(),
            )?,
            "an ordinary parent hid the selected spelling becoming unavailable",
        )
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
    fn policy_parent_symlink_to_file_refuses_descendants() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let file = base.join("regular-policy");
        fs::write(&file, "selected.rs\n")?;
        let alias = base.join("selected-policy");
        std::os::unix::fs::symlink(&file, &alias)?;
        selected(&root, std::slice::from_ref(&alias))?;
        require(
            matches!(
                selected(&root, &[alias.join("config.toml")]),
                Err(CliError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotADirectory
            ),
            "a policy symlink to a regular file admitted a child path",
        )
    }

    #[test]
    fn policy_parent_files_refuse_descendants_without_rejecting_leaf_or_missing_inputs()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let file = base.join("regular-policy");
        fs::write(&file, "selected.rs\n")?;
        let inputs = selected(&root, std::slice::from_ref(&file))?;
        require(
            inputs.paths.contains(&file),
            "regular policy leaf was rejected",
        )?;
        #[cfg(windows)]
        {
            let ordinary_base =
                PathBuf::from(projectatlas_core::lossless_native_path_display(&base)?);
            let mut spelling = ordinary_base.into_os_string();
            spelling.push("\\regular-policy\\..\\regular-policy");
            let canceled_file = PathBuf::from(spelling);
            require(
                fs::read_to_string(&canceled_file)? == "selected.rs\n",
                "ordinary Win32 parent normalization did not cancel the file component",
            )?;
            selected(&root, &[canceled_file])?;
        }
        for suffix in ["config.toml", "nested/config.toml"] {
            require(
                matches!(
                    selected(&root, &[file.join(suffix)]),
                    Err(CliError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotADirectory
                ),
                "existing policy parent file was accepted as a missing directory",
            )?;
        }
        let missing = base.join("missing/nested/config.toml");
        let inputs = selected(&root, std::slice::from_ref(&missing))?;
        require(
            inputs.paths.contains(&missing) && inputs.directories.contains(&base),
            "genuinely absent policy parents lost their creation coverage",
        )
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
        fs::write(base.join("rules"), "selected-rule\n")?;
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
        #[cfg(windows)]
        {
            // Ordinary Win32 parsing removes `intermediate/..` before lookup;
            // changing that sibling cannot change the selected rules file.
            fs::remove_dir(base.join("intermediate"))?;
            require(
                fs::read_to_string(&parent_path)? == "selected-rule\n",
                "ordinary Win32 parent syntax unexpectedly depends on the removed sibling",
            )?;
        }
        selection.publish(Arc::new(selected(&root, &[parent_path])?))?;
        require(
            selection.may_affect(&event(base.join("rules"))),
            "selected rules file was filtered",
        )?;
        #[cfg(not(windows))]
        require(
            selection.may_affect(&event(base.join("intermediate"))),
            "structural parent component was filtered",
        )?;
        #[cfg(windows)]
        require(
            !selection.may_affect(&event(base.join("intermediate"))),
            "normalized Win32 selection retained an unrelated lexical component",
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
