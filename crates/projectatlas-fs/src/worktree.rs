//! Read-only structural discovery for Git worktrees and common control directories.

use super::{
    DEFAULT_SCAN_TIMEOUT, FsError, FsResult, GIT_DIRECTORY_POINTER_MAX_BYTES,
    check_registered_worktree,
};
use projectatlas_core::{IndexCancellation, IndexWorkControl, IndexWorkStage};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Read-only structural classification for one selected path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryStructure {
    /// No containing structural Git checkout or common directory was found.
    NonGit {
        /// Canonical exact non-Git root supplied to discovery.
        selected_root: PathBuf,
    },
    /// A structurally validated Git repository and its worktree inventory.
    Git(GitRepositoryStructure),
    /// A Git marker was present, but its structural evidence was unsafe or inconsistent.
    InvalidGit {
        /// Canonical containing path whose Git marker was inspected.
        selected_root: PathBuf,
        /// Exact typed structural problem.
        issue: GitStructureIssue,
    },
}

/// Structurally validated Git repository evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRepositoryStructure {
    /// Canonical Git common directory shared by every discovered worktree.
    pub common_directory: PathBuf,
    /// How the supplied path selected this repository.
    pub selection: GitRepositorySelection,
    /// Deterministic primary-then-registration inventory.
    pub worktrees: Vec<GitWorktreeEntry>,
}

/// Selection made from the supplied path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitRepositorySelection {
    /// The supplied path was inside one exact checked-out worktree.
    Worktree {
        /// Canonical selected worktree root.
        root: PathBuf,
        /// Structural role of the selected worktree.
        role: GitWorktreeRole,
        /// Canonical Git administrative directory for this worktree.
        administrative_directory: PathBuf,
    },
    /// The supplied path addressed the common control directory instead of source.
    CommonManager {
        /// Whether the structurally active inventory permits automatic source selection.
        source_selection: GitManagerSourceSelection,
    },
}

/// Bounded manager-side source selection derived only from active structural entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitManagerSourceSelection {
    /// No active checked-out source root was structurally discoverable.
    None,
    /// Exactly one active source root can be selected without guessing.
    Unambiguous {
        /// Canonical active worktree root.
        root: PathBuf,
    },
    /// Multiple active roots exist, so an adapter must require deliberate selection.
    Ambiguous {
        /// Number of structurally active roots in the bounded inventory.
        worktree_count: usize,
    },
}

/// Structural role of one checked-out worktree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitWorktreeRole {
    /// Primary checkout whose `.git` directory is the common directory.
    Primary,
    /// Linked checkout with reciprocal common-directory registration evidence.
    Linked,
}

/// One Git worktree administrative entry and its current structural state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitWorktreeEntry {
    /// Structural role represented by this entry.
    pub role: GitWorktreeRole,
    /// Canonical Git administrative directory, stable across a Git-managed move.
    ///
    /// This locator is structural evidence, not a durable identity by itself: a
    /// later recreation may reuse the same administrative path. Persistence stores
    /// the opaque filesystem identity that distinguishes that lifecycle boundary.
    pub administrative_directory: PathBuf,
    /// Current read-only registration state.
    pub state: GitWorktreeState,
}

/// Current structural state of one worktree registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitWorktreeState {
    /// Reciprocal control paths identify one existing checked-out root.
    Active {
        /// Canonical checked-out source root.
        root: PathBuf,
        /// Canonical `.git` directory or control file owned by that root.
        git_control_path: PathBuf,
    },
    /// The registration remains, but its recorded checkout control path is absent.
    Missing {
        /// Recorded `.git` control path, resolved relative to the administrative entry.
        git_control_path: PathBuf,
    },
    /// The registration exists but cannot be admitted as reciprocal identity evidence.
    Invalid {
        /// Exact typed structural problem.
        issue: GitStructureIssue,
    },
}

/// Exact structural problem found while inspecting Git control metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitStructureIssue {
    /// Control path at which the problem was observed.
    pub path: PathBuf,
    /// Closed problem classification and any comparison evidence.
    pub kind: GitStructureIssueKind,
}

/// Closed classifications for malformed or conflicting Git structural evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitStructureIssueKind {
    /// A control path is a symbolic link or junction and is not followed as identity evidence.
    SymbolicLink,
    /// A control path has the wrong filesystem type.
    UnsupportedPathType,
    /// Control metadata could not be read for an operating-system reason.
    FilesystemUnavailable {
        /// Stable standard-library IO classification.
        error_kind: io::ErrorKind,
    },
    /// A bounded control file exceeded its declared byte limit.
    PointerTooLarge {
        /// Maximum admitted bytes.
        limit_bytes: u64,
        /// Observed file bytes.
        observed_bytes: u64,
    },
    /// A control file was not valid UTF-8.
    PointerNotUtf8,
    /// A control file did not contain exactly one non-empty path record in its expected form.
    MalformedPointer,
    /// A selected control pointer addressed a path that does not exist.
    MissingPointerTarget,
    /// A candidate common directory did not have the required Git control structure.
    InvalidCommonDirectory,
    /// Local Git configuration can relocate or suppress the addressed source root.
    UnsupportedSourceConfiguration,
    /// A linked administrative directory was not an immediate registered child of the common directory.
    RegistrationOutsideCommonDirectory,
    /// A registration is missing its required `gitdir` pointer.
    MissingRegistrationPointer,
    /// Reciprocal worktree control evidence pointed at a different path.
    ReciprocalControlMismatch {
        /// Path required by the registration being validated.
        expected: PathBuf,
        /// Different path observed in the reciprocal pointer.
        observed: PathBuf,
    },
    /// A worktree resolved to a different common directory.
    CommonDirectoryMismatch {
        /// Common directory required by the selected repository.
        expected: PathBuf,
        /// Different common directory observed from the worktree.
        observed: PathBuf,
    },
}

/// Discover one repository structure without starting Git or mutating the filesystem.
///
/// The nearest containing worktree wins for nested paths. If the supplied path is
/// inside a common control directory, discovery returns manager selection instead
/// of treating Git metadata as source. A true non-Git path remains that exact path.
///
/// # Errors
///
/// Returns an error when the selected directory cannot be canonicalized, filesystem
/// metadata cannot be read, or the bounded registration inventory is exceeded.
pub fn discover_repository_structure(path: &Path) -> FsResult<RepositoryStructure> {
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_SCAN_TIMEOUT));
    discover_repository_structure_controlled(path, &control)
}

/// Discover one repository structure under caller-owned cancellation and deadline control.
///
/// # Errors
///
/// Returns an error when cooperative work stops, filesystem evidence cannot be read,
/// or the bounded registration inventory is exceeded. Malformed Git evidence is
/// returned as typed discovery state rather than silently downgraded to non-Git.
pub fn discover_repository_structure_controlled(
    path: &Path,
    control: &IndexWorkControl,
) -> FsResult<RepositoryStructure> {
    control.check(IndexWorkStage::RepositoryTraversal)?;
    if !path.is_dir() {
        return Err(FsError::RootNotDirectory(path.to_path_buf()));
    }
    let selected_root = canonicalize(path, path)?;

    for ancestor in selected_root.ancestors() {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        let git_control_path = ancestor.join(".git");
        match fs::symlink_metadata(&git_control_path) {
            Ok(_) => {
                return match inspect_worktree(ancestor) {
                    Ok(selected) => {
                        // A source-owned `.git` file already selects its reciprocal checkout exactly.
                        let selection_kind = if selected.role == GitWorktreeRole::Primary
                            && paths_equal(&selected.git_control_path, &selected.common_directory)
                            && (selected.common_directory_bare_setting == Some(true)
                                || !selected.common_directory_source_root_inference_safe)
                        {
                            GitRepositorySelectionKind::Manager
                        } else {
                            GitRepositorySelectionKind::Worktree
                        };
                        build_git_structure(selected, selection_kind, control)
                            .map(RepositoryStructure::Git)
                    }
                    Err(issue) => Ok(RepositoryStructure::InvalidGit {
                        selected_root: ancestor.to_path_buf(),
                        issue,
                    }),
                };
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(FsError::RepositoryBoundary {
                    path: git_control_path,
                    source,
                });
            }
        }

        if has_git_control_markers(ancestor)? {
            return match inspect_common_directory(ancestor) {
                Ok(common_directory) => build_git_structure(
                    SelectedWorktree {
                        root: ancestor.to_path_buf(),
                        git_control_path: common_directory.path.clone(),
                        administrative_directory: common_directory.path.clone(),
                        common_directory: common_directory.path,
                        common_directory_bare_setting: common_directory.bare_setting,
                        common_directory_source_root_inference_safe: common_directory
                            .source_root_inference_safe,
                        role: GitWorktreeRole::Primary,
                    },
                    GitRepositorySelectionKind::Manager,
                    control,
                )
                .map(RepositoryStructure::Git),
                Err(issue) => Ok(RepositoryStructure::InvalidGit {
                    selected_root: ancestor.to_path_buf(),
                    issue,
                }),
            };
        }
    }

    Ok(RepositoryStructure::NonGit { selected_root })
}

/// Internal validated selected-worktree evidence.
#[derive(Clone, Debug)]
struct SelectedWorktree {
    /// Canonical checked-out root, or addressed common directory for manager selection.
    root: PathBuf,
    /// Canonical source-owned `.git` path.
    git_control_path: PathBuf,
    /// Canonical worktree administrative directory.
    administrative_directory: PathBuf,
    /// Canonical repository common directory.
    common_directory: PathBuf,
    /// Explicit local `core.bare` setting, when present.
    common_directory_bare_setting: Option<bool>,
    /// Whether local config permits inferring source beside the common directory.
    common_directory_source_root_inference_safe: bool,
    /// Structural worktree role.
    role: GitWorktreeRole,
}

/// Validated common-directory identity and local bare-worktree policy.
#[derive(Clone, Debug)]
struct InspectedCommonDirectory {
    /// Canonical common control directory.
    path: PathBuf,
    /// Explicit local `core.bare` setting, when present.
    bare_setting: Option<bool>,
    /// Whether local config permits inferring source beside the common directory.
    source_root_inference_safe: bool,
}

/// Bounded local Git config facts needed for read-only source selection.
#[derive(Clone, Debug, Eq, PartialEq)]
struct GitLocalConfigPolicy {
    /// Explicit local `core.bare` setting, when present and include-free.
    bare_setting: Option<bool>,
    /// False when local worktree settings or unresolved includes may relocate source.
    source_root_inference_safe: bool,
    /// Whether Git reads per-worktree values from `config.worktree`.
    worktree_config_enabled: bool,
    /// Configured `core.worktree` value, resolved only by an exact pointer owner.
    worktree_setting: Option<PathBuf>,
    /// Whether the bounded source-selection subset was parsed without uncertainty.
    source_selection_policy_complete: bool,
}

/// Internal selection branch used while constructing the common inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitRepositorySelectionKind {
    /// A checked-out source root was selected.
    Worktree,
    /// The common manager itself was selected.
    Manager,
}

/// Build deterministic repository inventory from one validated selected structure.
fn build_git_structure(
    selected: SelectedWorktree,
    selection_kind: GitRepositorySelectionKind,
    control: &IndexWorkControl,
) -> FsResult<GitRepositoryStructure> {
    let common_directory = canonicalize(&selected.common_directory, &selected.common_directory)?;
    let primary_root = primary_worktree_root(
        &common_directory,
        selected.common_directory_bare_setting,
        selected.common_directory_source_root_inference_safe,
    )?;
    let primary_may_be_unlisted = primary_root.is_none()
        && (selected.common_directory_bare_setting.is_none()
            || !selected.common_directory_source_root_inference_safe)
        && common_directory.file_name().and_then(|name| name.to_str()) == Some(".git");
    let selected_primary = (selection_kind == GitRepositorySelectionKind::Worktree
        && selected.role == GitWorktreeRole::Primary)
        .then_some(selected.clone());
    let worktrees = worktree_inventory(
        &common_directory,
        primary_root.as_deref(),
        selected_primary.as_ref(),
        control,
    )?;

    let selection = match selection_kind {
        GitRepositorySelectionKind::Worktree => GitRepositorySelection::Worktree {
            root: selected.root,
            role: selected.role,
            administrative_directory: selected.administrative_directory,
        },
        GitRepositorySelectionKind::Manager => GitRepositorySelection::CommonManager {
            source_selection: manager_source_selection(&worktrees, primary_may_be_unlisted),
        },
    };

    Ok(GitRepositoryStructure {
        common_directory,
        selection,
        worktrees,
    })
}

/// Inspect one exact worktree root from its `.git` control path.
fn inspect_worktree(root: &Path) -> Result<SelectedWorktree, GitStructureIssue> {
    let root = canonicalize_issue(root, root)?;
    let git_control_path = root.join(".git");
    let metadata = structural_metadata(&git_control_path)?;
    if metadata.is_dir() {
        let common_directory = inspect_common_directory(&git_control_path)?;
        return Ok(SelectedWorktree {
            root,
            git_control_path: common_directory.path.clone(),
            administrative_directory: common_directory.path.clone(),
            common_directory: common_directory.path,
            common_directory_bare_setting: common_directory.bare_setting,
            common_directory_source_root_inference_safe: common_directory
                .source_root_inference_safe,
            role: GitWorktreeRole::Primary,
        });
    }
    if !metadata.is_file() {
        return Err(issue(
            git_control_path,
            GitStructureIssueKind::UnsupportedPathType,
        ));
    }

    let administrative_pointer = read_prefixed_pointer(&git_control_path, "gitdir:")?;
    let administrative_directory =
        resolve_existing_directory(&git_control_path, &root, &administrative_pointer)?;
    let common_pointer_path = administrative_directory.join("commondir");
    match fs::symlink_metadata(&common_pointer_path) {
        Ok(_) => {
            let common_pointer = read_plain_pointer(&common_pointer_path)?;
            let common_directory_path = resolve_existing_directory(
                &common_pointer_path,
                &administrative_directory,
                &common_pointer,
            )?;
            let common_directory = inspect_common_directory(&common_directory_path)?;
            validate_linked_administrative_directory(
                &administrative_directory,
                &common_directory.path,
            )?;
            validate_reciprocal_control(
                &administrative_directory,
                &git_control_path,
                &common_directory.path,
            )?;
            validate_pointer_source_configuration(
                &root,
                &common_directory.path,
                &administrative_directory,
                true,
            )?;
            Ok(SelectedWorktree {
                root,
                git_control_path: canonicalize_issue(&git_control_path, &git_control_path)?,
                administrative_directory,
                common_directory: common_directory.path,
                common_directory_bare_setting: common_directory.bare_setting,
                common_directory_source_root_inference_safe: common_directory
                    .source_root_inference_safe,
                role: GitWorktreeRole::Linked,
            })
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let common_directory = inspect_common_directory(&administrative_directory)?;
            validate_pointer_source_configuration(
                &root,
                &common_directory.path,
                &administrative_directory,
                false,
            )?;
            Ok(SelectedWorktree {
                root,
                git_control_path: canonicalize_issue(&git_control_path, &git_control_path)?,
                administrative_directory,
                common_directory: common_directory.path,
                common_directory_bare_setting: common_directory.bare_setting,
                common_directory_source_root_inference_safe: common_directory
                    .source_root_inference_safe,
                role: GitWorktreeRole::Primary,
            })
        }
        Err(_) => Err(issue(
            common_pointer_path,
            GitStructureIssueKind::UnsupportedPathType,
        )),
    }
}

/// Validate and canonicalize one Git common control directory.
fn inspect_common_directory(path: &Path) -> Result<InspectedCommonDirectory, GitStructureIssue> {
    let common_directory = canonicalize_issue(path, path)?;
    for (name, directory) in [("HEAD", false), ("objects", true), ("refs", true)] {
        let marker = common_directory.join(name);
        let metadata = structural_metadata(&marker)?;
        if metadata.is_dir() != directory || metadata.is_file() == directory {
            return Err(issue(
                common_directory,
                GitStructureIssueKind::InvalidCommonDirectory,
            ));
        }
    }
    let config = common_directory.join("config");
    let mut config_policy = match fs::symlink_metadata(&config) {
        Ok(_) => local_config_policy(&config)?,
        Err(source) if source.kind() == io::ErrorKind::NotFound => GitLocalConfigPolicy {
            bare_setting: None,
            source_root_inference_safe: true,
            worktree_config_enabled: false,
            worktree_setting: None,
            source_selection_policy_complete: true,
        },
        Err(source) => {
            return Err(issue(
                config,
                GitStructureIssueKind::FilesystemUnavailable {
                    error_kind: source.kind(),
                },
            ));
        }
    };
    if config_policy.worktree_config_enabled {
        let worktree_config = common_directory.join("config.worktree");
        match fs::symlink_metadata(&worktree_config) {
            Ok(_) => {
                let worktree_policy = local_config_policy(&worktree_config)?;
                config_policy.bare_setting = if worktree_policy.source_root_inference_safe {
                    worktree_policy.bare_setting.or(config_policy.bare_setting)
                } else {
                    None
                };
                config_policy.source_root_inference_safe &=
                    worktree_policy.source_root_inference_safe;
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(issue(
                    worktree_config,
                    GitStructureIssueKind::FilesystemUnavailable {
                        error_kind: source.kind(),
                    },
                ));
            }
        }
    }
    let registrations = common_directory.join("worktrees");
    match fs::symlink_metadata(&registrations) {
        Ok(_) => {
            let metadata = structural_metadata(&registrations)?;
            if !metadata.is_dir() {
                return Err(issue(
                    registrations,
                    GitStructureIssueKind::UnsupportedPathType,
                ));
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(issue(
                registrations,
                GitStructureIssueKind::FilesystemUnavailable {
                    error_kind: source.kind(),
                },
            ));
        }
    }
    Ok(InspectedCommonDirectory {
        path: common_directory,
        bare_setting: config_policy.bare_setting,
        source_root_inference_safe: config_policy.source_root_inference_safe,
    })
}

/// Return whether a path has enough structural markers to be treated as a Git manager candidate.
fn has_git_control_markers(path: &Path) -> FsResult<bool> {
    let head = path.join("HEAD");
    let objects = path.join("objects");
    let refs = path.join("refs");
    Ok(path_is_present(&head)? && path_is_present(&objects)? && path_is_present(&refs)?)
}

/// Read bounded local source-selection policy without following includes or starting Git.
/// Continued values fail closed because this bounded reader does not interpret full Git syntax.
fn local_config_policy(path: &Path) -> Result<GitLocalConfigPolicy, GitStructureIssue> {
    let text = read_bounded_text(path, GIT_DIRECTORY_POINTER_MAX_BYTES)?;
    if text.lines().any(|raw_line| {
        let line = raw_line.trim();
        !line.is_empty() && !line.starts_with('#') && !line.starts_with(';') && line.ends_with('\\')
    }) {
        return Ok(GitLocalConfigPolicy {
            bare_setting: None,
            source_root_inference_safe: false,
            worktree_config_enabled: false,
            worktree_setting: None,
            source_selection_policy_complete: false,
        });
    }
    let mut in_core = false;
    let mut in_extensions = false;
    let mut has_include = false;
    let mut bare_setting = None;
    let mut source_root_inference_safe = true;
    let mut worktree_config_enabled = false;
    let mut worktree_setting = None;
    let mut source_selection_policy_complete = true;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let Some((section_name, has_subsection)) = git_config_section(line) else {
                return Ok(GitLocalConfigPolicy {
                    bare_setting: None,
                    source_root_inference_safe: false,
                    worktree_config_enabled: false,
                    worktree_setting: None,
                    source_selection_policy_complete: false,
                });
            };
            has_include |= section_name.eq_ignore_ascii_case("include")
                || section_name.eq_ignore_ascii_case("includeif");
            in_core = !has_subsection && section_name.eq_ignore_ascii_case("core");
            in_extensions = !has_subsection && section_name.eq_ignore_ascii_case("extensions");
            continue;
        }
        if !in_core && !in_extensions {
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .map_or((line, "true"), |(key, value)| (key, value));
        let key = key.trim();
        if in_extensions && key.eq_ignore_ascii_case("worktreeconfig") {
            let Some(value) = git_config_value(raw_value) else {
                source_root_inference_safe = false;
                worktree_config_enabled = false;
                source_selection_policy_complete = false;
                continue;
            };
            if let Some(enabled) = parse_git_boolean(value) {
                worktree_config_enabled = enabled;
            } else {
                source_root_inference_safe = false;
                worktree_config_enabled = false;
                source_selection_policy_complete = false;
            }
            continue;
        }
        if !in_core {
            continue;
        }
        if key.eq_ignore_ascii_case("worktree") {
            let Some(value) = git_config_value(raw_value) else {
                source_root_inference_safe = false;
                worktree_setting = None;
                source_selection_policy_complete = false;
                continue;
            };
            worktree_setting = (!value.is_empty()).then(|| PathBuf::from(value));
            source_root_inference_safe = false;
            source_selection_policy_complete &= worktree_setting.is_some();
            continue;
        }
        if !key.eq_ignore_ascii_case("bare") {
            continue;
        }
        let Some(value) = git_config_value(raw_value) else {
            bare_setting = None;
            source_root_inference_safe = false;
            source_selection_policy_complete = false;
            continue;
        };
        if let Some(bare) = parse_git_boolean(value) {
            bare_setting = Some(bare);
        } else {
            bare_setting = None;
            source_root_inference_safe = false;
            source_selection_policy_complete = false;
        }
    }
    source_selection_policy_complete &= !has_include;
    Ok(GitLocalConfigPolicy {
        bare_setting: (!has_include).then_some(bare_setting).flatten(),
        source_root_inference_safe: source_root_inference_safe && !has_include,
        worktree_config_enabled,
        worktree_setting,
        source_selection_policy_complete,
    })
}

/// Parse one complete Git section header while allowing only a trailing comment.
fn git_config_section(line: &str) -> Option<(&str, bool)> {
    let mut quoted = false;
    let mut escaped = false;
    let mut header_end = line.len();
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => {
                header_end = index;
                break;
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return None;
    }
    let section = line[..header_end]
        .trim()
        .strip_prefix('[')?
        .strip_suffix(']')?
        .trim();
    if section.is_empty() || section.contains(['[', ']']) {
        return None;
    }
    let name_end = section
        .find(|character: char| character.is_ascii_whitespace())
        .unwrap_or(section.len());
    let name = &section[..name_end];
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
    {
        return None;
    }
    let subsection = section[name_end..].trim();
    if subsection.is_empty() {
        return Some((name, false));
    }
    let subsection = subsection.strip_prefix('"')?.strip_suffix('"')?;
    let mut escaped = false;
    for character in subsection.chars() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return None;
        }
    }
    (!escaped).then_some((name, true))
}

/// Remove only unquoted Git comments and unwrap one simple quoted value.
fn git_config_value(raw_value: &str) -> Option<&str> {
    let mut quoted = false;
    let mut value_end = raw_value.len();
    for (index, character) in raw_value.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '\\' if quoted => return None,
            '#' | ';' if !quoted => {
                value_end = index;
                break;
            }
            _ => {}
        }
    }
    if quoted {
        return None;
    }
    let value = raw_value[..value_end].trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    (!value.contains('"')).then_some(value)
}

/// Parse the bounded Git boolean forms that can affect source selection.
fn parse_git_boolean(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "" | "false" | "no" | "off" | "0" => Some(false),
        "true" | "yes" | "on" | "1" => Some(true),
        _ => value.parse::<i64>().ok().map(|value| value != 0),
    }
}

/// Reject pointer-owned configuration that selects source outside its owner.
fn validate_pointer_source_configuration(
    root: &Path,
    common_directory: &Path,
    administrative_directory: &Path,
    common_manager_may_be_bare: bool,
) -> Result<(), GitStructureIssue> {
    let common_config = common_directory.join("config");
    let common_policy = match fs::symlink_metadata(&common_config) {
        Ok(_) => local_config_policy(&common_config)?,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(issue(
                common_config,
                GitStructureIssueKind::FilesystemUnavailable {
                    error_kind: source.kind(),
                },
            ));
        }
    };
    validate_pointer_config_policy(
        &common_config,
        administrative_directory,
        root,
        &common_policy,
        common_manager_may_be_bare,
    )?;
    if !common_policy.worktree_config_enabled {
        return Ok(());
    }

    let worktree_config = administrative_directory.join("config.worktree");
    let worktree_policy = match fs::symlink_metadata(&worktree_config) {
        Ok(_) => local_config_policy(&worktree_config)?,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(issue(
                worktree_config,
                GitStructureIssueKind::FilesystemUnavailable {
                    error_kind: source.kind(),
                },
            ));
        }
    };
    validate_pointer_config_policy(
        &worktree_config,
        administrative_directory,
        root,
        &worktree_policy,
        false,
    )
}

/// Require one bounded config policy to preserve an exact pointer owner.
fn validate_pointer_config_policy(
    config_path: &Path,
    administrative_directory: &Path,
    root: &Path,
    policy: &GitLocalConfigPolicy,
    bare_allowed: bool,
) -> Result<(), GitStructureIssue> {
    if !policy.source_selection_policy_complete
        || !bare_allowed && policy.bare_setting == Some(true)
    {
        return Err(issue(
            config_path.to_path_buf(),
            GitStructureIssueKind::UnsupportedSourceConfiguration,
        ));
    }
    let Some(setting) = &policy.worktree_setting else {
        return Ok(());
    };
    let configured_root =
        resolve_existing_directory(config_path, administrative_directory, setting)?;
    if paths_equal(&configured_root, root) {
        Ok(())
    } else {
        Err(issue(
            config_path.to_path_buf(),
            GitStructureIssueKind::UnsupportedSourceConfiguration,
        ))
    }
}

/// Build the primary plus registered linked-worktree inventory.
fn worktree_inventory(
    common_directory: &Path,
    primary_root: Option<&Path>,
    selected_primary: Option<&SelectedWorktree>,
    control: &IndexWorkControl,
) -> FsResult<Vec<GitWorktreeEntry>> {
    let mut worktrees = Vec::new();
    if let Some(root) = primary_root {
        worktrees.push(primary_entry(root, common_directory)?);
    } else if let Some(selected) = selected_primary {
        worktrees.push(active_entry(selected));
    }

    let registrations = common_directory.join("worktrees");
    let entries = match fs::read_dir(&registrations) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(worktrees),
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: registrations,
                source,
            });
        }
    };
    let mut registration_paths = Vec::new();
    for (index, entry) in entries.enumerate() {
        check_registered_worktree(control, index)?;
        let entry = entry.map_err(|source| FsError::RepositoryBoundary {
            path: registrations.clone(),
            source,
        })?;
        registration_paths.push(entry.path());
    }
    registration_paths.sort();
    for registration in registration_paths {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        worktrees.push(inspect_registration(&registration, common_directory)?);
    }
    Ok(worktrees)
}

/// Build the ordinary primary-checkout entry.
fn primary_entry(root: &Path, common_directory: &Path) -> FsResult<GitWorktreeEntry> {
    let root = canonicalize(root, root)?;
    let git_control_path = canonicalize(&root.join(".git"), &root.join(".git"))?;
    Ok(GitWorktreeEntry {
        role: GitWorktreeRole::Primary,
        administrative_directory: common_directory.to_path_buf(),
        state: GitWorktreeState::Active {
            root,
            git_control_path,
        },
    })
}

/// Convert validated selected evidence into one active inventory entry.
fn active_entry(selected: &SelectedWorktree) -> GitWorktreeEntry {
    GitWorktreeEntry {
        role: selected.role,
        administrative_directory: selected.administrative_directory.clone(),
        state: GitWorktreeState::Active {
            root: selected.root.clone(),
            git_control_path: selected.git_control_path.clone(),
        },
    }
}

/// Inspect one common-directory `worktrees/*` administrative entry.
fn inspect_registration(
    registration: &Path,
    common_directory: &Path,
) -> FsResult<GitWorktreeEntry> {
    let administrative_directory = match structural_metadata(registration) {
        Ok(metadata) if metadata.is_dir() => canonicalize(registration, registration)?,
        Ok(_) => {
            return Ok(invalid_entry(
                registration.to_path_buf(),
                issue(
                    registration.to_path_buf(),
                    GitStructureIssueKind::UnsupportedPathType,
                ),
            ));
        }
        Err(issue) => return Ok(invalid_entry(registration.to_path_buf(), issue)),
    };
    let gitdir_path = administrative_directory.join("gitdir");
    let pointer = match fs::symlink_metadata(&gitdir_path) {
        Ok(_) => match read_plain_pointer(&gitdir_path) {
            Ok(pointer) => pointer,
            Err(issue) => return Ok(invalid_entry(administrative_directory, issue)),
        },
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(invalid_entry(
                administrative_directory,
                issue(
                    gitdir_path,
                    GitStructureIssueKind::MissingRegistrationPointer,
                ),
            ));
        }
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: gitdir_path,
                source,
            });
        }
    };
    let git_control_path = resolve_pointer(&administrative_directory, &pointer);
    match fs::symlink_metadata(&git_control_path) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(GitWorktreeEntry {
                role: GitWorktreeRole::Linked,
                administrative_directory,
                state: GitWorktreeState::Missing { git_control_path },
            });
        }
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: git_control_path,
                source,
            });
        }
        Ok(_) => {}
    }
    let git_control_path = match canonicalize_issue(&git_control_path, &git_control_path) {
        Ok(path) => path,
        Err(issue) => return Ok(invalid_entry(administrative_directory, issue)),
    };
    if git_control_path.file_name().and_then(|name| name.to_str()) != Some(".git") {
        return Ok(invalid_entry(
            administrative_directory,
            issue(git_control_path, GitStructureIssueKind::UnsupportedPathType),
        ));
    }
    let Some(root) = git_control_path.parent() else {
        return Ok(invalid_entry(
            administrative_directory,
            issue(git_control_path, GitStructureIssueKind::UnsupportedPathType),
        ));
    };
    let selected = match inspect_worktree(root) {
        Ok(selected) => selected,
        Err(issue) => return Ok(invalid_entry(administrative_directory, issue)),
    };
    if selected.role != GitWorktreeRole::Linked
        || !paths_equal(
            &selected.administrative_directory,
            &administrative_directory,
        )
    {
        return Ok(invalid_entry(
            administrative_directory.clone(),
            issue(
                git_control_path,
                GitStructureIssueKind::ReciprocalControlMismatch {
                    expected: administrative_directory,
                    observed: selected.administrative_directory,
                },
            ),
        ));
    }
    if !paths_equal(&selected.common_directory, common_directory) {
        return Ok(invalid_entry(
            administrative_directory,
            issue(
                git_control_path,
                GitStructureIssueKind::CommonDirectoryMismatch {
                    expected: common_directory.to_path_buf(),
                    observed: selected.common_directory,
                },
            ),
        ));
    }
    Ok(active_entry(&selected))
}

/// Return an invalid linked registration entry.
fn invalid_entry(administrative_directory: PathBuf, issue: GitStructureIssue) -> GitWorktreeEntry {
    GitWorktreeEntry {
        role: GitWorktreeRole::Linked,
        administrative_directory,
        state: GitWorktreeState::Invalid { issue },
    }
}

/// Validate that one linked administrative directory is an immediate common registration.
fn validate_linked_administrative_directory(
    administrative_directory: &Path,
    common_directory: &Path,
) -> Result<(), GitStructureIssue> {
    let registrations = common_directory.join("worktrees");
    let metadata = structural_metadata(&registrations)?;
    if !metadata.is_dir() {
        return Err(issue(
            registrations,
            GitStructureIssueKind::UnsupportedPathType,
        ));
    }
    if administrative_directory
        .parent()
        .is_some_and(|parent| paths_equal(parent, &registrations))
    {
        Ok(())
    } else {
        Err(issue(
            administrative_directory.to_path_buf(),
            GitStructureIssueKind::RegistrationOutsideCommonDirectory,
        ))
    }
}

/// Validate both directions of one linked-worktree registration.
fn validate_reciprocal_control(
    administrative_directory: &Path,
    expected_git_control_path: &Path,
    expected_common_directory: &Path,
) -> Result<(), GitStructureIssue> {
    let gitdir_path = administrative_directory.join("gitdir");
    let pointer = read_plain_pointer(&gitdir_path)?;
    let observed = resolve_existing_file(&gitdir_path, administrative_directory, &pointer)?;
    let expected = canonicalize_issue(expected_git_control_path, expected_git_control_path)?;
    if !paths_equal(&expected, &observed) {
        return Err(issue(
            gitdir_path,
            GitStructureIssueKind::ReciprocalControlMismatch { expected, observed },
        ));
    }

    let common_pointer_path = administrative_directory.join("commondir");
    let common_pointer = read_plain_pointer(&common_pointer_path)?;
    let observed_common = resolve_existing_directory(
        &common_pointer_path,
        administrative_directory,
        &common_pointer,
    )?;
    let expected_common = canonicalize_issue(expected_common_directory, expected_common_directory)?;
    if paths_equal(&expected_common, &observed_common) {
        Ok(())
    } else {
        Err(issue(
            common_pointer_path,
            GitStructureIssueKind::CommonDirectoryMismatch {
                expected: expected_common,
                observed: observed_common,
            },
        ))
    }
}

/// Infer an ordinary primary worktree from a common `.git` directory.
fn primary_worktree_root(
    common_directory: &Path,
    common_directory_bare_setting: Option<bool>,
    source_root_inference_safe: bool,
) -> FsResult<Option<PathBuf>> {
    if !source_root_inference_safe
        || common_directory_bare_setting != Some(false)
        || common_directory.file_name().and_then(|name| name.to_str()) != Some(".git")
    {
        return Ok(None);
    }
    let Some(parent) = common_directory.parent() else {
        return Ok(None);
    };
    let marker = parent.join(".git");
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FsError::RepositoryBoundary {
                path: marker,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(None);
    }
    let marker = canonicalize(&marker, &marker)?;
    if paths_equal(&marker, common_directory) {
        canonicalize(parent, parent).map(Some)
    } else {
        Ok(None)
    }
}

/// Derive manager auto-selection without branch, path-name, or recency guesses.
fn manager_source_selection(
    worktrees: &[GitWorktreeEntry],
    primary_may_be_unlisted: bool,
) -> GitManagerSourceSelection {
    let mut count = 0_usize;
    let mut only_root = None;
    for entry in worktrees {
        if let GitWorktreeState::Active { root, .. } = &entry.state {
            count = count.saturating_add(1);
            if count == 1 {
                only_root = Some(root.clone());
            }
        }
    }
    if primary_may_be_unlisted && count == 1 {
        return GitManagerSourceSelection::Ambiguous { worktree_count: 2 };
    }
    match (count, only_root) {
        (0, _) => GitManagerSourceSelection::None,
        (1, Some(root)) => GitManagerSourceSelection::Unambiguous { root },
        (worktree_count, _) => GitManagerSourceSelection::Ambiguous { worktree_count },
    }
}

/// Return an opaque identity for one current Git administrative-directory lifecycle.
///
/// The value remains stable when Git moves the checked-out worktree because the
/// administrative directory itself remains in place. Removing and recreating that
/// directory produces a different filesystem identity even when Git reuses its path.
///
/// # Errors
///
/// Returns an error when metadata cannot be read, the path is indirect or not a
/// directory, or the supported platform cannot provide stable lifecycle evidence.
pub fn git_administrative_identity(path: &Path) -> FsResult<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| FsError::RepositoryBoundary {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata_is_indirect(&metadata) || !metadata.is_dir() {
        return Err(FsError::RepositoryBoundary {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "Git administrative identity requires a direct directory",
            ),
        });
    }

    let mut identity = blake3::Hasher::new();
    identity.update(b"projectatlas-git-administrative-identity-v1\0");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        identity.update(b"unix\0");
        identity.update(&metadata.dev().to_le_bytes());
        identity.update(&metadata.ino().to_le_bytes());
        identity.update(&required_creation_nanos(path, metadata.created())?.to_le_bytes());
    }
    #[cfg(windows)]
    {
        let windows = windows_file_identity::read(path)?;
        identity.update(b"windows\0");
        identity.update(&windows.creation_time.to_le_bytes());
        identity.update(&windows.volume_serial_number.to_le_bytes());
        identity.update(&windows.file_id);
    }
    #[cfg(not(any(unix, windows)))]
    {
        identity.update(b"portable\0");
        identity.update(&required_creation_nanos(path, metadata.created())?.to_le_bytes());
    }
    Ok(identity.finalize().to_hex().to_string())
}

/// Require a non-reusable filesystem creation timestamp for lifecycle identity.
#[cfg(not(windows))]
fn required_creation_nanos(
    path: &Path,
    created: io::Result<std::time::SystemTime>,
) -> FsResult<u128> {
    let created = created.map_err(|source| FsError::RepositoryBoundary {
        path: path.to_path_buf(),
        source,
    })?;
    created
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|source| FsError::RepositoryBoundary {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })
}

/// Windows directory identity from the retained native handle.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the stable standard library does not expose Windows volume and 128-bit file identity; this bounded native query avoids a release dependency"
)]
mod windows_file_identity {
    use super::{FsError, FsResult, metadata_is_indirect};
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::{AsRawHandle, RawHandle};
    use std::path::Path;

    /// Permit concurrent readers while the directory identity handle is open.
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    /// Permit ordinary Git writes while the directory identity handle is open.
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    /// Permit ordinary Git deletion while the directory identity handle is open.
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    /// Admit a directory handle through standard Windows file opening.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    /// Inspect rather than traverse a replacement reparse point.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    /// Native `FileIdInfo` query discriminator.
    const FILE_ID_INFO_CLASS: i32 = 18;

    /// Native fixed-size `FILE_ID_INFO` output layout.
    #[repr(C)]
    #[derive(Default)]
    struct NativeFileIdInfo {
        /// Volume identity owning the file.
        volume_serial_number: u64,
        /// Filesystem-provided 128-bit file identity.
        file_id: [u8; 16],
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: RawHandle,
            information_class: i32,
            information: *mut c_void,
            information_bytes: u32,
        ) -> i32;
    }

    /// Stable fields combined with creation time by the lifecycle hash.
    pub(super) struct Identity {
        /// Windows creation timestamp from the retained handle.
        pub(super) creation_time: u64,
        /// Volume identity from `FileIdInfo`.
        pub(super) volume_serial_number: u64,
        /// Filesystem-provided 128-bit file identity.
        pub(super) file_id: [u8; 16],
    }

    /// Open one direct directory and return its retained-handle identity.
    pub(super) fn read(path: &Path) -> FsResult<Identity> {
        let directory = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|source| FsError::RepositoryBoundary {
                path: path.to_path_buf(),
                source,
            })?;
        let metadata = directory
            .metadata()
            .map_err(|source| FsError::RepositoryBoundary {
                path: path.to_path_buf(),
                source,
            })?;
        if metadata_is_indirect(&metadata) || !metadata.is_dir() {
            return Err(FsError::RepositoryBoundary {
                path: path.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Git administrative identity requires a direct directory handle",
                ),
            });
        }

        let mut native = NativeFileIdInfo::default();
        let information_bytes =
            u32::try_from(size_of::<NativeFileIdInfo>()).map_err(|_source| {
                FsError::RepositoryBoundary {
                    path: path.to_path_buf(),
                    source: io::Error::other("Windows file identity structure exceeds DWORD size"),
                }
            })?;
        // SAFETY: `directory` is a live owned handle and `native` is the exact
        // FILE_ID_INFO layout for the fixed-size FileIdInfo query.
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle(),
                FILE_ID_INFO_CLASS,
                (&raw mut native).cast(),
                information_bytes,
            )
        };
        if succeeded == 0 {
            return Err(FsError::RepositoryBoundary {
                path: path.to_path_buf(),
                source: io::Error::last_os_error(),
            });
        }
        Ok(Identity {
            creation_time: metadata.creation_time(),
            volume_serial_number: native.volume_serial_number,
            file_id: native.file_id,
        })
    }
}

/// Read exactly one `prefix path` pointer record.
fn read_prefixed_pointer(path: &Path, prefix: &str) -> Result<PathBuf, GitStructureIssue> {
    let text = read_bounded_text(path, GIT_DIRECTORY_POINTER_MAX_BYTES)?;
    let value = single_pointer_line(path, &text)?;
    let Some(value) = value.strip_prefix(prefix).map(str::trim) else {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::MalformedPointer,
        ));
    };
    path_value(path, value)
}

/// Read exactly one plain path pointer record.
fn read_plain_pointer(path: &Path) -> Result<PathBuf, GitStructureIssue> {
    let text = read_bounded_text(path, GIT_DIRECTORY_POINTER_MAX_BYTES)?;
    path_value(path, single_pointer_line(path, &text)?)
}

/// Read one bounded UTF-8 control file without following a symbolic-link leaf.
fn read_bounded_text(path: &Path, limit: u64) -> Result<String, GitStructureIssue> {
    let metadata = structural_metadata(path)?;
    if !metadata.is_file() {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::UnsupportedPathType,
        ));
    }
    if metadata.len() > limit {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::PointerTooLarge {
                limit_bytes: limit,
                observed_bytes: metadata.len(),
            },
        ));
    }
    let file = fs::File::open(path).map_err(|source| {
        issue(
            path.to_path_buf(),
            GitStructureIssueKind::FilesystemUnavailable {
                error_kind: source.kind(),
            },
        )
    })?;
    let mut bytes =
        Vec::with_capacity(usize::try_from(metadata.len().min(limit)).unwrap_or(usize::MAX));
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| {
            issue(
                path.to_path_buf(),
                GitStructureIssueKind::FilesystemUnavailable {
                    error_kind: source.kind(),
                },
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::PointerTooLarge {
                limit_bytes: limit,
                observed_bytes: bytes.len() as u64,
            },
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_source| issue(path.to_path_buf(), GitStructureIssueKind::PointerNotUtf8))
}

/// Return one non-empty path record and reject ambiguous extra content.
fn single_pointer_line<'a>(path: &Path, text: &'a str) -> Result<&'a str, GitStructureIssue> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(value) = lines.next() else {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::MalformedPointer,
        ));
    };
    if lines.next().is_some() || value.contains('\0') {
        return Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::MalformedPointer,
        ));
    }
    Ok(value)
}

/// Convert one non-empty path record into a platform path.
fn path_value(path: &Path, value: &str) -> Result<PathBuf, GitStructureIssue> {
    if value.is_empty() {
        Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::MalformedPointer,
        ))
    } else {
        Ok(PathBuf::from(value))
    }
}

/// Resolve one pointer relative to its owning administrative directory.
fn resolve_pointer(base: &Path, pointer: &Path) -> PathBuf {
    if pointer.is_absolute() {
        pointer.to_path_buf()
    } else {
        base.join(pointer)
    }
}

/// Resolve and validate an existing pointed-to directory.
fn resolve_existing_directory(
    pointer_path: &Path,
    base: &Path,
    pointer: &Path,
) -> Result<PathBuf, GitStructureIssue> {
    let target = resolve_pointer(base, pointer);
    let metadata = structural_metadata(&target).map_err(|issue| match issue.kind {
        GitStructureIssueKind::MissingPointerTarget => issue,
        _ => GitStructureIssue {
            path: pointer_path.to_path_buf(),
            kind: issue.kind,
        },
    })?;
    if !metadata.is_dir() {
        return Err(issue(
            pointer_path.to_path_buf(),
            GitStructureIssueKind::UnsupportedPathType,
        ));
    }
    canonicalize_issue(&target, pointer_path)
}

/// Resolve and validate an existing pointed-to file.
fn resolve_existing_file(
    pointer_path: &Path,
    base: &Path,
    pointer: &Path,
) -> Result<PathBuf, GitStructureIssue> {
    let target = resolve_pointer(base, pointer);
    let metadata = structural_metadata(&target).map_err(|issue| GitStructureIssue {
        path: pointer_path.to_path_buf(),
        kind: issue.kind,
    })?;
    if !metadata.is_file() {
        return Err(issue(
            pointer_path.to_path_buf(),
            GitStructureIssueKind::UnsupportedPathType,
        ));
    }
    canonicalize_issue(&target, pointer_path)
}

/// Read leaf metadata without accepting a symbolic link as control identity.
fn structural_metadata(path: &Path) -> Result<fs::Metadata, GitStructureIssue> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata_is_indirect(&metadata) => Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::SymbolicLink,
        )),
        Ok(metadata) => Ok(metadata),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::MissingPointerTarget,
        )),
        Err(source) => Err(issue(
            path.to_path_buf(),
            GitStructureIssueKind::FilesystemUnavailable {
                error_kind: source.kind(),
            },
        )),
    }
}

/// Return whether metadata represents a symbolic link or Windows reparse point.
fn metadata_is_indirect(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Return whether a path exists while preserving actual metadata IO failures.
fn path_is_present(path: &Path) -> FsResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(FsError::RepositoryBoundary {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Canonicalize one filesystem path into the crate's existing typed IO surface.
fn canonicalize(path: &Path, evidence_path: &Path) -> FsResult<PathBuf> {
    path.canonicalize()
        .map_err(|source| FsError::RepositoryBoundary {
            path: evidence_path.to_path_buf(),
            source,
        })
}

/// Canonicalize one structural path into typed invalid-Git evidence.
fn canonicalize_issue(path: &Path, evidence_path: &Path) -> Result<PathBuf, GitStructureIssue> {
    path.canonicalize().map_err(|source| {
        issue(
            evidence_path.to_path_buf(),
            if source.kind() == io::ErrorKind::NotFound {
                GitStructureIssueKind::MissingPointerTarget
            } else {
                GitStructureIssueKind::UnsupportedPathType
            },
        )
    })
}

/// Compare canonical platform paths, tolerating case-only spelling on Windows.
fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left == right
            || left
                .to_str()
                .zip(right.to_str())
                .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Construct one typed structural issue.
fn issue(path: PathBuf, kind: GitStructureIssueKind) -> GitStructureIssue {
    GitStructureIssue { path, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::{IndexWorkFailure, IndexWorkResource};
    use std::error::Error;
    use std::ffi::OsStr;
    use std::process::Command;

    #[test]
    fn structural_discovery_covers_real_git_worktree_lifecycle_matrix() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("primary checkout");
        fs::create_dir(&primary)?;
        run_git(&primary, ["init"])?;
        run_git(&primary, ["config", "user.name", "ProjectAtlas Test"])?;
        run_git(
            &primary,
            ["config", "user.email", "projectatlas@example.invalid"],
        )?;
        fs::create_dir(primary.join("src"))?;
        fs::write(primary.join("src").join("main.rs"), "fn main() {}\n")?;
        run_git(&primary, ["add", "."])?;
        run_git(&primary, ["commit", "-m", "fixture"])?;

        let config_path = primary.join(".git").join("config");
        let config = fs::read(&config_path)?;
        fs::remove_file(&config_path)?;
        let configless = require_git(discover_repository_structure(&primary.join("src"))?)?;
        require_worktree_selection(
            &configless,
            &primary.canonicalize()?,
            GitWorktreeRole::Primary,
        )?;
        let configless_manager =
            require_git(discover_repository_structure(&primary.join(".git"))?)?;
        require(
            configless_manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "configless manager inferred a primary checkout without positive non-bare evidence",
        )?;
        fs::write(&config_path, &config)?;

        run_git(&primary, ["config", "core.bare", ""])?;
        let effective_bare = Command::new("git")
            .arg("--git-dir")
            .arg(primary.join(".git"))
            .args(["config", "--bool", "core.bare"])
            .output()?;
        require(
            effective_bare.status.success()
                && String::from_utf8(effective_bare.stdout)?.trim() == "false",
            "Git fixture did not interpret an empty core.bare value as false",
        )?;
        let empty_bare_manager =
            require_git(discover_repository_structure(&primary.join(".git"))?)?;
        require(
            empty_bare_manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::Unambiguous {
                        root: primary.canonicalize()?,
                    },
                },
            "empty core.bare value hid the valid primary checkout",
        )?;
        fs::write(&config_path, &config)?;

        let configured_worktree = temp.path().join("configured external worktree");
        fs::create_dir(&configured_worktree)?;
        run_command(
            Command::new("git")
                .current_dir(&primary)
                .args(["config", "core.worktree"])
                .arg(&configured_worktree),
        )?;
        let effective_worktree = Command::new("git")
            .arg("--git-dir")
            .arg(primary.join(".git"))
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        require(
            effective_worktree.status.success()
                && paths_equal(
                    &PathBuf::from(String::from_utf8(effective_worktree.stdout)?.trim())
                        .canonicalize()?,
                    &configured_worktree.canonicalize()?,
                ),
            "Git fixture did not relocate its configured worktree",
        )?;
        for selected in [&primary, &primary.join(".git")] {
            let relocated = require_git(discover_repository_structure(selected)?)?;
            require(
                relocated.selection
                    == GitRepositorySelection::CommonManager {
                        source_selection: GitManagerSourceSelection::None,
                    },
                "core.worktree inferred the common-directory parent as source",
            )?;
        }
        run_git(&primary, ["config", "--unset", "core.worktree"])?;

        run_git(&primary, ["config", "extensions.worktreeConfig", "true"])?;
        run_command(
            Command::new("git")
                .current_dir(&primary)
                .args(["config", "--worktree", "core.worktree"])
                .arg(&configured_worktree),
        )?;
        let worktree_config_path = primary.join(".git").join("config.worktree");
        require(
            worktree_config_path.is_file(),
            "Git fixture did not create config.worktree",
        )?;
        let effective_worktree = Command::new("git")
            .arg("--git-dir")
            .arg(primary.join(".git"))
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        require(
            effective_worktree.status.success()
                && Path::new(String::from_utf8(effective_worktree.stdout)?.trim())
                    .canonicalize()?
                    == configured_worktree.canonicalize()?,
            "Git fixture did not honor config.worktree core.worktree",
        )?;
        for selected in [&primary, &primary.join(".git")] {
            let relocated = require_git(discover_repository_structure(selected)?)?;
            require(
                relocated.selection
                    == GitRepositorySelection::CommonManager {
                        source_selection: GitManagerSourceSelection::None,
                    },
                "config.worktree core.worktree inferred the common-directory parent as source",
            )?;
        }
        run_git(
            &primary,
            ["config", "--worktree", "--unset", "core.worktree"],
        )?;
        run_git(&primary, ["config", "--worktree", "core.bare", "true"])?;
        let effective_bare = Command::new("git")
            .arg("--git-dir")
            .arg(primary.join(".git"))
            .args(["config", "--bool", "core.bare"])
            .output()?;
        require(
            effective_bare.status.success()
                && String::from_utf8(effective_bare.stdout)?.trim() == "true",
            "Git fixture did not honor config.worktree core.bare",
        )?;
        for selected in [&primary, &primary.join(".git")] {
            let per_worktree_bare = require_git(discover_repository_structure(selected)?)?;
            require(
                per_worktree_bare.selection
                    == GitRepositorySelection::CommonManager {
                        source_selection: GitManagerSourceSelection::None,
                    },
                "config.worktree core.bare invented the common-directory parent as source",
            )?;
        }
        run_git(&primary, ["config", "--worktree", "--unset", "core.bare"])?;
        run_git(&primary, ["config", "--unset", "extensions.worktreeConfig"])?;

        let submodule_source = temp.path().join("submodule source");
        fs::create_dir(&submodule_source)?;
        run_git(&submodule_source, ["init"])?;
        run_git(
            &submodule_source,
            ["config", "user.name", "ProjectAtlas Test"],
        )?;
        run_git(
            &submodule_source,
            ["config", "user.email", "projectatlas@example.invalid"],
        )?;
        fs::write(submodule_source.join("lib.rs"), "pub fn submodule() {}\n")?;
        run_git(&submodule_source, ["add", "."])?;
        run_git(&submodule_source, ["commit", "-m", "submodule fixture"])?;
        let submodule = primary.join("vendor").join("submodule");
        run_command(
            Command::new("git")
                .current_dir(&primary)
                .args(["-c", "protocol.file.allow=always", "submodule", "add"])
                .arg(&submodule_source)
                .arg("vendor/submodule"),
        )?;
        let submodule_structure = require_git(discover_repository_structure(&submodule)?)?;
        require_worktree_selection(
            &submodule_structure,
            &submodule.canonicalize()?,
            GitWorktreeRole::Primary,
        )?;
        let submodule_administrative_directory =
            active_entry_for_root(&submodule_structure, &submodule.canonicalize()?)?
                .administrative_directory
                .clone();
        let submodule_config_path = submodule_administrative_directory.join("config");
        let submodule_config = fs::read(&submodule_config_path)?;
        let quoted_pointer_worktree = submodule.with_file_name("submodule#external;worktree");
        fs::create_dir(&quoted_pointer_worktree)?;
        run_command(
            Command::new("git")
                .current_dir(&submodule)
                .args(["config", "core.worktree"])
                .arg(&quoted_pointer_worktree),
        )?;
        let effective_pointer_worktree = Command::new("git")
            .current_dir(&submodule)
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        require(
            effective_pointer_worktree.status.success()
                && paths_equal(
                    &PathBuf::from(String::from_utf8(effective_pointer_worktree.stdout)?.trim())
                        .canonicalize()?,
                    &quoted_pointer_worktree.canonicalize()?,
                ),
            "Git fixture did not preserve the quoted core.worktree comment marker",
        )?;
        require_invalid_kind(
            discover_repository_structure(&submodule)?,
            |kind| matches!(kind, GitStructureIssueKind::UnsupportedSourceConfiguration),
            "primary pointer core.worktree was admitted as pointer-owned source",
        )?;
        fs::write(&submodule_config_path, submodule_config)?;
        run_git(&primary, ["add", ".gitmodules", "vendor/submodule"])?;
        run_git(&primary, ["commit", "-m", "submodule checkout fixture"])?;

        let lookalike = primary.join("src").join("application metadata");
        fs::create_dir(&lookalike)?;
        fs::write(lookalike.join("HEAD"), "ordinary application data\n")?;
        fs::write(lookalike.join("config"), "ordinary application data\n")?;
        let lookalike_structure = require_git(discover_repository_structure(&lookalike)?)?;
        require_worktree_selection(
            &lookalike_structure,
            &primary.canonicalize()?,
            GitWorktreeRole::Primary,
        )?;

        let nested_linked = primary.join("arbitrary container").join("linked checkout");
        add_worktree(&primary, "nested-linked", &nested_linked)?;
        let config = fs::read(&config_path)?;
        fs::remove_file(&config_path)?;
        let configless_mixed_manager =
            require_git(discover_repository_structure(&primary.join(".git"))?)?;
        require(
            configless_mixed_manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::Ambiguous { worktree_count: 2 },
                },
            "configless mixed manager routed to its sole inventoried linked checkout",
        )?;
        fs::write(&config_path, config)?;
        let outside_linked = temp
            .path()
            .join("outside arbitrary 工作树")
            .join("linked chëckout");
        add_worktree(&primary, "outside-linked", &outside_linked)?;
        let nested_cwd = outside_linked.join("deep").join("cwd");
        fs::create_dir_all(&nested_cwd)?;

        run_git(&primary, ["config", "extensions.worktreeConfig", "true"])?;
        run_command(
            Command::new("git")
                .current_dir(&nested_linked)
                .args(["config", "--worktree", "core.worktree"])
                .arg(&configured_worktree),
        )?;
        let linked_effective_root = Command::new("git")
            .current_dir(&nested_linked)
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        require(
            linked_effective_root.status.success()
                && paths_equal(
                    &PathBuf::from(String::from_utf8(linked_effective_root.stdout)?.trim())
                        .canonicalize()?,
                    &configured_worktree.canonicalize()?,
                ),
            "Git fixture did not honor linked config.worktree core.worktree",
        )?;
        require_invalid_kind(
            discover_repository_structure(&nested_linked)?,
            |kind| matches!(kind, GitStructureIssueKind::UnsupportedSourceConfiguration),
            "linked config.worktree core.worktree was admitted as pointer-owned source",
        )?;
        run_git(
            &nested_linked,
            ["config", "--worktree", "--unset", "core.worktree"],
        )?;
        run_git(
            &nested_linked,
            ["config", "--worktree", "core.bare", "true"],
        )?;
        require_invalid_kind(
            discover_repository_structure(&nested_linked)?,
            |kind| matches!(kind, GitStructureIssueKind::UnsupportedSourceConfiguration),
            "linked config.worktree core.bare was admitted as checked-out source",
        )?;
        run_git(
            &nested_linked,
            ["config", "--worktree", "--unset", "core.bare"],
        )?;
        run_git(&primary, ["config", "--unset", "extensions.worktreeConfig"])?;

        let primary_structure = require_git(discover_repository_structure(&primary.join("src"))?)?;
        require_worktree_selection(
            &primary_structure,
            &primary.canonicalize()?,
            GitWorktreeRole::Primary,
        )?;
        require_active_roots(
            &primary_structure,
            [&primary, &nested_linked, &outside_linked],
        )?;

        let linked_structure = require_git(discover_repository_structure(&nested_cwd)?)?;
        require_worktree_selection(
            &linked_structure,
            &outside_linked.canonicalize()?,
            GitWorktreeRole::Linked,
        )?;
        require(
            linked_structure.common_directory == primary.join(".git").canonicalize()?,
            "linked checkout did not resolve the primary common directory",
        )?;

        let manager = require_git(discover_repository_structure(&primary.join(".git"))?)?;
        require(
            manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::Ambiguous { worktree_count: 3 },
                },
            "multi-worktree manager guessed or omitted its ambiguous selection",
        )?;

        let copied_root = temp.path().join("copied registration");
        fs::create_dir(&copied_root)?;
        fs::copy(nested_linked.join(".git"), copied_root.join(".git"))?;
        let copied = discover_repository_structure(&copied_root)?;
        require(
            matches!(
                copied,
                RepositoryStructure::InvalidGit {
                    issue: GitStructureIssue {
                        kind: GitStructureIssueKind::ReciprocalControlMismatch { .. },
                        ..
                    },
                    ..
                }
            ),
            "a copied one-way .git control file was admitted as reciprocal identity",
        )?;

        let before_move = active_entry_for_root(&manager, &outside_linked.canonicalize()?)?
            .administrative_directory
            .clone();
        let relocated = temp
            .path()
            .join("relocated arbitrary container")
            .join("checkout");
        fs::create_dir_all(
            relocated
                .parent()
                .ok_or_else(|| io::Error::other("relocated fixture path has no parent"))?,
        )?;
        move_worktree(&primary, &outside_linked, &relocated)?;
        fs::create_dir_all(relocated.join("nested"))?;
        let relocated_structure =
            require_git(discover_repository_structure(&relocated.join("nested"))?)?;
        let relocated_root = relocated.canonicalize()?;
        require_worktree_selection(
            &relocated_structure,
            &relocated_root,
            GitWorktreeRole::Linked,
        )?;
        require(
            active_entry_for_root(&relocated_structure, &relocated_root)?.administrative_directory
                == before_move,
            "Git-managed relocation did not retain its administrative identity evidence",
        )?;

        fs::remove_dir_all(&relocated)?;
        let after_removal = require_git(discover_repository_structure(&primary.join(".git"))?)?;
        require(
            after_removal.worktrees.iter().any(|entry| {
                entry.administrative_directory == before_move
                    && matches!(entry.state, GitWorktreeState::Missing { .. })
            }),
            "externally removed worktree was not retained as a typed missing registration",
        )?;

        let bare = temp.path().join("bare manager.git");
        clone_bare(&primary, &bare)?;
        let bare_structure = require_git(discover_repository_structure(&bare)?)?;
        require(
            bare_structure.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "bare manager without worktrees exposed a source selection",
        )?;
        let bare_linked = temp.path().join("bare manager checkout");
        add_bare_worktree(&bare, &bare_linked)?;
        let bare_with_source = require_git(discover_repository_structure(&bare)?)?;
        require(
            bare_with_source.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::Unambiguous {
                        root: bare_linked.canonicalize()?,
                    },
                },
            "bare manager did not expose its one unambiguous registered worktree",
        )?;

        let dot_git_container = temp.path().join("bare dot-git container");
        fs::create_dir(&dot_git_container)?;
        let bare_dot_git = dot_git_container.join(".git");
        clone_bare(&primary, &bare_dot_git)?;
        for selected in [&bare_dot_git, &dot_git_container] {
            let structure = require_git(discover_repository_structure(selected)?)?;
            require(
                structure.selection
                    == GitRepositorySelection::CommonManager {
                        source_selection: GitManagerSourceSelection::None,
                    },
                "bare repository named .git invented its unrelated parent as source",
            )?;
        }
        let bare_config_path = bare_dot_git.join("config");
        let bare_config = fs::read(&bare_config_path)?;
        run_command(
            Command::new("git")
                .arg("--git-dir")
                .arg(&bare_dot_git)
                .args(["config", "extensions.bare", "false"]),
        )?;
        let effective_bare = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "core.bare"])
            .output()?;
        require(
            effective_bare.status.success()
                && String::from_utf8(effective_bare.stdout)?.trim() == "true",
            "Git fixture let a non-core bare key override core.bare",
        )?;
        let non_core_bare = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            non_core_bare.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "non-core bare key invented the manager parent as source",
        )?;
        fs::write(&bare_config_path, &bare_config)?;

        run_command(
            Command::new("git")
                .arg("--git-dir")
                .arg(&bare_dot_git)
                .args(["config", "extensions.worktreeConfig", ""]),
        )?;
        fs::write(
            bare_dot_git.join("config.worktree"),
            "[core]\n bare = false\n",
        )?;
        let effective_worktree_config = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "extensions.worktreeConfig"])
            .output()?;
        require(
            effective_worktree_config.status.success()
                && String::from_utf8(effective_worktree_config.stdout)?.trim() == "false",
            "Git fixture did not interpret an empty extensions.worktreeConfig value as false",
        )?;
        let empty_worktree_config = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            empty_worktree_config.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "empty extensions.worktreeConfig enabled config.worktree and invented a source",
        )?;
        fs::remove_file(bare_dot_git.join("config.worktree"))?;
        fs::write(&bare_config_path, &bare_config)?;

        fs::write(
            &bare_config_path,
            "[core]\n bare = true\n[extensions]\n worktreeConfig = 2\n",
        )?;
        fs::write(
            bare_dot_git.join("config.worktree"),
            "[core]\n bare = false\n",
        )?;
        let numeric_worktree_config = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "extensions.worktreeConfig"])
            .output()?;
        require(
            numeric_worktree_config.status.success()
                && String::from_utf8(numeric_worktree_config.stdout)?.trim() == "true",
            "Git fixture did not accept a nonzero decimal worktreeConfig boolean",
        )?;
        let numeric_worktree_config = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            numeric_worktree_config.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::Unambiguous {
                        root: dot_git_container.canonicalize()?,
                    },
                },
            "nonzero decimal worktreeConfig hid the exact configured source",
        )?;
        fs::remove_file(bare_dot_git.join("config.worktree"))?;
        fs::write(&bare_config_path, &bare_config)?;

        fs::write(
            &bare_config_path,
            "[core]\n bare = true\n[extensions]\n worktreeConfig = maybe\n",
        )?;
        fs::write(
            bare_dot_git.join("config.worktree"),
            "[core]\n bare = false\n",
        )?;
        let invalid_worktree_config = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "extensions.worktreeConfig"])
            .output()?;
        require(
            !invalid_worktree_config.status.success(),
            "Git fixture accepted an invalid extensions.worktreeConfig boolean",
        )?;
        let invalid_worktree_config = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            invalid_worktree_config.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "invalid extensions.worktreeConfig enabled config.worktree and invented a source",
        )?;
        fs::remove_file(bare_dot_git.join("config.worktree"))?;
        fs::write(&bare_config_path, &bare_config)?;

        for malformed_config in [
            "[core\n bare = false\n",
            "[core] trailing garbage\n bare = false\n",
        ] {
            fs::write(&bare_config_path, malformed_config)?;
            let malformed_bare = Command::new("git")
                .arg("--git-dir")
                .arg(&bare_dot_git)
                .args(["config", "--bool", "core.bare"])
                .output()?;
            require(
                !malformed_bare.status.success(),
                "Git fixture accepted a malformed section header",
            )?;
            let malformed_bare = require_git(discover_repository_structure(&bare_dot_git)?)?;
            require(
                malformed_bare.selection
                    == GitRepositorySelection::CommonManager {
                        source_selection: GitManagerSourceSelection::None,
                    },
                "malformed section header invented the manager parent as source",
            )?;
        }
        fs::write(&bare_config_path, &bare_config)?;

        fs::write(
            &bare_config_path,
            "[core]\n bare = true\n[extensions]\n worktreeConfig = fals\\\ne\n",
        )?;
        fs::write(
            bare_dot_git.join("config.worktree"),
            "[core]\n bare = false\n",
        )?;
        let effective_worktree_config = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "extensions.worktreeConfig"])
            .output()?;
        require(
            effective_worktree_config.status.success()
                && String::from_utf8(effective_worktree_config.stdout)?.trim() == "false",
            "Git fixture did not join the continued extensions.worktreeConfig value",
        )?;
        let continued_worktree_config = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            continued_worktree_config.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "continued extensions.worktreeConfig value invented the manager parent as source",
        )?;
        fs::remove_file(bare_dot_git.join("config.worktree"))?;
        fs::write(&bare_config_path, &bare_config)?;

        fs::remove_file(&bare_config_path)?;
        let configless_bare_manager = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            configless_bare_manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "configless .git manager inferred a primary checkout without non-bare evidence",
        )?;
        fs::write(&bare_config_path, bare_config)?;

        let included_config = temp.path().join("included bare config");
        fs::write(&included_config, "[core]\n bare = true\n")?;
        let included_path = included_config
            .to_string_lossy()
            .replace('\\', "/")
            .replace('"', "\\\"");
        fs::write(
            bare_dot_git.join("config"),
            format!("[core]\n bare = false\n[include]\n path = \"{included_path}\"\n"),
        )?;
        let effective_bare = Command::new("git")
            .arg("--git-dir")
            .arg(&bare_dot_git)
            .args(["config", "--bool", "core.bare"])
            .output()?;
        require(
            effective_bare.status.success()
                && String::from_utf8(effective_bare.stdout)?.trim() == "true",
            "Git fixture include did not override the local core.bare value",
        )?;
        let included_bare_manager = require_git(discover_repository_structure(&bare_dot_git)?)?;
        require(
            included_bare_manager.selection
                == GitRepositorySelection::CommonManager {
                    source_selection: GitManagerSourceSelection::None,
                },
            "unresolved config include invented the manager parent as source",
        )?;

        let non_git = temp.path().join("plain directory");
        let non_git_nested = non_git.join("nested").join("cwd");
        fs::create_dir_all(non_git.join(".projectatlas"))?;
        fs::create_dir_all(&non_git_nested)?;
        require(
            discover_repository_structure(&non_git_nested)?
                == RepositoryStructure::NonGit {
                    selected_root: non_git_nested.canonicalize()?,
                },
            "plain directory did not preserve the exact caller-selected non-Git root",
        )?;
        Ok(())
    }

    #[test]
    fn structural_discovery_is_git_process_independent_and_rejects_unsafe_control_files()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let handwritten = temp.path().join("handwritten checkout");
        write_structural_primary(&handwritten)?;
        let structure = require_git(discover_repository_structure(&handwritten)?)?;
        require_worktree_selection(
            &structure,
            &handwritten.canonicalize()?,
            GitWorktreeRole::Primary,
        )?;

        let malformed = temp.path().join("malformed pointer");
        fs::create_dir(&malformed)?;
        fs::write(malformed.join(".git"), "gitdir: first\ngitdir: second\n")?;
        require_invalid_kind(
            discover_repository_structure(&malformed)?,
            |kind| matches!(kind, GitStructureIssueKind::MalformedPointer),
            "ambiguous multi-record .git pointer was not rejected",
        )?;

        let oversized = temp.path().join("oversized pointer");
        fs::create_dir(&oversized)?;
        fs::write(
            oversized.join(".git"),
            vec![b'x'; GIT_DIRECTORY_POINTER_MAX_BYTES as usize + 1],
        )?;
        require_invalid_kind(
            discover_repository_structure(&oversized)?,
            |kind| matches!(kind, GitStructureIssueKind::PointerTooLarge { .. }),
            "oversized .git pointer was not rejected at the byte bound",
        )?;

        let non_utf8 = temp.path().join("non utf8 pointer");
        fs::create_dir(&non_utf8)?;
        fs::write(non_utf8.join(".git"), [0xff, 0xfe])?;
        require_invalid_kind(
            discover_repository_structure(&non_utf8)?,
            |kind| matches!(kind, GitStructureIssueKind::PointerNotUtf8),
            "non-UTF-8 .git pointer was not rejected",
        )?;

        let symlinked = temp.path().join("symlinked control");
        fs::create_dir(&symlinked)?;
        create_control_symlink(&handwritten.join(".git"), &symlinked.join(".git"))?;
        require_invalid_kind(
            discover_repository_structure(&symlinked)?,
            |kind| matches!(kind, GitStructureIssueKind::SymbolicLink),
            "symbolic-link Git control metadata was followed as identity evidence",
        )?;

        let common_directory = handwritten.join(".git");
        let external_registrations = temp.path().join("external registrations");
        fs::create_dir(&external_registrations)?;
        let registrations = common_directory.join("worktrees");
        create_control_symlink(&external_registrations, &registrations)?;
        require_invalid_kind(
            discover_repository_structure(&handwritten)?,
            |kind| matches!(kind, GitStructureIssueKind::SymbolicLink),
            "indirect worktree registration container was followed outside the common root",
        )?;
        remove_control_symlink(&registrations)?;

        fs::create_dir(&registrations)?;
        let external_registration = temp.path().join("external registration");
        fs::create_dir(&external_registration)?;
        let indirect_registration = registrations.join("indirect registration");
        create_control_symlink(&external_registration, &indirect_registration)?;
        let structure = require_git(discover_repository_structure(&handwritten)?)?;
        require(
            structure.worktrees.iter().any(|entry| {
                matches!(
                    &entry.state,
                    GitWorktreeState::Invalid {
                        issue: GitStructureIssue {
                            kind: GitStructureIssueKind::SymbolicLink,
                            ..
                        }
                    }
                )
            }),
            "indirect registration entry was not retained as typed invalid evidence",
        )?;
        remove_control_symlink(&indirect_registration)?;

        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let canceled = discover_repository_structure_controlled(
            &handwritten,
            &IndexWorkControl::new(cancellation, None),
        );
        require(
            matches!(
                canceled,
                Err(FsError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }))
            ),
            "pre-canceled structural discovery did not stop before filesystem traversal",
        )?;
        Ok(())
    }

    #[test]
    fn structural_discovery_enforces_the_registration_count_bound_without_partial_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("bounded checkout");
        write_structural_primary(&repo)?;
        let registrations = repo.join(".git").join("worktrees");
        fs::create_dir(&registrations)?;
        for index in 0..=projectatlas_core::MAX_GIT_WORKTREE_REGISTRATIONS {
            fs::create_dir(registrations.join(format!("registration-{index:04}")))?;
        }

        let result = discover_repository_structure(&repo);
        require(
            matches!(
                result,
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded {
                        stage: IndexWorkStage::RepositoryTraversal,
                        resource: IndexWorkResource::Entries,
                        limit,
                        observed,
                    }
                )) if limit == projectatlas_core::MAX_GIT_WORKTREE_REGISTRATIONS as u64
                    && observed == projectatlas_core::MAX_GIT_WORKTREE_REGISTRATIONS as u64 + 1
            ),
            "registration overflow returned partial or untyped repository state",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_identity_requires_a_creation_timestamp() {
        let result = required_creation_nanos(
            Path::new("administrative-directory"),
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "creation time unavailable",
            )),
        );
        assert!(matches!(
            result,
            Err(FsError::RepositoryBoundary { source, .. })
                if source.kind() == io::ErrorKind::Unsupported
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_lifecycle_identity_includes_stable_volume_and_file_identity()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let administrative_directory = temp.path().join("administrative directory");
        fs::create_dir(&administrative_directory)?;
        let first_native = windows_file_identity::read(&administrative_directory)?;
        let first = git_administrative_identity(&administrative_directory)?;
        let stable_native = windows_file_identity::read(&administrative_directory)?;
        let stable = git_administrative_identity(&administrative_directory)?;
        require(
            first_native.volume_serial_number == stable_native.volume_serial_number
                && first_native.file_id == stable_native.file_id
                && first == stable,
            "Windows directory identity changed within one lifecycle",
        )?;

        fs::remove_dir(&administrative_directory)?;
        fs::create_dir(&administrative_directory)?;
        let replacement_native = windows_file_identity::read(&administrative_directory)?;
        let replacement = git_administrative_identity(&administrative_directory)?;
        require(
            first_native.volume_serial_number != replacement_native.volume_serial_number
                || first_native.file_id != replacement_native.file_id,
            "Windows replacement reused the original volume and file identity",
        )?;
        require(
            first != replacement,
            "Windows replacement reused the original lifecycle hash",
        )?;
        Ok(())
    }

    /// Run one Git command with fixed UTF-8 arguments.
    fn run_git<const N: usize>(repo: &Path, arguments: [&str; N]) -> Result<(), Box<dyn Error>> {
        run_command(Command::new("git").current_dir(repo).args(arguments))
    }

    /// Add one linked worktree at an arbitrary exact path.
    fn add_worktree(repo: &Path, branch: &str, path: &Path) -> Result<(), Box<dyn Error>> {
        run_command(
            Command::new("git")
                .current_dir(repo)
                .args(["worktree", "add", "-b", branch])
                .arg(path),
        )
    }

    /// Move one linked worktree through Git so reciprocal metadata is updated.
    fn move_worktree(repo: &Path, from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
        run_command(
            Command::new("git")
                .current_dir(repo)
                .args([OsStr::new("worktree"), OsStr::new("move")])
                .arg(from)
                .arg(to),
        )
    }

    /// Clone one bare common manager from an existing repository.
    fn clone_bare(repo: &Path, bare: &Path) -> Result<(), Box<dyn Error>> {
        let parent = repo
            .parent()
            .ok_or_else(|| io::Error::other("fixture repository has no parent"))?;
        run_command(
            Command::new("git")
                .current_dir(parent)
                .args([OsStr::new("clone"), OsStr::new("--bare")])
                .arg(repo)
                .arg(bare),
        )
    }

    /// Add one detached source worktree to a bare common manager.
    fn add_bare_worktree(bare: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
        run_command(
            Command::new("git")
                .arg("--git-dir")
                .arg(bare)
                .args(["worktree", "add", "--detach"])
                .arg(path)
                .arg("HEAD"),
        )
    }

    /// Execute a prepared fixture command and preserve its stdout/stderr on failure.
    fn run_command(command: &mut Command) -> Result<(), Box<dyn Error>> {
        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "fixture Git command failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ))
            .into())
        }
    }

    /// Write the minimum standard Git control shape used by process-independent tests.
    fn write_structural_primary(root: &Path) -> Result<(), Box<dyn Error>> {
        let common = root.join(".git");
        write_structural_common(&common)
    }

    /// Write the minimum standard common-control shape without invoking Git.
    fn write_structural_common(common: &Path) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(common.join("objects"))?;
        fs::create_dir(common.join("refs"))?;
        fs::write(common.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(common.join("config"), "[core]\n bare = false\n")?;
        Ok(())
    }

    /// Create a directory control symlink.
    #[cfg(unix)]
    fn create_control_symlink(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }

    /// Create a directory control symlink or junction.
    #[cfg(windows)]
    fn create_control_symlink(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                run_command(
                    Command::new("cmd.exe")
                        .args(["/D", "/C", "mklink", "/J"])
                        .arg(link)
                        .arg(target),
                )?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Remove only the test-owned directory indirection leaf.
    #[cfg(unix)]
    fn remove_control_symlink(link: &Path) -> Result<(), Box<dyn Error>> {
        fs::remove_file(link)?;
        Ok(())
    }

    /// Remove only the test-owned directory indirection leaf.
    #[cfg(windows)]
    fn remove_control_symlink(link: &Path) -> Result<(), Box<dyn Error>> {
        fs::remove_dir(link)?;
        Ok(())
    }

    /// Extract validated Git state from a discovery result.
    fn require_git(
        structure: RepositoryStructure,
    ) -> Result<GitRepositoryStructure, Box<dyn Error>> {
        match structure {
            RepositoryStructure::Git(structure) => Ok(structure),
            other => {
                Err(io::Error::other(format!("expected Git structure, found {other:?}")).into())
            }
        }
    }

    /// Require the exact selected worktree root and role.
    fn require_worktree_selection(
        structure: &GitRepositoryStructure,
        expected_root: &Path,
        expected_role: GitWorktreeRole,
    ) -> Result<(), Box<dyn Error>> {
        require(
            matches!(
                &structure.selection,
                GitRepositorySelection::Worktree { root, role, .. }
                    if paths_equal(root, expected_root) && *role == expected_role
            ),
            "repository selection did not identify the expected exact worktree",
        )
    }

    /// Require all expected exact roots to appear active.
    fn require_active_roots<const N: usize>(
        structure: &GitRepositoryStructure,
        expected: [&Path; N],
    ) -> Result<(), Box<dyn Error>> {
        for root in expected {
            let root = root.canonicalize()?;
            let _ = active_entry_for_root(structure, &root)?;
        }
        Ok(())
    }

    /// Return one active entry for an exact root.
    fn active_entry_for_root<'a>(
        structure: &'a GitRepositoryStructure,
        expected_root: &Path,
    ) -> Result<&'a GitWorktreeEntry, Box<dyn Error>> {
        structure
            .worktrees
            .iter()
            .find(|entry| {
                matches!(
                    &entry.state,
                    GitWorktreeState::Active { root, .. } if paths_equal(root, expected_root)
                )
            })
            .ok_or_else(|| {
                io::Error::other(format!(
                    "missing active structural worktree {}",
                    expected_root.display()
                ))
                .into()
            })
    }

    /// Require one selected invalid-Git problem classification.
    fn require_invalid_kind(
        structure: RepositoryStructure,
        matches_kind: impl FnOnce(&GitStructureIssueKind) -> bool,
        message: &str,
    ) -> Result<(), Box<dyn Error>> {
        let matches = match structure {
            RepositoryStructure::InvalidGit { issue, .. } => matches_kind(&issue.kind),
            RepositoryStructure::NonGit { .. } | RepositoryStructure::Git(_) => false,
        };
        require(matches, message)
    }

    /// Require one test condition without panicking from a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }
}
