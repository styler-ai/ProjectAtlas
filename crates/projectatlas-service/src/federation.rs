//! Call-scoped read-only composition across explicitly supplied project roots.

use super::analysis::{RelationAnalysisDraft, RelationAnalysisQuery, RelationAnalysisReport};
use super::relations::{
    DetailedRelationBudget, DetailedRelationPageDraft, DetailedRelationQuery,
    DetailedRelationReport, ExternalRelationIdentity, external_relation_identities,
    relation_request_control, serialized_equivalent_bytes,
};
use super::{ServiceError, ServiceResult, canonical_root_digest, selected_project_binding};
use projectatlas_core::graph::{
    ExtendedRelationKind, ExternalSelector, GraphLimitKind, GraphRelationKind, LogicalRelation,
    ProjectInstanceId, RelationResolution,
};
use projectatlas_core::language::ContentClassification;
use projectatlas_core::symbols::RelationKind;
use projectatlas_core::{CanonicalProjectRoot, IndexGeneration, IndexWorkControl, IndexWorkStage};
use projectatlas_db::{AtlasStore, DbError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Minimum number of explicit roots that constitutes a federated call.
const MIN_FEDERATED_ROOTS: usize = 2;
/// Maximum roots and simultaneously open read snapshots in one call.
const MAX_FEDERATED_ROOTS: usize = 8;
/// Maximum total size of participating project databases.
pub const MAX_FEDERATED_DATABASE_BYTES: u64 = 64 * 1_024 * 1_024 * 1_024;
/// Maximum current source bytes inspected while validating all roots.
pub const MAX_FEDERATED_INPUT_BYTES: u64 = 16 * 1_024 * 1_024 * 1_024;
/// Maximum time allowed to finish and drop every participating read snapshot.
const MAX_FEDERATED_CLOSE_MS: u64 = 1_000;

/// Current opaque federated-continuation representation.
const FEDERATED_CURSOR_VERSION: u16 = 1;
/// Absolute serialized federated-continuation ceiling.
const FEDERATED_CURSOR_MAX_BYTES: usize = 128 * 1_024;
/// Domain separator for machine-path-free root binding.
const FEDERATED_ROOT_DIGEST_DOMAIN: &str = "projectatlas:federated-root:v1";

/// Relation families whose exact typed external identities can rendezvous across roots.
const FEDERATED_RENDEZVOUS_RELATIONS: [GraphRelationKind; 6] = [
    GraphRelationKind::Legacy(RelationKind::Imports),
    GraphRelationKind::Legacy(RelationKind::Calls),
    GraphRelationKind::Legacy(RelationKind::DependsOn),
    GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo),
    GraphRelationKind::Extended(ExtendedRelationKind::Configures),
    GraphRelationKind::Extended(ExtendedRelationKind::Deploys),
];

/// Validate the explicit root count before an adapter opens any database.
///
/// # Errors
///
/// Returns an error unless the call supplies between two and eight roots.
pub fn validate_federated_root_count(count: usize) -> ServiceResult<()> {
    if (MIN_FEDERATED_ROOTS..=MAX_FEDERATED_ROOTS).contains(&count) {
        Ok(())
    } else {
        Err(ServiceError::InvalidInput(format!(
            "federation requires {MIN_FEDERATED_ROOTS}..={MAX_FEDERATED_ROOTS} explicit ordered roots"
        )))
    }
}

/// Exact read-only freshness work completed before one root joined federation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FederatedInputWork {
    /// Repository entries inspected by exact source verification.
    pub filesystem_entries: u64,
    /// Current source bytes hashed by exact source verification.
    pub filesystem_bytes: u64,
    /// `SQLite` statements used by exact source verification.
    pub sqlite_read_statements: u64,
    /// Indexed nodes decoded by exact source verification.
    pub decoded_nodes: u64,
    /// Wall time spent opening and verifying this participant.
    pub elapsed_ms: u64,
}

impl FederatedInputWork {
    /// Add one participant's verification work without counter overflow.
    fn checked_add(self, other: Self) -> ServiceResult<Self> {
        Ok(Self {
            filesystem_entries: checked_sum(
                self.filesystem_entries,
                other.filesystem_entries,
                "federated filesystem-entry work",
            )?,
            filesystem_bytes: checked_sum(
                self.filesystem_bytes,
                other.filesystem_bytes,
                "federated input bytes",
            )?,
            sqlite_read_statements: checked_sum(
                self.sqlite_read_statements,
                other.sqlite_read_statements,
                "federated freshness statements",
            )?,
            decoded_nodes: checked_sum(
                self.decoded_nodes,
                other.decoded_nodes,
                "federated freshness nodes",
            )?,
            elapsed_ms: checked_sum(
                self.elapsed_ms,
                other.elapsed_ms,
                "federated freshness time",
            )?,
        })
    }
}

/// One already verified root-bound read-only store admitted by an adapter.
pub struct FederatedStore {
    /// Root-bound read-only database handle.
    store: AtlasStore,
    /// Database reopened after snapshot close for generation validation.
    database_path: PathBuf,
    /// Canonical explicit project root.
    root: PathBuf,
    /// Participating database file size.
    database_bytes: u64,
    /// Exact adapter-owned freshness work.
    input_work: FederatedInputWork,
    /// Optional validated worktree alias supplied by the adapter.
    worktree: Option<String>,
}

impl FederatedStore {
    /// Admit one root-bound read-only snapshot into a later federated call.
    ///
    /// # Errors
    ///
    /// Returns an error when the store is writable, has no active read snapshot,
    /// names another root, or its database path is not an existing regular file.
    pub fn new(
        store: AtlasStore,
        database_path: PathBuf,
        root: PathBuf,
        input_work: FederatedInputWork,
    ) -> ServiceResult<Self> {
        Self::new_with_worktree(store, database_path, root, input_work, None)
    }

    /// Admit one labelled root-bound read-only snapshot into a federated call.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::new`]. Alias validation remains adapter-owned.
    pub fn new_with_worktree(
        store: AtlasStore,
        database_path: PathBuf,
        root: PathBuf,
        input_work: FederatedInputWork,
        worktree: Option<String>,
    ) -> ServiceResult<Self> {
        if !store.is_read_only() || !store.has_active_read_snapshot() {
            return Err(ServiceError::InvalidInput(
                "federation accepts only read-only stores with active snapshots".to_string(),
            ));
        }
        let explicit_root = CanonicalProjectRoot::from_path(&root)
            .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
        if !store.project_root_identity_matches(&explicit_root) {
            return Err(ServiceError::InvalidInput(
                "federated store does not match its explicit root".to_string(),
            ));
        }
        let metadata = fs::metadata(&database_path).map_err(|source| ServiceError::Io {
            path: database_path.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(ServiceError::InvalidInput(
                "federated database path is not a regular file".to_string(),
            ));
        }
        Ok(Self {
            store,
            database_path,
            root,
            database_bytes: metadata.len(),
            input_work,
            worktree,
        })
    }

    /// Borrow the verified primary store while an adapter constructs exact selectors.
    #[must_use]
    pub const fn store(&self) -> &AtlasStore {
        &self.store
    }

    /// Close this participant without producing a federated result.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot finish the active read snapshot.
    pub fn finish(self) -> ServiceResult<()> {
        self.store.finish_index_read_snapshot().map_err(Into::into)
    }
}

/// One ordered participant retained without exposing its machine-local root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FederatedParticipant {
    /// Zero-based order supplied by the caller.
    pub order: u32,
    /// Registered worktree alias when the adapter selected aliases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Project-qualified identity captured by the read-only store.
    pub project: ProjectInstanceId,
    /// Complete graph generation captured for this root.
    pub generation: IndexGeneration,
    /// Authored-purpose revision captured for this root.
    pub authored_purpose_revision: u64,
}

/// One project-qualified relation supporting an exact typed external rendezvous.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FederatedRelationEvidence {
    /// Registered worktree alias when the adapter selected aliases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Participating project identity.
    pub project: ProjectInstanceId,
    /// Participating graph generation.
    pub generation: IndexGeneration,
    /// Exact local source entity that emitted the relation.
    pub source: projectatlas_core::graph::GraphEntity,
    /// Registry-owned role of the source file, when the source is file-bearing.
    pub classification: Option<ContentClassification>,
    /// Complete typed logical relation.
    pub relation: LogicalRelation,
}

/// Exact shared external identity observed in at least two projects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FederatedRendezvous {
    /// Closed relation family shared by the evidence.
    pub relation: GraphRelationKind,
    /// Exact typed external namespace and identity; similar text is not enough.
    pub external: ExternalSelector,
    /// Deterministically ordered project-qualified evidence.
    pub evidence: Vec<FederatedRelationEvidence>,
}

/// Aggregate bounded work for one federated response.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FederatedRelationWork {
    /// Exact source-verification work across all roots.
    pub input: FederatedInputWork,
    /// Aggregate bytes of participating project database files.
    pub participating_database_bytes: u64,
    /// Maximum read snapshots held simultaneously.
    pub simultaneously_open_snapshots: u32,
    /// Relation rows decoded by cross-root rendezvous reads.
    pub rendezvous_database_rows: u64,
    /// Serialized-equivalent bytes decoded by cross-root rendezvous reads.
    pub rendezvous_database_bytes: u64,
    /// Exact typed rendezvous evidence retained in memory.
    pub rendezvous_relations: u32,
    /// Peak aggregate primary and federation-owned intermediate bytes.
    pub intermediate_bytes: u64,
    /// Milliseconds spent finishing and dropping every participant snapshot.
    pub close_ms: u64,
    /// Total verification, query, and close elapsed milliseconds.
    pub elapsed_ms: u64,
    /// Exact bytes emitted by the selected adapter envelope.
    pub rendered_output_bytes: u64,
}

/// Federated detailed-relation response on the existing relation route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FederatedDetailedRelationReport {
    /// Ordered validated participants.
    pub participants: Vec<FederatedParticipant>,
    /// Worktree alias of the first-root result when aliases selected federation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_worktree: Option<String>,
    /// Ordered aliases required to resume the primary continuation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_worktrees: Option<Vec<String>>,
    /// Existing detailed relation result anchored in the first root.
    pub primary: DetailedRelationReport,
    /// Exact typed cross-root external rendezvous evidence.
    pub rendezvous: Vec<FederatedRendezvous>,
    /// Whether primary traversal or cross-root rendezvous work was truncated.
    pub truncated: bool,
    /// Stable aggregate limits reached by either stage.
    pub reached_limits: Vec<GraphLimitKind>,
    /// Exact aggregate work for this response.
    pub work: FederatedRelationWork,
}

/// Federated closed-analysis response on the existing relation route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FederatedAnalysisReport {
    /// Ordered validated participants.
    pub participants: Vec<FederatedParticipant>,
    /// Worktree alias of the first-root result when aliases selected federation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_worktree: Option<String>,
    /// Ordered aliases required to resume the primary continuation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_worktrees: Option<Vec<String>>,
    /// Existing analysis result anchored in the first root.
    pub primary: RelationAnalysisReport,
    /// Exact typed cross-root external rendezvous evidence.
    pub rendezvous: Vec<FederatedRendezvous>,
    /// Whether primary analysis or cross-root rendezvous work was truncated.
    pub truncated: bool,
    /// Stable aggregate limits reached by either stage.
    pub reached_limits: Vec<GraphLimitKind>,
    /// Exact aggregate work for this response.
    pub work: FederatedRelationWork,
}

/// Unrendered federated detailed response that fits the exact adapter envelope.
pub struct FederatedDetailedRelationDraft {
    /// Existing first-root relation draft.
    primary: DetailedRelationPageDraft,
    /// Immutable participant and rendezvous state.
    context: FederatedContext,
}

impl FederatedDetailedRelationDraft {
    /// Fit the largest primary-row prefix to the exact federated adapter envelope.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when rendering fails, cancellation fires, a
    /// cursor cannot be wrapped, or the empty federated envelope is oversized.
    pub fn fit_output<F, E>(
        self,
        control: Option<&IndexWorkControl>,
        mut encode: F,
    ) -> Result<(FederatedDetailedRelationReport, String), E>
    where
        F: FnMut(&FederatedDetailedRelationReport) -> Result<String, E>,
        E: From<ServiceError>,
    {
        let context = self.context;
        let (primary, encoded) = self.primary.fit_output(control, |primary| {
            let report = context.detailed_report(primary.clone()).map_err(E::from)?;
            encode(&report)
        })?;
        let report = context.detailed_report(primary).map_err(E::from)?;
        Ok((report, encoded))
    }
}

/// Unrendered federated analysis response that fits the exact adapter envelope.
pub struct FederatedAnalysisDraft {
    /// Existing first-root analysis draft.
    primary: RelationAnalysisDraft,
    /// Immutable participant and rendezvous state.
    context: FederatedContext,
}

impl FederatedAnalysisDraft {
    /// Fit the largest analysis-finding prefix to the exact federated envelope.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when rendering fails, cancellation fires, a
    /// cursor cannot be wrapped, or the empty federated envelope is oversized.
    pub fn fit_output<F, E, O>(self, mut encode: F) -> Result<(FederatedAnalysisReport, O), E>
    where
        F: FnMut(&FederatedAnalysisReport, &IndexWorkControl) -> Result<O, E>,
        E: From<ServiceError>,
        O: AsRef<[u8]>,
    {
        let context = self.context;
        let control = self.primary.control().clone();
        let (primary, encoded) = self.primary.fit_output(|primary, control| {
            let report = context
                .analysis_report(primary.clone(), Some(control))
                .map_err(E::from)?;
            encode(&report, control)
        })?;
        check_control(Some(&control)).map_err(E::from)?;
        let report = context
            .analysis_report(primary, Some(&control))
            .map_err(E::from)?;
        Ok((report, encoded))
    }
}

/// Shared immutable state used while fitting either adapter envelope.
#[derive(Clone)]
struct FederatedContext {
    /// Ordered public participant identities.
    participants: Vec<FederatedParticipant>,
    /// Complete participant identity bound into continuations.
    cursor_participants: Vec<FederatedCursorParticipant>,
    /// Call-only typed external identity groups.
    rendezvous: Vec<FederatedRendezvous>,
    /// Aggregate limits reached while reading rendezvous evidence.
    rendezvous_limits: Vec<GraphLimitKind>,
    /// Work completed before adapter output fitting.
    base_work: FederatedRelationWork,
    /// Temporary exact identity-set bytes retained during rendezvous discovery.
    rendezvous_identity_bytes: u64,
    /// Existing aggregate relation and output budget.
    budget: DetailedRelationBudget,
}

impl FederatedContext {
    /// Compose one detailed envelope and bind its inner continuation.
    fn detailed_report(
        &self,
        mut primary: DetailedRelationReport,
    ) -> ServiceResult<FederatedDetailedRelationReport> {
        primary.continuation = wrap_continuation(
            primary.continuation.as_deref(),
            &self.cursor_participants,
            self.budget,
        )?;
        let mut reached_limits = primary.reached_limits.clone();
        for limit in &self.rendezvous_limits {
            push_limit(&mut reached_limits, *limit);
        }
        let mut work = self.base_work.clone();
        work.rendered_output_bytes = primary.work.rendered_output_bytes;
        work.intermediate_bytes = federated_intermediate_bytes(
            primary
                .work
                .intermediate_bytes
                .saturating_add(self.rendezvous_identity_bytes),
            &self.participants,
            &self.rendezvous,
            primary.continuation.as_deref(),
            self.budget,
        )?;
        Ok(FederatedDetailedRelationReport {
            participants: self.participants.clone(),
            primary_worktree: self
                .participants
                .first()
                .and_then(|participant| participant.worktree.clone()),
            continuation_worktrees: federation_continuation_worktrees(
                &self.participants,
                primary.continuation.is_some(),
            ),
            truncated: primary.truncated || !self.rendezvous_limits.is_empty(),
            primary,
            rendezvous: self.rendezvous.clone(),
            reached_limits,
            work,
        })
    }

    /// Compose one analysis envelope and bind its inner continuation.
    fn analysis_report(
        &self,
        mut primary: RelationAnalysisReport,
        control: Option<&IndexWorkControl>,
    ) -> ServiceResult<FederatedAnalysisReport> {
        check_control(control)?;
        primary.continuation = wrap_continuation(
            primary.continuation.as_deref(),
            &self.cursor_participants,
            self.budget,
        )?;
        let mut reached_limits = primary.reached_limits.clone();
        for limit in &self.rendezvous_limits {
            check_control(control)?;
            push_limit(&mut reached_limits, *limit);
        }
        let mut work = self.base_work.clone();
        work.rendered_output_bytes = primary.work.rendered_output_bytes;
        work.intermediate_bytes = federated_intermediate_bytes(
            primary
                .work
                .peak_intermediate_bytes
                .saturating_add(self.rendezvous_identity_bytes),
            &self.participants,
            &self.rendezvous,
            primary.continuation.as_deref(),
            self.budget,
        )?;
        check_control(control)?;
        Ok(FederatedAnalysisReport {
            participants: self.participants.clone(),
            primary_worktree: self
                .participants
                .first()
                .and_then(|participant| participant.worktree.clone()),
            continuation_worktrees: federation_continuation_worktrees(
                &self.participants,
                primary.continuation.is_some(),
            ),
            truncated: primary.truncated || !self.rendezvous_limits.is_empty(),
            primary,
            rendezvous: self.rendezvous.clone(),
            reached_limits,
            work,
        })
    }
}

/// Return the complete ordered aliases only when a continuation and all labels exist.
fn federation_continuation_worktrees(
    participants: &[FederatedParticipant],
    has_continuation: bool,
) -> Option<Vec<String>> {
    has_continuation
        .then(|| {
            participants
                .iter()
                .map(|participant| participant.worktree.clone())
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
}

/// Versioned outer continuation for an ordered participant set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FederatedCursor {
    /// Outer wire version.
    version: u16,
    /// Ordered roots, projects, and generations captured by the first page.
    participants: Vec<FederatedCursorParticipant>,
    /// Existing relation or analysis continuation.
    inner: String,
}

/// Machine-path-free participant identity retained by a continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FederatedCursorParticipant {
    /// Durable project identity.
    project: ProjectInstanceId,
    /// Domain-separated exact root digest.
    root_digest: [u8; 32],
    /// Complete graph generation.
    generation: IndexGeneration,
    /// Authored-purpose generation affecting hydration.
    authored_purpose_revision: u64,
}

/// Participant metadata captured while every snapshot remains open.
struct CapturedFederation {
    /// Public project-qualified participant state.
    participants: Vec<FederatedParticipant>,
    /// Continuation-bound participant state.
    cursor_participants: Vec<FederatedCursorParticipant>,
    /// Aggregate exact-source verification work.
    input_work: FederatedInputWork,
    /// Aggregate participating database bytes.
    database_bytes: u64,
}

/// Bounded cross-root rendezvous rows and measured work.
struct RendezvousLoad {
    /// Retained exact typed groups.
    rows: Vec<FederatedRendezvous>,
    /// Database rows inspected.
    database_rows: u64,
    /// Serialized-equivalent decoded database bytes.
    database_bytes: u64,
    /// Retained logical relation evidence rows.
    relation_rows: u32,
    /// Aggregate ceilings reached during the read.
    reached_limits: Vec<GraphLimitKind>,
}

/// Closed participant details used for post-read generation validation.
struct ClosedParticipant {
    /// Exact database path.
    database_path: PathBuf,
    /// Canonical expected root.
    root: PathBuf,
    /// Binding captured before snapshot close.
    cursor: FederatedCursorParticipant,
}

/// Load one federated detailed relation page through already verified stores.
///
/// # Errors
///
/// Returns an error before any response is produced when any participant,
/// cursor, budget, query, cleanup, or post-read generation validation fails.
pub fn load_federated_detailed_relations(
    stores: Vec<FederatedStore>,
    query: &DetailedRelationQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<FederatedDetailedRelationDraft> {
    let started = Instant::now();
    let budget = query.budget;
    let captured = capture_federation(&stores, control)?;
    let mut primary_query = query.clone();
    primary_query.cursor = decode_continuation(
        query.cursor.as_deref(),
        &captured.cursor_participants,
        budget,
    )?;
    let operation: ServiceResult<(DetailedRelationPageDraft, RendezvousLoad, u64)> = (|| {
        check_control(control)?;
        let primary = super::relations::load_detailed_relation_page(
            stores[0].store(),
            &primary_query,
            control,
        )?;
        let candidate = primary.report_for_prefix(primary.candidate_rows())?;
        let rendezvous_identities = external_relation_identities(&candidate);
        let rendezvous_identity_bytes = serialized_equivalent_bytes(&rendezvous_identities)?;
        let primary_edges = u64::from(candidate.work.inspected_edges);
        let primary_rows = u64::from(candidate.work.database_returned_rows);
        let remaining_edges = u64::from(budget.edges()).saturating_sub(primary_edges);
        let rendezvous = load_rendezvous(
            &stores,
            query,
            &rendezvous_identities,
            remaining_edges,
            aggregate_row_limit(budget).saturating_sub(primary_rows),
            budget
                .intermediate_bytes()
                .saturating_sub(candidate.work.intermediate_bytes)
                .saturating_sub(rendezvous_identity_bytes),
            started,
            control,
        )?;
        Ok((primary, rendezvous, rendezvous_identity_bytes))
    })();
    let (closed, close_ms, close_error) = close_participants(stores);
    let (primary, rendezvous, rendezvous_identity_bytes) = operation?;
    if let Some(error) = close_error {
        return Err(error);
    }
    revalidate_participants(&closed, control)?;
    let base_work = base_work(&captured, &rendezvous, close_ms, elapsed_ms(started))?;
    Ok(FederatedDetailedRelationDraft {
        primary,
        context: FederatedContext {
            participants: captured.participants,
            cursor_participants: captured.cursor_participants,
            rendezvous: rendezvous.rows,
            rendezvous_limits: rendezvous.reached_limits,
            base_work,
            rendezvous_identity_bytes,
            budget,
        },
    })
}

/// Load one federated closed analysis through already verified stores.
///
/// # Errors
///
/// Returns an error before any response is produced when any participant,
/// cursor, budget, query, cleanup, or post-read generation validation fails.
pub fn load_federated_relation_analysis(
    stores: Vec<FederatedStore>,
    query: &RelationAnalysisQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<FederatedAnalysisDraft> {
    let started = Instant::now();
    let budget = query.relations.budget;
    let captured = capture_federation(&stores, control)?;
    let mut primary_query = query.clone();
    primary_query.relations.cursor = decode_continuation(
        query.relations.cursor.as_deref(),
        &captured.cursor_participants,
        budget,
    )?;
    let operation: ServiceResult<(RelationAnalysisDraft, RendezvousLoad)> = (|| {
        check_control(control)?;
        let mut primary = super::analysis::load_relation_analysis_for_federation(
            stores[0].store(),
            &primary_query,
            control,
        )?;
        let rendezvous_identities = primary.take_external_relation_identities();
        let candidate = primary.candidate_report();
        let primary_edges = u64::from(candidate.work.relations.inspected_edges)
            .saturating_add(u64::from(candidate.work.closure_inspected_edges));
        let primary_rows = u64::from(candidate.work.relations.database_returned_rows)
            .saturating_add(u64::from(candidate.work.closure_inspected_edges));
        let remaining_edges = u64::from(budget.edges()).saturating_sub(primary_edges);
        let rendezvous = load_rendezvous(
            &stores,
            &query.relations,
            &rendezvous_identities,
            remaining_edges,
            aggregate_row_limit(budget).saturating_sub(primary_rows),
            budget
                .intermediate_bytes()
                .saturating_sub(candidate.work.peak_intermediate_bytes),
            started,
            control,
        )?;
        Ok((primary, rendezvous))
    })();
    let (closed, close_ms, close_error) = close_participants(stores);
    let (primary, rendezvous) = operation?;
    if let Some(error) = close_error {
        return Err(error);
    }
    revalidate_participants(&closed, control)?;
    let base_work = base_work(&captured, &rendezvous, close_ms, elapsed_ms(started))?;
    Ok(FederatedAnalysisDraft {
        primary,
        context: FederatedContext {
            participants: captured.participants,
            cursor_participants: captured.cursor_participants,
            rendezvous: rendezvous.rows,
            rendezvous_limits: rendezvous.reached_limits,
            base_work,
            rendezvous_identity_bytes: 0,
            budget,
        },
    })
}

/// Validate all open participants and capture their ordered snapshot identity.
fn capture_federation(
    stores: &[FederatedStore],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<CapturedFederation> {
    validate_federated_root_count(stores.len())?;
    let mut roots = BTreeSet::new();
    let mut projects = Vec::new();
    let mut participants = Vec::with_capacity(stores.len());
    let mut cursor_participants = Vec::with_capacity(stores.len());
    let mut input_work = FederatedInputWork::default();
    let mut database_bytes = 0_u64;
    for (order, participant) in stores.iter().enumerate() {
        check_control(control)?;
        if !participant.store.is_read_only() || !participant.store.has_active_read_snapshot() {
            return Err(ServiceError::InvalidInput(
                "federated participant lost its read-only snapshot".to_string(),
            ));
        }
        let binding = selected_project_binding(&participant.store)?;
        let root_digest = federated_root_digest(&binding.project_root_identity)?;
        if !roots.insert(root_digest) || projects.contains(&binding.project_instance_id) {
            return Err(ServiceError::InvalidInput(
                "federated roots or project identities must be unique".to_string(),
            ));
        }
        projects.push(binding.project_instance_id);
        let generation = participant
            .store
            .repository_graph_generation()?
            .ok_or_else(|| {
                ServiceError::InvalidInput(
                    "federated root has no complete repository graph generation".to_string(),
                )
            })?;
        let authored_purpose_revision = participant.store.authored_purpose_revision()?;
        let order = u32::try_from(order).map_err(|_overflow| {
            ServiceError::InvalidInput("federated root order overflowed".to_string())
        })?;
        participants.push(FederatedParticipant {
            order,
            worktree: participant.worktree.clone(),
            project: binding.project_instance_id,
            generation,
            authored_purpose_revision,
        });
        cursor_participants.push(FederatedCursorParticipant {
            project: binding.project_instance_id,
            root_digest,
            generation,
            authored_purpose_revision,
        });
        input_work = input_work.checked_add(participant.input_work)?;
        database_bytes = checked_sum(
            database_bytes,
            participant.database_bytes,
            "participating database bytes",
        )?;
        if input_work.filesystem_bytes > MAX_FEDERATED_INPUT_BYTES {
            return Err(ServiceError::InvalidInput(format!(
                "federated source verification exceeds {MAX_FEDERATED_INPUT_BYTES} bytes"
            )));
        }
        if database_bytes > MAX_FEDERATED_DATABASE_BYTES {
            return Err(ServiceError::InvalidInput(format!(
                "participating databases exceed {MAX_FEDERATED_DATABASE_BYTES} bytes"
            )));
        }
    }
    Ok(CapturedFederation {
        participants,
        cursor_participants,
        input_work,
        database_bytes,
    })
}

/// Load exact typed external identities shared by at least two projects.
fn load_rendezvous(
    stores: &[FederatedStore],
    query: &DetailedRelationQuery,
    identities: &BTreeSet<ExternalRelationIdentity>,
    mut remaining_edges: u64,
    mut remaining_rows: u64,
    mut remaining_intermediate_bytes: u64,
    started: Instant,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<RendezvousLoad> {
    if identities.is_empty()
        || !matches!(
            query.resolution,
            super::relations::RelationResolutionFilter::Any
                | super::relations::RelationResolutionFilter::External
        )
    {
        return Ok(RendezvousLoad {
            rows: Vec::new(),
            database_rows: 0,
            database_bytes: 0,
            relation_rows: 0,
            reached_limits: Vec::new(),
        });
    }
    let deadline = started
        .checked_add(Duration::from_millis(query.budget.deadline_ms()))
        .unwrap_or(started);
    let request_control = relation_request_control(control, deadline);
    let control = Some(&request_control);
    let families = query.relation.map_or_else(
        || FEDERATED_RENDEZVOUS_RELATIONS.to_vec(),
        |relation| vec![relation],
    );
    let mut groups: BTreeMap<(String, String, String), FederatedRendezvous> = BTreeMap::new();
    let mut database_rows = 0_u64;
    let mut database_bytes = 0_u64;
    let mut reached_limits = Vec::new();
    let mut queries_left = stores.len().saturating_mul(families.len());
    'queries: for family in families {
        for participant in stores {
            if elapsed_ms(started) >= query.budget.deadline_ms() {
                push_limit(&mut reached_limits, GraphLimitKind::Deadline);
                break 'queries;
            }
            check_control(control)?;
            if remaining_edges == 0 {
                push_limit(&mut reached_limits, GraphLimitKind::Edges);
                break 'queries;
            }
            if remaining_rows == 0 {
                push_limit(&mut reached_limits, GraphLimitKind::Rows);
                break 'queries;
            }
            if remaining_intermediate_bytes == 0 {
                push_limit(&mut reached_limits, GraphLimitKind::IntermediateBytes);
                break 'queries;
            }
            let fair_share = remaining_edges
                .div_ceil(u64::try_from(queries_left).unwrap_or(u64::MAX))
                .max(1)
                .min(u64::from(projectatlas_core::graph::GraphLimits::MAX_ROWS));
            let limit = u32::try_from(fair_share).map_err(|_overflow| {
                ServiceError::InvalidInput("federated relation page limit overflowed".to_string())
            })?;
            let page = participant
                .store
                .repository_graph_classified_relation_family_rows(
                    family,
                    query.content_selection,
                    limit,
                    control,
                )?;
            queries_left = queries_left.saturating_sub(1);
            let decoded_rows = u64::try_from(page.rows.len()).map_err(|_overflow| {
                ServiceError::InvalidInput("federated database row count overflowed".to_string())
            })?;
            let inspected_rows = decoded_rows.saturating_add(u64::from(page.truncated));
            database_rows = checked_sum(database_rows, inspected_rows, "federated database rows")?;
            if inspected_rows > remaining_rows {
                return Err(ServiceError::InvalidInput(
                    "federated database row budget was exhausted".to_string(),
                ));
            }
            remaining_rows = remaining_rows.saturating_sub(inspected_rows);
            remaining_edges = remaining_edges.saturating_sub(decoded_rows);
            if page.truncated {
                push_limit(&mut reached_limits, GraphLimitKind::Edges);
            }
            for row in page.rows {
                let encoded_bytes = serialized_equivalent_bytes(&(
                    &row.detail.source,
                    &row.detail.relation,
                    row.source_classification,
                ))?;
                if encoded_bytes > remaining_intermediate_bytes {
                    push_limit(&mut reached_limits, GraphLimitKind::IntermediateBytes);
                    break 'queries;
                }
                remaining_intermediate_bytes =
                    remaining_intermediate_bytes.saturating_sub(encoded_bytes);
                database_bytes =
                    checked_sum(database_bytes, encoded_bytes, "federated decoded bytes")?;
                if !super::relations::relation_matches(&row.detail.relation, query) {
                    continue;
                }
                let RelationResolution::External { external, .. } =
                    row.detail.relation.resolution()
                else {
                    continue;
                };
                let key = (
                    family.as_str().to_string(),
                    external.system.as_str().to_string(),
                    external.identity.as_str().to_string(),
                );
                if !identities.contains(&key) {
                    continue;
                }
                let project = row.detail.relation.key().project();
                let generation = row.detail.relation.generation();
                let group = groups.entry(key).or_insert_with(|| FederatedRendezvous {
                    relation: family,
                    external: external.clone(),
                    evidence: Vec::new(),
                });
                group.evidence.push(FederatedRelationEvidence {
                    worktree: participant.worktree.clone(),
                    project,
                    generation,
                    source: row.detail.source,
                    classification: row.source_classification,
                    relation: row.detail.relation,
                });
            }
        }
    }
    let rows = groups
        .into_values()
        .filter(|group| {
            let mut projects = Vec::new();
            for evidence in &group.evidence {
                if !projects.contains(&evidence.project) {
                    projects.push(evidence.project);
                }
            }
            projects.len() >= 2
        })
        .collect::<Vec<_>>();
    let relation_rows = u32::try_from(rows.iter().map(|row| row.evidence.len()).sum::<usize>())
        .map_err(|_overflow| {
            ServiceError::InvalidInput("federated rendezvous count overflowed".to_string())
        })?;
    Ok(RendezvousLoad {
        rows,
        database_rows,
        database_bytes,
        relation_rows,
        reached_limits,
    })
}

/// Finish and drop every captured participant snapshot.
fn close_participants(
    stores: Vec<FederatedStore>,
) -> (Vec<ClosedParticipant>, u64, Option<ServiceError>) {
    let started = Instant::now();
    let mut closed = Vec::with_capacity(stores.len());
    let mut first_error = None;
    for participant in stores {
        let binding = participant.store.captured_project_binding();
        let generation = participant.store.repository_graph_generation();
        let purpose_revision = participant.store.authored_purpose_revision();
        if let Err(error) = participant.store.finish_index_read_snapshot()
            && first_error.is_none()
        {
            first_error = Some(ServiceError::Db(error));
        }
        drop(participant.store);
        match (binding, generation, purpose_revision) {
            (Ok(binding), Ok(Some(generation)), Ok(authored_purpose_revision)) => {
                match federated_root_digest(&binding.project_root_identity) {
                    Ok(root_digest) => closed.push(ClosedParticipant {
                        database_path: participant.database_path,
                        root: participant.root,
                        cursor: FederatedCursorParticipant {
                            project: binding.project_instance_id,
                            root_digest,
                            generation,
                            authored_purpose_revision,
                        },
                    }),
                    Err(error) if first_error.is_none() => first_error = Some(error),
                    Err(_) => {}
                }
            }
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error))
                if first_error.is_none() =>
            {
                first_error = Some(ServiceError::Db(error));
            }
            _ => {
                if first_error.is_none() {
                    first_error = Some(ServiceError::InvalidInput(
                        "federated participant lost its graph generation before close".to_string(),
                    ));
                }
            }
        }
    }
    let elapsed = elapsed_ms(started);
    if elapsed > MAX_FEDERATED_CLOSE_MS && first_error.is_none() {
        first_error = Some(ServiceError::InvalidInput(format!(
            "federated snapshots took {elapsed} ms to close; limit is {MAX_FEDERATED_CLOSE_MS} ms"
        )));
    }
    (closed, elapsed, first_error)
}

/// Reopen each database sequentially and reject a changed participant.
fn revalidate_participants(
    participants: &[ClosedParticipant],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<()> {
    for participant in participants {
        check_control(control)?;
        let store =
            AtlasStore::open_read_only_for_project(&participant.database_path, &participant.root)?;
        let binding = selected_project_binding(&store)?;
        let generation = store.repository_graph_generation()?.ok_or_else(|| {
            ServiceError::InvalidInput(
                "federated root lost its complete graph generation".to_string(),
            )
        })?;
        let purpose_revision = store.authored_purpose_revision()?;
        let current = FederatedCursorParticipant {
            project: binding.project_instance_id,
            root_digest: federated_root_digest(&binding.project_root_identity)?,
            generation,
            authored_purpose_revision: purpose_revision,
        };
        store.finish_index_read_snapshot()?;
        drop(store);
        if current.project != participant.cursor.project
            || current.root_digest != participant.cursor.root_digest
        {
            return Err(ServiceError::RelationCursorStale {
                field: "federated project binding",
            });
        }
        if current.generation != participant.cursor.generation {
            return Err(ServiceError::RelationCursorStale {
                field: "federated graph generation",
            });
        }
        if current.authored_purpose_revision != participant.cursor.authored_purpose_revision {
            return Err(ServiceError::RelationCursorStale {
                field: "federated authored-purpose revision",
            });
        }
    }
    Ok(())
}

/// Compose work measured before adapter-specific output fitting.
fn base_work(
    captured: &CapturedFederation,
    rendezvous: &RendezvousLoad,
    close_ms: u64,
    elapsed_ms: u64,
) -> ServiceResult<FederatedRelationWork> {
    Ok(FederatedRelationWork {
        input: captured.input_work,
        participating_database_bytes: captured.database_bytes,
        simultaneously_open_snapshots: u32::try_from(captured.participants.len()).map_err(
            |_overflow| {
                ServiceError::InvalidInput("federated open-snapshot count overflowed".to_string())
            },
        )?,
        rendezvous_database_rows: rendezvous.database_rows,
        rendezvous_database_bytes: rendezvous.database_bytes,
        rendezvous_relations: rendezvous.relation_rows,
        intermediate_bytes: 0,
        close_ms,
        elapsed_ms: captured.input_work.elapsed_ms.saturating_add(elapsed_ms),
        rendered_output_bytes: 0,
    })
}

/// Decode and validate an outer continuation against every current root.
fn decode_continuation(
    encoded: Option<&str>,
    expected: &[FederatedCursorParticipant],
    budget: DetailedRelationBudget,
) -> ServiceResult<Option<String>> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.is_empty()
        || encoded.len() > FEDERATED_CURSOR_MAX_BYTES
        || encoded.len() > budget.intermediate_bytes() as usize
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "federated cursor length is empty or above the product ceiling",
        });
    }
    let cursor: FederatedCursor =
        serde_json::from_str(encoded).map_err(|_source| ServiceError::RelationCursorInvalid {
            reason: "federated cursor JSON is malformed or contains unknown fields",
        })?;
    if cursor.version != FEDERATED_CURSOR_VERSION {
        return Err(ServiceError::RelationCursorStale {
            field: "federated cursor version",
        });
    }
    if cursor.participants.len() != expected.len() {
        return Err(ServiceError::RelationCursorStale {
            field: "federated roots",
        });
    }
    for (actual, expected) in cursor.participants.iter().zip(expected) {
        if actual.project != expected.project || actual.root_digest != expected.root_digest {
            return Err(ServiceError::RelationCursorStale {
                field: "federated roots",
            });
        }
        if actual.generation != expected.generation {
            return Err(ServiceError::RelationCursorStale {
                field: "federated graph generation",
            });
        }
        if actual.authored_purpose_revision != expected.authored_purpose_revision {
            return Err(ServiceError::RelationCursorStale {
                field: "federated authored-purpose revision",
            });
        }
    }
    if cursor.inner.is_empty() {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "federated cursor omitted its inner continuation",
        });
    }
    Ok(Some(cursor.inner))
}

/// Bind an existing inner continuation to the complete participant set.
fn wrap_continuation(
    inner: Option<&str>,
    participants: &[FederatedCursorParticipant],
    budget: DetailedRelationBudget,
) -> ServiceResult<Option<String>> {
    let Some(inner) = inner else {
        return Ok(None);
    };
    let encoded = serde_json::to_string(&FederatedCursor {
        version: FEDERATED_CURSOR_VERSION,
        participants: participants.to_vec(),
        inner: inner.to_string(),
    })?;
    if encoded.len() > FEDERATED_CURSOR_MAX_BYTES
        || encoded.len() > budget.intermediate_bytes() as usize
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "encoded federated cursor exceeds the intermediate-state ceiling",
        });
    }
    Ok(Some(encoded))
}

/// Charge retained federation state against the existing intermediate ceiling.
fn federated_intermediate_bytes(
    primary_bytes: u64,
    participants: &[FederatedParticipant],
    rendezvous: &[FederatedRendezvous],
    cursor: Option<&str>,
    budget: DetailedRelationBudget,
) -> ServiceResult<u64> {
    let cursor_bytes = u64::try_from(cursor.map_or(0, str::len)).map_err(|_overflow| {
        ServiceError::InvalidInput("federated cursor byte count overflowed".to_string())
    })?;
    let federation_bytes = checked_sum(
        serialized_equivalent_bytes(&(participants, rendezvous))?,
        cursor_bytes,
        "federated intermediate bytes",
    )?;
    let total = checked_sum(
        primary_bytes,
        federation_bytes,
        "federated intermediate bytes",
    )?;
    if total > budget.intermediate_bytes() {
        return Err(ServiceError::InvalidInput(
            "federated aggregate intermediate-byte budget was exhausted".to_string(),
        ));
    }
    Ok(total)
}

/// Derive the aggregate row-work ceiling from the existing typed budget.
fn aggregate_row_limit(budget: DetailedRelationBudget) -> u64 {
    u64::from(budget.nodes())
        .saturating_add(u64::from(budget.edges()))
        .saturating_add(u64::from(budget.occurrences_total()))
        .saturating_add(u64::from(budget.page_rows()))
}

/// Bind a root without returning its machine-local path.
fn federated_root_digest(root: &CanonicalProjectRoot) -> ServiceResult<[u8; 32]> {
    canonical_root_digest(FEDERATED_ROOT_DIGEST_DOMAIN, root)
}

/// Observe request cancellation at a repository-traversal boundary.
fn check_control(control: Option<&IndexWorkControl>) -> ServiceResult<()> {
    if let Some(control) = control {
        control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(DbError::from)?;
    }
    Ok(())
}

/// Retain one stable reached-limit value.
fn push_limit(limits: &mut Vec<GraphLimitKind>, limit: GraphLimitKind) {
    if !limits.contains(&limit) {
        limits.push(limit);
    }
}

/// Add two work counters without silent overflow.
fn checked_sum(left: u64, right: u64, context: &'static str) -> ServiceResult<u64> {
    left.checked_add(right)
        .ok_or_else(|| ServiceError::InvalidInput(format!("{context} overflowed")))
}

/// Return a saturating millisecond duration for public work reports.
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relations::{RelationAnchor, RelationDirection, RelationResolutionFilter};
    use projectatlas_core::graph::{
        Completeness, EntitySelector, GraphEntity, GraphIdentityText, GraphLimits,
        RepositoryFilePath,
    };
    use projectatlas_core::language::ContentSelection;
    use projectatlas_core::symbols::RelationKind;
    use projectatlas_core::{IndexCancellation, Node, NodeKind};
    use projectatlas_db::sqlite_progress_test_observer::{
        SqliteReadProgressEvent, observe_sqlite_read_progress,
    };
    use std::cell::Cell;
    use std::error::Error;
    use std::io;
    use std::path::Path;
    use std::rc::Rc;

    #[cfg(unix)]
    #[test]
    fn federation_root_digest_preserves_non_utf8_root_collisions() -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let native = temp
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'r', b'o', b'o', 0x80]));
        let replacement = temp.path().join("roo�");
        fs::create_dir(&native)?;
        fs::create_dir(&replacement)?;
        let native_root = CanonicalProjectRoot::from_path(&native)?;
        let replacement_root = CanonicalProjectRoot::from_path(&replacement)?;
        require(
            federated_root_digest(&native_root)? != federated_root_digest(&replacement_root)?,
            "federation identity collapsed non-UTF-8 and replacement roots",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn federation_recanonicalizes_case_only_root_and_rejects_case_sensitive_sibling()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let original_root = temp.path().join("FederatedCaseRoot");
        let staging_root = temp.path().join("FederatedCaseRootStaging");
        let renamed_root = temp.path().join("federatedcaseroot");
        let original_database = original_root.join("projectatlas.db");
        publish_fixture(
            &original_root,
            &original_database,
            IndexGeneration::new(1),
            1,
        )?;
        let duplicate_database = temp.path().join("federated-duplicate.db");
        publish_fixture(
            &original_root,
            &duplicate_database,
            IndexGeneration::new(1),
            1,
        )?;
        let captured_root = CanonicalProjectRoot::from_path(&original_root)?;
        fs::rename(&original_root, &staging_root)?;
        fs::rename(&staging_root, &renamed_root)?;

        let renamed_identity = CanonicalProjectRoot::from_path(&renamed_root)?;
        let renamed_database = renamed_root.join("projectatlas.db");
        let before_repair_store =
            AtlasStore::open_read_only_for_project(&renamed_database, &renamed_root)?;
        let before_repair = crate::relations::load_detailed_relations(
            &before_repair_store,
            &relation_query(
                None,
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            None,
        )?;
        before_repair_store.finish_index_read_snapshot()?;
        drop(before_repair_store);
        let captured_digest = federated_root_digest(&captured_root)?;
        require(
            captured_digest == federated_root_digest(&renamed_identity)?,
            "case-only root rename changed the service root digest",
        )?;
        let repair_store = AtlasStore::open_for_project(&renamed_database, &renamed_root)?;
        drop(repair_store);
        let after_repair_store =
            AtlasStore::open_read_only_for_project(&renamed_database, &renamed_root)?;
        let resumed = crate::relations::load_detailed_relations(
            &after_repair_store,
            &relation_query(
                Some(
                    before_repair
                        .continuation
                        .ok_or("case-only rename fixture omitted a relation continuation")?,
                ),
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            None,
        );
        after_repair_store.finish_index_read_snapshot()?;
        drop(after_repair_store);
        require(
            resumed.is_ok(),
            "case-only root rename invalidated a relation continuation",
        )?;

        let repaired_binding =
            AtlasStore::open_read_only(&renamed_database)?.captured_project_binding()?;
        let duplicate_binding =
            AtlasStore::open_read_only(&duplicate_database)?.captured_project_binding()?;
        require(
            repaired_binding.project_instance_id != duplicate_binding.project_instance_id,
            "duplicate federation fixture reused the repaired project identity",
        )?;
        require(
            repaired_binding.project_root_identity.encode()?
                != duplicate_binding.project_root_identity.encode()?,
            "duplicate federation fixture did not retain old and fresh root spellings",
        )?;

        let secondary_root = temp.path().join("federated-secondary");
        let secondary_database = secondary_root.join("projectatlas.db");
        publish_fixture(
            &secondary_root,
            &secondary_database,
            IndexGeneration::new(1),
            1,
        )?;
        let participants = open_participants(&[
            (renamed_root.clone(), renamed_database.clone()),
            (secondary_root, secondary_database),
        ])?;
        let draft = load_federated_detailed_relations(
            participants,
            &relation_query(
                None,
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            None,
        )?;
        let (report, _) = draft.fit_output(None, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })?;
        require(
            report.participants.len() == 2,
            "federated query rejected a case-only root rename",
        )?;

        let duplicate = load_federated_detailed_relations(
            open_participants(&[
                (renamed_root.clone(), renamed_database),
                (renamed_root, duplicate_database),
            ])?,
            &relation_query(
                None,
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            None,
        );
        require(
            matches!(
                duplicate,
                Err(ServiceError::InvalidInput(message))
                    if message == "federated roots or project identities must be unique"
            ),
            "federation admitted two databases for one case-renamed root",
        )?;

        let case_sensitive_parent = temp.path().join("federated-case-sensitive-parent");
        fs::create_dir(&case_sensitive_parent)?;
        let enabled = std::process::Command::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&case_sensitive_parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            return Ok(());
        }
        let stored_root = case_sensitive_parent.join("Repo");
        let selected_root = case_sensitive_parent.join("repo");
        let stored_database = stored_root.join("projectatlas.db");
        fs::create_dir(&stored_root)?;
        fs::create_dir(&selected_root)?;
        publish_fixture(&stored_root, &stored_database, IndexGeneration::new(1), 1)?;
        let store = AtlasStore::open_read_only(&stored_database)?;
        let result = FederatedStore::new(
            store,
            stored_database,
            selected_root,
            FederatedInputWork::default(),
        );
        require(
            matches!(result, Err(ServiceError::InvalidInput(message)) if message == "federated store does not match its explicit root"),
            "federated admission accepted a case-sensitive sibling",
        )?;
        Ok(())
    }

    #[test]
    fn federation_is_project_qualified_fresh_bounded_and_handle_free() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let mut participants = Vec::new();
        for index in 0..4 {
            let root = temp.path().join(format!("project-{index}"));
            let database = root.join("projectatlas.db");
            publish_fixture(&root, &database, IndexGeneration::new(1), 1)?;
            participants.push((root, database));
        }
        let before = participants
            .iter()
            .map(|(_, database)| fs::read(database))
            .collect::<Result<Vec<_>, _>>()?;
        let query = relation_query(
            None,
            RelationResolutionFilter::External,
            RelationDirection::Outbound,
        )?;
        let draft =
            load_federated_detailed_relations(open_participants(&participants)?, &query, None)?;
        let (report, encoded) = draft.fit_output(None, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })?;
        require(
            report.participants.len() == 4
                && report.work.simultaneously_open_snapshots == 4
                && report.rendezvous.len() == 2
                && report.rendezvous.iter().all(|row| row.evidence.len() == 4),
            "many-root rendezvous or snapshot accounting changed",
        )?;
        let projects = report
            .rendezvous
            .iter()
            .flat_map(|row| row.evidence.iter().map(|evidence| evidence.project))
            .collect::<BTreeSet<_>>();
        require(
            projects.len() == 4
                && report
                    .rendezvous
                    .iter()
                    .flat_map(|row| &row.evidence)
                    .all(|evidence| {
                        evidence.classification == Some(ContentClassification::Source)
                            && matches!(
                                evidence.source.selector(),
                                EntitySelector::File { path } if path.as_str() == "src/same.rs"
                            )
                    }),
            "same relative paths collapsed across project identities",
        )?;
        let classified_query = relation_query_with_selection(
            None,
            RelationResolutionFilter::External,
            RelationDirection::Outbound,
            ContentSelection::Source,
        )?;
        let classified = load_federated_detailed_relations(
            open_participants(&participants)?,
            &classified_query,
            None,
        )?;
        let (classified, _) = classified.fit_output(None, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })?;
        require(
            classified.rendezvous == report.rendezvous
                && classified.primary.content_selection == Some(ContentSelection::Source),
            "classified federation changed eligible rendezvous evidence or lost selection",
        )?;
        require(
            encoded.len() <= query.budget.output_bytes() as usize
                && report.work.intermediate_bytes <= query.budget.intermediate_bytes(),
            "federated output escaped its aggregate byte budgets",
        )?;
        let rendezvous_identities = external_relation_identities(&report.primary);
        let encoded_identity_bytes =
            u64::try_from(serde_json::to_vec(&rendezvous_identities)?.len())?;
        require(
            !rendezvous_identities.is_empty()
                && serialized_equivalent_bytes(&rendezvous_identities)? == encoded_identity_bytes,
            "streamed federation identity accounting diverged from exact JSON bytes",
        )?;
        let after = participants
            .iter()
            .map(|(_, database)| fs::read(database))
            .collect::<Result<Vec<_>, _>>()?;
        require(
            before == after,
            "read-only federation changed database bytes",
        )?;

        let mut analysis = load_federated_relation_analysis(
            open_participants(&participants)?,
            &RelationAnalysisQuery {
                relations: query,
                mode: crate::analysis::RelationAnalysisMode::Architecture,
                trace_target: None,
                vcs: None,
                include_communities: false,
                include_cycles: false,
                include_dead_code: false,
            },
            None,
        )?;
        require(
            analysis
                .primary
                .take_external_relation_identities()
                .is_empty(),
            "federated analysis retained temporary rendezvous identities through output fitting",
        )?;
        let (analysis, _) = analysis.fit_output(|report, _control| {
            serde_json::to_vec(report).map_err(ServiceError::from)
        })?;
        require(
            analysis.rendezvous.len() == 2
                && analysis
                    .rendezvous
                    .iter()
                    .flat_map(|row| &row.evidence)
                    .all(|evidence| {
                        matches!(
                            evidence.source.selector(),
                            EntitySelector::File { path } if path.as_str() == "src/same.rs"
                        )
                    }),
            "analysis rendezvous escaped the primary anchored traversal",
        )?;

        let inbound_query = relation_query(
            None,
            RelationResolutionFilter::External,
            RelationDirection::Inbound,
        )?;
        let inbound = load_federated_detailed_relations(
            open_participants(&participants)?,
            &inbound_query,
            None,
        )?;
        let (inbound, _) = inbound.fit_output(None, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })?;
        require(
            inbound.rendezvous.is_empty() && inbound.work.rendezvous_database_rows == 0,
            "inbound traversal scanned or returned unrelated external rendezvous",
        )?;
        let inbound_analysis = load_federated_relation_analysis(
            open_participants(&participants)?,
            &RelationAnalysisQuery {
                relations: inbound_query,
                mode: crate::analysis::RelationAnalysisMode::Architecture,
                trace_target: None,
                vcs: None,
                include_communities: false,
                include_cycles: false,
                include_dead_code: false,
            },
            None,
        )?;
        let (inbound_analysis, _) = inbound_analysis.fit_output(|report, _control| {
            serde_json::to_vec(report).map_err(ServiceError::from)
        })?;
        require(
            inbound_analysis.rendezvous.is_empty()
                && inbound_analysis.work.rendezvous_database_rows == 0,
            "inbound analysis scanned or returned unrelated external rendezvous",
        )?;

        let resolved = load_federated_detailed_relations(
            open_participants(&participants)?,
            &relation_query(
                None,
                RelationResolutionFilter::Resolved,
                RelationDirection::Outbound,
            )?,
            None,
        )?;
        let (resolved, _) = resolved.fit_output(None, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })?;
        require(
            resolved.rendezvous.is_empty(),
            "federation ignored the requested resolution filter",
        )?;

        let cursor = report
            .primary
            .continuation
            .ok_or("first federated page omitted its continuation")?;
        publish_fixture(
            &participants[3].0,
            &participants[3].1,
            IndexGeneration::new(2),
            1,
        )?;
        let stale = load_federated_detailed_relations(
            open_participants(&participants)?,
            &relation_query(
                Some(cursor),
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            None,
        );
        require(
            matches!(
                stale,
                Err(ServiceError::RelationCursorStale {
                    field: "federated graph generation"
                })
            ),
            "a changed secondary generation did not stale the outer cursor",
        )?;

        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        control.cancel();
        let canceled = load_federated_detailed_relations(
            open_participants(&participants)?,
            &relation_query(
                None,
                RelationResolutionFilter::External,
                RelationDirection::Outbound,
            )?,
            Some(&control),
        );
        require(canceled.is_err(), "pre-canceled federation returned rows")?;
        for (index, (_, database)) in participants.iter().enumerate() {
            let moved = database.with_extension(format!("closed-{index}"));
            fs::rename(database, &moved)?;
            fs::rename(moved, database)?;
        }
        Ok(())
    }

    #[test]
    fn federation_deadline_interrupts_active_rendezvous_and_releases_snapshots()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let primary_root = temp.path().join("primary");
        let primary_database = primary_root.join("projectatlas.db");
        publish_fixture(&primary_root, &primary_database, IndexGeneration::new(1), 1)?;
        let secondary_root = temp.path().join("secondary");
        let secondary_database = secondary_root.join("projectatlas.db");
        publish_fixture(
            &secondary_root,
            &secondary_database,
            IndexGeneration::new(1),
            usize::try_from(GraphLimits::MAX_ROWS)?,
        )?;
        let participants = vec![
            (primary_root, primary_database),
            (secondary_root, secondary_database),
        ];
        let mut query = relation_query(
            None,
            RelationResolutionFilter::External,
            RelationDirection::Outbound,
        )?;
        query.budget = query.budget.with_aggregate_limits(
            Some(GraphLimits::MAX_ROWS),
            None,
            None,
            None,
            None,
            None,
        )?;
        let control = IndexWorkControl::with_deadline(
            IndexCancellation::new(),
            Instant::now() + Duration::from_secs(1),
        );
        let family_query_active = Rc::new(Cell::new(false));
        let family_query_entered_live = Rc::new(Cell::new(false));
        let callback_entered_live = Rc::new(Cell::new(false));
        let callback_interrupted = Rc::new(Cell::new(false));
        let stores = open_participants(&participants)?;
        let deadline = observe_sqlite_read_progress(
            {
                let observer_control = control.clone();
                let family_query_active = Rc::clone(&family_query_active);
                let family_query_entered_live = Rc::clone(&family_query_entered_live);
                let callback_entered_live = Rc::clone(&callback_entered_live);
                let callback_interrupted = Rc::clone(&callback_interrupted);
                move |event| match event {
                    SqliteReadProgressEvent::RepositoryRelationFamilyQueryEntered => {
                        family_query_active.set(true);
                        if observer_control
                            .check(IndexWorkStage::RepositoryTraversal)
                            .is_ok()
                        {
                            family_query_entered_live.set(true);
                        }
                    }
                    SqliteReadProgressEvent::RepositoryRelationFamilyQueryExited => {
                        family_query_active.set(false);
                    }
                    SqliteReadProgressEvent::CallbackEntered { stage }
                        if family_query_active.get() && !callback_entered_live.get() =>
                    {
                        if observer_control.check(stage).is_ok() {
                            callback_entered_live.set(true);
                            while observer_control.check(stage).is_ok() {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                    }
                    SqliteReadProgressEvent::CallbackEvaluated {
                        interrupted: true, ..
                    } if family_query_active.get() && callback_entered_live.get() => {
                        callback_interrupted.set(true);
                    }
                    _ => {}
                }
            },
            || load_federated_detailed_relations(stores, &query, Some(&control)),
        );
        require(
            matches!(
                deadline,
                Err(ServiceError::Db(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                )))
            ),
            "active rendezvous query was not interrupted with its typed deadline",
        )?;
        require(
            family_query_entered_live.get()
                && callback_entered_live.get()
                && callback_interrupted.get()
                && !family_query_active.get(),
            "rendezvous deadline did not enter a live family query and interrupt it through SQLite",
        )?;
        for (index, (_, database)) in participants.iter().enumerate() {
            let moved = database.with_extension(format!("deadline-closed-{index}"));
            fs::rename(database, &moved)?;
            fs::rename(moved, database)?;
        }
        Ok(())
    }

    /// Publish three anchored imports plus the requested unrelated same-family imports.
    fn publish_fixture(
        root: &Path,
        database: &Path,
        generation: IndexGeneration,
        unrelated_relations: usize,
    ) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/same.rs"), "pub fn same() {}\n")?;
        fs::write(root.join("src/unrelated.rs"), "pub fn unrelated() {}\n")?;
        let mut store = AtlasStore::open_for_project(database, root)?;
        let project = store
            .project_instance_id()?
            .ok_or("federation fixture project identity is missing")?;
        let source = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/same.rs"))?,
            },
            generation,
        )?;
        let unrelated_source = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/unrelated.rs"))?,
            },
            generation,
        )?;
        let mut entities = vec![source.clone(), unrelated_source.clone()];
        let mut relations = Vec::new();
        for identity in ["package/a", "package/b", "package/c"] {
            let external = GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("registry.example")?,
                        identity: GraphIdentityText::new(identity)?,
                    },
                },
                generation,
            )?;
            relations.push(LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::external(&external)?,
                projectatlas_core::graph::ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            entities.push(external);
        }
        for index in 0..unrelated_relations {
            let identity = if unrelated_relations == 1 {
                "package/unrelated".to_string()
            } else {
                format!("package/unrelated/{index:05}")
            };
            let unrelated_external = GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("registry.example")?,
                        identity: GraphIdentityText::new(identity)?,
                    },
                },
                generation,
            )?;
            relations.push(LogicalRelation::new(
                &unrelated_source,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::external(&unrelated_external)?,
                projectatlas_core::graph::ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            entities.push(unrelated_external);
        }
        let mut publication = store.begin_index_publication("federation-fixture")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            fixture_folder_node("src"),
            fixture_file_node("src/same.rs"),
            fixture_file_node("src/unrelated.rs"),
        ])?;
        publication.upsert_file_content_classification_batch(&[
            projectatlas_db::FileContentClassification {
                path: "src/same.rs".to_string(),
                classification: ContentClassification::Source,
            },
            projectatlas_db::FileContentClassification {
                path: "src/unrelated.rs".to_string(),
                classification: ContentClassification::Documentation,
            },
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(project, &entities, &relations, &[], &[])?;
        publication.complete()?;
        Ok(())
    }

    /// Open every fixture through the production root-bound read-only adapter.
    fn open_participants(
        participants: &[(PathBuf, PathBuf)],
    ) -> Result<Vec<FederatedStore>, Box<dyn Error>> {
        participants
            .iter()
            .map(|(root, database)| {
                Ok(FederatedStore::new(
                    AtlasStore::open_read_only_for_project(database, root)?,
                    database.clone(),
                    root.clone(),
                    FederatedInputWork::default(),
                )?)
            })
            .collect()
    }

    /// Build one bounded anchored relation request.
    fn relation_query(
        cursor: Option<String>,
        resolution: RelationResolutionFilter,
        direction: RelationDirection,
    ) -> Result<DetailedRelationQuery, Box<dyn Error>> {
        relation_query_with_selection(
            cursor,
            resolution,
            direction,
            ContentSelection::UnspecifiedLegacy,
        )
    }

    /// Build one bounded anchored relation request with explicit content selection.
    fn relation_query_with_selection(
        cursor: Option<String>,
        resolution: RelationResolutionFilter,
        direction: RelationDirection,
        content_selection: ContentSelection,
    ) -> Result<DetailedRelationQuery, Box<dyn Error>> {
        Ok(DetailedRelationQuery {
            anchor: RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/same.rs"))?,
            },
            direction,
            relation: Some(GraphRelationKind::Legacy(RelationKind::Imports)),
            minimum_confidence: projectatlas_core::graph::ConfidenceClass::Low,
            resolution,
            include_occurrences: false,
            budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                2,
                1,
                1,
                512 * 1_024,
            )?)
            .with_aggregate_limits(Some(100), None, None, None, None, None)?,
            cursor,
            content_selection,
        })
    }

    /// Build one indexed source-file row.
    fn fixture_file_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: Some("src".to_string()),
            extension: Some("rs".to_string()),
            language: Some("Rust".to_string()),
            size_bytes: Some(17),
            mtime_ns: Some(1),
            content_hash: Some("fixture-hash".to_string()),
        }
    }

    /// Build one indexed source-folder row.
    fn fixture_folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: Some(".".to_string()),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: Some(1),
            content_hash: None,
        }
    }

    /// Return one readable test failure.
    fn require(condition: bool, message: &str) -> Result<(), io::Error> {
        condition
            .then_some(())
            .ok_or_else(|| io::Error::other(message))
    }
}
