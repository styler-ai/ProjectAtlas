//! VCS-aware impact and conservative dead-code analysis.

use super::{
    AnalysisFinding, AnalysisFindingKind, AnalysisStatus, DetailedRelationNode, GitImpactSelection,
    LocalEdge, RelationAnalysisQuery, RelationAnchor, ServiceResult, SupplementalWork, VcsImpact,
    analysis_nodes_for, check_control, dependency_relation, entity_matches_anchor, entity_path,
    load_admitted_symbols, symbol_identity, usage_indegrees,
};
use projectatlas_core::IndexWorkControl;
use projectatlas_db::AtlasStore;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Read;
use std::mem::size_of;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Maximum diagnostic bytes admitted alongside Git stdout.
const GIT_DIAGNOSTIC_MAX_BYTES: u64 = 64 * 1024;
/// Retained digest bytes bound into an impact continuation.
const GIT_DIGEST_BYTES: u64 = 32;
/// Conservative multiplier for temporary repository-path validation storage.
const GIT_PATH_VALIDATION_MULTIPLIER: u64 = 32;
/// Fixed allowance for temporary path-validation containers.
const GIT_PATH_VALIDATION_FIXED_BYTES: u64 = 256;
/// Repository-selection variables that must not override the selected project root.
const GIT_REPOSITORY_ENVIRONMENT_VARIABLES: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// Compute VCS impact and conservative dead-code findings.
pub(super) fn impact_findings(
    store: &AtlasStore,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    topology_complete: bool,
    dead_code_scope_complete: bool,
    vcs: &VcsImpact,
    changed_paths: &[String],
    query: &RelationAnalysisQuery,
    symbol_byte_budget: u64,
    supplemental_work: &mut SupplementalWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    check_control(control)?;
    let mut findings = if query.include_dead_code {
        dead_code_findings(
            store,
            nodes,
            edges,
            dead_code_scope_complete,
            &query.relations.anchor,
            symbol_byte_budget,
            supplemental_work,
            control,
        )?
    } else {
        Vec::new()
    };
    if let VcsImpact::Available { .. } = vcs {
        let mut changed = BTreeSet::new();
        for path in changed_paths {
            check_control(control)?;
            changed.insert(path.clone());
        }
        let mut seeds = Vec::new();
        for (key, node) in nodes {
            check_control(control)?;
            if entity_path(&node.entity).is_some_and(|path| changed.contains(path)) {
                seeds.push(key.clone());
            }
        }
        let mut reverse = BTreeMap::<String, Vec<String>>::new();
        for edge in edges.iter().filter(|edge| dependency_relation(edge.kind)) {
            check_control(control)?;
            reverse
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone());
        }
        let mut distance = BTreeMap::<String, u32>::new();
        let mut queue = VecDeque::new();
        for seed in seeds {
            check_control(control)?;
            distance.insert(seed.clone(), 0);
            queue.push_back(seed);
        }
        while let Some(target) = queue.pop_front() {
            check_control(control)?;
            let next_distance = distance
                .get(&target)
                .copied()
                .unwrap_or_default()
                .saturating_add(1);
            for dependent in reverse.get(&target).into_iter().flatten() {
                check_control(control)?;
                if !distance.contains_key(dependent) {
                    distance.insert(dependent.clone(), next_distance);
                    queue.push_back(dependent.clone());
                }
            }
        }
        if distance.is_empty() {
            findings.push(AnalysisFinding {
                kind: AnalysisFindingKind::Impact,
                status: if topology_complete {
                    AnalysisStatus::Absent
                } else {
                    AnalysisStatus::Inconclusive
                },
                summary: if topology_complete {
                    "no admitted relation node intersects the selected VCS paths"
                } else {
                    "no admitted relation node intersects the selected VCS paths, but topology evidence is incomplete"
                }
                .to_string(),
                nodes: Vec::new(),
                metric: Some(0),
                evidence: None,
            });
        }
        for (key, hops) in distance {
            check_control(control)?;
            findings.push(AnalysisFinding {
                kind: AnalysisFindingKind::Impact,
                status: AnalysisStatus::Candidate,
                summary: if hops == 0 {
                    "relation node is owned by a VCS-selected changed path"
                } else {
                    "relation node statically depends on a VCS-selected changed path"
                }
                .to_string(),
                nodes: analysis_nodes_for(nodes, std::slice::from_ref(&key)),
                metric: Some(u64::from(hops)),
                evidence: None,
            });
        }
    } else if let VcsImpact::Unavailable { reason, .. } = vcs {
        findings.push(AnalysisFinding {
            kind: AnalysisFindingKind::Impact,
            status: AnalysisStatus::Inconclusive,
            summary: format!("VCS-aware impact is unavailable: {reason}"),
            nodes: Vec::new(),
            metric: None,
            evidence: None,
        });
    }
    check_control(control)?;
    Ok(findings)
}

/// Compute dead-code findings before the larger VCS impact result set is retained.
fn dead_code_findings(
    store: &AtlasStore,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    scope_complete: bool,
    anchor: &RelationAnchor,
    symbol_byte_budget: u64,
    supplemental_work: &mut SupplementalWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    check_control(control)?;
    if !scope_complete {
        return Ok(vec![AnalysisFinding {
            kind: AnalysisFindingKind::DeadCode,
            status: AnalysisStatus::Inconclusive,
            summary: "dead-code candidates require a complete all-family, all-confidence, resolved inbound exact-symbol scope"
                .to_string(),
            nodes: Vec::new(),
            metric: None,
            evidence: None,
        }]);
    }
    #[cfg(test)]
    super::analysis_test_observer::notify(
        super::analysis_test_observer::AnalysisPhaseEvent::DeadCodeDiscovery,
    );
    check_control(control)?;
    let symbols = load_admitted_symbols(store, nodes, symbol_byte_budget, control)?;
    supplemental_work.hydrated_symbols = supplemental_work
        .hydrated_symbols
        .saturating_add(symbols.rows_retained);
    supplemental_work.hydrated_symbol_bytes = supplemental_work
        .hydrated_symbol_bytes
        .saturating_add(symbols.retained_bytes);
    supplemental_work.hydrated_symbol_peak_bytes = supplemental_work
        .hydrated_symbol_peak_bytes
        .max(symbols.peak_bytes);
    supplemental_work.symbol_hydration_truncated |= !symbols.complete;
    supplemental_work
        .reached_limits
        .extend(symbols.reached_limits.iter().copied());
    if !symbols.complete {
        return Ok(vec![AnalysisFinding {
            kind: AnalysisFindingKind::DeadCode,
            status: AnalysisStatus::Inconclusive,
            summary: "dead-code symbol hydration crossed its path, row, or byte ceiling"
                .to_string(),
            nodes: Vec::new(),
            metric: None,
            evidence: None,
        }]);
    }
    let indegree = usage_indegrees(nodes, edges, control)?;
    let mut candidate = None;
    for (key, node) in nodes {
        check_control(control)?;
        if !entity_matches_anchor(&node.entity, anchor)
            || indegree.get(key).copied().unwrap_or_default() != 0
        {
            continue;
        }
        let Some((path, name, kind, parent, signature)) = symbol_identity(&node.entity) else {
            continue;
        };
        if let Some(rows) = symbols.rows_for_path(path) {
            for symbol in rows {
                check_control(control)?;
                if !symbol.exported
                    && symbol.name == name
                    && symbol.kind == kind
                    && symbol.parent.as_deref() == parent
                    && symbol.signature == signature
                {
                    candidate = Some(key.clone());
                    break;
                }
            }
        }
        break;
    }
    drop(symbols);
    check_control(control)?;
    Ok(candidate.map_or_else(Vec::new, |key| {
        vec![AnalysisFinding {
            kind: AnalysisFindingKind::DeadCode,
            status: AnalysisStatus::Candidate,
            summary: "non-exported declaration has no trusted inbound relation in complete scope"
                .to_string(),
            nodes: analysis_nodes_for(nodes, std::slice::from_ref(&key)),
            metric: Some(0),
            evidence: None,
        }]
    }))
}

/// Bounded normalized VCS evidence and its retained work.
pub(super) struct LoadedVcs {
    /// Typed public VCS availability.
    pub(super) report: VcsImpact,
    /// Deterministically sorted normalized changed paths.
    pub(super) changed_paths: Vec<String>,
    /// Conservative peak Git request, stream, normalization, and digest bytes.
    pub(super) retained_bytes: u64,
}

/// Bounded Git stdout plus the aggregate stream-memory peak that produced it.
struct GitCommandOutput {
    /// NUL-delimited Git stdout retained for normalization.
    stdout: Vec<u8>,
    /// Concurrent stdout and stderr vector storage.
    stream_peak_bytes: u64,
}

/// Failed Git execution with the largest stream allocation already observed.
struct GitCommandError {
    /// Stable typed-unavailability diagnostic.
    reason: String,
    /// Concurrent stdout and stderr vector storage observed before failure.
    peak_bytes: u64,
}

/// Deterministic normalized paths and their aggregate normalization peak.
struct NormalizedGitPaths {
    /// Sorted unique repository-relative paths.
    paths: Vec<String>,
    /// Raw stdout plus path-vector and validation transient storage.
    peak_bytes: u64,
}

/// Failed normalization with the largest aggregate allocation already observed.
struct GitPathNormalizationError {
    /// Stable typed-unavailability diagnostic.
    reason: String,
    /// Raw stdout plus normalization storage retained at the failure boundary.
    peak_bytes: u64,
}

/// Digest the normalized changed-path set for cursor freshness.
pub(super) fn digest_vcs_paths(
    paths: &[String],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<[u8; 32]> {
    check_control(control)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"projectatlas:analysis-vcs:v1\0");
    for path in paths {
        check_control(control)?;
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
    }
    check_control(control)?;
    Ok(*hasher.finalize().as_bytes())
}

/// Load one bounded normalized changed-path set from Git.
pub(super) fn load_vcs_paths(
    root: &Path,
    selection: GitImpactSelection,
    byte_limit: u64,
    deadline: Instant,
    control: Option<&IndexWorkControl>,
) -> LoadedVcs {
    let retained_request_bytes =
        vcs_selection_owned_bytes(&selection).saturating_add(GIT_DIGEST_BYTES);
    if byte_limit == 0 {
        return LoadedVcs {
            report: VcsImpact::Unavailable {
                selection,
                reason: "relation traversal exhausted the shared analysis byte budget".to_string(),
            },
            changed_paths: Vec::new(),
            retained_bytes: retained_request_bytes,
        };
    }
    let command_request_bytes =
        retained_request_bytes.saturating_add(vcs_selection_owned_bytes(&selection));
    let Some(command_budget) = byte_limit.checked_sub(command_request_bytes) else {
        return LoadedVcs {
            report: VcsImpact::Unavailable {
                selection,
                reason: "VCS request metadata exhausted the shared analysis byte budget"
                    .to_string(),
            },
            changed_paths: Vec::new(),
            retained_bytes: retained_request_bytes,
        };
    };
    let mut command = git_command(root);
    match &selection {
        GitImpactSelection::WorkingTree => {
            command.args([
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--no-renames",
            ]);
        }
        GitImpactSelection::Index => {
            command.args([
                "diff",
                "--cached",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--",
            ]);
        }
        GitImpactSelection::RevisionRange { base, head } => {
            if !valid_revision(base) || !valid_revision(head) {
                return LoadedVcs {
                    report: VcsImpact::Unavailable {
                        selection,
                        reason: "revision expressions contain unsupported characters".to_string(),
                    },
                    changed_paths: Vec::new(),
                    retained_bytes: retained_request_bytes,
                };
            }
            command.args([
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--end-of-options",
                base,
                head,
                "--",
            ]);
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    match run_git(command, command_budget, deadline, control) {
        Ok(output) => match parse_git_paths(&selection, &output, command_budget, control) {
            Ok(normalized) => LoadedVcs {
                report: VcsImpact::Available {
                    selection,
                    changed_path_count: normalized.paths.len() as u64,
                },
                changed_paths: normalized.paths,
                retained_bytes: command_request_bytes
                    .saturating_add(output.stream_peak_bytes.max(normalized.peak_bytes)),
            },
            Err(failure) => LoadedVcs {
                report: VcsImpact::Unavailable {
                    selection,
                    reason: failure.reason,
                },
                changed_paths: Vec::new(),
                retained_bytes: command_request_bytes
                    .saturating_add(output.stream_peak_bytes.max(failure.peak_bytes)),
            },
        },
        Err(failure) => LoadedVcs {
            report: VcsImpact::Unavailable {
                selection,
                reason: failure.reason,
            },
            changed_paths: Vec::new(),
            retained_bytes: command_request_bytes.saturating_add(failure.peak_bytes),
        },
    }
}

/// Build one Git command bound to the explicitly selected repository root.
pub(super) fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    clear_git_repository_environment(&mut command);
    command
}

/// Remove ambient repository selection while preserving the normal process environment.
fn clear_git_repository_environment(command: &mut Command) {
    for variable in GIT_REPOSITORY_ENVIRONMENT_VARIABLES {
        command.env_remove(variable);
    }
}

/// Run one shell-free bounded Git command with cancellation and deadline checks.
fn run_git(
    mut command: Command,
    byte_limit: u64,
    deadline: Instant,
    control: Option<&IndexWorkControl>,
) -> Result<GitCommandOutput, GitCommandError> {
    check_control(control).map_err(|error| GitCommandError {
        reason: error.to_string(),
        peak_bytes: 0,
    })?;
    let stream_headers = u64::try_from(size_of::<Vec<u8>>())
        .unwrap_or(u64::MAX)
        .saturating_mul(2);
    let payload_budget = byte_limit
        .checked_sub(stream_headers.saturating_add(2))
        .ok_or_else(|| GitCommandError {
            reason: "VCS stream metadata exceeded the analysis byte budget".to_string(),
            peak_bytes: 0,
        })?;
    let stderr_limit = GIT_DIAGNOSTIC_MAX_BYTES.min(payload_budget / 4).max(1);
    let stdout_limit = payload_budget
        .checked_sub(stderr_limit)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| GitCommandError {
            reason: "VCS stdout has no remaining analysis byte budget".to_string(),
            peak_bytes: 0,
        })?;
    let mut child = command.spawn().map_err(|error| GitCommandError {
        reason: format!("git could not start: {error}"),
        peak_bytes: 0,
    })?;
    let Some(stdout) = child.stdout.take() else {
        let cleanup = terminate_and_reap_git(&mut child);
        return Err(GitCommandError {
            reason: with_cleanup("git stdout was unavailable", cleanup),
            peak_bytes: 0,
        });
    };
    let Some(stderr) = child.stderr.take() else {
        drop(stdout);
        let cleanup = terminate_and_reap_git(&mut child);
        return Err(GitCommandError {
            reason: with_cleanup("git stderr was unavailable", cleanup),
            peak_bytes: 0,
        });
    };
    let stdout_reader = thread::spawn(move || read_bounded(stdout, stdout_limit));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, stderr_limit));
    let status = loop {
        if let Err(error) = check_control(control) {
            break Err(error.to_string());
        }
        if Instant::now() >= deadline {
            break Err("git exceeded the analysis deadline".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => break Err(format!("git status failed: {error}")),
        }
    };
    let cleanup = status
        .as_ref()
        .err()
        .and_then(|_reason| terminate_and_reap_git(&mut child));
    let (stdout, stderr) = join_git_readers(stdout_reader, stderr_reader);
    let stream_peak_bytes =
        git_stream_peak_bytes(&stdout).saturating_add(git_stream_peak_bytes(&stderr));
    let status = status.map_err(|reason| GitCommandError {
        reason: with_cleanup(&reason, cleanup),
        peak_bytes: stream_peak_bytes,
    })?;
    let stdout = stdout.map_err(|failure| GitCommandError {
        reason: failure.reason,
        peak_bytes: stream_peak_bytes,
    })?;
    let stderr = stderr.map_err(|failure| GitCommandError {
        reason: failure.reason,
        peak_bytes: stream_peak_bytes,
    })?;
    if stream_peak_bytes > byte_limit {
        return Err(GitCommandError {
            reason: "git streams exceeded the aggregate analysis byte budget".to_string(),
            peak_bytes: stream_peak_bytes,
        });
    }
    if !status.success() {
        let reason = String::from_utf8_lossy(&stderr);
        return Err(GitCommandError {
            reason: format!("git exited with {status}: {}", reason.trim()),
            peak_bytes: stream_peak_bytes,
        });
    }
    Ok(GitCommandOutput {
        stdout,
        stream_peak_bytes,
    })
}

/// Kill and reap one failed Git child, retaining a bounded cleanup diagnostic.
fn terminate_and_reap_git(child: &mut Child) -> Option<String> {
    let kill_error = child.kill().err();
    let wait_error = child.wait().err();
    match (kill_error, wait_error) {
        (None, None) => None,
        (Some(kill), None) => Some(format!("git termination failed: {kill}")),
        (None, Some(wait)) => Some(format!("git reap failed: {wait}")),
        (Some(kill), Some(wait)) => Some(format!(
            "git termination failed: {kill}; git reap failed: {wait}"
        )),
    }
}

/// Preserve the root process failure and append cleanup failure when present.
fn with_cleanup(reason: &str, cleanup: Option<String>) -> String {
    cleanup.map_or_else(
        || reason.to_string(),
        |cleanup| format!("{reason}; {cleanup}"),
    )
}

/// Join both pipe readers before propagating either failure.
fn join_git_readers(
    stdout_reader: JoinHandle<Result<Vec<u8>, GitCommandError>>,
    stderr_reader: JoinHandle<Result<Vec<u8>, GitCommandError>>,
) -> (
    Result<Vec<u8>, GitCommandError>,
    Result<Vec<u8>, GitCommandError>,
) {
    let stdout = stdout_reader
        .join()
        .map_err(|panic| GitCommandError {
            reason: format!("git stdout reader failed: {}", thread_panic_message(&panic)),
            peak_bytes: 0,
        })
        .and_then(std::convert::identity);
    let stderr = stderr_reader
        .join()
        .map_err(|panic| GitCommandError {
            reason: format!("git stderr reader failed: {}", thread_panic_message(&panic)),
            peak_bytes: 0,
        })
        .and_then(std::convert::identity);
    (stdout, stderr)
}

/// Return one reader's retained vector storage on success or failure.
fn git_stream_peak_bytes(result: &Result<Vec<u8>, GitCommandError>) -> u64 {
    result
        .as_ref()
        .map_or_else(|failure| failure.peak_bytes, owned_byte_vector_bytes)
}

/// Return a bounded diagnostic for a reader-thread panic payload.
fn thread_panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.as_str()
    } else {
        "non-string panic payload"
    }
}

/// Read a child stream through one strict byte ceiling.
fn read_bounded(mut reader: impl Read, limit: u64) -> Result<Vec<u8>, GitCommandError> {
    let take = limit.saturating_add(1);
    let mut bytes = Vec::new();
    if let Err(error) = reader.by_ref().take(take).read_to_end(&mut bytes) {
        return Err(GitCommandError {
            reason: format!("git output read failed: {error}"),
            peak_bytes: owned_byte_vector_bytes(&bytes),
        });
    }
    if bytes.len() as u64 > limit {
        return Err(GitCommandError {
            reason: "git output exceeded the analysis byte budget".to_string(),
            peak_bytes: owned_byte_vector_bytes(&bytes),
        });
    }
    Ok(bytes)
}

/// Normalize NUL-delimited Git path output into exact repository keys.
fn parse_git_paths(
    selection: &GitImpactSelection,
    output: &GitCommandOutput,
    byte_limit: u64,
    control: Option<&IndexWorkControl>,
) -> Result<NormalizedGitPaths, GitPathNormalizationError> {
    let mut paths = Vec::new();
    let mut peak_bytes = output.stream_peak_bytes;
    if let Err(error) = check_control(control) {
        return Err(GitPathNormalizationError {
            reason: error.to_string(),
            peak_bytes,
        });
    }
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        if let Err(error) = check_control(control) {
            return Err(GitPathNormalizationError {
                reason: error.to_string(),
                peak_bytes,
            });
        }
        let raw = if matches!(selection, GitImpactSelection::WorkingTree) {
            raw.get(3..).ok_or_else(|| GitPathNormalizationError {
                reason: "git status row was malformed".to_string(),
                peak_bytes,
            })?
        } else {
            raw
        };
        let path = std::str::from_utf8(raw).map_err(|source| GitPathNormalizationError {
            reason: format!("git returned a non-UTF-8 path: {source}"),
            peak_bytes,
        })?;
        let validation_transient = u64::try_from(path.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(GIT_PATH_VALIDATION_MULTIPLIER)
            .saturating_add(GIT_PATH_VALIDATION_FIXED_BYTES);
        let normalized =
            projectatlas_core::validated_repo_file_key(Path::new(path)).map_err(|error| {
                GitPathNormalizationError {
                    reason: format!("git returned an invalid repository path: {error}"),
                    peak_bytes: peak_bytes.max(
                        owned_byte_vector_bytes(&output.stdout)
                            .saturating_add(validation_transient),
                    ),
                }
            })?;
        paths.push(normalized);
        let normalization_bytes = owned_byte_vector_bytes(&output.stdout)
            .saturating_add(owned_string_vector_bytes(&paths))
            .saturating_add(validation_transient);
        peak_bytes = peak_bytes.max(normalization_bytes);
        if peak_bytes > byte_limit {
            return Err(GitPathNormalizationError {
                reason: "Git path normalization exceeded the aggregate analysis byte budget"
                    .to_string(),
                peak_bytes,
            });
        }
    }
    if let Err(error) = check_control(control) {
        return Err(GitPathNormalizationError {
            reason: error.to_string(),
            peak_bytes,
        });
    }
    paths.sort();
    if let Err(error) = check_control(control) {
        return Err(GitPathNormalizationError {
            reason: error.to_string(),
            peak_bytes,
        });
    }
    paths.dedup();
    if let Err(error) = check_control(control) {
        return Err(GitPathNormalizationError {
            reason: error.to_string(),
            peak_bytes,
        });
    }
    Ok(NormalizedGitPaths { paths, peak_bytes })
}

/// Conservatively count one retained byte vector by allocated capacity.
fn owned_byte_vector_bytes(bytes: &Vec<u8>) -> u64 {
    u64::try_from(size_of::<Vec<u8>>())
        .unwrap_or(u64::MAX)
        .saturating_add(u64::try_from(bytes.capacity()).unwrap_or(u64::MAX))
}

/// Conservatively count one retained path vector and every owned string buffer.
fn owned_string_vector_bytes(paths: &Vec<String>) -> u64 {
    let vector_bytes = u64::try_from(size_of::<Vec<String>>())
        .unwrap_or(u64::MAX)
        .saturating_add(
            u64::try_from(paths.capacity())
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::try_from(size_of::<String>()).unwrap_or(u64::MAX)),
        );
    paths.iter().fold(vector_bytes, |bytes, path| {
        bytes.saturating_add(u64::try_from(path.capacity()).unwrap_or(u64::MAX))
    })
}

/// Count the retained selector and owned revision buffers.
fn vcs_selection_owned_bytes(selection: &GitImpactSelection) -> u64 {
    let bytes = u64::try_from(size_of::<GitImpactSelection>()).unwrap_or(u64::MAX);
    match selection {
        GitImpactSelection::WorkingTree | GitImpactSelection::Index => bytes,
        GitImpactSelection::RevisionRange { base, head } => bytes
            .saturating_add(u64::try_from(base.capacity()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(head.capacity()).unwrap_or(u64::MAX)),
    }
}

/// Validate one bounded revision token accepted by the shell-free Git adapter.
fn valid_revision(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.contains("..")
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b'^' | b'~')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::ffi::OsStr;
    use std::fs;
    use std::io;

    #[test]
    fn git_command_removes_ambient_repository_selection() -> Result<(), Box<dyn Error>> {
        let command = git_command(Path::new("."));
        for variable in GIT_REPOSITORY_ENVIRONMENT_VARIABLES {
            if !command
                .get_envs()
                .any(|(name, value)| name == OsStr::new(variable) && value.is_none())
            {
                return Err(io::Error::other(format!(
                    "{variable} was not removed from the Git child environment"
                ))
                .into());
            }
        }
        if !command.get_envs().any(|(name, value)| {
            name == OsStr::new("GIT_OPTIONAL_LOCKS") && value == Some(OsStr::new("0"))
        }) {
            return Err(io::Error::other("Git child did not disable optional locks").into());
        }
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let initialized = git_command(&root).args(["init", "--quiet"]).status()?;
        if !initialized.success() {
            return Err(io::Error::other("test Git repository initialization failed").into());
        }
        let reported = git_command(&root)
            .args(["rev-parse", "--local-env-vars"])
            .output()?;
        if !reported.status.success() {
            return Err(io::Error::other(format!(
                "Git repository-local environment inventory failed: {}",
                String::from_utf8_lossy(&reported.stderr)
            ))
            .into());
        }
        for variable in String::from_utf8(reported.stdout)?.lines() {
            if !GIT_REPOSITORY_ENVIRONMENT_VARIABLES.contains(&variable) {
                return Err(io::Error::other(format!(
                    "Git reported an unhandled repository-local variable: {variable}"
                ))
                .into());
            }
        }
        Ok(())
    }
}
