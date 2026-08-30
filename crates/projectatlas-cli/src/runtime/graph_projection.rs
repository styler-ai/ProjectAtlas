//! Normalize parser-owned symbol facts into one generation-bound repository graph.

use super::{
    CliError, INDEX_FRESHNESS_SAMPLE_LIMIT, IndexReadStatus, IndexRefreshReason,
    IndexRefreshRequired, IndexRefreshScope, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
    IndexWorkStage, MAX_SYMBOL_FILE_BYTES, Node, NodeKind, SourceReadFailure, SymbolBuildStage,
    SymbolProjectionChange, lossless_project_root_display, normalize_native_path_display,
    read_source_bytes_controlled, source_changed_during_derivation,
};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector,
    ExtendedRelationKind, ExternalSelector, GraphContractError, GraphEntity, GraphIdentityField,
    GraphIdentityRejection, GraphIdentityRejectionReason, GraphIdentityText, GraphLimitKind,
    GraphLimits, GraphRelationKind, LogicalRelation, LogicalRelationKey, MAX_GRAPH_IDENTITY_BYTES,
    PackageSelector, ProjectInstanceId, QUALIFIED_SYMBOL_SCOPE_PREFIX, RelationDependencyKey,
    RelationOccurrence, RelationResolution, RepositoryFilePath, RepositoryNodePath,
    ResolutionKeyDomain, SourceSpan, SymbolSelector,
};
use projectatlas_core::language::{SemanticProviderOwner, SymbolParserOwner, language_capability};
use projectatlas_core::symbols::{
    ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_db::{
    AtlasStore, IndexPublicationGuard, RepositoryAffectedSourceFootprint,
    RepositoryResolutionCandidate,
};
use projectatlas_fs::RootScanPolicy;
#[cfg(test)]
use projectatlas_fs::ScanOptions;
use projectatlas_symbols::{
    ConfiguredModuleResolution, MAX_RESOLUTION_KEYS_PER_FACT, MarkdownFactCompleteness,
    MarkdownFactLimit, MarkdownFacts, ResolutionKeyProjection, ResolutionProjectionContext,
    ResolutionProjectionError, ResolutionProjectionFact, derive_resolution_keys_with_context,
    extract_markdown_facts_controlled, parse_import_references,
};
use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, btree_map::Entry};
use std::fs::{self, File, OpenOptions};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempDirBuilder, TempDir};

/// Maximum canonical keys or distinct source paths admitted by one incremental closure.
const MAX_INCREMENTAL_RESOLUTION_ITEMS: u32 = GraphLimits::MAX_ROWS;
/// Maximum simultaneous parser/entity/key bytes before rows spill to typed `SQLite` staging.
const MAX_IN_MEMORY_GRAPH_WORK_BYTES: u64 = 512 * 1_024 * 1_024;
/// Maximum aggregate normalized graph rows admitted by one incremental closure.
const MAX_INCREMENTAL_GRAPH_ROWS: u64 = GraphLimits::MAX_ROWS as u64;
/// Maximum conservative graph bytes admitted before requesting a complete refresh.
const MAX_INCREMENTAL_GRAPH_BYTES: u64 = MAX_IN_MEMORY_GRAPH_WORK_BYTES;
/// Maximum persisted key bindings retained by one complete in-memory graph projection.
const MAX_GRAPH_KEY_BINDINGS: u64 = 8_000_000;
/// Conservative fixed bytes counted for each staged graph or binding row.
const STAGED_GRAPH_ROW_BYTES: u64 = 128;
/// Conservative bytes for one generated document relation and its retained indexes.
///
/// This covers the relation, occurrence, dependency, unresolved-reason map entry,
/// final reason tuple, and duplicate-validation digest; candidate text is added
/// separately from the already-counted Markdown fact bytes.
const DOCUMENT_PROJECTION_ROW_BYTES: u64 = STAGED_GRAPH_ROW_BYTES * 8;
/// Maximum graph-map rows processed between cooperative cancellation checks.
const GRAPH_WORK_CHECK_INTERVAL: usize = 256;
/// Borrowed entity references inserted per disposable staging call.
const GRAPH_STAGE_ENTITY_BATCH_SIZE: usize = 1_024;
/// Maximum normalized graph rows retained between disposable staging writes.
const GRAPH_STAGE_ROW_BATCH_SIZE: usize = 8_192;
/// Direct child prefix owned by disposable graph staging.
const GRAPH_STAGE_DIRECTORY_PREFIX: &str = "graph-stage-";
/// Stable cross-process lease protecting active staging and restart cleanup.
const GRAPH_STAGE_LEASE_FILE_NAME: &str = "repository-graph-stage.lock";
/// Typed disposable database inside every owned staging directory.
const GRAPH_STAGE_DATABASE_FILE_NAME: &str = "projectatlas.db";
/// Internal invariant failure for a staging owner already entering teardown.
const GRAPH_STAGE_OWNER_UNAVAILABLE: &str = "repository graph staging owner is unavailable";
/// Maximum persisted symbol-graph paths reconstructed between work checks.
const PERSISTED_GRAPH_PATHS_PER_CHUNK: usize = 256;
/// Stable fallback for a parser relation that omitted a usable display target.
const UNKNOWN_REFERENCE: &str = "unknown-reference";
/// Stable package manager for currently extracted Cargo manifest ownership.
const CARGO_PACKAGE_MANAGER: &str = "cargo";
/// Stable external namespace for the Rust standard-library distribution.
const RUST_TOOLCHAIN_SYSTEM: &str = "rust-toolchain";
/// Stable external namespace for explicitly qualified Node.js built-ins.
const NODE_SYSTEM: &str = "node";
/// Honest coverage diagnostic for fallback or structural relation extraction.
const PARTIAL_COVERAGE_REASON: &str = "parser does not prove complete relationship coverage";
/// External namespace for content-free configuration identities.
const CONFIGURATION_SYSTEM: &str = "configuration";
/// External namespace for static environment-variable identities.
const ENVIRONMENT_SYSTEM: &str = "environment-variable";
/// External namespace for content-free deployment platform identities.
const DEPLOYMENT_SYSTEM: &str = "deployment-platform";
/// Canonical provider owner for exact repository document targets.
const DOCUMENT_PATH_PROVIDER: &str = "projectatlas-document";
/// Canonical resolver language for repository-relative file and heading identities.
const DOCUMENT_PATH_LANGUAGE: &str = "repository-path";
/// Canonical resolver language for case-fold collision invalidation only.
const DOCUMENT_CASEFOLD_LANGUAGE: &str = "repository-path-casefold";
/// Honest content-free coverage diagnostic for bounded Markdown facts.
const DOCUMENT_PARTIAL_COVERAGE_REASON: &str =
    "markdown fact extraction reached a declared limit or unsupported structure";
/// Maximum typed identity rejection details retained for one publication.
const MAX_GRAPH_IDENTITY_REJECTIONS: usize = GraphLimits::MAX_ROWS as usize;
/// Stable namespace for invalid symbol/package parser facts.
const SYMBOL_FACT_INDEX_NAMESPACE: u64 = 1_u64 << 56;
/// Stable namespace for invalid source-relation parser facts.
const RELATION_FACT_INDEX_NAMESPACE: u64 = 2_u64 << 56;
/// Stable namespace for invalid derived-relation parser facts.
const DERIVED_RELATION_FACT_INDEX_NAMESPACE: u64 = 3_u64 << 56;
/// Stable namespace for invalid Markdown selector parser facts.
const MARKDOWN_FACT_INDEX_NAMESPACE: u64 = 4_u64 << 56;

/// Build a deterministic internal identity without retaining parser text.
fn parser_fact_index(namespace: u64, index: usize) -> u64 {
    match u64::try_from(index) {
        Ok(index) => namespace.saturating_add(index),
        Err(_) => u64::MAX,
    }
}

/// Relation observation paired with each symbol in one parser graph.
struct PairedImportRelations {
    /// Relation index for each paired import symbol.
    by_symbol: Vec<Option<usize>>,
    /// Test-only proof that pairing scans each bounded input row once.
    #[cfg(test)]
    work_items: usize,
}

/// Pair import symbols and relations once by source line and occurrence.
///
/// Tree-sitter emits an import declaration as both a symbol and an import
/// relation. Pairing by source line and occurrence keeps those observations
/// tied to one parser fact while retaining distinct same-line imports.
fn paired_import_relations(
    graph: &SymbolGraph,
    control: &IndexWorkControl,
) -> Result<PairedImportRelations, CliError> {
    let mut relations_by_line = HashMap::<usize, (Vec<usize>, usize)>::new();
    #[cfg(test)]
    let mut work_items = 0_usize;
    for (relation_index, relation) in graph.relations.iter().enumerate() {
        check_graph_work(control, relation_index)?;
        #[cfg(test)]
        {
            work_items = work_items.saturating_add(1);
        }
        if relation.kind == RelationKind::Imports {
            relations_by_line
                .entry(relation.line)
                .or_default()
                .0
                .push(relation_index);
        }
    }
    let mut by_symbol = vec![None; graph.symbols.len()];
    for (symbol_index, symbol) in graph.symbols.iter().enumerate() {
        check_graph_work(control, symbol_index)?;
        #[cfg(test)]
        {
            work_items = work_items.saturating_add(1);
        }
        if symbol.kind != SymbolKind::Import {
            continue;
        }
        let Some((relation_indices, next_ordinal)) = relations_by_line.get_mut(&symbol.line_start)
        else {
            continue;
        };
        by_symbol[symbol_index] = relation_indices.get(*next_ordinal).copied();
        *next_ordinal = (*next_ordinal).saturating_add(1);
    }
    Ok(PairedImportRelations {
        by_symbol,
        #[cfg(test)]
        work_items,
    })
}

/// Reuse the relation ordinal for its paired import symbol observation.
fn symbol_parser_fact_index(symbol_index: usize, paired_relation_index: Option<usize>) -> u64 {
    paired_relation_index.map_or_else(
        || parser_fact_index(SYMBOL_FACT_INDEX_NAMESPACE, symbol_index),
        |relation_index| parser_fact_index(RELATION_FACT_INDEX_NAMESPACE, relation_index),
    )
}

/// Bounded source span attached to one rejected parser identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IdentitySpan {
    /// One-based first line containing the parser fact.
    start_line: usize,
    /// Zero-based first column when the parser provides one.
    start_column: usize,
    /// One-based last line containing the parser fact.
    end_line: usize,
    /// Zero-based exclusive end column when the parser provides one.
    end_column: usize,
}

/// Use one source span for paired import symbol and relation observations.
fn symbol_identity_span(
    graph: &SymbolGraph,
    symbol_index: usize,
    paired_relation_index: Option<usize>,
) -> IdentitySpan {
    let Some(symbol) = graph.symbols.get(symbol_index) else {
        return IdentitySpan {
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
        };
    };
    let start_line = symbol.line_start.max(1);
    let end_line = symbol.line_end.max(symbol.line_start).max(1);
    if let Some(relation_index) = paired_relation_index
        && let Some(relation) = graph.relations.get(relation_index)
    {
        let line = relation.line.max(1);
        return IdentitySpan {
            start_line: line,
            start_column: 0,
            end_line: line,
            end_column: 0,
        };
    }
    IdentitySpan {
        start_line,
        start_column: 0,
        end_line,
        end_column: 0,
    }
}

/// Internal identity for one observed parser fact, independent of retained
/// detail rows. The detail vector is deliberately capped, but omission
/// counts must still deduplicate repeated projections after that cap.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IdentityFactKey {
    /// Exact bounded source span of the parser fact.
    span: IdentitySpan,
    /// Stable local discriminator for the parser strategy.
    parser: u8,
    /// Admission owner for distinguishing parser identity from its derived
    /// resolution-key outcome when both share one parser fact ordinal.
    owner: u8,
    /// Internal parser-fact ordinal.
    fact_index: u64,
}

/// Exact membership key for one retained typed identity rejection detail.
///
/// The ordered key is kept beside the deterministic detail vector so replay
/// checks do not scan every previously retained row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IdentityRejectionKey {
    /// Repository-relative source path containing the rejected fact.
    path: RepositoryNodePath,
    /// Exact bounded source span of the rejected fact.
    span: IdentitySpan,
    /// Parser strategy that produced the rejected fact.
    parser: u8,
    /// Identity field that failed admission.
    field: GraphIdentityField,
    /// Stable rejection category.
    reason: GraphIdentityRejectionReason,
    /// Internal parser-fact ordinal.
    fact_index: u64,
}

impl From<&GraphIdentityRejection> for IdentityRejectionKey {
    fn from(rejection: &GraphIdentityRejection) -> Self {
        Self {
            path: rejection.path.clone(),
            span: IdentitySpan {
                start_line: rejection.span.start_line() as usize,
                start_column: rejection.span.start_column() as usize,
                end_line: rejection.span.end_line() as usize,
                end_column: rejection.span.end_column() as usize,
            },
            parser: parser_fact_kind(rejection.parser),
            field: rejection.field,
            reason: rejection.reason,
            fact_index: rejection.fact_index,
        }
    }
}

/// Map parser strategy to a compact local fact-key discriminator.
fn parser_fact_kind(parser: ParserKind) -> u8 {
    match parser {
        ParserKind::TreeSitter => 0,
        ParserKind::Manifest => 1,
        ParserKind::Structural => 2,
        ParserKind::Fallback => 3,
    }
}

/// Map one rejection field to the admission owner that re-derives it.
fn identity_fact_owner(fields: &[(GraphIdentityField, GraphIdentityRejectionReason)]) -> u8 {
    u8::from(
        fields
            .iter()
            .any(|(field, _reason)| *field == GraphIdentityField::ResolutionKey),
    )
}

/// Return whether a persisted fact is replaced by the current derivation.
fn is_rederived_identity_fact(fact: &IdentityFactKey) -> bool {
    fact.owner == 1
        || matches!(
            fact.fact_index & !((1_u64 << 56) - 1),
            DERIVED_RELATION_FACT_INDEX_NAMESPACE | MARKDOWN_FACT_INDEX_NAMESPACE
        )
}

/// Conservative bytes for one observed fact and its path/count map entries.
fn identity_fact_retained_bytes(
    path: &str,
    new_observed_path: bool,
    new_count_path: bool,
) -> Result<u64, CliError> {
    let path_bytes = u64::try_from(path.len()).map_err(|error| {
        CliError::InvalidInput(format!(
            "identity rejection path length overflowed: {error}"
        ))
    })?;
    let path_entries = u64::from(u8::from(new_observed_path)) + u64::from(u8::from(new_count_path));
    let path_bytes = path_bytes
        .checked_mul(path_entries)
        .and_then(|bytes| bytes.checked_add(STAGED_GRAPH_ROW_BYTES.checked_mul(path_entries)?))
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))?;
    path_bytes
        .checked_add(STAGED_GRAPH_ROW_BYTES)
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))
}

/// Conservative bytes for a count-only path retained after detail rows cap.
fn identity_count_path_retained_bytes(path: &str) -> Result<u64, CliError> {
    let path_bytes = u64::try_from(path.len()).map_err(|error| {
        CliError::InvalidInput(format!(
            "identity rejection path length overflowed: {error}"
        ))
    })?;
    path_bytes
        .checked_add(STAGED_GRAPH_ROW_BYTES)
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))
}

/// Count one persisted identity fact set and its owning path entry.
fn identity_fact_set_retained_bytes(
    path: &str,
    facts: &BTreeSet<IdentityFactKey>,
) -> Result<u64, CliError> {
    let path_bytes = u64::try_from(path.len()).map_err(|error| {
        CliError::InvalidInput(format!(
            "identity rejection path length overflowed: {error}"
        ))
    })?;
    let fact_bytes = STAGED_GRAPH_ROW_BYTES
        .checked_mul(u64::try_from(facts.len()).map_err(|error| {
            CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
        })?)
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))?;
    path_bytes
        .checked_add(STAGED_GRAPH_ROW_BYTES)
        .and_then(|bytes| bytes.checked_add(fact_bytes))
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))
}

/// Count one observed-fact map path and its retained facts.
fn identity_observed_path_retained_bytes(path: &str, fact_count: usize) -> Result<u64, CliError> {
    let path_bytes = u64::try_from(path.len()).map_err(|error| {
        CliError::InvalidInput(format!(
            "identity rejection path length overflowed: {error}"
        ))
    })?;
    let fact_bytes = STAGED_GRAPH_ROW_BYTES
        .checked_mul(u64::try_from(fact_count).map_err(|error| {
            CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
        })?)
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))?;
    path_bytes
        .checked_add(STAGED_GRAPH_ROW_BYTES)
        .and_then(|bytes| bytes.checked_add(fact_bytes))
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))
}

/// Count the bounded map/set entries retained for observed identity facts.
fn identity_maps_retained_bytes(
    observed_facts: &BTreeMap<String, BTreeSet<IdentityFactKey>>,
    rejected_facts_by_path: &BTreeMap<String, u64>,
) -> Result<u64, CliError> {
    let mut retained = 0_u64;
    for (path, facts) in observed_facts {
        retained = retained
            .checked_add(identity_observed_path_retained_bytes(path, facts.len())?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
    }
    for path in rejected_facts_by_path.keys() {
        retained = retained
            .checked_add(identity_count_path_retained_bytes(path)?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
    }
    Ok(retained)
}

/// Count one retained rejection-membership key and its owned path text.
fn identity_rejection_key_retained_bytes(path: &str) -> Result<u64, CliError> {
    let path_bytes = u64::try_from(path.len()).map_err(|error| {
        CliError::InvalidInput(format!(
            "identity rejection path length overflowed: {error}"
        ))
    })?;
    path_bytes
        .checked_add(STAGED_GRAPH_ROW_BYTES)
        .ok_or_else(|| CliError::InvalidInput("identity rejection bytes overflowed".to_string()))
}

/// Count the bounded keyed membership state for retained rejection details.
fn identity_rejection_keys_retained_bytes(
    keys: &BTreeSet<IdentityRejectionKey>,
) -> Result<u64, CliError> {
    keys.iter().try_fold(0_u64, |retained, key| {
        retained
            .checked_add(identity_rejection_key_retained_bytes(key.path.as_str())?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })
    })
}

/// Count the path markers retained for publication-time detail evictions.
fn identity_rejection_drop_paths_retained_bytes(paths: &BTreeSet<String>) -> Result<u64, CliError> {
    paths.iter().try_fold(0_u64, |retained, path| {
        retained
            .checked_add(identity_count_path_retained_bytes(path)?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })
    })
}

/// Return the exact incremental identity-state total owned by one report.
fn identity_admission_retained_bytes(report: &GraphIdentityAdmission) -> u64 {
    report.observed_fact_bytes
}

/// Return the typed graph-work limit reached before retaining identity state.
fn identity_fact_budget_failure_for_limit(limit: u64, retained_bytes: u64) -> CliError {
    IndexWorkFailure::resource_limit(
        IndexWorkStage::SymbolParsing,
        IndexWorkResource::OutputBytes,
        limit,
        retained_bytes,
    )
    .into()
}

/// Return the production graph-work limit reached before retaining identity state.
fn identity_fact_budget_failure(retained_bytes: u64) -> CliError {
    identity_fact_budget_failure_for_limit(MAX_IN_MEMORY_GRAPH_WORK_BYTES, retained_bytes)
}

/// Check complete retained identity state against an injected or production limit.
fn checked_identity_admission_budget(
    report: &GraphIdentityAdmission,
    control: &IndexWorkControl,
    limit: u64,
) -> Result<u64, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let retained_bytes = identity_admission_retained_bytes(report);
    if retained_bytes > limit {
        return Err(identity_fact_budget_failure_for_limit(
            limit,
            retained_bytes,
        ));
    }
    Ok(retained_bytes)
}

/// Bounded admission report shared by entity, relation, and key projection.
#[derive(Clone, Debug, Default)]
pub(super) struct GraphIdentityAdmission {
    /// Whether the parser-output graphs have passed the shared source boundary.
    source_admitted: bool,
    /// Retained typed details, capped independently from the count.
    rejections: Vec<GraphIdentityRejection>,
    /// Exact membership for the retained typed detail vector.
    rejection_keys: BTreeSet<IdentityRejectionKey>,
    /// Number of parser facts rejected per source path.
    rejected_facts_by_path: BTreeMap<String, u64>,
    /// Every observed rejected parser fact, retained independently of detail
    /// rows so the global detail ceiling cannot make replays overcount.
    observed_facts: BTreeMap<String, BTreeSet<IdentityFactKey>>,
    /// Exact bytes retained by every identity map, detail key, and
    /// reconciliation entry, charged to the graph-work budget incrementally.
    observed_fact_bytes: u64,
    /// Persisted typed fact identities retained separately from current
    /// observations so removed outcomes can be subtracted exactly.
    reused_rejection_facts: BTreeMap<String, BTreeSet<IdentityFactKey>>,
    /// Original parser relation ordinal for each admitted relation after
    /// source-identity filtering. This remains private because it is only
    /// needed to keep derived rejection facts tied to parser observations.
    relation_fact_indices: BTreeMap<String, Vec<usize>>,
    /// Resolution keys derived once at admission and carried into projection.
    resolution_projections: BTreeMap<String, ResolutionKeyProjection>,
    /// Omission counts retained from the current generation for safely reused
    /// graphs and reconciled with the current pass after it regenerates any
    /// key, Markdown, or derived details it owns.
    reused_rejection_counts: BTreeMap<String, u64>,
    /// Parser-baseline omission retained for reused structural/fallback
    /// graphs. This stays separate from identity omissions because parser
    /// coverage counts only relations when no identity fact was rejected.
    reused_parser_rejection_counts: BTreeMap<String, u64>,
    /// Number of persisted typed facts retained for each reused path. This is
    /// the baseline for exact post-rederivation count reconciliation.
    reused_rejection_detail_counts: BTreeMap<String, u64>,
    /// Reused paths whose persisted detail ceiling leaves old identities
    /// unknowable. These paths require a complete source reparse before a
    /// replacement publication can be exact.
    reused_rejection_details_incomplete: BTreeSet<String>,
    /// Paths where a distinct typed rejection detail was evicted by the
    /// publication-wide detail ceiling.
    rejection_details_dropped_by_path: BTreeSet<String>,
    /// Test-only proof of one derivation per source path and generation.
    #[cfg(test)]
    resolution_derivations: BTreeMap<(String, IndexGeneration), usize>,
    /// Test-only count of source rows inspected while pairing import facts.
    #[cfg(test)]
    paired_import_pairing_work: usize,
}

impl GraphIdentityAdmission {
    /// Record one parser fact and every failed identity field without retaining raw input.
    fn record(
        &mut self,
        path: &str,
        span: IdentitySpan,
        parser: ParserKind,
        fact_index: u64,
        fields: &[(GraphIdentityField, GraphIdentityRejectionReason)],
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::SymbolParsing)?;
        let path = RepositoryNodePath::new(Path::new(path)).map_err(invalid_graph_contract)?;
        let fact_span = span;
        let span = SourceSpan::new(
            u32::try_from(span.start_line).map_err(|error| {
                CliError::InvalidInput(format!("identity rejection start line overflowed: {error}"))
            })?,
            u32::try_from(span.start_column).map_err(|error| {
                CliError::InvalidInput(format!(
                    "identity rejection start column overflowed: {error}"
                ))
            })?,
            u32::try_from(span.end_line).map_err(|error| {
                CliError::InvalidInput(format!("identity rejection end line overflowed: {error}"))
            })?,
            u32::try_from(span.end_column).map_err(|error| {
                CliError::InvalidInput(format!("identity rejection end column overflowed: {error}"))
            })?,
        )
        .map_err(invalid_graph_contract)?;
        let fact_key = IdentityFactKey {
            span: fact_span,
            parser: parser_fact_kind(parser),
            owner: identity_fact_owner(fields),
            fact_index,
        };
        let path_key = path.as_str().to_owned();
        let observed_path_is_new = !self.observed_facts.contains_key(&path_key);
        let count_path_is_new = !self.rejected_facts_by_path.contains_key(&path_key);
        let fact_is_new = self
            .observed_facts
            .get(&path_key)
            .is_none_or(|facts| !facts.contains(&fact_key));
        let mut new_rejection_keys = Vec::new();
        let mut dropped_rejection_detail = false;
        for (field, reason) in fields {
            let key = IdentityRejectionKey {
                path: path.clone(),
                span: fact_span,
                parser: parser_fact_kind(parser),
                field: *field,
                reason: *reason,
                fact_index,
            };
            if !self.rejection_keys.contains(&key)
                && !new_rejection_keys.iter().any(|existing| existing == &key)
            {
                if self.rejections.len() + new_rejection_keys.len() >= MAX_GRAPH_IDENTITY_REJECTIONS
                {
                    dropped_rejection_detail = true;
                } else {
                    new_rejection_keys.push(key);
                }
            }
        }
        let dropped_path_is_new = dropped_rejection_detail
            && !self
                .rejection_details_dropped_by_path
                .contains(path.as_str());
        let mut additional_bytes = if fact_is_new {
            identity_fact_retained_bytes(path.as_str(), observed_path_is_new, count_path_is_new)?
        } else {
            0
        };
        for key in &new_rejection_keys {
            additional_bytes = additional_bytes
                .checked_add(identity_rejection_key_retained_bytes(key.path.as_str())?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        if dropped_path_is_new {
            additional_bytes = additional_bytes
                .checked_add(identity_count_path_retained_bytes(path.as_str())?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        self.reserve_identity_bytes(additional_bytes, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
        if dropped_rejection_detail {
            self.rejection_details_dropped_by_path
                .insert(path.as_str().to_owned());
        }
        if fact_is_new {
            self.observed_facts
                .entry(path_key.clone())
                .or_default()
                .insert(fact_key);
            let count = self.rejected_facts_by_path.entry(path_key).or_default();
            *count = count.checked_add(1).ok_or_else(|| {
                CliError::InvalidInput("identity rejection count overflowed".to_string())
            })?;
        }
        for key in new_rejection_keys {
            let field = key.field;
            let reason = key.reason;
            self.rejection_keys.insert(key);
            self.rejections.push(GraphIdentityRejection {
                path: path.clone(),
                span,
                parser,
                field,
                reason,
                fact_index,
            });
        }
        Ok(())
    }

    /// Reserve one newly owned identity-state entry before retaining it.
    fn reserve_identity_bytes(
        &mut self,
        additional_bytes: u64,
        control: &IndexWorkControl,
        limit: u64,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::SymbolParsing)?;
        let retained_bytes = self
            .observed_fact_bytes
            .checked_add(additional_bytes)
            .ok_or_else(|| {
                identity_fact_budget_failure_for_limit(limit, self.observed_fact_bytes)
            })?;
        if retained_bytes > limit {
            return Err(identity_fact_budget_failure_for_limit(
                limit,
                retained_bytes,
            ));
        }
        self.observed_fact_bytes = retained_bytes;
        Ok(())
    }

    /// Adjust the cached identity-state total when a merge replaces entries.
    fn adjust_identity_bytes(&mut self, before: u64, after: u64) -> Result<(), CliError> {
        if after >= before {
            self.observed_fact_bytes = self
                .observed_fact_bytes
                .checked_add(after - before)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        } else {
            self.observed_fact_bytes = self
                .observed_fact_bytes
                .checked_sub(before - after)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes underflowed".to_string())
                })?;
        }
        Ok(())
    }

    /// Merge one incoming observed-fact/count path without recounting the
    /// existing report.
    fn merge_observed_identity_path(
        &mut self,
        path: &str,
        incoming_facts: Option<&BTreeSet<IdentityFactKey>>,
        incoming_total: u64,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::SymbolParsing)?;
        let existing_total = self.rejected_facts_by_path.get(path).copied().unwrap_or(0);
        let existing_observed = self.observed_facts.get(path).map_or(0, BTreeSet::len);
        let existing_unobserved = existing_total
            .checked_sub(u64::try_from(existing_observed).map_err(|error| {
                CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
            })?)
            .ok_or_else(|| {
                CliError::InvalidInput(
                    "observed identity facts exceeded rejection count".to_string(),
                )
            })?;
        let incoming_observed = incoming_facts.map_or(0, BTreeSet::len);
        let incoming_unobserved = incoming_total
            .checked_sub(u64::try_from(incoming_observed).map_err(|error| {
                CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
            })?)
            .ok_or_else(|| {
                CliError::InvalidInput(
                    "observed identity facts exceeded rejection count".to_string(),
                )
            })?;
        let new_observed = incoming_facts.map_or(0, |facts| {
            facts
                .iter()
                .filter(|fact| {
                    self.observed_facts
                        .get(path)
                        .is_none_or(|existing| !existing.contains(*fact))
                })
                .count()
        });
        let observed = existing_observed
            .checked_add(new_observed)
            .ok_or_else(|| CliError::InvalidInput("identity fact count overflowed".to_string()))?;
        let total = u64::try_from(observed)
            .map_err(|error| {
                CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
            })?
            .checked_add(existing_unobserved)
            .and_then(|count| count.checked_add(incoming_unobserved))
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection count overflowed".to_string())
            })?;

        let mut before_bytes = 0_u64;
        if self.observed_facts.contains_key(path) {
            before_bytes = before_bytes
                .checked_add(identity_observed_path_retained_bytes(
                    path,
                    existing_observed,
                )?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        if self.rejected_facts_by_path.contains_key(path) {
            before_bytes = before_bytes
                .checked_add(identity_count_path_retained_bytes(path)?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        let mut after_bytes = 0_u64;
        if observed > 0 {
            after_bytes = after_bytes
                .checked_add(identity_observed_path_retained_bytes(path, observed)?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        if total > 0 {
            after_bytes = after_bytes
                .checked_add(identity_count_path_retained_bytes(path)?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        }
        self.adjust_identity_bytes(before_bytes, after_bytes)?;

        if observed == 0 {
            self.observed_facts.remove(path);
        } else if let Some(facts) = incoming_facts
            && !facts.is_empty()
        {
            self.observed_facts
                .entry(path.to_string())
                .or_default()
                .extend(facts.iter().cloned());
        }
        if total == 0 {
            self.rejected_facts_by_path.remove(path);
            self.observed_facts.remove(path);
        } else {
            self.rejected_facts_by_path.insert(path.to_string(), total);
        }
        Ok(())
    }

    /// Reserve a per-path reconciliation entry before retaining its path key.
    fn reserve_reused_path_bytes(
        &mut self,
        path: &str,
        already_retained: bool,
        control: &IndexWorkControl,
        limit: u64,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::SymbolParsing)?;
        if !already_retained {
            self.reserve_identity_bytes(identity_count_path_retained_bytes(path)?, control, limit)?;
        }
        Ok(())
    }

    /// Add rejected-fact count that exceeded the bounded typed-detail rows.
    fn record_rejected_fact_count(
        &mut self,
        path: &str,
        count: usize,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        if count == 0 {
            control.check(IndexWorkStage::SymbolParsing)?;
            return Ok(());
        }
        control.check(IndexWorkStage::SymbolParsing)?;
        let path = RepositoryNodePath::new(Path::new(path)).map_err(invalid_graph_contract)?;
        let count = u64::try_from(count).map_err(|error| {
            CliError::InvalidInput(format!("identity rejection count overflowed: {error}"))
        })?;
        let path = path.as_str().to_owned();
        if !self.rejected_facts_by_path.contains_key(&path) {
            self.reserve_reused_path_bytes(&path, false, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
        }
        let entry = self.rejected_facts_by_path.entry(path).or_default();
        *entry = entry.checked_add(count).ok_or_else(|| {
            CliError::InvalidInput("identity rejection count overflowed".to_string())
        })?;
        Ok(())
    }

    /// Merge one report while retaining the global detail bound.
    fn merge(&mut self, other: Self, control: &IndexWorkControl) -> Result<(), CliError> {
        control.check(IndexWorkStage::SymbolParsing)?;
        let peak_retained_bytes = self
            .observed_fact_bytes
            .checked_add(other.observed_fact_bytes)
            .ok_or_else(|| identity_fact_budget_failure(self.observed_fact_bytes))?;
        if peak_retained_bytes > MAX_IN_MEMORY_GRAPH_WORK_BYTES {
            return Err(identity_fact_budget_failure(peak_retained_bytes));
        }
        #[cfg(test)]
        {
            self.paired_import_pairing_work = self
                .paired_import_pairing_work
                .checked_add(other.paired_import_pairing_work)
                .ok_or_else(|| {
                    CliError::InvalidInput("paired import work count overflowed".to_string())
                })?;
        }
        self.source_admitted |= other.source_admitted;
        if other
            .resolution_projections
            .keys()
            .any(|path| self.resolution_projections.contains_key(path))
        {
            return Err(CliError::InvalidInput(
                "duplicate resolution projection admission".to_string(),
            ));
        }
        let other_observed_facts = other.observed_facts;
        let other_rejected_facts_by_path = other.rejected_facts_by_path;
        let other_reused_rejection_facts = other.reused_rejection_facts;
        let other_rejection_details_dropped_by_path = other.rejection_details_dropped_by_path;
        for (path, incoming_total) in &other_rejected_facts_by_path {
            self.merge_observed_identity_path(
                path,
                other_observed_facts.get(path),
                *incoming_total,
                control,
            )?;
        }
        for (path, facts) in &other_observed_facts {
            if !other_rejected_facts_by_path.contains_key(path) {
                self.merge_observed_identity_path(path, Some(facts), 0, control)?;
            }
        }
        for (path, facts) in other_reused_rejection_facts {
            control.check(IndexWorkStage::SymbolParsing)?;
            let existing_facts = self.reused_rejection_facts.get(&path);
            let existing_count = existing_facts.map_or(0, BTreeSet::len);
            let new_count = facts
                .iter()
                .filter(|fact| existing_facts.is_none_or(|existing| !existing.contains(*fact)))
                .count();
            let after_count = existing_count.checked_add(new_count).ok_or_else(|| {
                CliError::InvalidInput("identity fact count overflowed".to_string())
            })?;
            let before_bytes = existing_facts.map_or(Ok(0), |facts| {
                identity_fact_set_retained_bytes(&path, facts)
            })?;
            let after_bytes = if after_count == 0 {
                0
            } else {
                identity_observed_path_retained_bytes(&path, after_count)?
            };
            self.adjust_identity_bytes(before_bytes, after_bytes)?;
            if after_count == 0 {
                self.reused_rejection_facts.remove(&path);
            } else {
                self.reused_rejection_facts
                    .entry(path)
                    .or_default()
                    .extend(facts);
            }
        }
        for (path, indices) in other.relation_fact_indices {
            control.check(IndexWorkStage::SymbolParsing)?;
            if let Some(existing) = self.relation_fact_indices.get(&path) {
                if existing != &indices {
                    return Err(CliError::InvalidInput(
                        "conflicting relation fact admission".to_string(),
                    ));
                }
            } else {
                self.relation_fact_indices.insert(path, indices);
            }
        }
        control.check(IndexWorkStage::SymbolParsing)?;
        let rejection_key_bytes = extend_bounded_identity_rejections_with_drop_paths(
            &mut self.rejections,
            &mut self.rejection_keys,
            other.rejections,
            &mut self.rejection_details_dropped_by_path,
        )?;
        self.observed_fact_bytes = self
            .observed_fact_bytes
            .checked_add(rejection_key_bytes)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
        for path in other_rejection_details_dropped_by_path {
            control.check(IndexWorkStage::SymbolParsing)?;
            if self.rejection_details_dropped_by_path.insert(path.clone()) {
                self.reserve_identity_bytes(
                    identity_count_path_retained_bytes(&path)?,
                    control,
                    MAX_IN_MEMORY_GRAPH_WORK_BYTES,
                )?;
            }
        }
        self.resolution_projections
            .extend(other.resolution_projections);
        for (path, count) in other.reused_rejection_counts {
            control.check(IndexWorkStage::SymbolParsing)?;
            let new_path = !self.reused_rejection_counts.contains_key(&path);
            let path_bytes = if new_path {
                identity_count_path_retained_bytes(&path)?
            } else {
                0
            };
            if self
                .reused_rejection_counts
                .insert(path.clone(), count)
                .is_some()
            {
                return Err(CliError::InvalidInput(
                    "duplicate reused identity rejection count".to_string(),
                ));
            }
            if new_path {
                self.reserve_identity_bytes(path_bytes, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
            }
        }
        for (path, count) in other.reused_parser_rejection_counts {
            control.check(IndexWorkStage::SymbolParsing)?;
            let new_path = !self.reused_parser_rejection_counts.contains_key(&path);
            let path_bytes = if new_path {
                identity_count_path_retained_bytes(&path)?
            } else {
                0
            };
            if self
                .reused_parser_rejection_counts
                .insert(path.clone(), count)
                .is_some()
            {
                return Err(CliError::InvalidInput(
                    "duplicate reused parser rejection count".to_string(),
                ));
            }
            if new_path {
                self.reserve_identity_bytes(path_bytes, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
            }
        }
        for (path, count) in other.reused_rejection_detail_counts {
            control.check(IndexWorkStage::SymbolParsing)?;
            let new_path = !self.reused_rejection_detail_counts.contains_key(&path);
            let path_bytes = if new_path {
                identity_count_path_retained_bytes(&path)?
            } else {
                0
            };
            if self
                .reused_rejection_detail_counts
                .insert(path.clone(), count)
                .is_some()
            {
                return Err(CliError::InvalidInput(
                    "duplicate reused identity rejection detail count".to_string(),
                ));
            }
            if new_path {
                self.reserve_identity_bytes(path_bytes, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
            }
        }
        control.check(IndexWorkStage::SymbolParsing)?;
        for path in other.reused_rejection_details_incomplete {
            let path_bytes = identity_count_path_retained_bytes(&path)?;
            if self.reused_rejection_details_incomplete.insert(path) {
                self.reserve_identity_bytes(path_bytes, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
            }
        }
        if self.observed_fact_bytes > MAX_IN_MEMORY_GRAPH_WORK_BYTES {
            return Err(identity_fact_budget_failure(self.observed_fact_bytes));
        }
        #[cfg(test)]
        for (key, count) in other.resolution_derivations {
            let entry = self.resolution_derivations.entry(key).or_default();
            *entry = entry.saturating_add(count);
        }
        Ok(())
    }

    /// Return the number of rejected parser facts for one path.
    fn rejected_facts_for(&self, path: &str) -> u64 {
        self.rejected_facts_by_path.get(path).copied().unwrap_or(0)
    }

    /// Return the source-admission details owned by one publication path set.
    fn for_paths(&self, paths: &BTreeSet<String>) -> Result<Self, CliError> {
        let rejected_facts_by_path = self
            .rejected_facts_by_path
            .iter()
            .filter(|(path, _count)| paths.contains(path.as_str()))
            .map(|(path, count)| (path.clone(), *count))
            .collect::<BTreeMap<_, _>>();
        let observed_facts = self
            .observed_facts
            .iter()
            .filter(|(path, _facts)| paths.contains(*path))
            .map(|(path, facts)| (path.clone(), facts.clone()))
            .collect::<BTreeMap<_, _>>();
        let rejections = self
            .rejections
            .iter()
            .filter(|rejection| paths.contains(rejection.path.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let rejection_keys = self
            .rejection_keys
            .iter()
            .filter(|key| paths.contains(key.path.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>();
        let rejection_details_dropped_by_path = self
            .rejection_details_dropped_by_path
            .iter()
            .filter(|path| paths.contains(path.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>();
        let marker_bytes =
            identity_rejection_drop_paths_retained_bytes(&rejection_details_dropped_by_path)?;
        let observed_fact_bytes =
            identity_maps_retained_bytes(&observed_facts, &rejected_facts_by_path)?
                .checked_add(identity_rejection_keys_retained_bytes(&rejection_keys)?)
                .and_then(|bytes| bytes.checked_add(marker_bytes))
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
        Ok(Self {
            rejection_keys,
            rejections,
            source_admitted: self.source_admitted,
            observed_fact_bytes,
            reused_rejection_facts: BTreeMap::new(),
            rejected_facts_by_path,
            observed_facts,
            relation_fact_indices: self
                .relation_fact_indices
                .iter()
                .filter(|(path, _indices)| paths.contains(path.as_str()))
                .map(|(path, indices)| (path.clone(), indices.clone()))
                .collect(),
            resolution_projections: BTreeMap::new(),
            reused_rejection_counts: BTreeMap::new(),
            reused_parser_rejection_counts: BTreeMap::new(),
            reused_rejection_detail_counts: BTreeMap::new(),
            reused_rejection_details_incomplete: BTreeSet::new(),
            rejection_details_dropped_by_path,
            #[cfg(test)]
            resolution_derivations: BTreeMap::new(),
            #[cfg(test)]
            paired_import_pairing_work: 0,
        })
    }

    /// Return whether this report contains any rejected source facts.
    fn has_rejections(&self) -> bool {
        !self.rejected_facts_by_path.is_empty()
    }

    /// Return reused paths that cannot be reconciled without reparsing source.
    fn incomplete_reused_rejection_paths(&self) -> &BTreeSet<String> {
        &self.reused_rejection_details_incomplete
    }

    /// Return whether a path lost a distinct typed rejection detail at the
    /// publication-wide ceiling.
    fn rejection_details_dropped_for(&self, path: &str) -> bool {
        self.rejection_details_dropped_by_path.contains(path)
    }

    /// Reconcile a reused path's persisted total with current re-derived facts.
    fn rejected_facts_for_graph(&self, path: &str, derived: &Self) -> Result<u64, CliError> {
        let Some(persisted) = self.reused_rejection_counts.get(path).copied() else {
            return self
                .rejected_facts_for(path)
                .checked_add(derived.rejected_facts_for(path))
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection count overflowed".to_string())
                });
        };
        let persisted_parser = self
            .reused_parser_rejection_counts
            .get(path)
            .copied()
            .unwrap_or(0);
        let persisted_identity = persisted.checked_sub(persisted_parser).ok_or_else(|| {
            CliError::InvalidInput("persisted parser rejection count exceeded total".to_string())
        })?;
        let mut current_facts = BTreeSet::<&IdentityFactKey>::new();
        if let Some(facts) = self.observed_facts.get(path) {
            current_facts.extend(facts);
        }
        if let Some(facts) = derived.observed_facts.get(path) {
            current_facts.extend(facts);
        }
        let current_known = u64::try_from(current_facts.len()).map_err(|error| {
            CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
        })?;
        let current_total = self
            .rejected_facts_for(path)
            .checked_add(derived.rejected_facts_for(path))
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection count overflowed".to_string())
            })?;
        let current_unknown = current_total.checked_sub(current_known).ok_or_else(|| {
            CliError::InvalidInput("observed identity facts exceeded rejection count".to_string())
        })?;
        let persisted_facts = self.reused_rejection_facts.get(path);
        let persisted_known = persisted_facts.map_or(0, BTreeSet::len);
        let persisted_known = u64::try_from(persisted_known).map_err(|error| {
            CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
        })?;
        let persisted_unknown =
            persisted_identity
                .checked_sub(persisted_known)
                .ok_or_else(|| {
                    CliError::InvalidInput(
                        "persisted identity facts exceeded rejection count".to_string(),
                    )
                })?;
        let mut all_facts = current_facts;
        if let Some(facts) = persisted_facts {
            all_facts.extend(
                facts
                    .iter()
                    .filter(|fact| !is_rederived_identity_fact(fact)),
            );
        }
        u64::try_from(all_facts.len())
            .map_err(|error| {
                CliError::InvalidInput(format!("identity fact count overflowed: {error}"))
            })?
            .checked_add(persisted_unknown)
            .and_then(|count| count.checked_add(current_unknown))
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection count overflowed".to_string())
            })
    }

    /// Return the parser-baseline omission retained for one reused path.
    fn parser_rejection_for_graph(&self, path: &str) -> u64 {
        self.reused_parser_rejection_counts
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    /// Return whether parser-output graphs have passed the shared source boundary.
    pub(super) fn source_admitted(&self) -> bool {
        self.source_admitted
    }

    /// Return paths that need a graph publication because admission changed them.
    fn paths(&self) -> impl Iterator<Item = &str> {
        self.rejected_facts_by_path.keys().map(String::as_str)
    }

    /// Return the already-admitted resolution projection for one graph path.
    fn resolution_projection(&self, path: &str) -> Option<&ResolutionKeyProjection> {
        self.resolution_projections.get(path)
    }

    /// Translate an admitted relation index back to its parser ordinal.
    fn relation_parser_index(&self, path: &str, admitted_index: usize) -> usize {
        self.relation_fact_indices
            .get(path)
            .and_then(|indices| indices.get(admitted_index))
            .copied()
            .unwrap_or(admitted_index)
    }

    /// Count one resolution-key derivation for the test-visible ownership proof.
    #[cfg(test)]
    fn record_resolution_derivation(&mut self, path: &str, generation: IndexGeneration) {
        let count = self
            .resolution_derivations
            .entry((path.to_string(), generation))
            .or_default();
        *count = count.saturating_add(1);
    }
}

/// Build keyed membership for a retained typed detail vector.
fn identity_rejection_key_set(
    rejections: &[GraphIdentityRejection],
) -> BTreeSet<IdentityRejectionKey> {
    rejections.iter().map(IdentityRejectionKey::from).collect()
}

/// Merge typed details under the one-publication storage ceiling.
#[cfg(test)]
fn extend_bounded_identity_rejections(
    target: &mut Vec<GraphIdentityRejection>,
    target_keys: &mut BTreeSet<IdentityRejectionKey>,
    incoming: impl IntoIterator<Item = GraphIdentityRejection>,
) -> Result<u64, CliError> {
    let mut dropped_paths = BTreeSet::new();
    extend_bounded_identity_rejections_with_drop_paths(
        target,
        target_keys,
        incoming,
        &mut dropped_paths,
    )
}

/// Merge typed details and retain the exact paths that lost a distinct row.
fn extend_bounded_identity_rejections_with_drop_paths(
    target: &mut Vec<GraphIdentityRejection>,
    target_keys: &mut BTreeSet<IdentityRejectionKey>,
    incoming: impl IntoIterator<Item = GraphIdentityRejection>,
    dropped_paths: &mut BTreeSet<String>,
) -> Result<u64, CliError> {
    let mut retained_bytes = 0_u64;
    for rejection in incoming {
        let key = IdentityRejectionKey::from(&rejection);
        if target_keys.contains(&key) {
            continue;
        }
        if target.len() >= MAX_GRAPH_IDENTITY_REJECTIONS {
            if dropped_paths.insert(rejection.path.as_str().to_owned()) {
                retained_bytes = retained_bytes
                    .checked_add(identity_count_path_retained_bytes(rejection.path.as_str())?)
                    .ok_or_else(|| {
                        CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                    })?;
            }
            continue;
        }
        target_keys.insert(key);
        retained_bytes = retained_bytes
            .checked_add(identity_rejection_key_retained_bytes(
                rejection.path.as_str(),
            )?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
        target.push(rejection);
    }
    Ok(retained_bytes)
}

/// Mark graph-scoped coverage when publication evicted a typed identity row.
fn mark_identity_rejection_coverage_limits(
    coverage: &mut [CoverageRecord],
    dropped_paths: &BTreeSet<String>,
) -> Result<(), CliError> {
    for row in coverage {
        let CoverageScope::Path { path } = row.scope() else {
            continue;
        };
        if row.relation().is_some()
            || !dropped_paths.contains(path.as_str())
            || row.reached_limit() == Some(GraphLimitKind::Rows)
        {
            continue;
        }
        *row = CoverageRecord::new(
            row.scope().clone(),
            row.relation(),
            row.state(),
            row.covered(),
            row.omitted(),
            row.generation(),
            row.reason().cloned(),
            Some(GraphLimitKind::Rows),
        )
        .map_err(invalid_graph_contract)?;
    }
    Ok(())
}

/// One graph mutation staged outside the database writer transaction.
pub(super) enum RepositoryGraphMutation {
    /// Replace the complete normalized graph.
    Full,
    /// Replace the exact admitted source closure while retaining unrelated rows.
    AffectedPaths(Vec<String>),
}

/// Complete normalized graph and canonical-key rows waiting for publication.
pub(super) struct StagedRepositoryGraph {
    /// Project identity owning every staged graph fact.
    project: ProjectInstanceId,
    /// Full or affected-path replacement selected by the staging caller.
    mutation: RepositoryGraphMutation,
    /// Stable typed entities ready for normalized persistence.
    entities: Vec<GraphEntity>,
    /// Deduplicated logical relationships ready for persistence.
    relations: Vec<LogicalRelation>,
    /// Exact source occurrences retained separately from logical relations.
    occurrences: Vec<RelationOccurrence>,
    /// Parser and relation coverage facts for staged source paths.
    coverage: Vec<CoverageRecord>,
    /// Canonical resolution keys exported by staged entities.
    entity_exports: Vec<EntityResolutionKey>,
    /// Canonical dependency keys retained by staged relations.
    relation_dependencies: Vec<RelationDependencyKey>,
    /// Closed reasons retained only for unresolved canonical document relations.
    document_unresolved_reasons: Vec<(LogicalRelationKey, DocumentTargetUnresolvedReason)>,
    /// Bounded typed parser-identity rejections committed with this generation.
    identity_rejections: Vec<GraphIdentityRejection>,
    /// Test-only resolution derivation counts used to prove incremental reuse.
    #[cfg(test)]
    resolution_derivations: BTreeMap<(String, IndexGeneration), usize>,
    /// Test-visible peak of the entity projection's map-plus-entity estimate.
    #[cfg(test)]
    peak_retained_bytes: u64,
    /// Test-visible order proving projections moved before entity allocation.
    #[cfg(test)]
    projection_removals_before_entities: Vec<String>,
    /// Effective scan policy used to recheck non-indexed document targets before commit.
    scan_policy: RootScanPolicy,
    /// Non-indexed target states observed while resolving document candidates.
    document_target_states: Vec<(String, DocumentTargetUnresolvedReason)>,
    /// Optional disposable database replacing the in-memory row vectors.
    database: Option<StagedGraphDatabase>,
    /// Conservative bytes retained until the parent publication completes.
    retained_bytes: u64,
}

/// File-backed graph rows staged outside the main database writer transaction.
struct StagedGraphDatabase {
    /// Open typed store copied into the main publication.
    store: Option<AtlasStore>,
    /// Owning directory removed after the store closes.
    directory: Option<TempDir>,
    /// Cross-process lease preventing restart cleanup while the stage is active.
    _lease: File,
}

impl StagedGraphDatabase {
    /// Return the live typed staging store.
    fn store(&self) -> Result<&AtlasStore, CliError> {
        self.store
            .as_ref()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }

    /// Return the live typed staging store mutably during preparation.
    fn store_mut(&mut self) -> Result<&mut AtlasStore, CliError> {
        self.store
            .as_mut()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }

    /// Return the live disposable staging directory.
    fn directory(&self) -> Result<&TempDir, CliError> {
        self.directory
            .as_ref()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }
}

impl Drop for StagedGraphDatabase {
    fn drop(&mut self) {
        let prepared = self.store.take().is_some();
        let Some(directory) = self.directory.take() else {
            return;
        };
        let database_path = directory.path().join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let direct_database = fs::symlink_metadata(&database_path).is_ok_and(|metadata| {
            metadata.file_type().is_file() && !metadata.file_type().is_symlink()
        });
        if prepared
            && direct_database
            && remove_owned_graph_stage_payload(directory.path(), &database_path, None).is_ok()
        {
            drop(directory);
        } else {
            let _retained_path: PathBuf = directory.keep();
        }
    }
}

impl StagedRepositoryGraph {
    /// Return conservative retained bytes counted toward the parent staging budget.
    pub(super) const fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    /// Refuse publication if a consulted non-indexed target changed after resolution.
    pub(super) fn revalidate_document_targets(&self, root: &Path) -> Result<(), CliError> {
        if self.document_target_states.is_empty() {
            return Ok(());
        }
        let current = DocumentResolutionIndex::new(root, &[], &self.scan_policy)?;
        for (path, expected) in &self.document_target_states {
            if current.absent_reason(path)? != *expected {
                return Err(source_changed_during_derivation(root, path));
            }
        }
        Ok(())
    }

    /// Apply the complete staged graph through the parent publication transaction.
    pub(super) fn apply(
        &self,
        publication: &mut IndexPublicationGuard<'_>,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::Publication)?;
        if let Some(database) = &self.database {
            if !database.directory()?.path().is_dir() {
                return Err(CliError::InvalidInput(
                    "repository graph staging directory is unavailable".to_string(),
                ));
            }
            if !matches!(self.mutation, RepositoryGraphMutation::Full) {
                return Err(CliError::InvalidInput(
                    "database-backed graph staging only supports full replacement".to_string(),
                ));
            }
            publication.replace_repository_graph_from_staging(
                self.project,
                database.store()?,
                Some(control),
            )?;
            publication
                .replace_graph_identity_rejections(self.project, &self.identity_rejections)?;
            if !self.document_unresolved_reasons.is_empty() {
                publication.set_document_unresolved_reasons_controlled(
                    &self.document_unresolved_reasons,
                    control,
                )?;
            }
            control.check(IndexWorkStage::Publication)?;
            return Ok(());
        }
        match &self.mutation {
            RepositoryGraphMutation::Full => {
                publication.replace_repository_graph_with_resolution_keys(
                    self.project,
                    &self.entities,
                    &self.relations,
                    &self.occurrences,
                    &self.coverage,
                    &self.entity_exports,
                    &self.relation_dependencies,
                )?;
                publication
                    .replace_graph_identity_rejections(self.project, &self.identity_rejections)?;
            }
            RepositoryGraphMutation::AffectedPaths(paths) => {
                publication.replace_repository_graph_for_paths_with_resolution_keys(
                    self.project,
                    paths,
                    &self.entities,
                    &self.relations,
                    &self.occurrences,
                    &self.coverage,
                    &self.entity_exports,
                    &self.relation_dependencies,
                )?;
                publication
                    .replace_graph_identity_rejections(self.project, &self.identity_rejections)?;
            }
        }
        if !self.document_unresolved_reasons.is_empty() {
            publication.set_document_unresolved_reasons_controlled(
                &self.document_unresolved_reasons,
                control,
            )?;
        }
        control.check(IndexWorkStage::Publication)?;
        Ok(())
    }
}

/// Stage a complete repository graph from current parser output plus safe reused graphs.
pub(super) fn stage_full_repository_graph(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    nodes: &[Node],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    cleanup_abandoned_repository_graph_staging(store, root, control)?;
    let project = selected_project(store)?;
    let generation = next_generation(base_generation)?;
    let paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let loaded_graphs = complete_symbol_graphs(store, &paths, symbols, control)?;
    let (graphs, mut identity_admission) = admit_symbol_graphs(loaded_graphs, control)?;
    let changed_paths = symbols
        .changes
        .iter()
        .map(|change| match change {
            SymbolProjectionChange::Parsed(parsed) => parsed.path.as_str(),
            SymbolProjectionChange::Clear { path, .. } => path.as_str(),
        })
        .collect::<BTreeSet<_>>();
    let reused_paths = paths
        .iter()
        .filter(|path| !changed_paths.contains(path.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    hydrate_reused_identity_admission(
        store,
        project,
        &reused_paths,
        &graphs,
        &mut identity_admission,
        control,
    )?;
    if !identity_admission
        .incomplete_reused_rejection_paths()
        .is_empty()
    {
        return Err(dependency_closure_limit(
            root,
            identity_admission
                .incomplete_reused_rejection_paths()
                .iter()
                .cloned(),
            identity_admission.incomplete_reused_rejection_paths().len(),
        ));
    }
    identity_admission.merge(symbols.identity_admission.for_paths(&paths)?, control)?;
    let mut document_facts = complete_markdown_facts(root, nodes, &graphs, symbols, control)?;
    admit_markdown_facts(
        &mut document_facts,
        &graphs,
        &mut identity_admission,
        control,
    )?;
    control.check(IndexWorkStage::SymbolParsing)?;
    let configured_modules =
        super::module_resolution::load_configured_module_resolution(root, nodes, control)?;
    let packages = PackageIndex::from_graphs(&graphs)?;
    admit_resolution_key_failures(
        project,
        generation,
        &graphs,
        &packages,
        &configured_modules,
        &mut identity_admission,
        control,
    )?;
    ensure_admitted_resolution_projections(&graphs, &identity_admission.resolution_projections)?;
    let entity_projection = build_entity_projection_with_config(
        project,
        generation,
        nodes,
        &graphs,
        &packages,
        &configured_modules,
        Some(&mut identity_admission.resolution_projections),
        true,
        control,
    )?;
    debug_assert!(identity_admission.resolution_projections.is_empty());
    let candidates = resolution_registry_from_exports(&entity_projection, control)?;
    enforce_resolution_staging_budget(&entity_projection, &candidates)?;
    let document_projection_bytes = document_projection_retained_bytes(&document_facts, control)?;
    let graph_work_bytes = symbols
        .retained_bytes
        .saturating_add(identity_admission.observed_fact_bytes)
        .saturating_add(entity_projection.retained_bytes)
        .saturating_add(candidates.retained_bytes)
        .saturating_add(document_fact_map_retained_bytes(&document_facts))
        .saturating_add(document_projection_bytes);
    if graph_work_bytes > MAX_IN_MEMORY_GRAPH_WORK_BYTES {
        finish_projection_in_database_with_documents(
            root,
            nodes,
            project,
            generation,
            &graphs,
            &document_facts,
            &identity_admission,
            entity_projection,
            &candidates,
            scan_policy,
            control,
        )
    } else {
        finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            root,
            nodes,
            &document_facts,
            &identity_admission,
            entity_projection,
            &candidates,
            scan_policy,
            control,
        )
    }
}

/// Stage one bounded dependency-aware graph closure for an incremental publication.
pub(super) fn stage_incremental_repository_graph(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    expected_nodes: &[Node],
    direct_paths: &[String],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    stage_incremental_repository_graph_with_limit(
        store,
        root,
        base_generation,
        expected_nodes,
        direct_paths,
        scan_policy,
        symbols,
        control,
        super::MAX_PUBLICATION_STAGING_BYTES,
    )
}

#[cfg(test)]
fn stage_incremental_repository_graph_with_test_limit(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    expected_nodes: &[Node],
    direct_paths: &[String],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
    staging_limit: u64,
) -> Result<StagedRepositoryGraph, CliError> {
    stage_incremental_repository_graph_with_limit(
        store,
        root,
        base_generation,
        expected_nodes,
        direct_paths,
        scan_policy,
        symbols,
        control,
        staging_limit,
    )
}

#[allow(clippy::too_many_arguments)]
/// Stage an incremental graph with the supplied projection staging limit.
fn stage_incremental_repository_graph_with_limit(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    expected_nodes: &[Node],
    direct_paths: &[String],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
    staging_limit: u64,
) -> Result<StagedRepositoryGraph, CliError> {
    let project = selected_project(store)?;
    let generation = next_generation(base_generation)?;
    let configured_modules =
        super::module_resolution::load_configured_module_resolution(root, expected_nodes, control)?;
    let direct_paths = direct_paths.iter().cloned().collect::<BTreeSet<_>>();
    enforce_incremental_count(
        root,
        "direct source paths",
        direct_paths.len(),
        &direct_paths,
    )?;
    let _direct_footprint =
        admitted_persisted_footprint(store, project, root, &direct_paths, control)?;

    let current_file_paths = expected_nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let direct_graph_paths = direct_paths
        .intersection(&current_file_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let manifest_paths = expected_nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File && is_cargo_manifest_path(&node.path))
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let package_only_paths = manifest_paths
        .difference(&direct_graph_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let loaded_direct_graphs =
        complete_symbol_graphs(store, &direct_graph_paths, symbols, control)?;
    let (direct_graphs, mut direct_identity_admission) =
        admit_symbol_graphs(loaded_direct_graphs, control)?;
    direct_identity_admission.merge(
        symbols.identity_admission.for_paths(&direct_graph_paths)?,
        control,
    )?;
    let package_graphs = complete_symbol_graphs(store, &package_only_paths, symbols, control)?;
    let (package_graphs, _package_context_admission) =
        admit_symbol_graphs(package_graphs, control)?;
    let package_index_graphs = direct_graphs
        .iter()
        .map(Cow::as_ref)
        .chain(package_graphs.iter().map(Cow::as_ref))
        .collect::<Vec<_>>();
    let direct_packages = PackageIndex::from_graphs(&package_index_graphs)?;
    admit_resolution_key_failures(
        project,
        generation,
        &direct_graphs,
        &direct_packages,
        &configured_modules,
        &mut direct_identity_admission,
        control,
    )?;

    let old_exports = store.repository_export_keys_for_paths(
        project,
        &direct_paths.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if old_exports.truncated {
        return Err(dependency_closure_limit(
            root,
            direct_paths.iter().cloned(),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut changed_keys = old_exports.rows.into_iter().collect::<BTreeSet<_>>();
    for path in &direct_paths {
        changed_keys.insert(document_file_resolution_key(project, path)?);
        changed_keys.insert(document_casefold_resolution_key(project, path)?);
    }
    for graph in &direct_graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        let projection = direct_identity_admission
            .resolution_projection(&graph.path)
            .ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "resolution keys were not admitted for graph {}",
                    graph.path
                ))
            })?;
        changed_keys.extend(projection.source_keys().iter().cloned());
        for symbol in projection.symbol_keys() {
            changed_keys.extend(symbol.keys().iter().cloned());
        }
        for symbol in &graph.symbols {
            if symbol.kind == SymbolKind::Heading {
                changed_keys.insert(document_heading_resolution_key(
                    project,
                    &graph.path,
                    &symbol.signature,
                )?);
            }
        }
    }
    enforce_incremental_count(
        root,
        "old and new export keys",
        changed_keys.len(),
        &direct_paths,
    )?;

    let inbound = store.repository_affected_source_paths(
        project,
        &changed_keys.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if inbound.truncated {
        return Err(dependency_closure_limit(
            root,
            inbound.rows.iter().map(|path| path.as_str().to_string()),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut affected_paths = direct_paths;
    affected_paths.extend(inbound.rows.into_iter().map(String::from));
    affected_paths.extend(direct_identity_admission.paths().map(ToString::to_string));
    enforce_incremental_count(
        root,
        "affected source paths",
        affected_paths.len(),
        &affected_paths,
    )?;
    control.check(IndexWorkStage::SymbolParsing)?;
    let persisted_footprint =
        admitted_persisted_footprint(store, project, root, &affected_paths, control)?;

    let affected_graph_paths = affected_paths
        .intersection(&current_file_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let newly_affected_graph_paths = affected_graph_paths
        .difference(&direct_graph_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let loaded_affected_graphs =
        complete_symbol_graphs(store, &newly_affected_graph_paths, symbols, control)?;
    let (newly_affected_graphs, mut affected_identity_admission) =
        admit_symbol_graphs(loaded_affected_graphs, control)?;
    hydrate_reused_identity_admission(
        store,
        project,
        &newly_affected_graph_paths,
        &newly_affected_graphs,
        &mut affected_identity_admission,
        control,
    )?;
    if !affected_identity_admission
        .incomplete_reused_rejection_paths()
        .is_empty()
    {
        return Err(dependency_closure_limit(
            root,
            affected_identity_admission
                .incomplete_reused_rejection_paths()
                .iter()
                .cloned(),
            affected_identity_admission
                .incomplete_reused_rejection_paths()
                .len(),
        ));
    }
    let admitted_graphs = direct_graphs
        .iter()
        .map(Cow::as_ref)
        .chain(newly_affected_graphs.iter().map(Cow::as_ref))
        .chain(package_graphs.iter().map(Cow::as_ref))
        .collect::<Vec<_>>();
    let packages = PackageIndex::from_graphs(&admitted_graphs)?;
    admit_resolution_key_failures(
        project,
        generation,
        &newly_affected_graphs,
        &packages,
        &configured_modules,
        &mut affected_identity_admission,
        control,
    )?;
    direct_identity_admission.merge(affected_identity_admission, control)?;
    enforce_incremental_projection_budget(
        root,
        &affected_paths,
        0,
        direct_identity_admission.observed_fact_bytes,
    )?;
    let affected_graphs = direct_graphs
        .iter()
        .filter(|graph| affected_graph_paths.contains(&graph.path))
        .map(Cow::as_ref)
        .chain(newly_affected_graphs.iter().map(Cow::as_ref))
        .collect::<Vec<_>>();
    ensure_admitted_resolution_projections(
        &affected_graphs,
        &direct_identity_admission.resolution_projections,
    )?;
    let mut document_facts =
        complete_markdown_facts(root, expected_nodes, &affected_graphs, symbols, control)?;
    admit_markdown_facts(
        &mut document_facts,
        &affected_graphs,
        &mut direct_identity_admission,
        control,
    )?;
    let affected_nodes = expected_nodes
        .iter()
        .filter(|node| affected_paths.contains(&node.path))
        .cloned()
        .collect::<Vec<_>>();
    let entity_projection = build_entity_projection_with_config_limit(
        project,
        generation,
        &affected_nodes,
        &affected_graphs,
        &packages,
        &configured_modules,
        Some(&mut direct_identity_admission.resolution_projections),
        false,
        control,
        staging_limit,
    )?;
    debug_assert!(direct_identity_admission.resolution_projections.is_empty());

    let mut dependency_keys = entity_projection
        .keys_by_graph
        .values()
        .flat_map(ResolutionKeyProjection::relation_keys)
        .flat_map(|relation| relation.keys().iter().cloned())
        .collect::<BTreeSet<_>>();
    dependency_keys.extend(document_dependency_keys(project, &document_facts)?);
    enforce_incremental_count(
        root,
        "affected dependency keys",
        dependency_keys.len(),
        &affected_paths,
    )?;
    let persisted = store.repository_resolution_candidates_for_keys(
        project,
        &dependency_keys.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if persisted.truncated {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut candidates = resolution_registry_from_persisted(
        project,
        generation,
        persisted.rows,
        &affected_paths,
        control,
    )?;
    merge_resolution_registries(
        &mut candidates,
        resolution_registry_from_exports(&entity_projection, control)?,
        control,
    )?;
    let document_projection_bytes = document_projection_retained_bytes(&document_facts, control)?;
    enforce_incremental_projection_budget(
        root,
        &affected_paths,
        0,
        entity_projection
            .retained_bytes
            .saturating_add(candidates.retained_bytes)
            .saturating_add(document_fact_map_retained_bytes(&document_facts))
            .saturating_add(document_projection_bytes)
            .saturating_add(direct_identity_admission.observed_fact_bytes),
    )?;
    let staged = finish_projection_with_documents(
        project,
        generation,
        RepositoryGraphMutation::AffectedPaths(affected_paths.iter().cloned().collect()),
        &affected_graphs,
        root,
        expected_nodes,
        &document_facts,
        &direct_identity_admission,
        entity_projection,
        &candidates,
        scan_policy,
        control,
    )?;
    enforce_incremental_projection_limits(root, &affected_paths, persisted_footprint, &staged)?;
    Ok(staged)
}

/// Entity/key facts prepared before relation resolution.
struct EntityProjection {
    /// Entities keyed by their compact digest for collision-safe lookup.
    entity_by_digest: BTreeMap<String, GraphEntity>,
    /// File and symbol owners keyed by parser graph path.
    owners_by_graph: BTreeMap<String, GraphOwners>,
    /// Canonical resolution-key projections keyed by parser graph path.
    keys_by_graph: BTreeMap<String, ResolutionKeyProjection>,
    /// Canonical resolution keys exported by staged entities.
    entity_exports: Vec<EntityResolutionKey>,
    /// Conservative bytes retained by entities and export keys.
    retained_bytes: u64,
    /// Test-visible peak of the map-plus-entity staging estimate.
    #[cfg(test)]
    peak_retained_bytes: u64,
    /// Test-visible order proving each projection moved before entity allocation.
    #[cfg(test)]
    projection_removals_before_entities: Vec<String>,
}

/// File and symbol entities associated with one parser graph.
struct GraphOwners {
    /// Digest of the file entity owning the parser graph.
    file_digest: String,
    /// Optional stable entity digest corresponding to each parser symbol row.
    symbol_digests: Vec<Option<String>>,
}

/// Borrowed per-graph symbol lookup used by every relation in that file.
struct GraphSymbolIndex<'graph> {
    /// Parser symbol indices grouped by exact source name.
    indices_by_name: BTreeMap<&'graph str, Vec<usize>>,
}

impl<'graph> GraphSymbolIndex<'graph> {
    /// Build one bounded name index instead of rescanning all symbols per relation.
    fn new(graph: &'graph SymbolGraph, control: &IndexWorkControl) -> Result<Self, CliError> {
        let mut indices_by_name = BTreeMap::new();
        for (index, symbol) in graph.symbols.iter().enumerate() {
            check_graph_work(control, index)?;
            indices_by_name
                .entry(symbol.name.as_str())
                .or_insert_with(Vec::new)
                .push(index);
        }
        Ok(Self { indices_by_name })
    }

    /// Return exact parser symbol rows for one source name.
    fn get(&self, name: &str) -> &[usize] {
        self.indices_by_name.get(name).map_or(&[], Vec::as_slice)
    }
}

/// One conservative additive relation derived from already bounded parser facts.
struct DerivedRelationFact {
    /// Additive graph relation kind.
    kind: ExtendedRelationKind,
    /// Parser-compatible source, target, span, context, and trust facts.
    relation: SymbolRelation,
    /// Resolution strategy for the derived target.
    target: DerivedRelationTarget,
}

/// Closed target classes accepted by additive relation projection.
enum DerivedRelationTarget {
    /// Resolve through the same typed provider keys as one parser relation.
    Parser {
        /// Canonical provider-owned dependency keys.
        keys: Vec<CanonicalResolutionKey>,
    },
    /// Resolve one statically visible repository-relative file path.
    RepositoryPath,
    /// Bind one content-free external namespace and identity.
    External {
        /// Closed external namespace.
        system: &'static str,
    },
}

/// Package names assigned by longest manifest-directory ownership.
struct PackageIndex {
    /// Cargo packages ordered from the most-specific repository root.
    packages: Vec<PackageOwner>,
}

/// One Cargo package and its owning repository prefix.
struct PackageOwner {
    /// Repository prefix owned by this package.
    root: String,
    /// Canonical package name from the manifest graph.
    name: String,
    /// Repository-relative manifest path proving ownership.
    manifest: String,
}

impl PackageIndex {
    /// Build deterministic longest-prefix package ownership from manifest graphs.
    fn from_graphs(graphs: &[impl Borrow<SymbolGraph>]) -> Result<Self, CliError> {
        let mut packages = Vec::new();
        for graph in graphs {
            let graph = graph.borrow();
            for symbol in &graph.symbols {
                if symbol.kind != SymbolKind::Package {
                    continue;
                }
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?;
                let root = graph
                    .path
                    .rsplit_once('/')
                    .map_or(String::new(), |(parent, _manifest)| parent.to_string());
                packages.push(PackageOwner {
                    root,
                    name: symbol.name.clone(),
                    manifest: graph.path.clone(),
                });
            }
        }
        packages.sort_by(|left, right| {
            right
                .root
                .len()
                .cmp(&left.root.len())
                .then_with(|| left.root.cmp(&right.root))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.manifest.cmp(&right.manifest))
        });
        packages.dedup_by(|left, right| {
            left.root == right.root && left.name == right.name && left.manifest == right.manifest
        });
        Ok(Self { packages })
    }

    /// Return the most-specific Cargo package owning one repository path.
    fn package_name(&self, path: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|package| repository_path_belongs_to(path, &package.root))
            .map(|package| package.name.as_str())
    }
}

/// Return whether one path belongs to a normalized repository prefix.
fn repository_path_belongs_to(path: &str, root: &str) -> bool {
    root.is_empty()
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Return whether one normalized repository path is a Cargo package manifest.
fn is_cargo_manifest_path(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Qualify graph-only parent identity from the parser's existing containment rows.
///
/// Sorting and active-scope lookup are `O(n log n)`. Identity work is
/// `O(n * MAX_GRAPH_IDENTITY_BYTES)`: every retained parent is bounded, and an
/// overflowing candidate is hashed without materializing more than one bounded
/// parent plus one admitted component.
fn qualified_symbol_parents(
    graph: &SymbolGraph,
) -> Result<Vec<Option<GraphIdentityText>>, CliError> {
    let mut order = (0..graph.symbols.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (graph.symbols[index].line_start, index));
    let mut active_by_name = BTreeMap::<&str, Vec<usize>>::new();
    let mut qualified_names = vec![None::<GraphIdentityText>; graph.symbols.len()];
    let mut parents = vec![None; graph.symbols.len()];
    for index in order {
        let symbol = &graph.symbols[index];
        let name = source_symbol_identity(symbol.name.clone())?;
        let mut parent = symbol
            .parent
            .clone()
            .map(source_symbol_identity)
            .transpose()?;
        if let Some(immediate_parent) = parent.as_ref()
            && let Some(candidates) = active_by_name.get_mut(immediate_parent.as_str())
        {
            while candidates.last().is_some_and(|&candidate_index| {
                graph.symbols[candidate_index].line_end < symbol.line_end
            }) {
                candidates.pop();
            }
            if let Some(qualified_parent) = candidates
                .last()
                .and_then(|&candidate_index| qualified_names[candidate_index].clone())
            {
                parent = Some(qualified_parent);
            }
        }
        qualified_names[index] = Some(match parent.as_ref() {
            Some(parent) => qualified_symbol_identity(parent, &name)?,
            None => name,
        });
        parents[index] = parent;
        active_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(index);
    }
    Ok(parents)
}

/// Admit one parser-owned symbol identity outside the derived-scope namespace.
fn source_symbol_identity(value: String) -> Result<GraphIdentityText, CliError> {
    let identity = GraphIdentityText::new(value).map_err(invalid_graph_contract)?;
    if identity.as_str().starts_with(QUALIFIED_SYMBOL_SCOPE_PREFIX) {
        return Err(invalid_graph_contract(
            GraphContractError::InvalidIdentityText {
                reason: "source symbol identity uses the reserved derived-scope namespace",
            },
        ));
    }
    Ok(identity)
}

/// Derive one exact or compact qualified scope from validated components.
fn qualified_symbol_identity(
    parent: &GraphIdentityText,
    name: &GraphIdentityText,
) -> Result<GraphIdentityText, CliError> {
    const SEPARATOR: &str = "::";
    const DIGEST_DOMAIN: &str = "projectatlas.graph.qualified-symbol-scope.v1";
    let qualified_len = parent.as_str().len() + SEPARATOR.len() + name.as_str().len();
    if qualified_len <= MAX_GRAPH_IDENTITY_BYTES {
        let mut qualified = String::with_capacity(qualified_len);
        qualified.push_str(parent.as_str());
        qualified.push_str(SEPARATOR);
        qualified.push_str(name.as_str());
        return GraphIdentityText::new(qualified).map_err(invalid_graph_contract);
    }

    let compact_len =
        QUALIFIED_SYMBOL_SCOPE_PREFIX.len() + 64 + SEPARATOR.len() + name.as_str().len();
    if compact_len > MAX_GRAPH_IDENTITY_BYTES {
        return Err(invalid_graph_contract(
            GraphContractError::InvalidIdentityText {
                reason: "derived scope cannot retain its nearest admitted symbol name",
            },
        ));
    }
    let mut hasher = blake3::Hasher::new_derive_key(DIGEST_DOMAIN);
    hasher.update(parent.as_str().as_bytes());
    hasher.update(SEPARATOR.as_bytes());
    hasher.update(name.as_str().as_bytes());
    let digest = hasher.finalize().to_hex();
    let mut compact = String::with_capacity(compact_len);
    compact.push_str(QUALIFIED_SYMBOL_SCOPE_PREFIX);
    compact.push_str(digest.as_str());
    compact.push_str(SEPARATOR);
    compact.push_str(name.as_str());
    GraphIdentityText::new(compact).map_err(invalid_graph_contract)
}

/// Project file, symbol, package, and canonical export facts from parser graphs.
#[cfg(test)]
fn build_entity_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    include_project: bool,
    control: &IndexWorkControl,
) -> Result<EntityProjection, CliError> {
    build_entity_projection_with_config(
        project,
        generation,
        nodes,
        graphs,
        packages,
        &ConfiguredModuleResolution::default(),
        None,
        include_project,
        control,
    )
}

/// Project entity/key facts with one shared configured-module snapshot.
#[allow(clippy::too_many_arguments)]
fn build_entity_projection_with_config(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    configured_modules: &ConfiguredModuleResolution,
    admitted_projections: Option<&mut BTreeMap<String, ResolutionKeyProjection>>,
    include_project: bool,
    control: &IndexWorkControl,
) -> Result<EntityProjection, CliError> {
    build_entity_projection_with_config_limit(
        project,
        generation,
        nodes,
        graphs,
        packages,
        configured_modules,
        admitted_projections,
        include_project,
        control,
        super::MAX_PUBLICATION_STAGING_BYTES,
    )
}

/// Project entity/key facts with a caller-selected staging budget for proof seams.
#[allow(clippy::too_many_arguments)]
fn build_entity_projection_with_config_limit(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    configured_modules: &ConfiguredModuleResolution,
    mut admitted_projections: Option<&mut BTreeMap<String, ResolutionKeyProjection>>,
    include_project: bool,
    control: &IndexWorkControl,
    staging_limit: u64,
) -> Result<EntityProjection, CliError> {
    let mut entity_by_digest = BTreeMap::new();
    let mut entity_exports = Vec::new();
    let mut entity_bytes = 0_u64;
    if include_project {
        let entity = GraphEntity::new(project, EntitySelector::Project, generation)
            .map_err(invalid_graph_contract)?;
        entity_bytes = entity_bytes.saturating_add(entity_retained_bytes(&entity));
        insert_entity(&mut entity_by_digest, entity)?;
    }
    for node in nodes {
        control.check(IndexWorkStage::SymbolParsing)?;
        let selector = match node.kind {
            NodeKind::Folder => EntitySelector::Folder {
                path: RepositoryNodePath::new(Path::new(&node.path))
                    .map_err(invalid_graph_contract)?,
            },
            NodeKind::File => EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&node.path))
                    .map_err(invalid_graph_contract)?,
            },
        };
        let entity =
            GraphEntity::new(project, selector, generation).map_err(invalid_graph_contract)?;
        entity_bytes = entity_bytes.saturating_add(entity_retained_bytes(&entity));
        if node.kind == NodeKind::File {
            for key in [
                document_file_resolution_key(project, &node.path)?,
                document_casefold_resolution_key(project, &node.path)?,
            ] {
                entity_exports.push(
                    EntityResolutionKey::new(entity.key().clone(), key)
                        .map_err(invalid_graph_contract)?,
                );
                entity_bytes = entity_bytes.saturating_add(STAGED_GRAPH_ROW_BYTES);
            }
        }
        insert_entity(&mut entity_by_digest, entity)?;
    }

    let mut owners_by_graph = BTreeMap::new();
    let mut keys_by_graph = BTreeMap::new();
    let mut retained_bytes = 0_u64;
    let mut admitted_map_bytes = admitted_projections
        .as_deref()
        .map(resolution_projection_map_retained_bytes)
        .unwrap_or_default();
    enforce_resolution_registry_budget_with_limit(
        admitted_map_bytes.saturating_add(entity_bytes),
        staging_limit,
    )?;
    #[cfg(test)]
    let mut peak_retained_bytes = admitted_map_bytes.saturating_add(entity_bytes);
    #[cfg(test)]
    let mut projection_removals_before_entities = Vec::new();
    for graph in graphs {
        let graph = graph.borrow();
        control.check(IndexWorkStage::SymbolParsing)?;
        let resolution = if let Some(projections) = admitted_projections.as_deref_mut() {
            let resolution = projections.remove(&graph.path).ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "resolution keys were not admitted for graph {}",
                    graph.path
                ))
            })?;
            admitted_map_bytes = admitted_map_bytes.saturating_sub(
                resolution_projection_map_entry_retained_bytes(&graph.path, &resolution),
            );
            #[cfg(test)]
            projection_removals_before_entities.push(graph.path.clone());
            resolution
        } else {
            resolution_projection_with_config(
                project,
                packages.package_name(&graph.path),
                graph,
                configured_modules,
            )?
        };
        let resolution_bytes = resolution_retained_bytes(&resolution);
        entity_bytes = entity_bytes.saturating_add(resolution_bytes);
        #[cfg(test)]
        {
            peak_retained_bytes =
                peak_retained_bytes.max(admitted_map_bytes.saturating_add(entity_bytes));
        }
        enforce_resolution_registry_budget_with_limit(
            admitted_map_bytes.saturating_add(entity_bytes),
            staging_limit,
        )?;
        let file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&graph.path))
                    .map_err(invalid_graph_contract)?,
            },
            generation,
        )
        .map_err(invalid_graph_contract)?;
        let file_digest = file.key().digest().to_string();
        entity_bytes = entity_bytes.saturating_add(entity_retained_bytes(&file));
        insert_entity(&mut entity_by_digest, file)?;
        let mut symbol_digests = Vec::with_capacity(graph.symbols.len());
        entity_bytes = entity_bytes.saturating_add(
            STAGED_GRAPH_ROW_BYTES
                .saturating_mul(u64::try_from(graph.symbols.len()).unwrap_or(u64::MAX)),
        );
        let qualified_parents = qualified_symbol_parents(graph)?;
        for (symbol, qualified_parent) in graph.symbols.iter().zip(qualified_parents) {
            control.check(IndexWorkStage::SymbolParsing)?;
            let entity = match symbol.kind {
                SymbolKind::Import | SymbolKind::Dependency | SymbolKind::Workspace => None,
                SymbolKind::Package => Some(
                    GraphEntity::new(
                        project,
                        EntitySelector::Package {
                            package: PackageSelector {
                                manager: GraphIdentityText::new(CARGO_PACKAGE_MANAGER)
                                    .map_err(invalid_graph_contract)?,
                                name: GraphIdentityText::new(symbol.name.clone())
                                    .map_err(invalid_graph_contract)?,
                                manifest: RepositoryFilePath::new(Path::new(&graph.path))
                                    .map_err(invalid_graph_contract)?,
                            },
                        },
                        generation,
                    )
                    .map_err(invalid_graph_contract)?,
                ),
                _ => Some(
                    GraphEntity::new(
                        project,
                        EntitySelector::Symbol {
                            symbol: SymbolSelector {
                                file: RepositoryFilePath::new(Path::new(&graph.path))
                                    .map_err(invalid_graph_contract)?,
                                name: GraphIdentityText::new(symbol.name.clone())
                                    .map_err(invalid_graph_contract)?,
                                kind: symbol.kind,
                                parent: qualified_parent,
                                signature: GraphIdentityText::new(
                                    if symbol.signature.trim().is_empty() {
                                        symbol.name.clone()
                                    } else {
                                        symbol.signature.trim().to_string()
                                    },
                                )
                                .map_err(invalid_graph_contract)?,
                            },
                        },
                        generation,
                    )
                    .map_err(invalid_graph_contract)?,
                ),
            };
            let entity_digest = entity
                .as_ref()
                .map(|entity| entity.key().digest().to_string());
            if let Some(entity) = entity {
                entity_bytes = entity_bytes.saturating_add(entity_retained_bytes(&entity));
                if symbol.kind == SymbolKind::Heading {
                    entity_exports.push(
                        EntityResolutionKey::new(
                            entity.key().clone(),
                            document_heading_resolution_key(
                                project,
                                &graph.path,
                                &symbol.signature,
                            )?,
                        )
                        .map_err(invalid_graph_contract)?,
                    );
                    entity_bytes = entity_bytes.saturating_add(STAGED_GRAPH_ROW_BYTES);
                }
                insert_entity(&mut entity_by_digest, entity)?;
            }
            symbol_digests.push(entity_digest);
        }
        let file = entity_by_digest
            .get(&file_digest)
            .ok_or_else(|| CliError::InvalidInput("graph file owner was not staged".to_string()))?;
        for key in resolution.source_keys() {
            entity_exports.push(
                EntityResolutionKey::new(file.key().clone(), key.clone())
                    .map_err(invalid_graph_contract)?,
            );
            entity_bytes = entity_bytes.saturating_add(STAGED_GRAPH_ROW_BYTES);
        }
        for symbol_keys in resolution.symbol_keys() {
            let Some(entity) = symbol_digests
                .get(symbol_keys.symbol_index())
                .and_then(Option::as_ref)
                .and_then(|digest| entity_by_digest.get(digest))
            else {
                continue;
            };
            for key in symbol_keys.keys() {
                entity_exports.push(
                    EntityResolutionKey::new(entity.key().clone(), key.clone())
                        .map_err(invalid_graph_contract)?,
                );
                entity_bytes = entity_bytes.saturating_add(STAGED_GRAPH_ROW_BYTES);
            }
        }
        entity_bytes = entity_bytes
            .saturating_add(STAGED_GRAPH_ROW_BYTES)
            .saturating_add(graph.path.len() as u64);
        #[cfg(test)]
        {
            peak_retained_bytes =
                peak_retained_bytes.max(admitted_map_bytes.saturating_add(entity_bytes));
        }
        enforce_resolution_registry_budget_with_limit(
            admitted_map_bytes.saturating_add(entity_bytes),
            staging_limit,
        )?;
        retained_bytes = retained_bytes.saturating_add(resolution_retained_bytes(&resolution));
        owners_by_graph.insert(
            graph.path.clone(),
            GraphOwners {
                file_digest,
                symbol_digests,
            },
        );
        keys_by_graph.insert(graph.path.clone(), resolution);
    }
    sort_dedup_exports(&mut entity_exports);
    enforce_key_binding_limit(entity_exports.len())?;
    retained_bytes = entity_by_digest
        .values()
        .fold(retained_bytes, |bytes, entity| {
            bytes.saturating_add(entity_retained_bytes(entity))
        })
        .saturating_add(
            STAGED_GRAPH_ROW_BYTES
                .saturating_mul(u64::try_from(entity_exports.len()).unwrap_or(u64::MAX)),
        );
    #[cfg(test)]
    {
        peak_retained_bytes = peak_retained_bytes.max(retained_bytes);
    }
    enforce_resolution_registry_budget_with_limit(retained_bytes, staging_limit)?;
    Ok(EntityProjection {
        entity_by_digest,
        owners_by_graph,
        keys_by_graph,
        entity_exports,
        retained_bytes,
        #[cfg(test)]
        peak_retained_bytes,
        #[cfg(test)]
        projection_removals_before_entities,
    })
}

/// Acquire the one project-local graph-staging lease without waiting.
fn try_graph_stage_lease(staging_parent: &Path) -> Result<Option<File>, CliError> {
    let path = staging_parent.join(GRAPH_STAGE_LEASE_FILE_NAME);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.file_type().is_file() || metadata.file_type().is_symlink())
    {
        return Err(CliError::InvalidInput(format!(
            "repository graph staging lease is not a direct file: {}",
            normalize_native_path_display(&path)
        )));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(source)) => Err(CliError::Io { path, source }),
    }
}

/// Remove inactive disposable graph stages owned by the selected project.
pub(super) fn cleanup_abandoned_repository_graph_staging(
    store: &AtlasStore,
    root: &Path,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    cleanup_abandoned_graph_staging(root, selected_project(store)?, control)
}

/// Remove only validated, inactive disposable graph stages left by an earlier process.
fn cleanup_abandoned_graph_staging(
    root: &Path,
    project: ProjectInstanceId,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    control.check(IndexWorkStage::Publication)?;
    let staging_parent = root.join(".projectatlas");
    if !staging_parent.is_dir() {
        return Ok(());
    }
    let Some(_lease) = try_graph_stage_lease(&staging_parent)? else {
        return Ok(());
    };
    cleanup_abandoned_graph_staging_while_locked(&staging_parent, root, project, control)
}

/// Remove stage payloads while retaining the validated ownership database as a crash marker.
fn remove_owned_graph_stage_payload(
    stage: &Path,
    database_path: &Path,
    control: Option<&IndexWorkControl>,
) -> Result<(), CliError> {
    let entries = fs::read_dir(stage).map_err(|source| CliError::Io {
        path: stage.to_path_buf(),
        source,
    })?;
    for entry in entries {
        if let Some(control) = control {
            control.check(IndexWorkStage::Publication)?;
        }
        let entry = entry.map_err(|source| CliError::Io {
            path: stage.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path == database_path {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })?;
        let result = if metadata.file_type().is_symlink() {
            remove_graph_stage_symlink(&path)
        } else if metadata.file_type().is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

/// Remove only a graph-stage symlink leaf, never its target.
#[cfg(windows)]
fn remove_graph_stage_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path).or_else(|_directory_error| fs::remove_file(path))
}

/// Remove only a graph-stage symlink leaf, never its target.
#[cfg(not(windows))]
fn remove_graph_stage_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

/// Remove direct child stages whose typed database binds the exact project.
fn cleanup_abandoned_graph_staging_while_locked(
    staging_parent: &Path,
    root: &Path,
    project: ProjectInstanceId,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let entries = fs::read_dir(staging_parent).map_err(|source| CliError::Io {
        path: staging_parent.to_path_buf(),
        source,
    })?;
    for entry in entries {
        control.check(IndexWorkStage::Publication)?;
        let entry = entry.map_err(|source| CliError::Io {
            path: staging_parent.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.starts_with(GRAPH_STAGE_DIRECTORY_PREFIX)
            || file_name.len() == GRAPH_STAGE_DIRECTORY_PREFIX.len()
        {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let database_path = path.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let database_metadata = match fs::symlink_metadata(&database_path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let _remove_empty_shell = fs::remove_dir(&path);
                continue;
            }
            Err(_) => continue,
        };
        if !database_metadata.file_type().is_file() || database_metadata.file_type().is_symlink() {
            continue;
        }
        let owned = AtlasStore::repository_graph_staging_belongs_to(&database_path, root, project)
            .unwrap_or(false);
        if !owned {
            continue;
        }
        control.check(IndexWorkStage::Publication)?;
        remove_owned_graph_stage_payload(&path, &database_path, Some(control))?;
        control.check(IndexWorkStage::Publication)?;
        fs::remove_file(&database_path).map_err(|source| CliError::Io {
            path: database_path,
            source,
        })?;
        fs::remove_dir(&path).map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

/// Spill a large full graph to typed disposable `SQLite` rows before main publication.
fn finish_projection_in_database_with_documents(
    root: &Path,
    nodes: &[Node],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    document_facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
    identity_admission: &GraphIdentityAdmission,
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    #[cfg(test)]
    let peak_retained_bytes = entities.peak_retained_bytes;
    #[cfg(test)]
    let projection_removals_before_entities = entities.projection_removals_before_entities.clone();
    let document_index = DocumentResolutionIndex::new(root, nodes, scan_policy)?;
    let staging_parent = root.join(".projectatlas");
    fs::create_dir_all(&staging_parent).map_err(|source| CliError::Io {
        path: staging_parent.clone(),
        source,
    })?;
    let lease = try_graph_stage_lease(&staging_parent)?.ok_or_else(|| {
        CliError::InvalidInput(
            "another repository graph staging operation is active for this project".to_string(),
        )
    })?;
    cleanup_abandoned_graph_staging_while_locked(&staging_parent, root, project, control)?;
    let directory = TempDirBuilder::new()
        .prefix(GRAPH_STAGE_DIRECTORY_PREFIX)
        .tempdir_in(&staging_parent)
        .map_err(|source| CliError::Io {
            path: staging_parent,
            source,
        })?;
    let mut database = StagedGraphDatabase {
        store: None,
        directory: Some(directory),
        _lease: lease,
    };
    let database_path = database
        .directory()?
        .path()
        .join(GRAPH_STAGE_DATABASE_FILE_NAME);
    database.store = Some(AtlasStore::create_repository_graph_staging(
        &database_path,
        root,
        project,
    )?);
    database.store_mut()?.replace_scan(nodes)?;
    let mut identity_rejections = identity_admission.rejections.clone();
    let mut identity_rejection_keys = identity_rejection_key_set(&identity_rejections);
    let mut identity_rejection_bytes =
        identity_rejection_keys_retained_bytes(&identity_rejection_keys)?
            .checked_add(identity_rejection_drop_paths_retained_bytes(
                &identity_admission.rejection_details_dropped_by_path,
            )?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
    let mut rejection_details_dropped_by_path =
        identity_admission.rejection_details_dropped_by_path.clone();
    {
        let mut staging = database
            .store_mut()?
            .begin_repository_graph_staging(project, generation)?;
        let mut entity_batch = Vec::with_capacity(GRAPH_STAGE_ENTITY_BATCH_SIZE);
        for entity in entities.entity_by_digest.values() {
            entity_batch.push(entity);
            if entity_batch.len() == GRAPH_STAGE_ENTITY_BATCH_SIZE {
                control.check(IndexWorkStage::Publication)?;
                staging.append_entity_refs(&entity_batch)?;
                entity_batch.clear();
            }
        }
        if !entity_batch.is_empty() {
            staging.append_entity_refs(&entity_batch)?;
            entity_batch.clear();
        }
        staging.append_batch(&[], &[], &[], &[], &entities.entity_exports, &[])?;
        let mut staged_rows = ProjectedGraphRows::default();
        for graph in graphs {
            let graph = graph.borrow();
            let rows = project_graph_rows(
                project,
                generation,
                graph,
                document_facts.get(&graph.path).map(Cow::as_ref),
                &document_index,
                &mut entities,
                candidates,
                identity_admission,
                control,
            )?;
            staged_rows.append(rows);
            identity_rejection_bytes = identity_rejection_bytes
                .checked_add(extend_bounded_identity_rejections_with_drop_paths(
                    &mut identity_rejections,
                    &mut identity_rejection_keys,
                    staged_rows.identity_rejections.iter().cloned(),
                    &mut rejection_details_dropped_by_path,
                )?)
                .ok_or_else(|| {
                    CliError::InvalidInput("identity rejection bytes overflowed".to_string())
                })?;
            mark_identity_rejection_coverage_limits(
                &mut staged_rows.coverage,
                &rejection_details_dropped_by_path,
            )?;
            staged_rows.identity_rejections.clear();
            if staged_rows.row_count() < GRAPH_STAGE_ROW_BATCH_SIZE {
                continue;
            }
            staging.append_batch(
                &staged_rows.external_entities,
                &staged_rows.relations,
                &staged_rows.occurrences,
                &staged_rows.coverage,
                &[],
                &staged_rows.relation_dependencies,
            )?;
            if !staged_rows.document_unresolved_reasons.is_empty() {
                staging.set_document_unresolved_reasons_controlled(
                    &staged_rows.document_unresolved_reasons,
                    control,
                )?;
            }
            staged_rows.clear();
        }
        if !staged_rows.is_empty() {
            staging.append_batch(
                &staged_rows.external_entities,
                &staged_rows.relations,
                &staged_rows.occurrences,
                &staged_rows.coverage,
                &[],
                &staged_rows.relation_dependencies,
            )?;
            if !staged_rows.document_unresolved_reasons.is_empty() {
                staging.set_document_unresolved_reasons_controlled(
                    &staged_rows.document_unresolved_reasons,
                    control,
                )?;
            }
        }
        staging.complete()?;
    }
    database.store()?.checkpoint_repository_graph_staging()?;
    database.store()?.begin_index_read_snapshot()?;
    let _staged_generation = database.store()?.repository_graph_generation()?;
    let document_target_states = document_index.observed_absent_states();
    let retained_bytes = database_path.as_os_str().as_encoded_bytes().len() as u64
        + document_target_states
            .iter()
            .map(|(path, _reason)| path.len() as u64 + STAGED_GRAPH_ROW_BYTES)
            .sum::<u64>()
        + identity_rejection_bytes;
    Ok(StagedRepositoryGraph {
        project,
        mutation: RepositoryGraphMutation::Full,
        entities: Vec::new(),
        relations: Vec::new(),
        occurrences: Vec::new(),
        coverage: Vec::new(),
        entity_exports: Vec::new(),
        relation_dependencies: Vec::new(),
        document_unresolved_reasons: Vec::new(),
        identity_rejections,
        #[cfg(test)]
        resolution_derivations: identity_admission.resolution_derivations.clone(),
        #[cfg(test)]
        peak_retained_bytes,
        #[cfg(test)]
        projection_removals_before_entities,
        scan_policy: scan_policy.clone(),
        document_target_states,
        database: Some(database),
        retained_bytes,
    })
}

/// Test-only compatibility wrapper for graph fixtures without Markdown facts.
#[cfg(test)]
fn finish_projection_in_database(
    root: &Path,
    nodes: &[Node],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    finish_projection_in_database_with_documents(
        root,
        nodes,
        project,
        generation,
        graphs,
        &BTreeMap::new(),
        &GraphIdentityAdmission::default(),
        entities,
        candidates,
        scan_policy,
        control,
    )
}

/// Resolve staged relationships and finish one complete normalized graph batch.
fn finish_projection_with_documents(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    mutation: RepositoryGraphMutation,
    graphs: &[impl Borrow<SymbolGraph>],
    root: &Path,
    nodes: &[Node],
    document_facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
    identity_admission: &GraphIdentityAdmission,
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    #[cfg(test)]
    let peak_retained_bytes = entities.peak_retained_bytes;
    #[cfg(test)]
    let projection_removals_before_entities = entities.projection_removals_before_entities.clone();
    let document_index = DocumentResolutionIndex::new(root, nodes, scan_policy)?;
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut coverage = Vec::new();
    let mut document_unresolved_reasons = BTreeMap::new();
    let mut identity_rejections = identity_admission.rejections.clone();
    let mut identity_rejection_keys = identity_rejection_key_set(&identity_rejections);
    let mut identity_rejection_bytes =
        identity_rejection_keys_retained_bytes(&identity_rejection_keys)?
            .checked_add(identity_rejection_drop_paths_retained_bytes(
                &identity_admission.rejection_details_dropped_by_path,
            )?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
    let mut rejection_details_dropped_by_path =
        identity_admission.rejection_details_dropped_by_path.clone();
    for graph in graphs {
        let graph = graph.borrow();
        let rows = project_graph_rows(
            project,
            generation,
            graph,
            document_facts.get(&graph.path).map(Cow::as_ref),
            &document_index,
            &mut entities,
            candidates,
            identity_admission,
            control,
        )?;
        for (relation_index, relation) in rows.relations.into_iter().enumerate() {
            insert_relation(
                &mut relations_by_digest,
                relation,
                &graph.path,
                "projected",
                relation_index,
            )?;
        }
        occurrences.extend(rows.occurrences);
        relation_dependencies.extend(rows.relation_dependencies);
        for (key, reason) in rows.document_unresolved_reasons {
            insert_document_unresolved_reason(&mut document_unresolved_reasons, key, reason)?;
        }
        coverage.extend(rows.coverage);
        identity_rejection_bytes = identity_rejection_bytes
            .checked_add(extend_bounded_identity_rejections_with_drop_paths(
                &mut identity_rejections,
                &mut identity_rejection_keys,
                rows.identity_rejections,
                &mut rejection_details_dropped_by_path,
            )?)
            .ok_or_else(|| {
                CliError::InvalidInput("identity rejection bytes overflowed".to_string())
            })?;
        mark_identity_rejection_coverage_limits(&mut coverage, &rejection_details_dropped_by_path)?;
        entities.retained_bytes = rows
            .external_entities
            .iter()
            .fold(entities.retained_bytes, |bytes, entity| {
                bytes.saturating_add(entity_retained_bytes(entity))
            });
        for entity in rows.external_entities {
            insert_entity(&mut entities.entity_by_digest, entity)?;
        }
    }
    let mut relations = relations_by_digest.into_values().collect::<Vec<_>>();
    relations.sort_by(|left, right| left.key().digest().cmp(right.key().digest()));
    occurrences.sort_by(|left, right| {
        left.relation()
            .digest()
            .cmp(right.relation().digest())
            .then_with(|| left.file().as_str().cmp(right.file().as_str()))
            .then_with(|| left.span().start_line().cmp(&right.span().start_line()))
            .then_with(|| left.span().start_column().cmp(&right.span().start_column()))
    });
    occurrences.dedup();
    sort_dedup_dependencies(&mut relation_dependencies);
    enforce_key_binding_limit(relation_dependencies.len())?;
    let added_rows = relations
        .len()
        .saturating_add(occurrences.len())
        .saturating_add(coverage.len())
        .saturating_add(relation_dependencies.len());
    let added_rows = added_rows.saturating_add(document_unresolved_reasons.len());
    let document_target_states = document_index.observed_absent_states();
    entities.retained_bytes = entities
        .retained_bytes
        .saturating_add(
            STAGED_GRAPH_ROW_BYTES.saturating_mul(u64::try_from(added_rows).unwrap_or(u64::MAX)),
        )
        .saturating_add(
            document_target_states
                .iter()
                .fold(0_u64, |bytes, (path, _reason)| {
                    bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES)
                        .saturating_add(path.len() as u64)
                }),
        );
    Ok(StagedRepositoryGraph {
        project,
        mutation,
        entities: entities.entity_by_digest.into_values().collect(),
        relations,
        occurrences,
        coverage,
        entity_exports: entities.entity_exports,
        relation_dependencies,
        document_unresolved_reasons: document_unresolved_reasons.into_values().collect(),
        identity_rejections,
        #[cfg(test)]
        resolution_derivations: identity_admission.resolution_derivations.clone(),
        #[cfg(test)]
        peak_retained_bytes,
        #[cfg(test)]
        projection_removals_before_entities,
        scan_policy: scan_policy.clone(),
        document_target_states,
        database: None,
        retained_bytes: entities
            .retained_bytes
            .saturating_add(identity_rejection_bytes),
    })
}

/// Test-only compatibility wrapper for graph fixtures without Markdown facts.
#[cfg(test)]
fn finish_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    mutation: RepositoryGraphMutation,
    graphs: &[impl Borrow<SymbolGraph>],
    entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let scan_policy = RootScanPolicy::discover(Path::new("."), &ScanOptions::default(), control)
        .map_err(|source| CliError::InvalidInput(source.to_string()))?;
    finish_projection_with_documents(
        project,
        generation,
        mutation,
        graphs,
        Path::new("."),
        &[],
        &BTreeMap::new(),
        &GraphIdentityAdmission::default(),
        entities,
        candidates,
        &scan_policy,
        control,
    )
}

/// One graph's normalized rows, bounded by the parser graph already in memory.
#[derive(Default)]
struct ProjectedGraphRows {
    /// Deduplicated logical relations owned by the graph.
    relations: Vec<LogicalRelation>,
    /// Exact supporting source occurrences.
    occurrences: Vec<RelationOccurrence>,
    /// Coverage rows for the graph.
    coverage: Vec<CoverageRecord>,
    /// Content-free external entities referenced by the graph.
    external_entities: Vec<GraphEntity>,
    /// Canonical dependency keys retained by the graph relations.
    relation_dependencies: Vec<RelationDependencyKey>,
    /// Closed reasons for unresolved canonical document relations.
    document_unresolved_reasons: Vec<(LogicalRelationKey, DocumentTargetUnresolvedReason)>,
    /// Derived relation identity rejections attached to this graph.
    identity_rejections: Vec<GraphIdentityRejection>,
}

impl ProjectedGraphRows {
    /// Append one graph while retaining a single bounded staging batch.
    fn append(&mut self, rows: Self) {
        self.relations.extend(rows.relations);
        self.occurrences.extend(rows.occurrences);
        self.coverage.extend(rows.coverage);
        self.external_entities.extend(rows.external_entities);
        self.relation_dependencies
            .extend(rows.relation_dependencies);
        self.document_unresolved_reasons
            .extend(rows.document_unresolved_reasons);
        self.identity_rejections.extend(rows.identity_rejections);
    }

    /// Return aggregate normalized rows retained by this staging batch.
    fn row_count(&self) -> usize {
        self.relations
            .len()
            .saturating_add(self.occurrences.len())
            .saturating_add(self.coverage.len())
            .saturating_add(self.external_entities.len())
            .saturating_add(self.relation_dependencies.len())
            .saturating_add(self.document_unresolved_reasons.len())
    }

    /// Return whether the staging batch is empty.
    fn is_empty(&self) -> bool {
        self.row_count() == 0
    }

    /// Release all successfully staged rows while preserving allocated capacity.
    fn clear(&mut self) {
        self.relations.clear();
        self.occurrences.clear();
        self.coverage.clear();
        self.external_entities.clear();
        self.relation_dependencies.clear();
        self.document_unresolved_reasons.clear();
        self.identity_rejections.clear();
    }
}

/// Exact staged file inventory plus platform-aware case and root checks.
struct DocumentResolutionIndex<'a> {
    /// Selected source root used only for bounded filesystem checks.
    root: PathBuf,
    /// Canonical selected root used to reject symlink escape.
    canonical_root: PathBuf,
    /// Exact case-preserving staged identities.
    kinds_by_path: HashMap<&'a str, NodeKind>,
    /// Unicode-lowercase identity counts used to refuse guesses and collisions.
    casefold_path_counts: HashMap<String, u32>,
    /// Effective repository admission policy shared with full and incremental scans.
    scan_policy: &'a RootScanPolicy,
    /// Consulted non-indexed states, memoized for repeated high-fan-out references.
    absent_states: RefCell<BTreeMap<String, DocumentTargetUnresolvedReason>>,
}

impl<'a> DocumentResolutionIndex<'a> {
    /// Build one bounded exact-identity view from the staged source inventory.
    fn new(
        root: &Path,
        nodes: &'a [Node],
        scan_policy: &'a RootScanPolicy,
    ) -> Result<Self, CliError> {
        let canonical_root = fs::canonicalize(root).map_err(|source| CliError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let kinds_by_path = nodes
            .iter()
            .map(|node| (node.path.as_str(), node.kind))
            .collect::<HashMap<_, _>>();
        let mut casefold_path_counts = HashMap::new();
        for node in nodes {
            let count = casefold_path_counts
                .entry(node.path.to_lowercase())
                .or_insert(0_u32);
            *count = count.saturating_add(1);
        }
        let retained_bytes = nodes.iter().fold(0_u64, |bytes, node| {
            bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES.saturating_mul(2))
                .saturating_add(node.path.len() as u64)
        });
        enforce_resolution_registry_budget(retained_bytes)?;
        Ok(Self {
            root: root.to_path_buf(),
            canonical_root,
            kinds_by_path,
            casefold_path_counts,
            scan_policy,
            absent_states: RefCell::new(BTreeMap::new()),
        })
    }

    /// Return a closed unresolved reason, or `None` for one exact admitted file.
    fn unresolved_reason(
        &self,
        path: &str,
    ) -> Result<Option<DocumentTargetUnresolvedReason>, CliError> {
        let casefold_count = self
            .casefold_path_counts
            .get(&path.to_lowercase())
            .copied()
            .unwrap_or(0);
        if casefold_count > 1 {
            return Ok(Some(DocumentTargetUnresolvedReason::CaseConflict));
        }
        Ok(match self.kinds_by_path.get(path).copied() {
            Some(NodeKind::File) => None,
            Some(NodeKind::Folder) => Some(DocumentTargetUnresolvedReason::Unsupported),
            None if casefold_count == 1 => Some(DocumentTargetUnresolvedReason::CaseConflict),
            None => Some(self.absent_reason(path)?),
        })
    }

    /// Distinguish absent source from excluded or escaping filesystem state.
    fn absent_reason(&self, path: &str) -> Result<DocumentTargetUnresolvedReason, CliError> {
        if let Some(reason) = self.absent_states.borrow().get(path).copied() {
            return Ok(reason);
        }
        let native = self.root.join(Path::new(path));
        let reason = if self
            .scan_policy
            .excludes_path(&native)
            .map_err(|source| CliError::InvalidInput(source.to_string()))?
        {
            DocumentTargetUnresolvedReason::Ignored
        } else {
            match fs::symlink_metadata(&native) {
                Ok(metadata) if metadata.file_type().is_dir() => {
                    DocumentTargetUnresolvedReason::Unsupported
                }
                Ok(_metadata) => match fs::canonicalize(&native) {
                    Ok(canonical) if !canonical.starts_with(&self.canonical_root) => {
                        DocumentTargetUnresolvedReason::OutsideRoot
                    }
                    Ok(_canonical) => DocumentTargetUnresolvedReason::Unsupported,
                    Err(_source) => DocumentTargetUnresolvedReason::Unsupported,
                },
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    self.missing_or_escaping_ancestor(&native)
                }
                Err(_source) => DocumentTargetUnresolvedReason::Unsupported,
            }
        };
        self.absent_states
            .borrow_mut()
            .insert(path.to_string(), reason);
        Ok(reason)
    }

    /// Return the bounded states actually consulted during document resolution.
    fn observed_absent_states(&self) -> Vec<(String, DocumentTargetUnresolvedReason)> {
        self.absent_states
            .borrow()
            .iter()
            .map(|(path, reason)| (path.clone(), *reason))
            .collect()
    }

    /// Check the nearest existing ancestor so a missing child cannot hide symlink escape.
    fn missing_or_escaping_ancestor(&self, native: &Path) -> DocumentTargetUnresolvedReason {
        let mut ancestor = native.parent();
        while let Some(path) = ancestor {
            if !path.starts_with(&self.root) {
                break;
            }
            match fs::canonicalize(path) {
                Ok(canonical) if !canonical.starts_with(&self.canonical_root) => {
                    return DocumentTargetUnresolvedReason::OutsideRoot;
                }
                Ok(_canonical) => return DocumentTargetUnresolvedReason::Missing,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = path.parent();
                }
                Err(_source) => return DocumentTargetUnresolvedReason::Unsupported,
            }
        }
        DocumentTargetUnresolvedReason::Missing
    }
}

/// One resolved or typed-unresolved document candidate plus invalidation keys.
struct DocumentResolutionOutcome {
    /// Existing normalized graph resolution envelope.
    resolution: RelationResolution,
    /// Closed reason required only for unresolved document relations.
    unresolved_reason: Option<DocumentTargetUnresolvedReason>,
    /// Exact file and optional heading keys that invalidate this relation.
    dependencies: Vec<CanonicalResolutionKey>,
}

/// Project one Markdown fact batch into canonical document relations and coverage.
#[allow(clippy::too_many_arguments)]
fn project_document_rows(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graph: &SymbolGraph,
    facts: &MarkdownFacts,
    owners: &GraphOwners,
    document_index: &DocumentResolutionIndex<'_>,
    candidates: &ProjectResolutionRegistry,
    staged_entities: &BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ProjectedGraphRows, CliError> {
    let file_source = staged_entities.get(&owners.file_digest).ok_or_else(|| {
        CliError::InvalidInput("document graph file owner was not staged".to_string())
    })?;
    let completeness = match facts.coverage.completeness {
        MarkdownFactCompleteness::Complete => Completeness::Complete,
        MarkdownFactCompleteness::Partial => Completeness::Partial,
    };
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut unresolved_reasons = BTreeMap::new();
    for (candidate_index, candidate) in facts.link_candidates.iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        if normalize_document_target(&graph.path, &candidate.selector)
            .is_ok_and(|target| target.path == graph.path && target.fragment.is_none())
        {
            continue;
        }
        let source = file_source;
        let outcome = resolve_document_candidate(
            project,
            &graph.path,
            &candidate.selector,
            document_index,
            candidates,
            staged_entities,
            control,
        )?;
        if matches!(
            &outcome.resolution,
            RelationResolution::Resolved { target, .. } if target == source.key()
        ) {
            continue;
        }
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            outcome.resolution,
            ConfidenceClass::High,
            completeness,
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "document",
            candidate_index,
        )?;
        if let Some(reason) = outcome.unresolved_reason {
            insert_document_unresolved_reason(
                &mut unresolved_reasons,
                relation.key().clone(),
                reason,
            )?;
        }
        let start_line = u32::try_from(candidate.source.line_start).map_err(|error| {
            CliError::InvalidInput(format!("document source line exceeds graph range: {error}"))
        })?;
        let end_line = u32::try_from(candidate.source.line_end).map_err(|error| {
            CliError::InvalidInput(format!("document end line exceeds graph range: {error}"))
        })?;
        let start_column = u32::try_from(candidate.source.column_start).map_err(|error| {
            CliError::InvalidInput(format!(
                "document source column exceeds graph range: {error}"
            ))
        })?;
        let end_column = u32::try_from(candidate.source.column_end).map_err(|error| {
            CliError::InvalidInput(format!("document end column exceeds graph range: {error}"))
        })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(start_line, start_column, end_line, end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        for key in outcome.dependencies {
            relation_dependencies.push(
                RelationDependencyKey::new(relation.key().clone(), key)
                    .map_err(invalid_graph_contract)?,
            );
        }
    }
    Ok(ProjectedGraphRows {
        relations: relations_by_digest.into_values().collect(),
        occurrences,
        coverage: vec![document_coverage(&graph.path, facts, generation)?],
        external_entities: Vec::new(),
        relation_dependencies,
        document_unresolved_reasons: unresolved_reasons.into_values().collect(),
        identity_rejections: Vec::new(),
    })
}

/// Resolve one normalized document target through exact staged/persisted identities.
#[allow(clippy::too_many_arguments)]
fn resolve_document_candidate(
    project: ProjectInstanceId,
    document_path: &str,
    selector: &str,
    document_index: &DocumentResolutionIndex<'_>,
    candidates: &ProjectResolutionRegistry,
    staged_entities: &BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<DocumentResolutionOutcome, CliError> {
    let target = match normalize_document_target(document_path, selector) {
        Ok(target) => target,
        Err(reason) => {
            return unresolved_document_outcome(selector, reason, Vec::new());
        }
    };
    let file_key = document_file_resolution_key(project, &target.path)?;
    let mut dependencies = vec![
        file_key.clone(),
        document_casefold_resolution_key(project, &target.path)?,
    ];
    let heading_key = target
        .fragment
        .as_deref()
        .map(|fragment| document_heading_resolution_key(project, &target.path, fragment))
        .transpose()?;
    if let Some(key) = heading_key.as_ref() {
        dependencies.push(key.clone());
    }
    if let Some(reason) = document_index.unresolved_reason(&target.path)? {
        return unresolved_document_outcome(selector, reason, dependencies);
    }
    if let Some(key) = heading_key.as_ref() {
        let matches = registry_resolution_matches(
            std::slice::from_ref(key),
            candidates,
            staged_entities,
            control,
        )?;
        match matches.count {
            0 => {
                return unresolved_document_outcome(
                    selector,
                    DocumentTargetUnresolvedReason::Missing,
                    dependencies,
                );
            }
            1 => {
                let target = matches.first.ok_or_else(|| {
                    CliError::InvalidInput("resolved document heading disappeared".to_string())
                })?;
                return Ok(DocumentResolutionOutcome {
                    resolution: RelationResolution::resolved(target)
                        .map_err(invalid_graph_contract)?,
                    unresolved_reason: None,
                    dependencies,
                });
            }
            _ => {
                return unresolved_document_outcome(
                    selector,
                    DocumentTargetUnresolvedReason::CaseConflict,
                    dependencies,
                );
            }
        }
    }
    let matches = registry_resolution_matches(
        std::slice::from_ref(&file_key),
        candidates,
        staged_entities,
        control,
    )?;
    match matches.count {
        1 => Ok(DocumentResolutionOutcome {
            resolution: RelationResolution::resolved(matches.first.ok_or_else(|| {
                CliError::InvalidInput("resolved document file disappeared".to_string())
            })?)
            .map_err(invalid_graph_contract)?,
            unresolved_reason: None,
            dependencies,
        }),
        0 => Err(CliError::InvalidInput(format!(
            "admitted document target lacked its exact graph identity: {}",
            target.path
        ))),
        _ => unresolved_document_outcome(
            selector,
            DocumentTargetUnresolvedReason::CaseConflict,
            dependencies,
        ),
    }
}

/// Construct one privacy-safe unresolved document outcome.
fn unresolved_document_outcome(
    selector: &str,
    reason: DocumentTargetUnresolvedReason,
    dependencies: Vec<CanonicalResolutionKey>,
) -> Result<DocumentResolutionOutcome, CliError> {
    Ok(DocumentResolutionOutcome {
        resolution: RelationResolution::Unresolved {
            reference: GraphIdentityText::new(selector).map_err(invalid_graph_contract)?,
        },
        unresolved_reason: Some(reason),
        dependencies,
    })
}

/// Project bounded Markdown fact coverage into the existing relation coverage table.
fn document_coverage(
    path: &str,
    facts: &MarkdownFacts,
    generation: IndexGeneration,
) -> Result<CoverageRecord, CliError> {
    let scope = CoverageScope::Path {
        path: RepositoryNodePath::new(Path::new(path)).map_err(invalid_graph_contract)?,
    };
    let covered = u64::try_from(facts.link_candidates.len()).unwrap_or(u64::MAX);
    let reached_limit = facts.coverage.limits.first().map(|limit| match limit {
        MarkdownFactLimit::HeadingCount | MarkdownFactLimit::CandidateCount => GraphLimitKind::Rows,
        MarkdownFactLimit::InputBytes
        | MarkdownFactLimit::LabelBytes
        | MarkdownFactLimit::SelectorBytes
        | MarkdownFactLimit::EvidenceBytes => GraphLimitKind::IntermediateBytes,
    });
    let relation = Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents));
    match facts.coverage.completeness {
        MarkdownFactCompleteness::Complete if covered == 0 => CoverageRecord::new(
            scope,
            relation,
            CoverageState::NoCandidates,
            0,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Complete => CoverageRecord::new(
            scope,
            relation,
            CoverageState::Complete,
            covered,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Partial if covered > 0 => CoverageRecord::new(
            scope,
            relation,
            CoverageState::Partial,
            covered,
            1,
            generation,
            Some(
                GraphIdentityText::new(DOCUMENT_PARTIAL_COVERAGE_REASON)
                    .map_err(invalid_graph_contract)?,
            ),
            reached_limit,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Partial => CoverageRecord::new(
            scope,
            relation,
            if facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::InputBytes)
            {
                CoverageState::Oversized
            } else {
                CoverageState::Failed
            },
            0,
            1,
            generation,
            Some(
                GraphIdentityText::new(DOCUMENT_PARTIAL_COVERAGE_REASON)
                    .map_err(invalid_graph_contract)?,
            ),
            reached_limit,
        )
        .map_err(invalid_graph_contract),
    }
}

/// Deduplicate unresolved reasons while rejecting one logical-key contradiction.
fn insert_document_unresolved_reason(
    reasons: &mut BTreeMap<String, (LogicalRelationKey, DocumentTargetUnresolvedReason)>,
    key: LogicalRelationKey,
    reason: DocumentTargetUnresolvedReason,
) -> Result<(), CliError> {
    let digest = key.digest().to_string();
    match reasons.entry(digest) {
        Entry::Vacant(entry) => {
            entry.insert((key, reason));
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == &(key, reason) => Ok(()),
        Entry::Occupied(_entry) => Err(CliError::InvalidInput(
            "document relation retained conflicting unresolved reasons".to_string(),
        )),
    }
}

/// Resolve one parser graph and release its temporary owner/key workspace.
fn project_graph_rows(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graph: &SymbolGraph,
    document_facts: Option<&MarkdownFacts>,
    document_index: &DocumentResolutionIndex<'_>,
    entities: &mut EntityProjection,
    candidates: &ProjectResolutionRegistry,
    identity_admission: &GraphIdentityAdmission,
    control: &IndexWorkControl,
) -> Result<ProjectedGraphRows, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let owners = entities
        .owners_by_graph
        .remove(&graph.path)
        .ok_or_else(|| CliError::InvalidInput("graph owners were not staged".to_string()))?;
    let resolution_keys = entities
        .keys_by_graph
        .remove(&graph.path)
        .ok_or_else(|| CliError::InvalidInput("graph keys were not staged".to_string()))?;
    let symbol_index = GraphSymbolIndex::new(graph, control)?;
    let keys_by_relation = resolution_keys
        .relation_keys()
        .iter()
        .map(|entry| (entry.relation_index(), entry.keys()))
        .collect::<BTreeMap<_, _>>();
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut coverage = Vec::new();
    let mut external_entities = BTreeMap::new();
    let mut document_unresolved_reasons = BTreeMap::new();
    for (relation_index, source_relation) in graph.relations.iter().enumerate() {
        control.check(IndexWorkStage::SymbolParsing)?;
        let source = relation_source(
            &owners,
            &entities.entity_by_digest,
            &symbol_index,
            source_relation,
        )?;
        let dependency_keys = keys_by_relation
            .get(&relation_index)
            .copied()
            .unwrap_or(&[]);
        let resolution = relation_resolution(
            project,
            generation,
            source_relation,
            &owners,
            graph,
            &symbol_index,
            dependency_keys,
            candidates,
            &entities.entity_by_digest,
            &mut external_entities,
            control,
        )?;
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::from_legacy(source_relation.kind),
            resolution,
            relation_confidence(source_relation.parser),
            relation_completeness(source_relation.parser),
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "logical",
            relation_index,
        )?;
        let line = u32::try_from(source_relation.line).map_err(|error| {
            CliError::InvalidInput(format!("relation source line exceeds graph range: {error}"))
        })?;
        let end_column =
            u32::try_from(source_relation.context.chars().count()).map_err(|error| {
                CliError::InvalidInput(format!(
                    "relation source context exceeds graph range: {error}"
                ))
            })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(line.max(1), 0, line.max(1), end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        for key in dependency_keys {
            relation_dependencies.push(
                RelationDependencyKey::new(relation.key().clone(), key.clone())
                    .map_err(invalid_graph_contract)?,
            );
        }
    }
    let mut derived_identity_admission = GraphIdentityAdmission::default();
    let mut admitted_derived_facts = Vec::new();
    for (derived_index, fact) in derived_relation_facts(graph, &keys_by_relation)
        .into_iter()
        .enumerate()
    {
        let mut failures = Vec::new();
        record_identity_failure(
            &mut failures,
            GraphIdentityField::RelationSource,
            &fact.relation.source_name,
        );
        record_identity_failure(
            &mut failures,
            GraphIdentityField::RelationTarget,
            &fact.relation.target_name,
        );
        if failures.is_empty() {
            admitted_derived_facts.push(fact);
        } else {
            derived_identity_admission.record(
                &graph.path,
                IdentitySpan {
                    start_line: fact.relation.line.max(1),
                    start_column: 0,
                    end_line: fact.relation.line.max(1),
                    end_column: 0,
                },
                fact.relation.parser,
                parser_fact_index(DERIVED_RELATION_FACT_INDEX_NAMESPACE, derived_index),
                &failures,
                control,
            )?;
        }
    }
    for (fact_index, fact) in admitted_derived_facts.into_iter().enumerate() {
        control.check(IndexWorkStage::SymbolParsing)?;
        let source = relation_source(
            &owners,
            &entities.entity_by_digest,
            &symbol_index,
            &fact.relation,
        )?;
        let resolution = derived_relation_resolution(
            project,
            generation,
            &fact,
            &owners,
            graph,
            &symbol_index,
            candidates,
            &entities.entity_by_digest,
            &mut external_entities,
            control,
        )?;
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::Extended(fact.kind),
            resolution,
            relation_confidence(fact.relation.parser),
            relation_completeness(fact.relation.parser),
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "derived",
            fact_index,
        )?;
        let line = u32::try_from(fact.relation.line).map_err(|error| {
            CliError::InvalidInput(format!(
                "derived relation source line exceeds graph range: {error}"
            ))
        })?;
        let end_column = u32::try_from(fact.relation.context.chars().count()).map_err(|error| {
            CliError::InvalidInput(format!(
                "derived relation source context exceeds graph range: {error}"
            ))
        })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(line.max(1), 0, line.max(1), end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        if let DerivedRelationTarget::Parser { keys } = fact.target {
            for key in keys {
                relation_dependencies.push(
                    RelationDependencyKey::new(relation.key().clone(), key)
                        .map_err(invalid_graph_contract)?,
                );
            }
        }
    }
    if let Some(facts) = document_facts {
        let rows = project_document_rows(
            project,
            generation,
            graph,
            facts,
            &owners,
            document_index,
            candidates,
            &entities.entity_by_digest,
            control,
        )?;
        for (relation_index, relation) in rows.relations.into_iter().enumerate() {
            insert_relation(
                &mut relations_by_digest,
                relation,
                &graph.path,
                "document",
                relation_index,
            )?;
        }
        occurrences.extend(rows.occurrences);
        coverage.extend(rows.coverage);
        relation_dependencies.extend(rows.relation_dependencies);
        for (key, reason) in rows.document_unresolved_reasons {
            insert_document_unresolved_reason(&mut document_unresolved_reasons, key, reason)?;
        }
    }
    let mut relations = relations_by_digest.into_values().collect::<Vec<_>>();
    relations.sort_by(|left, right| left.key().digest().cmp(right.key().digest()));
    occurrences.sort_by(|left, right| {
        left.relation()
            .digest()
            .cmp(right.relation().digest())
            .then_with(|| left.file().as_str().cmp(right.file().as_str()))
            .then_with(|| left.span().start_line().cmp(&right.span().start_line()))
            .then_with(|| left.span().start_column().cmp(&right.span().start_column()))
    });
    occurrences.dedup();
    sort_dedup_dependencies(&mut relation_dependencies);
    enforce_key_binding_limit(relation_dependencies.len())?;
    entities.retained_bytes = entities
        .retained_bytes
        .saturating_sub(resolution_retained_bytes(&resolution_keys));
    let mut graph_coverage = vec![coverage_for_graph(
        graph,
        generation,
        identity_admission,
        &derived_identity_admission,
    )?];
    graph_coverage.extend(coverage);
    Ok(ProjectedGraphRows {
        relations,
        occurrences,
        coverage: graph_coverage,
        external_entities: external_entities.into_values().collect(),
        relation_dependencies,
        document_unresolved_reasons: document_unresolved_reasons.into_values().collect(),
        identity_rejections: derived_identity_admission.rejections,
    })
}

/// Deduplicate relation occurrences while conservatively retaining ambiguity.
fn insert_relation(
    relations: &mut BTreeMap<String, LogicalRelation>,
    relation: LogicalRelation,
    graph_path: &str,
    relation_kind: &str,
    relation_index: usize,
) -> Result<(), CliError> {
    let digest = relation.key().digest().to_string();
    match relations.entry(digest) {
        Entry::Vacant(entry) => {
            entry.insert(relation);
            Ok(())
        }
        Entry::Occupied(mut entry) => {
            let existing = entry.get();
            if existing == &relation {
                return Ok(());
            }
            let mergeable = existing.key() == relation.key()
                && existing.source() == relation.source()
                && existing.kind() == relation.kind()
                && existing.confidence() == relation.confidence()
                && existing.completeness() == relation.completeness()
                && existing.generation() == relation.generation();
            if mergeable
                && let (
                    RelationResolution::Ambiguous {
                        reference: existing_reference,
                        candidates: existing_candidates,
                    },
                    RelationResolution::Ambiguous {
                        reference: incoming_reference,
                        candidates: incoming_candidates,
                    },
                ) = (existing.resolution(), relation.resolution())
                && existing_reference == incoming_reference
            {
                if incoming_candidates > existing_candidates {
                    entry.insert(relation);
                }
                return Ok(());
            }
            Err(CliError::InvalidInput(format!(
                "{relation_kind} relation digest retained conflicting facts for {graph_path} \
                 relation {relation_index}: existing={existing:?}, incoming={relation:?}"
            )))
        }
    }
}

/// Project-wide stable entities and their canonical resolution-key bindings.
#[derive(Default)]
struct ProjectResolutionRegistry {
    /// Persisted candidates not already owned by the staged entity projection.
    supplemental_entities_by_digest: BTreeMap<String, GraphEntity>,
    /// Canonical keys mapped only to sorted, deduplicated entity digests.
    candidate_digests_by_key: BTreeMap<CanonicalResolutionKey, BTreeSet<String>>,
    /// Conservative peak bytes retained by this temporary resolution registry.
    retained_bytes: u64,
}

impl ProjectResolutionRegistry {
    /// Insert one key-to-entity binding without cloning the entity per exported key.
    fn insert_candidate(
        &mut self,
        key: &CanonicalResolutionKey,
        entity: &GraphEntity,
    ) -> Result<(), CliError> {
        let digest = entity.key().digest().to_string();
        match self.supplemental_entities_by_digest.entry(digest.clone()) {
            Entry::Occupied(entry) if entry.get() != entity => {
                return Err(CliError::InvalidInput(
                    "graph entity digest retained conflicting selectors".to_string(),
                ));
            }
            Entry::Occupied(_entry) => {}
            Entry::Vacant(entry) => {
                self.retained_bytes = self
                    .retained_bytes
                    .saturating_add(entity_retained_bytes(entity))
                    .saturating_add(digest.len() as u64);
                entry.insert(entity.clone());
            }
        }
        self.insert_candidate_binding(key, digest)
    }

    /// Bind an entity already owned by the staged projection without cloning it.
    fn insert_staged_candidate(
        &mut self,
        key: &CanonicalResolutionKey,
        entity: &GraphEntity,
    ) -> Result<(), CliError> {
        self.insert_candidate_binding(key, entity.key().digest().to_string())
    }

    /// Insert one canonical-key binding for an entity owned by either registry.
    fn insert_candidate_binding(
        &mut self,
        key: &CanonicalResolutionKey,
        digest: String,
    ) -> Result<(), CliError> {
        if let Some(existing) = self.candidate_digests_by_key.get_mut(key) {
            if existing.insert(digest.clone()) {
                self.retained_bytes = self
                    .retained_bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(digest.len() as u64);
            }
        } else {
            self.retained_bytes = self
                .retained_bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(key.canonical_identity().len() as u64)
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(digest.len() as u64);
            self.candidate_digests_by_key
                .insert(key.clone(), BTreeSet::from([digest]));
        }
        enforce_resolution_registry_budget(self.retained_bytes)?;
        Ok(())
    }
}

/// Reject a temporary resolution registry before it can exceed publication memory.
fn enforce_resolution_registry_budget(retained_bytes: u64) -> Result<(), CliError> {
    enforce_resolution_registry_budget_with_limit(
        retained_bytes,
        super::MAX_PUBLICATION_STAGING_BYTES,
    )
}

/// Apply the resolution staging bound with an explicit limit for deterministic tests.
fn enforce_resolution_registry_budget_with_limit(
    retained_bytes: u64,
    limit: u64,
) -> Result<(), CliError> {
    if retained_bytes > limit {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::OutputBytes,
            limit,
            retained_bytes,
        )
        .into());
    }
    Ok(())
}

/// Reject the simultaneous entity projection and lookup registry peak.
fn enforce_resolution_staging_budget(
    projection: &EntityProjection,
    registry: &ProjectResolutionRegistry,
) -> Result<(), CliError> {
    enforce_resolution_registry_budget(
        projection
            .retained_bytes
            .saturating_add(registry.retained_bytes),
    )
}

/// Count conservative fixed and variable bytes retained by one graph entity.
fn entity_retained_bytes(entity: &GraphEntity) -> u64 {
    let selector_bytes = match entity.selector() {
        EntitySelector::Project => 0,
        EntitySelector::Folder { path } => path.as_str().len() as u64,
        EntitySelector::File { path } => path.as_str().len() as u64,
        EntitySelector::Package { package } => {
            (package.manager.as_str().len()
                + package.name.as_str().len()
                + package.manifest.as_str().len()) as u64
        }
        EntitySelector::Symbol { symbol } => {
            (symbol.file.as_str().len()
                + symbol.name.as_str().len()
                + symbol
                    .parent
                    .as_ref()
                    .map_or(0, |parent| parent.as_str().len())
                + symbol.signature.as_str().len()) as u64
        }
        EntitySelector::External { external } => {
            (external.system.as_str().len() + external.identity.as_str().len()) as u64
        }
    };
    STAGED_GRAPH_ROW_BYTES
        .saturating_add(entity.key().digest().len() as u64)
        .saturating_add(entity.key().canonical_identity().len() as u64)
        .saturating_add(selector_bytes)
}

/// Build current resolution candidates from newly staged entity exports.
fn resolution_registry_from_exports(
    projection: &EntityProjection,
    control: &IndexWorkControl,
) -> Result<ProjectResolutionRegistry, CliError> {
    let mut candidates = ProjectResolutionRegistry::default();
    for (index, binding) in projection.entity_exports.iter().enumerate() {
        check_graph_work(control, index)?;
        let digest = binding.entity().digest().to_string();
        let entity = projection.entity_by_digest.get(&digest).ok_or_else(|| {
            CliError::InvalidInput("resolution export owner was not staged".to_string())
        })?;
        candidates.insert_staged_candidate(binding.key(), entity)?;
    }
    Ok(candidates)
}

/// Rebind unaffected persisted candidates to the pending graph generation.
fn resolution_registry_from_persisted(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    candidates: Vec<RepositoryResolutionCandidate>,
    replaced_paths: &BTreeSet<String>,
    control: &IndexWorkControl,
) -> Result<ProjectResolutionRegistry, CliError> {
    let mut by_key = ProjectResolutionRegistry::default();
    for (index, candidate) in candidates.into_iter().enumerate() {
        check_graph_work(control, index)?;
        if entity_owner_path(candidate.entity()).is_some_and(|path| replaced_paths.contains(path)) {
            continue;
        }
        let entity = GraphEntity::new(project, candidate.entity().selector().clone(), generation)
            .map_err(invalid_graph_contract)?;
        by_key.insert_candidate(candidate.key(), &entity)?;
    }
    Ok(by_key)
}

/// Merge normalized candidate registries while retaining one entity per digest.
fn merge_resolution_registries(
    target: &mut ProjectResolutionRegistry,
    source: ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let ProjectResolutionRegistry {
        supplemental_entities_by_digest,
        candidate_digests_by_key,
        retained_bytes: _retained_bytes,
    } = source;
    for entity in supplemental_entities_by_digest.into_values() {
        check_graph_work(control, target.supplemental_entities_by_digest.len())?;
        let digest = entity.key().digest().to_string();
        match target.supplemental_entities_by_digest.entry(digest.clone()) {
            Entry::Occupied(entry) if entry.get() != &entity => {
                return Err(CliError::InvalidInput(
                    "resolution candidate entity retained conflicting selectors".to_string(),
                ));
            }
            Entry::Occupied(_entry) => {}
            Entry::Vacant(entry) => {
                target.retained_bytes = target
                    .retained_bytes
                    .saturating_add(entity_retained_bytes(&entity))
                    .saturating_add(digest.len() as u64);
                entry.insert(entity);
            }
        }
    }
    let mut bindings = 0_usize;
    for (key, candidates) in candidate_digests_by_key {
        for digest in candidates {
            check_graph_work(control, bindings)?;
            target.insert_candidate_binding(&key, digest)?;
            bindings = bindings.saturating_add(1);
        }
    }
    Ok(())
}

/// Observe cancellation and deadline state at bounded graph-map intervals.
fn check_graph_work(control: &IndexWorkControl, index: usize) -> Result<(), CliError> {
    if index.is_multiple_of(GRAPH_WORK_CHECK_INTERVAL) {
        control.check(IndexWorkStage::SymbolParsing)?;
    }
    Ok(())
}

/// Return the source-owner path for entities eligible to export graph keys.
fn entity_owner_path(entity: &GraphEntity) -> Option<&str> {
    match entity.selector() {
        EntitySelector::File { path } => Some(path.as_str()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str()),
        EntitySelector::Package { package } => Some(package.manifest.as_str()),
        EntitySelector::Project
        | EntitySelector::Folder { .. }
        | EntitySelector::External { .. } => None,
    }
}

/// Derive additive families only from exact bounded parser facts and path policy.
fn derived_relation_facts(
    graph: &SymbolGraph,
    keys_by_relation: &BTreeMap<usize, &[CanonicalResolutionKey]>,
) -> Vec<DerivedRelationFact> {
    let mut facts = Vec::new();
    let test_path = is_test_path(&graph.path);
    for (index, relation) in graph.relations.iter().enumerate() {
        if test_path && matches!(relation.kind, RelationKind::Imports | RelationKind::Calls) {
            let keys = keys_by_relation
                .get(&index)
                .copied()
                .unwrap_or_default()
                .to_vec();
            push_derived_relation(
                &mut facts,
                ExtendedRelationKind::Tests,
                relation.clone(),
                DerivedRelationTarget::Parser { keys },
            );
        }
        if relation.kind == RelationKind::Calls {
            if let Some(handler) = static_route_handler(relation) {
                push_derived_relation(
                    &mut facts,
                    ExtendedRelationKind::RoutesTo,
                    derived_parser_relation(relation, handler),
                    DerivedRelationTarget::Parser { keys: Vec::new() },
                );
            }
            if let Some(key) = static_environment_key(relation) {
                push_derived_relation(
                    &mut facts,
                    ExtendedRelationKind::Configures,
                    derived_parser_relation(relation, key),
                    DerivedRelationTarget::External {
                        system: ENVIRONMENT_SYSTEM,
                    },
                );
            }
            if let Some(kind) = static_data_access_kind(&relation.target_name)
                && let Some(path) = static_string_argument(&relation.context)
                    .and_then(normalize_static_repository_path)
            {
                push_derived_relation(
                    &mut facts,
                    kind,
                    derived_parser_relation(relation, path),
                    DerivedRelationTarget::RepositoryPath,
                );
            }
        }
    }
    if let Some(identity) = configuration_file_identity(&graph.path) {
        push_derived_relation(
            &mut facts,
            ExtendedRelationKind::Configures,
            file_owned_relation(graph, identity),
            DerivedRelationTarget::External {
                system: CONFIGURATION_SYSTEM,
            },
        );
    }
    if let Some(identity) = deployment_platform_identity(&graph.path) {
        push_derived_relation(
            &mut facts,
            ExtendedRelationKind::Deploys,
            file_owned_relation(graph, identity),
            DerivedRelationTarget::External {
                system: DEPLOYMENT_SYSTEM,
            },
        );
    }
    facts
}

/// Retain one additive fact from the already bounded parser graph.
fn push_derived_relation(
    facts: &mut Vec<DerivedRelationFact>,
    kind: ExtendedRelationKind,
    relation: SymbolRelation,
    target: DerivedRelationTarget,
) {
    facts.push(DerivedRelationFact {
        kind,
        relation,
        target,
    });
}

/// Copy source context while replacing only the statically selected target.
fn derived_parser_relation(relation: &SymbolRelation, target_name: String) -> SymbolRelation {
    SymbolRelation {
        path: relation.path.clone(),
        source_name: relation.source_name.clone(),
        target_name,
        kind: RelationKind::Calls,
        line: relation.line,
        context: relation.context.clone(),
        parser: relation.parser,
    }
}

/// Create one file-owned content-free configuration or deployment fact.
fn file_owned_relation(graph: &SymbolGraph, target_name: String) -> SymbolRelation {
    SymbolRelation {
        path: graph.path.clone(),
        source_name: "<module>".to_string(),
        target_name,
        kind: RelationKind::Calls,
        line: 1,
        context: graph.path.clone(),
        parser: graph.parser,
    }
}

/// Resolve one additive relation through its closed target class.
fn derived_relation_resolution<'a>(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    fact: &DerivedRelationFact,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    candidates: &'a ProjectResolutionRegistry,
    entities: &'a BTreeMap<String, GraphEntity>,
    external_entities: &mut BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<RelationResolution, CliError> {
    match &fact.target {
        DerivedRelationTarget::Parser { keys } => relation_resolution(
            project,
            generation,
            &fact.relation,
            owners,
            graph,
            symbol_index,
            keys,
            candidates,
            entities,
            external_entities,
            control,
        ),
        DerivedRelationTarget::RepositoryPath => {
            let candidate = GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(&fact.relation.target_name))
                        .map_err(invalid_graph_contract)?,
                },
                generation,
            )
            .map_err(invalid_graph_contract)?;
            match entities.get(candidate.key().digest()) {
                Some(entity) if entity == &candidate => {
                    RelationResolution::resolved(entity).map_err(invalid_graph_contract)
                }
                Some(_conflict) => Err(CliError::InvalidInput(
                    "static repository-path target collided with another graph entity".to_string(),
                )),
                None => Ok(RelationResolution::Unresolved {
                    reference: GraphIdentityText::new(fact.relation.target_name.clone())
                        .map_err(invalid_graph_contract)?,
                }),
            }
        }
        DerivedRelationTarget::External { system } => {
            let entity = GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new(*system).map_err(invalid_graph_contract)?,
                        identity: GraphIdentityText::new(fact.relation.target_name.clone())
                            .map_err(invalid_graph_contract)?,
                    },
                },
                generation,
            )
            .map_err(invalid_graph_contract)?;
            let resolution =
                RelationResolution::external(&entity).map_err(invalid_graph_contract)?;
            insert_entity(external_entities, entity)?;
            Ok(resolution)
        }
    }
}

/// Return whether a repository path is an accepted test-source location.
fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    normalized
        .split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
        || file.contains(".test.")
        || file.contains(".spec.")
        || file
            .split_once('.')
            .is_some_and(|(stem, _extension)| stem.ends_with("_test") || stem.starts_with("test_"))
}

/// Extract one exact handler identifier from a static route registration.
fn static_route_handler(relation: &SymbolRelation) -> Option<String> {
    let leaf = call_leaf(&relation.target_name);
    if !matches!(
        leaf,
        "route" | "add_route" | "map_get" | "map_post" | "map_put" | "map_patch" | "map_delete"
    ) {
        return None;
    }
    let route = static_string_argument(&relation.context)?;
    if !route.starts_with('/')
        || route.chars().any(char::is_control)
        || relation.context.matches(',').count() != 1
    {
        return None;
    }
    let handler = relation
        .context
        .rsplit_once(',')?
        .1
        .trim()
        .trim_end_matches([')', ';'])
        .trim();
    let valid_handler = !handler.is_empty()
        && handler != relation.source_name
        && handler
            .chars()
            .next()
            .is_some_and(|character| !character.is_ascii_digit())
        && handler.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | ':' | '.' | '$')
        });
    valid_handler.then(|| handler.trim_matches('$').to_string())
}

/// Extract one static environment key without retaining its value.
fn static_environment_key(relation: &SymbolRelation) -> Option<String> {
    let normalized = relation
        .target_name
        .trim()
        .trim_end_matches('!')
        .to_ascii_lowercase();
    if !(normalized.ends_with("env::var")
        || normalized.ends_with("env::var_os")
        || normalized.ends_with("os.getenv")
        || normalized.ends_with("getenvironmentvariable")
        || matches!(normalized.as_str(), "getenv" | "getenv_os"))
    {
        return None;
    }
    let key = static_string_argument(&relation.context)?;
    (!key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
    .then(|| key.to_string())
}

/// Classify a call as one accepted bounded static read or write API.
fn static_data_access_kind(target: &str) -> Option<ExtendedRelationKind> {
    let normalized = target.trim().trim_end_matches('!').to_ascii_lowercase();
    let leaf = call_leaf(&normalized);
    if matches!(
        leaf,
        "read" | "read_to_string" | "readfile" | "readfilesync"
    ) || normalized.ends_with("file::open")
    {
        Some(ExtendedRelationKind::Reads)
    } else if matches!(leaf, "write" | "writefile" | "writefilesync" | "create")
        || normalized.ends_with("file::create")
    {
        Some(ExtendedRelationKind::Writes)
    } else {
        None
    }
}

/// Return the normalized leaf of one qualified call target.
fn call_leaf(target: &str) -> &str {
    target
        .trim()
        .trim_end_matches('!')
        .rsplit([':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or_default()
}

/// Return the first argument only when it is one complete static string literal.
fn static_string_argument(context: &str) -> Option<&str> {
    let open = context.find('(')?;
    let argument = context[open + 1..].trim_start();
    let quote = argument.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let value = &argument[quote.len_utf8()..];
    let end = value.find(quote)?;
    let value = &value[..end];
    (!value.contains('\\')
        && !value.contains("${")
        && !value.contains("#{")
        && !value.chars().any(char::is_control))
    .then_some(value)
}

/// Normalize one static relative source literal into a repository path.
fn normalize_static_repository_path(value: &str) -> Option<String> {
    let value = value.replace('\\', "/");
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('~')
        || value.contains("://")
        || value.contains('$')
        || value.contains('{')
        || value.contains('}')
        || value
            .split('/')
            .next()
            .is_some_and(|part| part.contains(':'))
    {
        return None;
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part == "."
                || part == ".."
                || part.chars().any(char::is_control)
                || part.trim() != part =>
            {
                return None;
            }
            part => parts.push(part.to_string()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Classify one exact repository configuration filename without reading values.
fn configuration_file_identity(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let identity = if file == ".env" || file.starts_with(".env.") {
        "dotenv"
    } else if matches!(
        file,
        "config.json"
            | "config.yaml"
            | "config.yml"
            | "config.toml"
            | "settings.json"
            | "settings.yaml"
            | "settings.yml"
            | "settings.toml"
            | "appsettings.json"
    ) {
        "application-config"
    } else {
        return None;
    };
    Some(identity.to_string())
}

/// Classify one accepted infrastructure path into a content-free platform.
fn deployment_platform_identity(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let identity = if file_has_extension(file, "tf")
        || file
            .strip_suffix(".json")
            .is_some_and(|stem| file_has_extension(stem, "tf"))
    {
        "terraform"
    } else if file == "dockerfile"
        || file.starts_with("dockerfile.")
        || matches!(
            file,
            "compose.yaml" | "compose.yml" | "docker-compose.yaml" | "docker-compose.yml"
        )
    {
        "containers"
    } else if matches!(
        file,
        "chart.yaml" | "kustomization.yaml" | "kustomization.yml"
    ) || (normalized
        .split('/')
        .any(|segment| matches!(segment, "k8s" | "kubernetes" | "helm" | "kustomize"))
        && matches!(
            Path::new(file).extension().and_then(|value| value.to_str()),
            Some("yaml" | "yml" | "json")
        ))
    {
        "kubernetes"
    } else if file_has_extension(file, "bicep") {
        "azure-bicep"
    } else if file == "sam-template.yaml"
        || (file.contains("cloudformation")
            && matches!(
                Path::new(file).extension().and_then(|value| value.to_str()),
                Some("yaml" | "yml" | "json")
            ))
    {
        "cloudformation"
    } else if file == "playbook.yaml" || file == "playbook.yml" {
        "ansible"
    } else {
        return None;
    };
    Some(identity.to_string())
}

/// Match one exact extension without platform case assumptions.
fn file_has_extension(file: &str, expected: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

/// Resolve one parser relation's unique local source entity when possible.
fn relation_source<'a>(
    owners: &GraphOwners,
    entities: &'a BTreeMap<String, GraphEntity>,
    symbol_index: &GraphSymbolIndex<'_>,
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Result<&'a GraphEntity, CliError> {
    let file = entities.get(&owners.file_digest).ok_or_else(|| {
        CliError::InvalidInput("graph file owner entity was not staged".to_string())
    })?;
    let mut matched = None;
    for &index in symbol_index.get(&relation.source_name) {
        let digest = owners.symbol_digests.get(index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner index was not staged".to_string())
        })?;
        let Some(digest) = digest else {
            continue;
        };
        let entity = entities.get(digest).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner entity was not staged".to_string())
        })?;
        if matched.is_some() {
            return Ok(file);
        }
        matched = Some(entity);
    }
    Ok(matched.unwrap_or(file))
}

/// Resolve one relation from canonical dependency keys and bounded candidates.
#[derive(Clone, Copy)]
struct ResolutionMatches<'a> {
    /// First target in stable digest order when at least one target exists.
    first: Option<&'a GraphEntity>,
    /// Exact number of distinct target entities.
    count: u32,
}

/// Resolve one relation from canonical dependency keys and bounded candidates.
fn relation_resolution<'a>(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    relation: &projectatlas_core::symbols::SymbolRelation,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    dependency_keys: &[CanonicalResolutionKey],
    candidates: &'a ProjectResolutionRegistry,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    external_entities: &mut BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<RelationResolution, CliError> {
    let matches = match relation.kind {
        RelationKind::Contains => local_relation_matches(
            relation,
            owners,
            graph,
            symbol_index,
            staged_entities,
            control,
        )?,
        RelationKind::Calls => {
            let local = local_relation_matches(
                relation,
                owners,
                graph,
                symbol_index,
                staged_entities,
                control,
            )?;
            if local.count == 0 {
                registry_resolution_matches(dependency_keys, candidates, staged_entities, control)?
            } else {
                local
            }
        }
        RelationKind::Imports | RelationKind::DependsOn => {
            registry_resolution_matches(dependency_keys, candidates, staged_entities, control)?
        }
    };
    match matches.count {
        0 => {
            if let Some(external) = explicit_external_selector(graph, relation)? {
                let entity =
                    GraphEntity::new(project, EntitySelector::External { external }, generation)
                        .map_err(invalid_graph_contract)?;
                let resolution =
                    RelationResolution::external(&entity).map_err(invalid_graph_contract)?;
                insert_entity(external_entities, entity)?;
                return Ok(resolution);
            }
            Ok(RelationResolution::Unresolved {
                reference: GraphIdentityText::new(relation_reference(relation))
                    .map_err(invalid_graph_contract)?,
            })
        }
        1 => RelationResolution::resolved(
            matches
                .first
                .ok_or_else(|| CliError::InvalidInput("resolved target disappeared".to_string()))?,
        )
        .map_err(invalid_graph_contract),
        count => Ok(RelationResolution::Ambiguous {
            reference: GraphIdentityText::new(relation_reference(relation))
                .map_err(invalid_graph_contract)?,
            candidates: NonZeroU32::new(count).ok_or_else(|| {
                CliError::InvalidInput("ambiguous target count was zero".to_string())
            })?,
        }),
    }
}

/// Resolve exact declarations owned by the relation's source file.
fn local_relation_matches<'a>(
    relation: &projectatlas_core::symbols::SymbolRelation,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ResolutionMatches<'a>, CliError> {
    let mut targets = BTreeMap::<&str, &GraphEntity>::new();
    let target_name = relation.target_name.trim();
    let source_parent = if relation.kind == RelationKind::Calls {
        unique_source_parent(graph, symbol_index, &relation.source_name, control)?
    } else {
        None
    };
    for (candidate_index, &row_index) in symbol_index.get(target_name).iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        let symbol = graph.symbols.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol lookup index was invalid".to_string())
        })?;
        let digest = owners.symbol_digests.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner index was not staged".to_string())
        })?;
        let exact_match = match relation.kind {
            RelationKind::Contains => {
                symbol.name == target_name
                    && symbol.parent.as_deref() == Some(relation.source_name.as_str())
            }
            RelationKind::Calls => {
                symbol.name == target_name
                    && (symbol.parent.is_none()
                        || symbol.parent.as_deref() == Some(relation.source_name.as_str())
                        || source_parent.is_some_and(|source_parent| {
                            symbol.parent.as_deref() == Some(source_parent)
                        }))
            }
            RelationKind::Imports | RelationKind::DependsOn => false,
        };
        if exact_match && let Some(digest) = digest {
            let entity = staged_entities.get(digest).ok_or_else(|| {
                CliError::InvalidInput("graph symbol owner entity was not staged".to_string())
            })?;
            if !targets.contains_key(entity.key().digest()) {
                enforce_resolution_match_budget(targets.len().saturating_add(1))?;
            }
            targets.insert(entity.key().digest(), entity);
        }
    }
    let count = distinct_resolution_count(targets.len())?;
    Ok(ResolutionMatches {
        first: targets.into_values().next(),
        count,
    })
}

/// Return one unambiguous containing scope for the parser relation source.
fn unique_source_parent<'a>(
    graph: &'a SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    source_name: &str,
    control: &IndexWorkControl,
) -> Result<Option<&'a str>, CliError> {
    let mut parent = None;
    let mut source_found = false;
    for (candidate_index, &row_index) in symbol_index.get(source_name).iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        let symbol = graph.symbols.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol lookup index was invalid".to_string())
        })?;
        let candidate = symbol.parent.as_deref();
        if !source_found {
            parent = candidate;
            source_found = true;
        } else if parent != candidate {
            return Ok(None);
        }
    }
    Ok(parent)
}

/// Merge sorted candidate streams without materializing their full union.
fn registry_resolution_matches<'a>(
    dependency_keys: &[CanonicalResolutionKey],
    candidates: &'a ProjectResolutionRegistry,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ResolutionMatches<'a>, CliError> {
    enforce_resolution_match_budget(dependency_keys.len().saturating_mul(2))?;
    let mut streams = dependency_keys
        .iter()
        .filter_map(|key| candidates.candidate_digests_by_key.get(key))
        .map(BTreeSet::iter)
        .collect::<Vec<_>>();
    let mut frontier = BinaryHeap::new();
    for (stream_index, stream) in streams.iter_mut().enumerate() {
        if let Some(digest) = stream.next() {
            frontier.push(Reverse((digest.as_str(), stream_index)));
        }
    }

    let mut first = None;
    let mut last_digest = None;
    let mut count = 0_u32;
    let mut visited = 0_usize;
    while let Some(Reverse((digest, stream_index))) = frontier.pop() {
        check_graph_work(control, visited)?;
        visited = visited.saturating_add(1);
        if last_digest != Some(digest) {
            let entity = staged_entities
                .get(digest)
                .or_else(|| candidates.supplemental_entities_by_digest.get(digest))
                .ok_or_else(|| {
                    CliError::InvalidInput(
                        "resolution candidate entity was not registered".to_string(),
                    )
                })?;
            first.get_or_insert(entity);
            count = count.checked_add(1).ok_or_else(|| {
                IndexWorkFailure::resource_limit(
                    IndexWorkStage::SymbolParsing,
                    IndexWorkResource::RelationRows,
                    u64::from(u32::MAX),
                    u64::from(u32::MAX) + 1,
                )
            })?;
            last_digest = Some(digest);
        }
        if let Some(next) = streams[stream_index].next() {
            frontier.push(Reverse((next.as_str(), stream_index)));
        }
    }
    Ok(ResolutionMatches { first, count })
}

/// Convert one exact distinct target count without truncation.
fn distinct_resolution_count(count: usize) -> Result<u32, CliError> {
    u32::try_from(count).map_err(|_conversion_error| {
        IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::RelationRows,
            u64::from(u32::MAX),
            u64::try_from(count).unwrap_or(u64::MAX),
        )
        .into()
    })
}

/// Bound temporary maps and heap storage used while selecting relation targets.
fn enforce_resolution_match_budget(rows: usize) -> Result<(), CliError> {
    enforce_resolution_registry_budget(
        STAGED_GRAPH_ROW_BYTES.saturating_mul(u64::try_from(rows).unwrap_or(u64::MAX)),
    )
}

/// Return one explicit external identity without guessing ordinary missing packages.
fn explicit_external_selector(
    graph: &SymbolGraph,
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Result<Option<ExternalSelector>, CliError> {
    let semantic_provider = graph
        .language
        .as_deref()
        .and_then(language_capability)
        .and_then(|capability| capability.effective_semantic_provider());
    let classified = match (semantic_provider, relation.kind) {
        (Some(SemanticProviderOwner::Cargo), RelationKind::DependsOn) => {
            external_reference_identity(&relation.target_name)
                .map(|identity| (CARGO_PACKAGE_MANAGER, identity))
        }
        (Some(SemanticProviderOwner::Rust), RelationKind::Imports | RelationKind::Calls) => {
            rust_toolchain_identity(relation).map(|identity| (RUST_TOOLCHAIN_SYSTEM, identity))
        }
        (Some(SemanticProviderOwner::EcmaScript), RelationKind::Imports) => {
            node_builtin_identity(relation).map(|identity| (NODE_SYSTEM, identity))
        }
        (
            Some(
                SemanticProviderOwner::Python
                | SemanticProviderOwner::Unavailable
                | SemanticProviderOwner::Cargo
                | SemanticProviderOwner::EcmaScript,
            )
            | None,
            _,
        )
        | (Some(SemanticProviderOwner::Rust), RelationKind::Contains | RelationKind::DependsOn) => {
            None
        }
    };
    classified
        .map(|(system, identity)| {
            Ok(ExternalSelector {
                system: GraphIdentityText::new(system).map_err(invalid_graph_contract)?,
                identity: GraphIdentityText::new(identity).map_err(invalid_graph_contract)?,
            })
        })
        .transpose()
}

/// Normalize one explicit non-empty external reference.
fn external_reference_identity(reference: &str) -> Option<String> {
    let reference = reference.trim();
    (!reference.is_empty()).then(|| reference.to_string())
}

/// Recognize only explicitly qualified Rust toolchain roots.
fn rust_toolchain_identity(
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Option<String> {
    if relation.kind == RelationKind::Imports {
        let mut references = parse_import_references(&relation.target_name).into_iter();
        let first = rust_import_identity(&references.next()?)?;
        return references.try_fold(first, |common, reference| {
            let identity = rust_import_identity(&reference)?;
            common_rust_path(&common, &identity)
        });
    }
    let target = relation_reference(relation);
    rust_toolchain_root(&target)?;
    Some(target)
}

/// Return one explicitly toolchain-owned Rust import identity.
fn rust_import_identity(reference: &projectatlas_symbols::ImportReference) -> Option<String> {
    let module = reference.module();
    rust_toolchain_root(module)?;
    Some(reference.imported().map_or_else(
        || module.to_string(),
        |imported| format!("{module}::{imported}"),
    ))
}

/// Return the non-empty common Rust module path shared by two imports.
fn common_rust_path(left: &str, right: &str) -> Option<String> {
    let components = left
        .split("::")
        .zip(right.split("::"))
        .take_while(|(left, right)| left == right)
        .map(|(component, _right)| component)
        .collect::<Vec<_>>();
    (!components.is_empty()).then(|| components.join("::"))
}

/// Return the accepted toolchain root of one Rust module path.
fn rust_toolchain_root(path: &str) -> Option<&str> {
    let root = path.trim().split("::").next()?;
    matches!(root, "std" | "core" | "alloc").then_some(root)
}

/// Recognize only the explicit `node:` built-in module scheme.
fn node_builtin_identity(relation: &projectatlas_core::symbols::SymbolRelation) -> Option<String> {
    parse_import_references(&relation.target_name)
        .into_iter()
        .find_map(|reference| {
            reference
                .module()
                .strip_prefix("node:")
                .filter(|identity| !identity.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            quoted_ecmascript_module(&relation.target_name)
                .and_then(|module| module.strip_prefix("node:"))
                .filter(|identity| !identity.is_empty())
                .map(ToString::to_string)
        })
}

/// Read the explicit quoted module from one parser-validated ECMAScript import.
fn quoted_ecmascript_module(import: &str) -> Option<&str> {
    let import = import.trim();
    if !import.starts_with("import ") {
        return None;
    }
    let (quote_index, quote) = import
        .char_indices()
        .find(|(_index, character)| matches!(character, '\'' | '"'))?;
    let remainder = &import[quote_index + quote.len_utf8()..];
    let end = remainder.find(quote)?;
    Some(&remainder[..end])
}

/// Normalize an empty parser reference into one stable diagnostic identity.
fn nonempty_reference(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        UNKNOWN_REFERENCE.to_string()
    } else {
        value.to_string()
    }
}

/// Keep call arguments and literal values out of persisted relation diagnostics.
fn relation_reference(relation: &SymbolRelation) -> String {
    let mut value = if relation.kind == RelationKind::Calls {
        relation
            .target_name
            .split_once('(')
            .map_or(relation.target_name.as_str(), |(target, _arguments)| target)
    } else {
        &relation.target_name
    };
    if relation.kind == RelationKind::Calls && value.contains(['\'', '"']) {
        value = call_leaf(value);
    }
    nonempty_reference(value)
}

/// Project parser trust into one path-scoped coverage record.
fn coverage_for_graph(
    graph: &SymbolGraph,
    generation: IndexGeneration,
    identity_admission: &GraphIdentityAdmission,
    derived_identity_admission: &GraphIdentityAdmission,
) -> Result<CoverageRecord, CliError> {
    let scope = CoverageScope::Path {
        path: RepositoryNodePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
    };
    let identity_omitted =
        identity_admission.rejected_facts_for_graph(&graph.path, derived_identity_admission)?;
    let parser_omitted = identity_admission.parser_rejection_for_graph(&graph.path);
    let omitted = identity_omitted
        .checked_add(parser_omitted)
        .ok_or_else(|| CliError::InvalidInput("identity rejection count overflowed".to_string()))?;
    let identity_details_dropped = identity_admission.rejection_details_dropped_for(&graph.path)
        || derived_identity_admission.rejection_details_dropped_for(&graph.path);
    // Graph-scoped `rows` is the existing persisted coverage slot for the
    // publication-wide typed identity-detail ceiling. It is set only from
    // the admission fact that a distinct detail was actually evicted.
    let reached_limit = (omitted > 0 && identity_details_dropped).then_some(GraphLimitKind::Rows);
    let covered = if identity_omitted > 0 {
        u64::try_from(graph.symbols.len().saturating_add(graph.relations.len())).unwrap_or(u64::MAX)
    } else {
        u64::try_from(graph.relations.len()).unwrap_or(u64::MAX)
    };
    if omitted > 0 {
        let state = if covered > 0 {
            CoverageState::Partial
        } else {
            CoverageState::Failed
        };
        return CoverageRecord::new(
            scope,
            None,
            state,
            covered,
            omitted,
            generation,
            Some(GraphIdentityText::new(PARTIAL_COVERAGE_REASON).map_err(invalid_graph_contract)?),
            reached_limit,
        )
        .map_err(invalid_graph_contract);
    }
    match graph.parser {
        ParserKind::TreeSitter | ParserKind::Manifest => CoverageRecord::new(
            scope,
            None,
            CoverageState::Complete,
            covered,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        ParserKind::Structural | ParserKind::Fallback if covered > 0 => CoverageRecord::new(
            scope,
            None,
            CoverageState::Partial,
            covered,
            1,
            generation,
            Some(GraphIdentityText::new(PARTIAL_COVERAGE_REASON).map_err(invalid_graph_contract)?),
            None,
        )
        .map_err(invalid_graph_contract),
        ParserKind::Structural | ParserKind::Fallback => CoverageRecord::new(
            scope,
            None,
            CoverageState::Failed,
            0,
            1,
            generation,
            Some(GraphIdentityText::new(PARTIAL_COVERAGE_REASON).map_err(invalid_graph_contract)?),
            None,
        )
        .map_err(invalid_graph_contract),
    }
}

/// Map parser strength to relation confidence.
fn relation_confidence(parser: ParserKind) -> ConfidenceClass {
    match parser {
        ParserKind::TreeSitter | ParserKind::Manifest => ConfidenceClass::Exact,
        ParserKind::Structural => ConfidenceClass::Medium,
        ParserKind::Fallback => ConfidenceClass::Low,
    }
}

/// Map parser strength to relation completeness.
fn relation_completeness(parser: ParserKind) -> Completeness {
    match parser {
        ParserKind::TreeSitter | ParserKind::Manifest => Completeness::Complete,
        ParserKind::Structural | ParserKind::Fallback => Completeness::Partial,
    }
}

/// Carry current-generation rejection state for graphs safely reused by a stage.
///
/// A publication replaces graph and rejection rows for its selected paths.
/// Reused sanitized graphs no longer contain rejected parser values, so the
/// persisted typed facts and total path coverage are hydrated before normal
/// admission re-derives current key, Markdown, and derived facts. The detail
/// count is retained separately to reconcile those two observations exactly.
fn hydrate_reused_identity_admission(
    store: &AtlasStore,
    project: ProjectInstanceId,
    reused_paths: &BTreeSet<String>,
    graphs: &[impl Borrow<SymbolGraph>],
    report: &mut GraphIdentityAdmission,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    if reused_paths.is_empty() {
        return Ok(());
    }
    let reused_node_paths = reused_paths
        .iter()
        .map(|path| RepositoryNodePath::new(Path::new(path)).map_err(invalid_graph_contract))
        .collect::<Result<Vec<_>, _>>()?;
    let mut persisted_rejections = Vec::new();
    let mut persisted_coverage = BTreeMap::new();
    let mut persisted_rejection_details_dropped_by_path = BTreeSet::new();
    for paths in reused_node_paths.chunks(PERSISTED_GRAPH_PATHS_PER_CHUNK) {
        control.check(IndexWorkStage::SymbolParsing)?;
        let rejection_rows = match store.repository_graph_identity_rejections(
            project,
            paths,
            GraphLimits::MAX_ROWS,
            Some(control),
        ) {
            Ok(rows) => rows,
            Err(projectatlas_db::DbError::GraphRowShape { table, reason })
                if table == "project_identity"
                    && reason == "typed graph generation does not match complete publication" =>
            {
                // A full stage can repair an older incomplete publication, but
                // it cannot safely carry diagnostic rows from that state.
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        persisted_rejections.extend(rejection_rows);
        let coverage = match store.repository_graph_path_coverage(project, paths, Some(control)) {
            Ok(coverage) => coverage,
            Err(projectatlas_db::DbError::GraphRowShape { table, reason })
                if table == "project_identity"
                    && reason == "typed graph generation does not match complete publication" =>
            {
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        if coverage.truncated {
            return Err(CliError::InvalidInput(
                "reused graph coverage exceeded the publication row ceiling".to_string(),
            ));
        }
        for row in coverage.rows {
            let CoverageScope::Path { path } = row.scope() else {
                continue;
            };
            if row.relation().is_none() && row.omitted() > 0 {
                control.check(IndexWorkStage::SymbolParsing)?;
                persisted_coverage.insert(path.as_str().to_owned(), row.omitted());
            }
            if row.relation().is_none() && row.reached_limit() == Some(GraphLimitKind::Rows) {
                control.check(IndexWorkStage::SymbolParsing)?;
                persisted_rejection_details_dropped_by_path.insert(path.as_str().to_owned());
            }
        }
    }
    let mut persisted_fact_counts = BTreeMap::<String, BTreeSet<IdentityFactKey>>::new();
    for path in persisted_rejection_details_dropped_by_path {
        control.check(IndexWorkStage::SymbolParsing)?;
        if report
            .rejection_details_dropped_by_path
            .insert(path.clone())
        {
            report.reserve_reused_path_bytes(
                &path,
                false,
                control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
        }
    }
    for rejection in persisted_rejections {
        control.check(IndexWorkStage::SymbolParsing)?;
        let namespace = rejection.fact_index & !((1_u64 << 56) - 1);
        let path = rejection.path.as_str().to_owned();
        persisted_fact_counts
            .entry(path.clone())
            .or_default()
            .insert(IdentityFactKey {
                span: IdentitySpan {
                    start_line: rejection.span.start_line() as usize,
                    start_column: rejection.span.start_column() as usize,
                    end_line: rejection.span.end_line() as usize,
                    end_column: rejection.span.end_column() as usize,
                },
                parser: parser_fact_kind(rejection.parser),
                owner: u8::from(rejection.field == GraphIdentityField::ResolutionKey),
                fact_index: rejection.fact_index,
            });
        // Derived relations have their own admission report during projection;
        // hydrating them into the parser report would double-count coverage.
        if namespace == DERIVED_RELATION_FACT_INDEX_NAMESPACE {
            continue;
        }
        control.check(IndexWorkStage::SymbolParsing)?;
        let rejection_key_bytes = extend_bounded_identity_rejections_with_drop_paths(
            &mut report.rejections,
            &mut report.rejection_keys,
            std::iter::once(rejection),
            &mut report.rejection_details_dropped_by_path,
        )?;
        report.reserve_identity_bytes(
            rejection_key_bytes,
            control,
            MAX_IN_MEMORY_GRAPH_WORK_BYTES,
        )?;
    }
    for (path, facts) in persisted_fact_counts {
        control.check(IndexWorkStage::SymbolParsing)?;
        if !report.reused_rejection_facts.contains_key(&path) {
            report.reserve_identity_bytes(
                identity_fact_set_retained_bytes(&path, &facts)?,
                control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
            report.reused_rejection_facts.insert(path.clone(), facts);
        }
        let detail_count = u64::try_from(
            report
                .reused_rejection_facts
                .get(&path)
                .map_or(0, BTreeSet::len),
        )
        .map_err(|error| {
            CliError::InvalidInput(format!(
                "identity rejection detail count overflowed: {error}"
            ))
        })?;
        if !report.reused_rejection_detail_counts.contains_key(&path) {
            report.reserve_reused_path_bytes(
                &path,
                false,
                control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
            report
                .reused_rejection_detail_counts
                .insert(path, detail_count);
        }
    }
    for (path, omitted) in persisted_coverage {
        // Persisted path coverage is authoritative for safely reused graphs,
        // including structural/fallback rows whose typed details were evicted
        // by the global rejection-detail ceiling.
        control.check(IndexWorkStage::SymbolParsing)?;
        if !report.reused_rejection_counts.contains_key(&path) {
            report.reserve_reused_path_bytes(
                &path,
                false,
                control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
            report.reused_rejection_counts.insert(path, omitted);
        }
    }
    let graph_parsers = graphs
        .iter()
        .map(|graph| {
            let graph = Borrow::<SymbolGraph>::borrow(graph);
            (graph.path.as_str(), graph.parser)
        })
        .collect::<BTreeMap<_, _>>();
    for path in reused_paths {
        let Some(_omitted) = report.reused_rejection_counts.get(path).copied() else {
            continue;
        };
        let details = report
            .reused_rejection_detail_counts
            .get(path)
            .copied()
            .unwrap_or(0);
        let parser = graph_parsers.get(path.as_str()).copied();
        let baseline = parser.map_or(0, |parser| {
            u64::from(matches!(
                parser,
                ParserKind::Structural | ParserKind::Fallback
            ))
        });
        if details == 0 && baseline > 0 {
            // A structural/fallback path with no retained typed identity fact
            // still owns its coarse parser omission. Keep that provenance out
            // of identity reconciliation so unchanged graphs retain the same
            // covered count as a clean parse.
            if !report.reused_parser_rejection_counts.contains_key(path) {
                report.reserve_reused_path_bytes(
                    path,
                    false,
                    control,
                    MAX_IN_MEMORY_GRAPH_WORK_BYTES,
                )?;
                report
                    .reused_parser_rejection_counts
                    .insert(path.clone(), baseline);
            }
        }
        // Fallback rows intentionally carry one coarse parser omission and do
        // not re-derive identity details. Structural and exact parsers must
        // be reparsed when a capped persisted row set cannot identify every
        // old fact without guessing.
        if parser != Some(ParserKind::Fallback)
            && report.rejection_details_dropped_for(path)
            && !report.reused_rejection_details_incomplete.contains(path)
        {
            report.reserve_reused_path_bytes(
                path,
                false,
                control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
            report
                .reused_rejection_details_incomplete
                .insert(path.clone());
        }
    }
    let retained_bytes =
        checked_identity_admission_budget(report, control, MAX_IN_MEMORY_GRAPH_WORK_BYTES)?;
    report.observed_fact_bytes = retained_bytes;
    Ok(())
}

/// Overlay staged symbol changes on persisted graphs for exact selected paths.
fn complete_symbol_graphs<'a>(
    store: &AtlasStore,
    paths: &BTreeSet<String>,
    symbols: &'a SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<Vec<Cow<'a, SymbolGraph>>, CliError> {
    let paths = paths.iter().cloned().collect::<Vec<_>>();
    let mut graphs = BTreeMap::new();
    for chunk in paths.chunks(PERSISTED_GRAPH_PATHS_PER_CHUNK) {
        control.check(IndexWorkStage::SymbolParsing)?;
        for graph in store.load_symbol_graphs_for_paths(chunk)? {
            graphs.insert(graph.path.clone(), Cow::Owned(graph));
        }
    }
    for (index, change) in symbols.changes.iter().enumerate() {
        check_graph_work(control, index)?;
        match change {
            SymbolProjectionChange::Parsed(parsed) if paths.binary_search(&parsed.path).is_ok() => {
                graphs.insert(parsed.path.clone(), Cow::Borrowed(&parsed.graph));
            }
            SymbolProjectionChange::Clear { path, .. } if paths.binary_search(path).is_ok() => {
                graphs.remove(path);
            }
            SymbolProjectionChange::Parsed(_) | SymbolProjectionChange::Clear { .. } => {}
        }
    }
    graphs.retain(|path, _graph| paths.binary_search(path).is_ok());
    Ok(graphs.into_values().collect())
}

/// Admit parser graphs at one shared boundary before any strict graph object is built.
fn admit_symbol_graphs<'a>(
    graphs: Vec<Cow<'a, SymbolGraph>>,
    control: &IndexWorkControl,
) -> Result<(Vec<Cow<'a, SymbolGraph>>, GraphIdentityAdmission), CliError> {
    let mut admitted = Vec::with_capacity(graphs.len());
    let mut report = GraphIdentityAdmission::default();
    for (index, graph) in graphs.into_iter().enumerate() {
        check_graph_work(control, index)?;
        let (graph, graph_report) = admit_symbol_graph(graph, control)?;
        report.merge(graph_report, control)?;
        admitted.push(graph);
    }
    Ok((admitted, report))
}

/// Retain valid parser facts while recording every invalid identity field.
fn admit_symbol_graph<'a>(
    graph: Cow<'a, SymbolGraph>,
    control: &IndexWorkControl,
) -> Result<(Cow<'a, SymbolGraph>, GraphIdentityAdmission), CliError> {
    let mut report = GraphIdentityAdmission::default();
    let paired_import_relations = paired_import_relations(&graph, control)?;
    #[cfg(test)]
    {
        report.paired_import_pairing_work = paired_import_relations.work_items;
    }
    let mut rejected_symbols = vec![false; graph.symbols.len()];
    for (index, symbol) in graph.symbols.iter().enumerate() {
        check_graph_work(control, index)?;
        let paired_relation_index = paired_import_relations.by_symbol[index];
        let span = symbol_identity_span(&graph, index, paired_relation_index);
        let mut failures = Vec::new();
        let name_field = if symbol.kind == SymbolKind::Package {
            GraphIdentityField::Package
        } else {
            GraphIdentityField::Symbol
        };
        record_identity_failure(&mut failures, name_field, &symbol.name);
        if let Some(parent) = symbol.parent.as_deref() {
            record_identity_failure(&mut failures, GraphIdentityField::Parent, parent);
        }
        let signature = if symbol.signature.is_empty() {
            &symbol.name
        } else {
            &symbol.signature
        };
        record_identity_failure(&mut failures, GraphIdentityField::Signature, signature);
        if !failures.is_empty() {
            rejected_symbols[index] = true;
            report.record(
                &graph.path,
                span,
                symbol.parser,
                symbol_parser_fact_index(index, paired_relation_index),
                &failures,
                control,
            )?;
        }
    }
    let mut rejected_relations = vec![false; graph.relations.len()];
    for (index, relation) in graph.relations.iter().enumerate() {
        check_graph_work(control, index)?;
        let span = IdentitySpan {
            start_line: relation.line.max(1),
            start_column: 0,
            end_line: relation.line.max(1),
            end_column: 0,
        };
        let mut failures = Vec::new();
        record_identity_failure(
            &mut failures,
            GraphIdentityField::RelationSource,
            &relation.source_name,
        );
        record_identity_failure(
            &mut failures,
            GraphIdentityField::RelationTarget,
            &relation.target_name,
        );
        if !failures.is_empty() {
            rejected_relations[index] = true;
            report.record(
                &graph.path,
                span,
                relation.parser,
                parser_fact_index(RELATION_FACT_INDEX_NAMESPACE, index),
                &failures,
                control,
            )?;
        }
    }
    if report.rejected_facts_by_path.is_empty() {
        return Ok((graph, report));
    }
    let mut graph = graph.into_owned();
    graph.symbols = graph
        .symbols
        .into_iter()
        .enumerate()
        .filter_map(|(index, symbol)| (!rejected_symbols[index]).then_some(symbol))
        .collect();
    let mut relation_fact_indices = Vec::with_capacity(graph.relations.len());
    graph.relations = graph
        .relations
        .into_iter()
        .enumerate()
        .filter_map(|(index, relation)| {
            (!rejected_relations[index]).then(|| {
                relation_fact_indices.push(index);
                relation
            })
        })
        .collect();
    if rejected_relations.iter().any(|rejected| *rejected) && !relation_fact_indices.is_empty() {
        report
            .relation_fact_indices
            .insert(graph.path.clone(), relation_fact_indices);
    }
    Ok((Cow::Owned(graph), report))
}

/// Sanitize parser output before any summary, persistence, or graph projection sink.
pub(super) fn admit_symbol_build_stage(
    staged: &mut SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<GraphIdentityAdmission, CliError> {
    let mut report = GraphIdentityAdmission::default();
    for (index, change) in staged.changes.iter_mut().enumerate() {
        check_graph_work(control, index)?;
        let SymbolProjectionChange::Parsed(parsed) = change else {
            continue;
        };
        let placeholder = SymbolGraph {
            path: parsed.path.clone(),
            language: None,
            parser: parsed.source_parser,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let graph = std::mem::replace(&mut parsed.graph, placeholder);
        let (admitted, graph_report) = admit_symbol_graph(Cow::Owned(graph), control)?;
        parsed.graph = admitted.into_owned();
        if let Some(markdown) = parsed.markdown_facts.as_deref_mut() {
            admit_markdown_fact_batch(
                &parsed.path,
                parsed.source_parser,
                markdown,
                &mut report,
                control,
            )?;
        }
        if graph_report.has_rejections() {
            // Parser summaries and generated suggestions are sinks too. Rebuild both
            // from the admitted graph so rejected identity text cannot survive there.
            parsed.summary = super::summarize_symbol_graph(&parsed.graph, None);
            parsed.summary_is_structural = false;
            if parsed.purpose_suggestion.is_some() {
                parsed.purpose_suggestion =
                    Some(super::suggest_file_purpose(&parsed.path, &parsed.summary));
            }
        }
        report.merge(graph_report, control)?;
    }
    let mut symbols = 0_usize;
    let mut relations = 0_usize;
    for change in &staged.changes {
        let SymbolProjectionChange::Parsed(parsed) = change else {
            continue;
        };
        symbols = symbols
            .checked_add(parsed.graph.symbols.len())
            .ok_or_else(|| {
                CliError::InvalidInput("admitted symbol report count overflowed".to_string())
            })?;
        relations = relations
            .checked_add(parsed.graph.relations.len())
            .ok_or_else(|| {
                CliError::InvalidInput("admitted relation report count overflowed".to_string())
            })?;
    }
    staged.report.symbols = symbols;
    staged.report.relations = relations;
    report.source_admitted = true;
    Ok(report)
}

/// Validate one source identity without retaining the rejected raw value.
fn record_identity_failure(
    failures: &mut Vec<(GraphIdentityField, GraphIdentityRejectionReason)>,
    field: GraphIdentityField,
    value: &str,
) {
    if let Err(error) = source_symbol_identity_error(value) {
        failures.push((field, GraphIdentityRejectionReason::from_error(&error)));
    }
}

/// Validate one source identity without allocating on the valid hot path.
fn source_symbol_identity_error(value: &str) -> Result<(), GraphContractError> {
    GraphIdentityText::validate(value)?;
    if value.starts_with(QUALIFIED_SYMBOL_SCOPE_PREFIX) {
        return Err(GraphContractError::InvalidIdentityText {
            reason: "source symbol identity uses the reserved derived-scope namespace",
        });
    }
    Ok(())
}

/// Validate derived canonical keys before the entity projection requests them.
fn admit_resolution_key_failures(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    configured_modules: &ConfiguredModuleResolution,
    report: &mut GraphIdentityAdmission,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    for (index, graph) in graphs.iter().enumerate() {
        let graph = graph.borrow();
        check_graph_work(control, index)?;
        #[cfg(not(test))]
        let _ = generation;
        if report.resolution_projections.contains_key(&graph.path) {
            return Err(CliError::InvalidInput(
                "duplicate resolution projection admission".to_string(),
            ));
        }
        #[cfg(test)]
        report.record_resolution_derivation(&graph.path, generation);
        let context = ResolutionProjectionContext::with_configured_modules(configured_modules);
        match derive_resolution_keys_with_context(
            project,
            packages.package_name(&graph.path),
            graph,
            context,
        ) {
            Ok(projection) => {
                report
                    .resolution_projections
                    .insert(graph.path.clone(), projection);
            }
            Err(ResolutionProjectionError::KeyLimit { requested, .. }) => {
                return Err(resolution_key_limit_failure(requested));
            }
            Err(ResolutionProjectionError::Contract(failure)) => {
                let (failures, rejected_count, projection) = (*failure).into_parts();
                for failure in &failures {
                    report.record(
                        &graph.path,
                        resolution_projection_span(graph, failure.fact()),
                        graph.parser,
                        resolution_projection_fact_index(report, &graph.path, failure.fact()),
                        &[(
                            GraphIdentityField::ResolutionKey,
                            GraphIdentityRejectionReason::from_error(failure.error()),
                        )],
                        control,
                    )?;
                }
                report.record_rejected_fact_count(
                    &graph.path,
                    rejected_count.saturating_sub(failures.len()),
                    control,
                )?;
                report
                    .resolution_projections
                    .insert(graph.path.clone(), projection);
            }
        }
    }
    Ok(())
}

/// Require one admitted resolution projection for every graph being published.
fn ensure_admitted_resolution_projections(
    graphs: &[impl Borrow<SymbolGraph>],
    projections: &BTreeMap<String, ResolutionKeyProjection>,
) -> Result<(), CliError> {
    if projections.len() != graphs.len()
        || graphs
            .iter()
            .any(|graph| !projections.contains_key(&graph.borrow().path))
    {
        return Err(CliError::InvalidInput(
            "resolution projection admission did not match staged graphs".to_string(),
        ));
    }
    Ok(())
}

/// Map one resolution fact into its deterministic internal rejection identity.
fn resolution_projection_fact_index(
    report: &GraphIdentityAdmission,
    path: &str,
    fact: ResolutionProjectionFact,
) -> u64 {
    match fact {
        ResolutionProjectionFact::Source => 0,
        ResolutionProjectionFact::Symbol(index) => {
            parser_fact_index(SYMBOL_FACT_INDEX_NAMESPACE, index)
        }
        ResolutionProjectionFact::Relation(index) => parser_fact_index(
            RELATION_FACT_INDEX_NAMESPACE,
            report.relation_parser_index(path, index),
        ),
    }
}

/// Select the narrowest parser span available for one rejected derived key.
fn resolution_projection_span(graph: &SymbolGraph, fact: ResolutionProjectionFact) -> IdentitySpan {
    match fact {
        ResolutionProjectionFact::Source => graph_identity_span(graph),
        ResolutionProjectionFact::Symbol(index) => graph.symbols.get(index).map_or_else(
            || graph_identity_span(graph),
            |symbol| IdentitySpan {
                start_line: symbol.line_start.max(1),
                start_column: 0,
                end_line: symbol.line_end.max(symbol.line_start).max(1),
                end_column: 0,
            },
        ),
        ResolutionProjectionFact::Relation(index) => graph.relations.get(index).map_or_else(
            || graph_identity_span(graph),
            |relation| IdentitySpan {
                start_line: relation.line.max(1),
                start_column: 0,
                end_line: relation.line.max(1),
                end_column: 0,
            },
        ),
    }
}

/// Bound one graph-wide rejection span when canonical-key derivation has no fact index.
fn graph_identity_span(graph: &SymbolGraph) -> IdentitySpan {
    let mut start_line = usize::MAX;
    let mut end_line = 1;
    for symbol in &graph.symbols {
        start_line = start_line.min(symbol.line_start.max(1));
        end_line = end_line.max(symbol.line_end.max(symbol.line_start).max(1));
    }
    for relation in &graph.relations {
        start_line = start_line.min(relation.line.max(1));
        end_line = end_line.max(relation.line.max(1));
    }
    IdentitySpan {
        start_line: if start_line == usize::MAX {
            1
        } else {
            start_line
        },
        start_column: 0,
        end_line,
        end_column: 0,
    }
}

/// Admit parser-owned Markdown selectors before document-key derivation.
fn admit_markdown_facts(
    facts: &mut BTreeMap<String, Cow<'_, MarkdownFacts>>,
    graphs: &[impl Borrow<SymbolGraph>],
    report: &mut GraphIdentityAdmission,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    for (graph_index, graph) in graphs.iter().enumerate() {
        check_graph_work(control, graph_index)?;
        let graph = graph.borrow();
        let Some(markdown) = facts.get_mut(&graph.path) else {
            continue;
        };
        if !markdown
            .link_candidates
            .iter()
            .any(|candidate| GraphIdentityText::validate(&candidate.selector).is_err())
        {
            continue;
        }
        admit_markdown_fact_batch(
            &graph.path,
            graph.parser,
            markdown.to_mut(),
            report,
            control,
        )?;
    }
    Ok(())
}

/// Admit one parser-owned Markdown fact batch before document-key derivation.
fn admit_markdown_fact_batch(
    path: &str,
    parser: ParserKind,
    markdown: &mut MarkdownFacts,
    report: &mut GraphIdentityAdmission,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let mut rejected = Vec::new();
    for (candidate_index, candidate) in markdown.link_candidates.iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        if let Err(error) = GraphIdentityText::validate(&candidate.selector) {
            rejected.push((
                candidate_index,
                candidate.source.line_start,
                candidate.source.column_start,
                candidate.source.line_end,
                candidate.source.column_end,
                GraphIdentityRejectionReason::from_error(&error),
            ));
        }
    }
    for (candidate_index, start_line, start_column, end_line, end_column, reason) in rejected {
        report.record(
            path,
            IdentitySpan {
                start_line,
                start_column,
                end_line,
                end_column,
            },
            parser,
            parser_fact_index(MARKDOWN_FACT_INDEX_NAMESPACE, candidate_index),
            &[(GraphIdentityField::RelationTarget, reason)],
            control,
        )?;
    }
    markdown
        .link_candidates
        .retain(|candidate| GraphIdentityText::validate(&candidate.selector).is_ok());
    Ok(())
}

/// Overlay staged Markdown facts and parse only persisted graphs not parsed in this operation.
fn complete_markdown_facts<'a>(
    root: &Path,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    symbols: &'a SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<BTreeMap<String, Cow<'a, MarkdownFacts>>, CliError> {
    let graph_paths = graphs
        .iter()
        .map(|graph| graph.borrow().path.as_str())
        .collect::<BTreeSet<_>>();
    let nodes_by_path = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| (node.path.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut facts = BTreeMap::new();
    for change in &symbols.changes {
        let SymbolProjectionChange::Parsed(parsed) = change else {
            continue;
        };
        if graph_paths.contains(parsed.path.as_str())
            && let Some(markdown) = parsed.markdown_facts.as_ref()
        {
            facts.insert(parsed.path.clone(), Cow::Borrowed(markdown.as_ref()));
        }
    }
    for graph in graphs {
        let graph = graph.borrow();
        control.check(IndexWorkStage::SymbolParsing)?;
        if facts.contains_key(&graph.path)
            || !graph
                .language
                .as_deref()
                .and_then(language_capability)
                .is_some_and(|capability| capability.symbol_parser == SymbolParserOwner::Markdown)
        {
            continue;
        }
        let node = nodes_by_path.get(graph.path.as_str()).ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Markdown graph path was absent from the staged file inventory: {}",
                graph.path
            ))
        })?;
        let native_path = root.join(Path::new(&graph.path));
        let bytes = match read_source_bytes_controlled(
            &native_path,
            MAX_SYMBOL_FILE_BYTES,
            IndexWorkStage::SymbolParsing,
            control,
        ) {
            Ok(bytes) => bytes,
            Err(SourceReadFailure::IndexWork(failure)) => return Err(failure.into()),
            Err(SourceReadFailure::LimitExceeded { .. }) => {
                return Err(source_changed_during_derivation(root, &graph.path));
            }
            Err(SourceReadFailure::Io(source)) => {
                return Err(CliError::Io {
                    path: native_path,
                    source,
                });
            }
        };
        if node
            .content_hash
            .as_deref()
            .is_none_or(|expected| blake3::hash(&bytes).to_hex().as_str() != expected)
        {
            return Err(source_changed_during_derivation(root, &graph.path));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_source| source_changed_during_derivation(root, &graph.path))?;
        facts.insert(
            graph.path.clone(),
            Cow::Owned(extract_markdown_facts_controlled(&content, control)?),
        );
    }
    enforce_resolution_registry_budget(document_fact_map_retained_bytes(&facts))?;
    Ok(facts)
}

/// Count map ownership plus evidence owned only for persisted graphs parsed on demand.
fn document_fact_map_retained_bytes(facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>) -> u64 {
    facts.iter().fold(0_u64, |bytes, (path, facts)| {
        let bytes = bytes
            .saturating_add(STAGED_GRAPH_ROW_BYTES)
            .saturating_add(path.len() as u64);
        if !matches!(facts, Cow::Owned(_)) {
            return bytes;
        }
        let heading_bytes = facts.headings.iter().fold(0_u64, |bytes, heading| {
            bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(heading.text.len() as u64)
                .saturating_add(heading.slug.len() as u64)
        });
        facts.link_candidates.iter().fold(
            bytes.saturating_add(heading_bytes),
            |bytes, candidate| {
                bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(candidate.selector.len() as u64)
                    .saturating_add(candidate.label.as_ref().map_or(0, String::len) as u64)
                    .saturating_add(
                        candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64
                    )
            },
        )
    })
}

/// Count document projection state that can survive the per-candidate filters.
///
/// Same-file links without a fragment are discarded before resolution and never
/// enter the emitted relation, occurrence, dependency, or reason state. Keep
/// those candidates out of the preflight estimate so incremental admission and
/// the post-projection aggregate use the same ownership boundary.
fn document_projection_retained_bytes(
    facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
    control: &IndexWorkControl,
) -> Result<u64, CliError> {
    let mut retained_bytes = 0_u64;
    let mut candidate_index = 0_usize;
    for (document_path, facts) in facts {
        for candidate in &facts.link_candidates {
            check_graph_work(control, candidate_index)?;
            candidate_index = candidate_index.saturating_add(1);
            if normalize_document_target(document_path, &candidate.selector).is_ok_and(|target| {
                target.path == document_path.as_str() && target.fragment.is_none()
            }) {
                continue;
            }
            retained_bytes = retained_bytes
                .saturating_add(DOCUMENT_PROJECTION_ROW_BYTES)
                .saturating_add(candidate.selector.len() as u64)
                .saturating_add(candidate.label.as_ref().map_or(0, String::len) as u64)
                .saturating_add(candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64);
        }
    }
    Ok(retained_bytes)
}

/// One normalized repository-local document target and optional heading fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DocumentTargetIdentity {
    /// Exact slash-separated path under the selected root.
    path: String,
    /// Static fragment retained only when it can address a heading export.
    fragment: Option<String>,
}

/// Normalize one parser-admitted selector relative to its owning document.
fn normalize_document_target(
    document_path: &str,
    selector: &str,
) -> Result<DocumentTargetIdentity, DocumentTargetUnresolvedReason> {
    let (path_and_query, fragment) = selector
        .split_once('#')
        .map_or((selector, None), |(path, fragment)| (path, Some(fragment)));
    let path = path_and_query
        .split_once('?')
        .map_or(path_and_query, |(path, _query)| path);
    let path = strip_document_line_selector(path);
    if path.is_empty() {
        return Err(DocumentTargetUnresolvedReason::NoStaticTarget);
    }
    let mut components = document_path
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _file)| {
            parent.split('/').map(str::to_owned).collect::<Vec<_>>()
        });
    for component in path.split('/') {
        match component {
            "" => return Err(DocumentTargetUnresolvedReason::Unsupported),
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(DocumentTargetUnresolvedReason::OutsideRoot);
                }
            }
            value if value.contains(':') => {
                return Err(DocumentTargetUnresolvedReason::Unsupported);
            }
            value => components.push(value.to_owned()),
        }
    }
    if components.is_empty() {
        return Err(DocumentTargetUnresolvedReason::NoStaticTarget);
    }
    let fragment = match fragment.map(str::trim) {
        None => None,
        Some(value)
            if !value.is_empty()
                && !value.contains(['/', '\\', '?', '#', '{', '}', '<', '>', '|', '*', '$'])
                && !value.chars().any(char::is_whitespace) =>
        {
            Some(value.to_lowercase())
        }
        Some(_value) => return Err(DocumentTargetUnresolvedReason::NoStaticTarget),
    };
    Ok(DocumentTargetIdentity {
        path: components.join("/"),
        fragment,
    })
}

/// Remove one supported `:12`, `:L12-L20`, or equivalent line selector.
fn strip_document_line_selector(path: &str) -> &str {
    let Some((identity, selector)) = path.rsplit_once(':') else {
        return path;
    };
    let selector = selector.strip_prefix('L').unwrap_or(selector);
    let valid = selector.split_once('-').map_or_else(
        || !selector.is_empty() && selector.chars().all(|character| character.is_ascii_digit()),
        |(start, end)| {
            !start.is_empty()
                && !end.is_empty()
                && start.chars().all(|character| character.is_ascii_digit())
                && end
                    .strip_prefix('L')
                    .unwrap_or(end)
                    .chars()
                    .all(|character| character.is_ascii_digit())
        },
    );
    if valid { identity } else { path }
}

/// Construct one exact file identity through the existing module-key domain.
fn document_file_resolution_key(
    project: ProjectInstanceId,
    path: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key(project, ResolutionKeyDomain::Module, path)
}

/// Construct one case-fold collision key through the existing module-key domain.
fn document_casefold_resolution_key(
    project: ProjectInstanceId,
    path: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key_with_language(
        project,
        ResolutionKeyDomain::Module,
        DOCUMENT_CASEFOLD_LANGUAGE,
        &path.to_lowercase(),
    )
}

/// Construct one exact Markdown heading identity through the declaration-key domain.
fn document_heading_resolution_key(
    project: ProjectInstanceId,
    path: &str,
    fragment: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key(
        project,
        ResolutionKeyDomain::Declaration,
        &format!("{path}#{fragment}"),
    )
}

/// Construct one project-qualified canonical document identity without a new key family.
fn document_resolution_key(
    project: ProjectInstanceId,
    domain: ResolutionKeyDomain,
    identity: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key_with_language(project, domain, DOCUMENT_PATH_LANGUAGE, identity)
}

/// Construct one project-qualified document key under a selected resolver language.
fn document_resolution_key_with_language(
    project: ProjectInstanceId,
    domain: ResolutionKeyDomain,
    resolver_language: &str,
    identity: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    let provider =
        GraphIdentityText::new(DOCUMENT_PATH_PROVIDER).map_err(invalid_graph_contract)?;
    let language = GraphIdentityText::new(resolver_language).map_err(invalid_graph_contract)?;
    let identity = GraphIdentityText::new(identity).map_err(invalid_graph_contract)?;
    Ok(CanonicalResolutionKey::new(
        project,
        domain,
        &provider,
        &language,
        None,
        None,
        Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents)),
        &identity,
    ))
}

/// Return all exact path and heading keys that can affect staged document relations.
fn document_dependency_keys(
    project: ProjectInstanceId,
    facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
) -> Result<BTreeSet<CanonicalResolutionKey>, CliError> {
    let mut keys = BTreeSet::new();
    for (document_path, facts) in facts {
        for candidate in &facts.link_candidates {
            let Ok(target) = normalize_document_target(document_path, &candidate.selector) else {
                continue;
            };
            keys.insert(document_file_resolution_key(project, &target.path)?);
            keys.insert(document_casefold_resolution_key(project, &target.path)?);
            if let Some(fragment) = target.fragment {
                keys.insert(document_heading_resolution_key(
                    project,
                    &target.path,
                    &fragment,
                )?);
            }
        }
    }
    Ok(keys)
}

/// Derive canonical resolution keys with repository compiler configuration.
fn resolution_projection_with_config(
    project: ProjectInstanceId,
    package: Option<&str>,
    graph: &SymbolGraph,
    configured_modules: &ConfiguredModuleResolution,
) -> Result<ResolutionKeyProjection, CliError> {
    let context = ResolutionProjectionContext::with_configured_modules(configured_modules);
    match derive_resolution_keys_with_context(project, package, graph, context) {
        Ok(projection) => Ok(projection),
        Err(ResolutionProjectionError::KeyLimit { requested, .. }) => {
            Err(resolution_key_limit_failure(requested))
        }
        Err(ResolutionProjectionError::Contract(failure)) => {
            let (mut failures, _rejected_count, _projection) = (*failure).into_parts();
            let Some(failure) = failures.pop() else {
                return Err(CliError::InvalidInput(
                    "resolution projection reported an empty contract failure".to_string(),
                ));
            };
            let (_fact, error) = failure.into_parts();
            Err(invalid_graph_contract(error))
        }
    }
}

/// Preserve resource-limit failures while admitting contract-invalid key facts.
fn resolution_key_limit_failure(requested: usize) -> CliError {
    IndexWorkFailure::resource_limit(
        IndexWorkStage::SymbolParsing,
        IndexWorkResource::RelationRows,
        u64::try_from(MAX_RESOLUTION_KEYS_PER_FACT).unwrap_or(u64::MAX),
        u64::try_from(requested).unwrap_or(u64::MAX),
    )
    .into()
}

/// Load the project identity required by normalized graph projection.
fn selected_project(store: &AtlasStore) -> Result<ProjectInstanceId, CliError> {
    store.project_instance_id()?.ok_or_else(|| {
        CliError::InvalidInput("repository graph requires a bound project identity".to_string())
    })
}

/// Return the pending graph generation after one successful publication.
fn next_generation(base: IndexGeneration) -> Result<IndexGeneration, CliError> {
    base.checked_next().ok_or_else(|| {
        CliError::InvalidInput("repository graph generation is exhausted".to_string())
    })
}

/// Insert one stable entity while rejecting digest collisions.
fn insert_entity(
    entities: &mut BTreeMap<String, GraphEntity>,
    entity: GraphEntity,
) -> Result<(), CliError> {
    let digest = entity.key().digest().to_string();
    if let Some(existing) = entities.get(&digest) {
        if !existing
            .key()
            .reconcile(entity.key())
            .map_err(invalid_graph_contract)?
        {
            return Err(CliError::InvalidInput(
                "graph entity digest retained conflicting ownership".to_string(),
            ));
        }
        return Ok(());
    }
    entities.insert(digest, entity);
    Ok(())
}

/// Sort and deduplicate entity export bindings by canonical identity and owner.
fn sort_dedup_exports(exports: &mut Vec<EntityResolutionKey>) {
    exports.sort_by(|left, right| {
        left.key()
            .cmp(right.key())
            .then_with(|| left.entity().digest().cmp(right.entity().digest()))
    });
    exports.dedup();
}

/// Sort and deduplicate relation dependency bindings by canonical key and owner.
fn sort_dedup_dependencies(dependencies: &mut Vec<RelationDependencyKey>) {
    dependencies.sort_by(|left, right| {
        left.key()
            .cmp(right.key())
            .then_with(|| left.relation().digest().cmp(right.relation().digest()))
    });
    dependencies.dedup();
}

/// Enforce the complete-projection ceiling for canonical key bindings.
fn enforce_key_binding_limit(count: usize) -> Result<(), CliError> {
    let observed = u64::try_from(count).unwrap_or(u64::MAX);
    if observed > MAX_GRAPH_KEY_BINDINGS {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::RelationRows,
            MAX_GRAPH_KEY_BINDINGS,
            observed,
        )
        .into());
    }
    Ok(())
}

/// Enforce one path/key count before incremental graph work expands further.
fn enforce_incremental_count<T>(
    root: &Path,
    _context: &'static str,
    count: usize,
    sample: &BTreeSet<T>,
) -> Result<(), CliError>
where
    T: ToString + Ord,
{
    if count > MAX_INCREMENTAL_RESOLUTION_ITEMS as usize {
        return Err(dependency_closure_limit(
            root,
            sample.iter().map(ToString::to_string),
            count,
        ));
    }
    Ok(())
}

/// Load and admit the persisted rows that an incremental replacement will remove.
fn admitted_persisted_footprint(
    store: &AtlasStore,
    project: ProjectInstanceId,
    root: &Path,
    affected_paths: &BTreeSet<String>,
    control: &IndexWorkControl,
) -> Result<RepositoryAffectedSourceFootprint, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let footprint = store.repository_affected_source_footprint(
        project,
        &affected_paths.iter().cloned().collect::<Vec<_>>(),
        u32::try_from(MAX_INCREMENTAL_GRAPH_ROWS).unwrap_or(u32::MAX),
    )?;
    control.check(IndexWorkStage::SymbolParsing)?;
    if footprint.truncated {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            usize::try_from(footprint.rows).unwrap_or(usize::MAX),
        ));
    }
    enforce_incremental_projection_budget(
        root,
        affected_paths,
        footprint.rows,
        footprint.retained_bytes,
    )?;
    Ok(footprint)
}

/// Enforce aggregate old-removal and new-insertion work for one closure.
fn enforce_incremental_projection_limits(
    root: &Path,
    affected_paths: &BTreeSet<String>,
    persisted: RepositoryAffectedSourceFootprint,
    staged: &StagedRepositoryGraph,
) -> Result<(), CliError> {
    let staged_rows = [
        affected_paths.len(),
        staged.entities.len(),
        staged.relations.len(),
        staged.occurrences.len(),
        staged.coverage.len(),
        staged.entity_exports.len(),
        staged.relation_dependencies.len(),
        staged.document_unresolved_reasons.len(),
    ]
    .into_iter()
    .fold(0_u64, |total, count| {
        total.saturating_add(u64::try_from(count).unwrap_or(u64::MAX))
    });
    enforce_incremental_projection_budget(
        root,
        affected_paths,
        persisted.rows.saturating_add(staged_rows),
        persisted
            .retained_bytes
            .saturating_add(staged.retained_bytes),
    )
}

/// Return typed full-refresh guidance when an incremental projection exceeds a bound.
fn enforce_incremental_projection_budget(
    root: &Path,
    affected_paths: &BTreeSet<String>,
    rows: u64,
    retained_bytes: u64,
) -> Result<(), CliError> {
    if rows > MAX_INCREMENTAL_GRAPH_ROWS || retained_bytes > MAX_INCREMENTAL_GRAPH_BYTES {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            affected_paths.len(),
        ));
    }
    Ok(())
}

/// Construct deterministic full-refresh guidance for an oversized dependency closure.
fn dependency_closure_limit(
    root: &Path,
    sample: impl IntoIterator<Item = String>,
    observed: usize,
) -> CliError {
    CliError::RefreshRequired(Box::new(IndexRefreshRequired {
        project_root: lossless_project_root_display(root),
        worktree: None,
        status: IndexReadStatus::RefreshRequired,
        reason: IndexRefreshReason::DependencyClosureLimit,
        scope: IndexRefreshScope::Full,
        changed: observed,
        added: 0,
        removed: 0,
        modified: observed,
        sample_paths: sample
            .into_iter()
            .take(INDEX_FRESHNESS_SAMPLE_LIMIT)
            .collect(),
    }))
}

/// Translate a graph-domain contract violation for the CLI boundary.
fn invalid_graph_contract(error: GraphContractError) -> CliError {
    CliError::from(error)
}

impl From<GraphContractError> for CliError {
    fn from(error: GraphContractError) -> Self {
        Self::InvalidInput(format!("repository graph projection failed: {error}"))
    }
}

/// Count conservative retained canonical resolution-key bytes.
fn resolution_retained_bytes(projection: &ResolutionKeyProjection) -> u64 {
    projection
        .source_keys()
        .iter()
        .chain(
            projection
                .symbol_keys()
                .iter()
                .flat_map(projectatlas_symbols::SymbolResolutionKeys::keys),
        )
        .chain(
            projection
                .relation_keys()
                .iter()
                .flat_map(projectatlas_symbols::RelationResolutionKeys::keys),
        )
        .fold(0_u64, |bytes, key| {
            bytes
                .saturating_add(32)
                .saturating_add(key.canonical_identity().len() as u64)
        })
}

/// Count one admitted projection entry while it remains in the source map.
fn resolution_projection_map_entry_retained_bytes(
    path: &str,
    projection: &ResolutionKeyProjection,
) -> u64 {
    STAGED_GRAPH_ROW_BYTES
        .saturating_add(path.len() as u64)
        .saturating_add(resolution_retained_bytes(projection))
}

/// Count all projection-map entries that coexist with the growing entity state.
fn resolution_projection_map_retained_bytes(
    projections: &BTreeMap<String, ResolutionKeyProjection>,
) -> u64 {
    projections.iter().fold(0_u64, |bytes, (path, projection)| {
        bytes.saturating_add(resolution_projection_map_entry_retained_bytes(
            path, projection,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CliError, DOCUMENT_PROJECTION_ROW_BYTES, DocumentResolutionIndex, DocumentTargetIdentity,
        GRAPH_STAGE_DATABASE_FILE_NAME, GRAPH_STAGE_DIRECTORY_PREFIX, GraphIdentityAdmission,
        GraphOwners, GraphSymbolIndex, MAX_IN_MEMORY_GRAPH_WORK_BYTES, MAX_INCREMENTAL_GRAPH_BYTES,
        MAX_INCREMENTAL_GRAPH_ROWS, PARTIAL_COVERAGE_REASON, PackageIndex,
        ProjectResolutionRegistry, QUALIFIED_SYMBOL_SCOPE_PREFIX, RepositoryGraphMutation,
        StagedRepositoryGraph, build_entity_projection, build_entity_projection_with_config,
        build_entity_projection_with_config_limit, cleanup_abandoned_graph_staging, coverage_for_graph,
        document_casefold_resolution_key, document_coverage, document_fact_map_retained_bytes,
        document_projection_retained_bytes, enforce_incremental_projection_budget,
        enforce_incremental_projection_limits, enforce_resolution_staging_budget,
        explicit_external_selector, finish_projection, finish_projection_in_database,
        finish_projection_in_database_with_documents, finish_projection_with_documents,
        identity_rejection_keys_retained_bytes, insert_relation, is_cargo_manifest_path,
        normalize_document_target, project_document_rows, qualified_symbol_identity,
        qualified_symbol_parents, registry_resolution_matches, relation_resolution,
        remove_owned_graph_stage_payload, repository_path_belongs_to,
        resolution_projection_map_retained_bytes, resolution_registry_from_exports,
        rust_toolchain_identity, source_symbol_identity, stage_full_repository_graph,
        stage_incremental_repository_graph, stage_incremental_repository_graph_with_test_limit,
        try_graph_stage_lease,
    };
    use crate::runtime::{
        IndexRefreshReason, IndexRefreshScope, SymbolBuildReport, SymbolBuildStage,
        SymbolParseSuccess, SymbolProjectionChange,
    };
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
        CoverageState, DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector,
        ExtendedRelationKind, GraphEntity, GraphIdentityField, GraphIdentityRejectionReason,
        GraphIdentityText, GraphLimitKind, GraphLimits, GraphRelationKind, LogicalRelation,
        MAX_GRAPH_IDENTITY_BYTES, PackageSelector, ProjectInstanceId, RelationDependencyKey,
        RelationResolution, RepositoryFilePath, RepositoryNodePath, ResolutionKeyDomain,
        ReusableTargetSelector, SymbolSelector,
    };
    use projectatlas_core::relation_capabilities::{
        RELATION_FAMILY_CAPABILITIES, RelationFamilyState,
    };
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SourceParseMetadata, SymbolGraph, SymbolKind,
        SymbolRelation,
    };
    use projectatlas_core::{
        IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
        IndexWorkStage, Node, NodeKind,
    };
    use projectatlas_db::{
        AtlasStore, RepositoryAffectedSourceFootprint, RepositoryGraphRelationQuery,
    };
    use projectatlas_fs::{RootScanPolicy, ScanOptions};
    use projectatlas_symbols::extract_symbol_graph;
    use projectatlas_symbols::{
        ConfiguredModuleResolution, EcmaScriptConfigKind, EcmaScriptModuleConfig,
        EcmaScriptPathMapping, MAX_DOCUMENT_LINK_CANDIDATES, MAX_DOCUMENT_SELECTOR_BYTES,
        MAX_MARKDOWN_EVIDENCE_BYTES, MAX_MARKDOWN_LABEL_BYTES, MarkdownFactLimit,
    };
    use rusqlite::Connection;
    use std::borrow::Cow;
    use std::collections::{BTreeMap, BTreeSet};
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use std::num::NonZeroU32;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn paired_multiline_import_rejection_is_one_parser_fact() -> Result<(), Box<dyn Error>> {
        let graph = extract_symbol_graph(
            "src/page.ts",
            Some("typescript"),
            "import {\n  helper\n} from './LeakedIdentity\u{0}module';\n",
        );
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (admitted, report) = super::admit_symbol_graph(Cow::Owned(graph), &control)?;
        require_eq(
            &report.rejected_facts_for("src/page.ts"),
            &1,
            "paired multiline import omission count",
        )?;
        require_eq(
            &report.rejections.len(),
            &3,
            "paired multiline import typed detail count",
        )?;
        require(
            report.rejections.iter().all(|rejection| {
                rejection.fact_index == report.rejections[0].fact_index
                    && rejection.span.start_line() == 1
                    && rejection.span.end_line() == 1
            }),
            "paired multiline import details did not share one parser span",
        )?;
        for field in [
            GraphIdentityField::RelationTarget,
            GraphIdentityField::Signature,
            GraphIdentityField::Symbol,
        ] {
            require(
                report
                    .rejections
                    .iter()
                    .any(|rejection| rejection.field == field),
                "paired multiline import lost a typed invalid field",
            )?;
        }
        require(
            admitted.symbols.is_empty() && admitted.relations.is_empty(),
            "paired multiline import retained an invalid parser fact",
        )?;
        Ok(())
    }

    #[test]
    fn paired_import_admission_scans_scale_bound_once_and_deduplicates_replay()
    -> Result<(), Box<dyn Error>> {
        const IMPORT_SYMBOLS: usize = 4_000;
        const IMPORT_RELATIONS: usize = 8_000;
        const PAIRING_WORK_ITEMS: usize = IMPORT_SYMBOLS + IMPORT_RELATIONS;
        let graph = |path: &str, invalid: bool| SymbolGraph {
            path: path.to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: (0..IMPORT_SYMBOLS)
                .map(|index| {
                    let name = if invalid {
                        format!("invalid\0import-{index}")
                    } else {
                        format!("import-{index}")
                    };
                    CodeSymbol {
                        path: path.to_string(),
                        language: Some("typescript".to_string()),
                        name: name.clone(),
                        kind: SymbolKind::Import,
                        signature: name,
                        exported: false,
                        documentation: None,
                        line_start: 1,
                        line_end: 2,
                        source_selector: None,
                        parent: None,
                        parser: ParserKind::TreeSitter,
                        detail: Some("import_statement".to_string()),
                    }
                })
                .collect(),
            relations: (0..IMPORT_RELATIONS)
                .map(|index| SymbolRelation {
                    path: path.to_string(),
                    source_name: path.to_string(),
                    target_name: if invalid {
                        format!("invalid\0module-{index}")
                    } else {
                        format!("module-{index}")
                    },
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import".to_string(),
                    parser: ParserKind::TreeSitter,
                })
                .collect(),
        };
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let (valid, valid_report) = super::admit_symbol_graph(
            Cow::Owned(graph("src/import-scale-valid.ts", false)),
            &control,
        )?;
        require_eq(
            &valid_report.paired_import_pairing_work,
            &PAIRING_WORK_ITEMS,
            "valid import pairing work",
        )?;
        require_eq(
            &valid.symbols.len(),
            &IMPORT_SYMBOLS,
            "valid import symbol count",
        )?;
        require_eq(
            &valid.relations.len(),
            &IMPORT_RELATIONS,
            "valid import relation count",
        )?;
        require_eq(
            &valid_report.rejected_facts_for("src/import-scale-valid.ts"),
            &0,
            "valid import rejection count",
        )?;

        let invalid_graph = graph("src/import-scale-invalid.ts", true);
        let (invalid, mut invalid_report) =
            super::admit_symbol_graph(Cow::Owned(invalid_graph.clone()), &control)?;
        require_eq(
            &invalid_report.paired_import_pairing_work,
            &PAIRING_WORK_ITEMS,
            "invalid import pairing work",
        )?;
        require(
            invalid.symbols.is_empty() && invalid.relations.is_empty(),
            "invalid import facts survived admission",
        )?;
        require_eq(
            &invalid_report.rejected_facts_for("src/import-scale-invalid.ts"),
            &u64::try_from(IMPORT_RELATIONS)?,
            "invalid import rejection count",
        )?;
        require_eq(
            &invalid_report.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "invalid import detail ceiling",
        )?;

        let (replayed, replay_report) =
            super::admit_symbol_graph(Cow::Owned(invalid_graph), &control)?;
        require(
            replayed.symbols.is_empty() && replayed.relations.is_empty(),
            "replayed invalid import facts survived admission",
        )?;
        require_eq(
            &replay_report.paired_import_pairing_work,
            &PAIRING_WORK_ITEMS,
            "replayed import pairing work",
        )?;
        invalid_report.merge(replay_report, &control)?;
        require_eq(
            &invalid_report.paired_import_pairing_work,
            &(PAIRING_WORK_ITEMS * 2),
            "merged replay import pairing work",
        )?;
        require_eq(
            &invalid_report.rejected_facts_for("src/import-scale-invalid.ts"),
            &u64::try_from(IMPORT_RELATIONS)?,
            "replayed import rejection count",
        )?;
        require_eq(
            &invalid_report.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "replayed import detail ceiling",
        )?;
        Ok(())
    }

    #[test]
    fn admission_counts_distinct_same_span_facts_but_deduplicates_replays()
    -> Result<(), Box<dyn Error>> {
        let mut admission = GraphIdentityAdmission::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failure = [(
            GraphIdentityField::RelationTarget,
            GraphIdentityRejectionReason::Empty,
        )];
        admission.record(
            "src/facts.ts",
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 1),
            &failure,
            &control,
        )?;
        admission.record(
            "src/facts.ts",
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 2),
            &failure,
            &control,
        )?;
        admission.record(
            "src/facts.ts",
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 1),
            &failure,
            &control,
        )?;
        require_eq(
            &admission.rejected_facts_for("src/facts.ts"),
            &2,
            "same-span distinct rejection count",
        )?;
        require_eq(
            &admission.rejections.len(),
            &2,
            "same-span distinct typed detail count",
        )?;
        require(
            admission.rejections[0].fact_index != admission.rejections[1].fact_index,
            "same-span distinct facts shared an internal identity",
        )?;
        Ok(())
    }

    #[test]
    fn rejection_membership_preserves_distinct_fields_across_replay_and_merge()
    -> Result<(), Box<dyn Error>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failures = [
            (
                GraphIdentityField::RelationSource,
                GraphIdentityRejectionReason::ControlCharacters,
            ),
            (
                GraphIdentityField::RelationTarget,
                GraphIdentityRejectionReason::ControlCharacters,
            ),
            (
                GraphIdentityField::Parent,
                GraphIdentityRejectionReason::ControlCharacters,
            ),
            (
                GraphIdentityField::Signature,
                GraphIdentityRejectionReason::ControlCharacters,
            ),
        ];
        let mut admission = GraphIdentityAdmission::default();
        admission.record(
            "src/multi-field.ts",
            span,
            ParserKind::TreeSitter,
            17,
            &failures,
            &control,
        )?;
        admission.record(
            "src/multi-field.ts",
            span,
            ParserKind::TreeSitter,
            17,
            &failures,
            &control,
        )?;
        require_eq(
            &admission.rejections.len(),
            &failures.len(),
            "same-fact replay collapsed distinct rejection fields",
        )?;
        for &(field, reason) in &failures {
            require(
                admission
                    .rejections
                    .iter()
                    .any(|rejection| rejection.field == field && rejection.reason == reason),
                "same-fact rejection field was lost",
            )?;
        }

        let mut aggregated = Vec::new();
        let mut aggregated_keys = BTreeSet::new();
        super::extend_bounded_identity_rejections(
            &mut aggregated,
            &mut aggregated_keys,
            admission.rejections.iter().cloned(),
        )?;
        super::extend_bounded_identity_rejections(
            &mut aggregated,
            &mut aggregated_keys,
            admission.rejections,
        )?;
        require_eq(
            &aggregated.len(),
            &failures.len(),
            "bounded aggregation collapsed distinct rejection fields",
        )?;
        require_eq(
            &aggregated_keys.len(),
            &failures.len(),
            "bounded aggregation membership did not deduplicate replay",
        )?;
        Ok(())
    }

    #[test]
    fn admission_merge_deduplicates_replayed_observed_facts() -> Result<(), Box<dyn Error>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failure = [(
            GraphIdentityField::RelationTarget,
            GraphIdentityRejectionReason::Empty,
        )];
        let mut first = GraphIdentityAdmission::default();
        for fact_index in 1..=2 {
            first.record(
                "src/merged.ts",
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, fact_index),
                &failure,
                &control,
            )?;
        }
        let mut replay = GraphIdentityAdmission::default();
        for fact_index in 1..=3 {
            replay.record(
                "src/merged.ts",
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, fact_index),
                &failure,
                &control,
            )?;
        }
        first.merge(replay, &control)?;
        require_eq(
            &first.rejected_facts_for("src/merged.ts"),
            &3,
            "merged replayed identity count",
        )?;
        require_eq(
            &first.rejections.len(),
            &3,
            "merged replayed typed detail count",
        )?;
        Ok(())
    }

    #[test]
    fn admission_deduplicates_replayed_facts_after_detail_ceiling() -> Result<(), Box<dyn Error>> {
        let mut admission = GraphIdentityAdmission::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failure = [(
            GraphIdentityField::RelationTarget,
            GraphIdentityRejectionReason::Empty,
        )];
        for fact_index in 0..super::MAX_GRAPH_IDENTITY_REJECTIONS {
            admission.record(
                "src/capped.ts",
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, fact_index),
                &failure,
                &control,
            )?;
        }
        require_eq(
            &admission.rejected_facts_for("src/capped.ts"),
            &u64::try_from(super::MAX_GRAPH_IDENTITY_REJECTIONS)?,
            "capped distinct rejection count",
        )?;
        require_eq(
            &admission.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "capped typed detail count",
        )?;
        admission.record(
            "src/capped.ts",
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 0),
            &failure,
            &control,
        )?;
        require_eq(
            &admission.rejected_facts_for("src/capped.ts"),
            &u64::try_from(super::MAX_GRAPH_IDENTITY_REJECTIONS)?,
            "replayed capped rejection count",
        )?;
        require_eq(
            &admission.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "replayed capped typed detail count",
        )?;
        admission.record(
            "src/capped.ts",
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(
                super::RELATION_FACT_INDEX_NAMESPACE,
                super::MAX_GRAPH_IDENTITY_REJECTIONS,
            ),
            &failure,
            &control,
        )?;
        require_eq(
            &admission.rejected_facts_for("src/capped.ts"),
            &u64::try_from(super::MAX_GRAPH_IDENTITY_REJECTIONS + 1)?,
            "new capped rejection count",
        )?;
        require_eq(
            &admission.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "new capped typed detail count",
        )?;
        let mut aggregated = Vec::new();
        let mut aggregated_keys = BTreeSet::new();
        super::extend_bounded_identity_rejections(
            &mut aggregated,
            &mut aggregated_keys,
            admission.rejections.iter().cloned(),
        )?;
        super::extend_bounded_identity_rejections(
            &mut aggregated,
            &mut aggregated_keys,
            admission.rejections,
        )?;
        require_eq(
            &aggregated.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "bounded aggregation changed the distinct detail cardinality",
        )?;
        require_eq(
            &aggregated_keys.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "bounded aggregation membership lost a distinct detail",
        )?;
        Ok(())
    }

    #[test]
    fn identity_rejection_limit_marker_is_causal_and_path_scoped() -> Result<(), Box<dyn Error>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let failure = [(
            GraphIdentityField::Symbol,
            GraphIdentityRejectionReason::Empty,
        )];
        let span = |line| super::IdentitySpan {
            start_line: line,
            start_column: 0,
            end_line: line,
            end_column: 0,
        };
        let mut admission = GraphIdentityAdmission::default();
        for fact_index in 0..super::MAX_GRAPH_IDENTITY_REJECTIONS {
            admission.record(
                "src/exact-cap.ts",
                span(fact_index + 1),
                ParserKind::TreeSitter,
                super::parser_fact_index(super::SYMBOL_FACT_INDEX_NAMESPACE, fact_index),
                &failure,
                &control,
            )?;
        }
        require(
            admission.rejection_details_dropped_by_path.is_empty(),
            "an exactly full retained detail set claimed an eviction",
        )?;

        let graph = |path: &str, parser: ParserKind| SymbolGraph {
            path: path.to_string(),
            language: Some("test".to_string()),
            parser,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let exact_cap = super::coverage_for_graph(
            &graph("src/exact-cap.ts", ParserKind::TreeSitter),
            IndexGeneration::new(1),
            &admission,
            &GraphIdentityAdmission::default(),
        )?;
        require_eq(
            &exact_cap.reached_limit(),
            &None,
            "exactly full retained details reported an eviction",
        )?;

        admission.record(
            "src/exact-parser.ts",
            span(1),
            ParserKind::TreeSitter,
            super::parser_fact_index(super::SYMBOL_FACT_INDEX_NAMESPACE, 10_000),
            &failure,
            &control,
        )?;
        admission.record(
            "src/markdown-structural.md",
            span(1),
            ParserKind::Structural,
            super::parser_fact_index(super::MARKDOWN_FACT_INDEX_NAMESPACE, 0),
            &failure,
            &control,
        )?;
        require(
            admission
                .rejection_details_dropped_by_path
                .contains("src/exact-parser.ts")
                && admission
                    .rejection_details_dropped_by_path
                    .contains("src/markdown-structural.md"),
            "distinct exact and structural evictions were not marked by path",
        )?;
        for (path, parser) in [
            ("src/exact-parser.ts", ParserKind::TreeSitter),
            ("src/markdown-structural.md", ParserKind::Structural),
        ] {
            let coverage = super::coverage_for_graph(
                &graph(path, parser),
                IndexGeneration::new(1),
                &admission,
                &GraphIdentityAdmission::default(),
            )?;
            require_eq(
                &coverage.reached_limit(),
                &Some(GraphLimitKind::Rows),
                "causal identity eviction did not reach path coverage",
            )?;
        }

        for parser in [ParserKind::Structural, ParserKind::Fallback] {
            let coverage = super::coverage_for_graph(
                &graph("src/baseline.rs", parser),
                IndexGeneration::new(1),
                &GraphIdentityAdmission::default(),
                &GraphIdentityAdmission::default(),
            )?;
            require_eq(
                &coverage.reached_limit(),
                &None,
                "parser baseline omission claimed identity-detail eviction",
            )?;
        }
        let unrelated = super::coverage_for_graph(
            &graph("src/unrelated.rs", ParserKind::TreeSitter),
            IndexGeneration::new(1),
            &admission,
            &GraphIdentityAdmission::default(),
        )?;
        require_eq(
            &unrelated.reached_limit(),
            &None,
            "an unrelated path inherited another path's identity eviction",
        )?;
        Ok(())
    }

    #[test]
    fn admission_merges_many_graphs_with_incremental_retained_bytes() -> Result<(), Box<dyn Error>>
    {
        const GRAPH_COUNT: usize = 1_000;
        const FACTS_PER_GRAPH: usize = 10;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failure = [(
            GraphIdentityField::RelationTarget,
            GraphIdentityRejectionReason::Empty,
        )];
        let mut admission = GraphIdentityAdmission::default();
        let mut expected_bytes = 0_u64;
        for graph_index in 0..GRAPH_COUNT {
            let path = format!("src/many-graphs-{graph_index}.ts");
            let mut graph = GraphIdentityAdmission::default();
            for fact_index in 0..FACTS_PER_GRAPH {
                graph.record(
                    &path,
                    span,
                    ParserKind::TreeSitter,
                    super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, fact_index),
                    &failure,
                    &control,
                )?;
            }
            admission.merge(graph, &control)?;
            let key_bytes = super::identity_rejection_key_retained_bytes(&path)?
                .checked_mul(u64::try_from(FACTS_PER_GRAPH)?)
                .ok_or_else(|| io::Error::other("many-graph identity bytes overflowed"))?;
            let graph_bytes = super::identity_observed_path_retained_bytes(&path, FACTS_PER_GRAPH)?
                .checked_add(super::identity_count_path_retained_bytes(&path)?)
                .and_then(|bytes| bytes.checked_add(key_bytes))
                .ok_or_else(|| io::Error::other("many-graph identity bytes overflowed"))?;
            expected_bytes = expected_bytes
                .checked_add(graph_bytes)
                .ok_or_else(|| io::Error::other("many-graph identity bytes overflowed"))?;
            require_eq(
                &admission.observed_fact_bytes,
                &expected_bytes,
                "incremental identity byte cache",
            )?;
        }
        require_eq(
            &admission.rejections.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "many-graph retained detail ceiling",
        )?;
        require_eq(
            &admission.rejection_keys.len(),
            &super::MAX_GRAPH_IDENTITY_REJECTIONS,
            "many-graph keyed detail ceiling",
        )?;
        require_eq(
            &super::identity_admission_retained_bytes(&admission),
            &expected_bytes,
            "incremental identity byte cache remained authoritative",
        )?;

        let replay_path = "src/many-graphs-999.ts";
        let mut replay = GraphIdentityAdmission::default();
        for fact_index in 0..FACTS_PER_GRAPH {
            replay.record(
                replay_path,
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, fact_index),
                &failure,
                &control,
            )?;
        }
        admission.merge(replay, &control)?;
        require_eq(
            &admission.observed_fact_bytes,
            &expected_bytes,
            "replayed graph did not inflate identity byte cache",
        )?;
        Ok(())
    }

    #[test]
    fn observed_identity_facts_charge_budget_at_minus_equal_plus_boundaries()
    -> Result<(), Box<dyn Error>> {
        let path = "src/budget.ts";
        let span = super::IdentitySpan {
            start_line: 7,
            start_column: 0,
            end_line: 7,
            end_column: 0,
        };
        let failure = [(
            GraphIdentityField::RelationTarget,
            GraphIdentityRejectionReason::Empty,
        )];
        let additional = super::identity_fact_retained_bytes(path, true, true)?
            .checked_add(super::identity_rejection_key_retained_bytes(path)?)
            .ok_or_else(|| io::Error::other("identity budget fixture overflowed"))?;
        let next_fact_additional = super::STAGED_GRAPH_ROW_BYTES
            .checked_add(super::identity_rejection_key_retained_bytes(path)?)
            .ok_or_else(|| io::Error::other("identity budget fixture overflowed"))?;

        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let mut below = GraphIdentityAdmission {
            observed_fact_bytes: MAX_IN_MEMORY_GRAPH_WORK_BYTES - additional + 1,
            ..GraphIdentityAdmission::default()
        };
        let error = below
            .record(
                path,
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 1),
                &failure,
                &control,
            )
            .err()
            .ok_or_else(|| io::Error::other("one byte over the identity budget was retained"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::SymbolParsing,
                    resource: IndexWorkResource::OutputBytes,
                    limit: MAX_IN_MEMORY_GRAPH_WORK_BYTES,
                    observed,
                }) if observed == MAX_IN_MEMORY_GRAPH_WORK_BYTES + 1
            ),
            "identity budget overflow did not return its typed refusal",
        )?;
        require(
            below.observed_facts.is_empty() && below.rejected_facts_by_path.is_empty(),
            "identity budget refusal retained partial state",
        )?;

        let mut equal = GraphIdentityAdmission {
            observed_fact_bytes: MAX_IN_MEMORY_GRAPH_WORK_BYTES - additional,
            ..GraphIdentityAdmission::default()
        };
        equal.record(
            path,
            span,
            ParserKind::TreeSitter,
            super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 1),
            &failure,
            &control,
        )?;
        require_eq(
            &equal.observed_fact_bytes,
            &MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            "identity budget exact boundary",
        )?;
        require_eq(
            &equal.rejected_facts_for(path),
            &1,
            "identity budget exact boundary count",
        )?;

        let error = equal
            .record(
                path,
                span,
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 2),
                &failure,
                &control,
            )
            .err()
            .ok_or_else(|| io::Error::other("one additional identity fact exceeded the budget"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::SymbolParsing,
                    resource: IndexWorkResource::OutputBytes,
                    limit: MAX_IN_MEMORY_GRAPH_WORK_BYTES,
                    observed,
                }) if observed == MAX_IN_MEMORY_GRAPH_WORK_BYTES + next_fact_additional
            ),
            "identity budget plus boundary did not return its typed refusal",
        )?;
        require_eq(
            &equal.rejected_facts_for(path),
            &1,
            "identity plus boundary changed retained count",
        )?;
        Ok(())
    }

    #[test]
    fn observed_identity_facts_honor_cancellation_before_retention() -> Result<(), Box<dyn Error>> {
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        cancellation.cancel();
        let mut admission = GraphIdentityAdmission::default();
        let error = admission
            .record(
                "src/canceled.ts",
                super::IdentitySpan {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                ParserKind::TreeSitter,
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 0),
                &[(
                    GraphIdentityField::RelationTarget,
                    GraphIdentityRejectionReason::Empty,
                )],
                &control,
            )
            .err()
            .ok_or_else(|| io::Error::other("canceled identity admission retained a fact"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::SymbolParsing
                })
            ),
            "identity admission cancellation was not typed",
        )?;
        require(
            admission.observed_facts.is_empty()
                && admission.rejected_facts_by_path.is_empty()
                && admission.observed_fact_bytes == 0,
            "canceled identity admission retained partial state",
        )?;
        Ok(())
    }

    #[test]
    fn reused_reconciliation_paths_charge_all_maps_at_injected_boundaries()
    -> Result<(), Box<dyn Error>> {
        let mut report = GraphIdentityAdmission::default();
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        for index in 0..32 {
            let path = format!("src/reused-{index}.ts");
            for _ in 0..4 {
                report.reserve_reused_path_bytes(
                    &path,
                    false,
                    &control,
                    MAX_IN_MEMORY_GRAPH_WORK_BYTES,
                )?;
            }
            report.reused_rejection_counts.insert(path.clone(), 2);
            report
                .reused_parser_rejection_counts
                .insert(path.clone(), 1);
            report
                .reused_rejection_detail_counts
                .insert(path.clone(), 0);
            report.reused_rejection_details_incomplete.insert(path);
        }
        let retained = super::identity_admission_retained_bytes(&report);
        require(
            retained > 0,
            "reused reconciliation bytes were not retained",
        )?;

        let below = retained - 1;
        let error = super::checked_identity_admission_budget(&report, &control, below)
            .err()
            .ok_or_else(|| io::Error::other("below-limit reused paths were accepted"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::SymbolParsing,
                    resource: IndexWorkResource::OutputBytes,
                    limit,
                    observed,
                }) if limit == below && observed == retained
            ),
            "below-limit reused paths did not return their typed refusal",
        )?;
        require_eq(
            &super::checked_identity_admission_budget(&report, &control, retained)?,
            &retained,
            "equal-limit reused paths",
        )?;
        require_eq(
            &super::checked_identity_admission_budget(&report, &control, retained + 1)?,
            &retained,
            "above-limit reused paths",
        )?;
        Ok(())
    }

    #[test]
    fn reused_reconciliation_retention_honors_cancellation_before_insertion()
    -> Result<(), Box<dyn Error>> {
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        cancellation.cancel();
        let mut report = GraphIdentityAdmission::default();
        let error = report
            .reserve_reused_path_bytes(
                "src/canceled-reuse.ts",
                false,
                &control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )
            .err()
            .ok_or_else(|| io::Error::other("canceled reused path was retained"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::SymbolParsing
                })
            ),
            "reused path cancellation was not typed",
        )?;
        require(
            report.reused_rejection_counts.is_empty()
                && report.reused_parser_rejection_counts.is_empty()
                && report.reused_rejection_detail_counts.is_empty()
                && report.reused_rejection_details_incomplete.is_empty()
                && report.observed_fact_bytes == 0,
            "canceled reused path retained reconciliation state",
        )?;

        let mut incoming = GraphIdentityAdmission::default();
        incoming
            .reused_rejection_counts
            .insert("src/incoming.ts".to_string(), 1);
        let error = report
            .merge(incoming, &control)
            .err()
            .ok_or_else(|| io::Error::other("canceled merge retained reconciliation state"))?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::SymbolParsing
                })
            ),
            "merge cancellation was not typed",
        )?;
        require(
            report.reused_rejection_counts.is_empty() && report.observed_fact_bytes == 0,
            "canceled merge retained reconciliation state",
        )?;
        Ok(())
    }

    #[test]
    fn reused_identity_counts_preserve_new_and_removed_derived_outcomes()
    -> Result<(), Box<dyn Error>> {
        let path = "src/reused.ts";
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let failure = [(
            GraphIdentityField::ResolutionKey,
            GraphIdentityRejectionReason::Oversized,
        )];
        let mut admission = GraphIdentityAdmission::default();
        for _ in 0..2 {
            admission.reserve_reused_path_bytes(
                path,
                false,
                &control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
        }
        admission
            .reused_rejection_counts
            .insert(path.to_string(), 3);
        admission
            .reused_rejection_detail_counts
            .insert(path.to_string(), 2);
        for fact_index in 0..2 {
            admission.record(
                path,
                super::IdentitySpan {
                    start_line: fact_index + 1,
                    start_column: 0,
                    end_line: fact_index + 1,
                    end_column: 0,
                },
                ParserKind::TreeSitter,
                fact_index as u64,
                &failure,
                &control,
            )?;
        }
        let persisted_facts = admission
            .observed_facts
            .get(path)
            .cloned()
            .ok_or("persisted identity fact keys are missing")?;
        admission.reserve_identity_bytes(
            super::identity_fact_set_retained_bytes(path, &persisted_facts)?,
            &control,
            MAX_IN_MEMORY_GRAPH_WORK_BYTES,
        )?;
        admission
            .reused_rejection_facts
            .insert(path.to_string(), persisted_facts);
        require_eq(
            &admission.rejected_facts_for_graph(path, &GraphIdentityAdmission::default())?,
            &3,
            "reused unchanged rejection count",
        )?;

        let mut derived = GraphIdentityAdmission::default();
        derived.record(
            path,
            super::IdentitySpan {
                start_line: 3,
                start_column: 0,
                end_line: 3,
                end_column: 0,
            },
            ParserKind::TreeSitter,
            2,
            &failure,
            &control,
        )?;
        require_eq(
            &admission.rejected_facts_for_graph(path, &derived)?,
            &4,
            "reused genuinely new rejection count",
        )?;

        let mut removed = GraphIdentityAdmission::default();
        for _ in 0..2 {
            removed.reserve_reused_path_bytes(
                path,
                false,
                &control,
                MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            )?;
        }
        removed.reused_rejection_counts.insert(path.to_string(), 2);
        removed
            .reused_rejection_detail_counts
            .insert(path.to_string(), 2);
        removed.reused_rejection_facts.insert(
            path.to_string(),
            admission
                .reused_rejection_facts
                .get(path)
                .cloned()
                .ok_or("persisted identity fact keys are missing")?,
        );
        let persisted_facts = removed
            .reused_rejection_facts
            .get(path)
            .cloned()
            .ok_or("persisted identity fact keys are missing")?;
        removed.reserve_identity_bytes(
            super::identity_fact_set_retained_bytes(path, &persisted_facts)?,
            &control,
            MAX_IN_MEMORY_GRAPH_WORK_BYTES,
        )?;
        removed.record(
            path,
            super::IdentitySpan {
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            ParserKind::TreeSitter,
            0,
            &failure,
            &control,
        )?;
        require_eq(
            &removed.rejected_facts_for_graph(path, &GraphIdentityAdmission::default())?,
            &1,
            "reused removed rejection count",
        )?;
        require_eq(
            &removed.rejected_facts_for_graph(path, &derived)?,
            &2,
            "reused removed plus new rejection count",
        )?;
        Ok(())
    }

    #[test]
    fn relation_rejections_keep_original_parser_ordinals_through_reopen_and_retry()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join(".projectatlas"))?;
        let database = root.join(".projectatlas/projectatlas.db");
        let components = (0..20)
            .map(|_| "d".repeat(200))
            .collect::<Vec<_>>()
            .join("/");
        let path = format!("src/{components}/page.ts");
        let invalid_target = "x".repeat(MAX_GRAPH_IDENTITY_BYTES + 1);
        let oversized_resolved_module = "m".repeat(100);
        let graph = SymbolGraph {
            path: path.clone(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: path.clone(),
                language: Some("typescript".to_string()),
                name: "caller".to_string(),
                kind: SymbolKind::Function,
                signature: "function caller()".to_string(),
                exported: true,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: None,
            }],
            relations: vec![
                SymbolRelation {
                    path: path.clone(),
                    source_name: "caller".to_string(),
                    target_name: invalid_target,
                    kind: RelationKind::Calls,
                    line: 1,
                    context: "caller()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: path.clone(),
                    source_name: "<module>".to_string(),
                    target_name: format!("import {{ run }} from './{oversized_resolved_module}';"),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "relation ordinal fixture".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        };
        let nodes = vec![test_file_node(&path, "typescript")];
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let symbols = symbol_build_stage_for_graphs(vec![graph.clone()]);
        let staged = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &symbols,
            &control,
        )?;
        let relation_rejections = staged
            .identity_rejections
            .iter()
            .filter(|row| {
                matches!(
                    row.field,
                    GraphIdentityField::RelationTarget | GraphIdentityField::ResolutionKey
                )
            })
            .collect::<Vec<_>>();
        require_eq(
            &relation_rejections.len(),
            &2,
            "same-line relation rejection detail count",
        )?;
        require(
            relation_rejections.iter().all(|row| {
                row.path.as_str() == path
                    && row.span.start_line() == 1
                    && row.span.end_line() == 1
                    && row.parser == ParserKind::TreeSitter
                    && row.reason == GraphIdentityRejectionReason::Oversized
            }),
            "same-line relation rejection provenance was not exact",
        )?;
        let mut fact_indices = relation_rejections
            .iter()
            .map(|row| row.fact_index)
            .collect::<Vec<_>>();
        fact_indices.sort_unstable();
        require_eq(
            &fact_indices,
            &vec![
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 0),
                super::parser_fact_index(super::RELATION_FACT_INDEX_NAMESPACE, 1),
            ],
            "same-line relation parser ordinals",
        )?;
        require_eq(
            &staged.relations.len(),
            &1,
            "valid relation retained after source admission",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &staged,
            &control,
            "relation-ordinal-full",
        )?;
        let first_generation = store
            .index_publication()?
            .ok_or("relation ordinal full publication is missing")?
            .generation;
        let path_key = RepositoryNodePath::new(Path::new(&path))?;
        let persisted = store.repository_graph_identity_rejections(
            store
                .project_instance_id()?
                .ok_or("relation ordinal project identity is missing")?,
            std::slice::from_ref(&path_key),
            16,
            None,
        )?;
        let persisted_fact_indices = persisted
            .iter()
            .filter(|row| {
                matches!(
                    row.field,
                    GraphIdentityField::RelationTarget | GraphIdentityField::ResolutionKey
                )
            })
            .map(|row| row.fact_index)
            .collect::<Vec<_>>();
        require_eq(
            &persisted_fact_indices,
            &fact_indices,
            "persisted same-line relation parser ordinals",
        )?;
        let persisted_wire = serde_json::to_string(&persisted)?;
        require(
            !persisted_wire.contains(&"x".repeat(32)),
            "persisted relation rejection retained invalid identity text",
        )?;
        drop(store);

        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let reopened = store.repository_graph_identity_rejections(
            store
                .project_instance_id()?
                .ok_or("reopened relation ordinal project identity is missing")?,
            std::slice::from_ref(&path_key),
            16,
            None,
        )?;
        require_eq(
            &reopened
                .iter()
                .filter(|row| {
                    matches!(
                        row.field,
                        GraphIdentityField::RelationTarget | GraphIdentityField::ResolutionKey
                    )
                })
                .map(|row| row.fact_index)
                .collect::<Vec<_>>(),
            &fact_indices,
            "reopened same-line relation parser ordinals",
        )?;

        let incremental_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let incremental_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &incremental_control)?;
        let fault_graph = graph.clone();
        let incremental_symbols = symbol_build_stage_for_graphs(vec![graph]);
        let incremental = stage_incremental_repository_graph(
            &store,
            &root,
            first_generation,
            &nodes,
            std::slice::from_ref(&path),
            &incremental_policy,
            &incremental_symbols,
            &incremental_control,
        )?;
        let incremental_rejections = incremental
            .identity_rejections
            .iter()
            .filter(|row| {
                matches!(
                    row.field,
                    GraphIdentityField::RelationTarget | GraphIdentityField::ResolutionKey
                )
            })
            .map(|row| row.fact_index)
            .collect::<Vec<_>>();
        require_eq(
            &incremental_rejections,
            &fact_indices,
            "incremental same-line relation parser ordinals",
        )?;
        let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled.cancel();
        {
            let mut publication = store.begin_index_publication("relation-ordinal-cancel")?;
            require(
                incremental.apply(&mut publication, &canceled).is_err(),
                "relation ordinal cancellation did not fail publication",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(first_generation),
            "generation after canceled relation ordinal publication",
        )?;
        {
            let mut publication = store.begin_index_publication("relation-ordinal-incremental")?;
            incremental.apply(&mut publication, &incremental_control)?;
            publication.complete()?;
        }
        let second_generation = store
            .index_publication()?
            .ok_or("relation ordinal incremental publication is missing")?
            .generation;
        drop(store);
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let reopened_incremental = store.repository_graph_identity_rejections(
            store
                .project_instance_id()?
                .ok_or("reopened incremental project identity is missing")?,
            std::slice::from_ref(&path_key),
            16,
            None,
        )?;
        require(
            reopened_incremental.iter().any(|row| {
                matches!(
                    row.field,
                    GraphIdentityField::RelationTarget | GraphIdentityField::ResolutionKey
                )
            }),
            "reopened incremental relation rejection is missing",
        )?;
        let mut fault = stage_incremental_repository_graph(
            &store,
            &root,
            second_generation,
            &nodes,
            std::slice::from_ref(&path),
            &incremental_policy,
            &symbol_build_stage_for_graphs(vec![fault_graph]),
            &incremental_control,
        )?;
        fault.identity_rejections.resize(
            usize::try_from(GraphLimits::MAX_ROWS)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
            fault.identity_rejections[0].clone(),
        );
        {
            let mut publication = store.begin_index_publication("relation-ordinal-fault")?;
            require(
                fault.apply(&mut publication, &incremental_control).is_err(),
                "relation ordinal late fault did not fail publication",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(second_generation),
            "generation after relation ordinal late fault",
        )?;
        Ok(())
    }

    #[test]
    fn sqlite_recovered_invalid_manifest_context_uses_shared_admission()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let invalid_manifest = package_graph("Cargo.toml", "bad\0package");
        let direct_graph = function_graph("src/lib.rs", 1);
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_symbol_graph(&invalid_manifest)?;
        store.replace_symbol_graph(&direct_graph)?;
        drop(store);

        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        let recovered = reopened
            .load_symbol_graphs_for_paths(&["Cargo.toml".to_string(), "src/lib.rs".to_string()])?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (admitted, report) =
            super::admit_symbol_graphs(recovered.into_iter().map(Cow::Owned).collect(), &control)?;
        require_eq(
            &report.rejected_facts_for("Cargo.toml"),
            &1,
            "recovered invalid manifest rejection count",
        )?;
        let manifest = admitted
            .iter()
            .find(|graph| graph.as_ref().path == "Cargo.toml")
            .ok_or("recovered manifest graph is missing")?;
        require(
            manifest.as_ref().symbols.is_empty(),
            "invalid recovered package identity reached package context",
        )?;
        let admitted_graphs = admitted.iter().map(Cow::as_ref).collect::<Vec<_>>();
        let packages = PackageIndex::from_graphs(&admitted_graphs)?;
        require_eq(
            &packages.package_name("src/lib.rs"),
            &None,
            "invalid recovered manifest silently supplied package ownership",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(source) if source.raw_os_error() == Some(1314) => {
                let status = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(link)
                    .arg(target)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(source)
                }
            }
            Err(source) => Err(source),
        }
    }

    #[cfg(unix)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[test]
    fn cargo_package_ownership_uses_the_longest_repository_prefix() -> Result<(), Box<dyn Error>> {
        let graphs = vec![
            package_graph("Cargo.toml", "workspace"),
            package_graph("crates/member/Cargo.toml", "member"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        require_eq(
            &packages.package_name("src/lib.rs"),
            &Some("workspace"),
            "root package ownership",
        )?;
        require_eq(
            &packages.package_name("crates/member/src/lib.rs"),
            &Some("member"),
            "nested package ownership",
        )?;
        require(
            repository_path_belongs_to("crates/member/src/lib.rs", "crates/member"),
            "member path was not owned by its package",
        )?;
        require(
            !repository_path_belongs_to("crates/membership/src/lib.rs", "crates/member"),
            "package prefix matched a partial segment",
        )?;
        require(is_cargo_manifest_path("Cargo.toml"), "root manifest")?;
        require(
            is_cargo_manifest_path("crates/member/Cargo.toml"),
            "nested manifest",
        )?;
        require(
            !is_cargo_manifest_path("docs/Cargo.toml.example"),
            "manifest suffix lookalike",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_projection_budget_requests_one_complete_refresh() -> Result<(), Box<dyn Error>> {
        let root = Path::new("repository");
        let affected_paths = BTreeSet::from(["src/lib.rs".to_string()]);
        for (rows, retained_bytes) in [
            (MAX_INCREMENTAL_GRAPH_ROWS + 1, 0),
            (0, MAX_INCREMENTAL_GRAPH_BYTES + 1),
        ] {
            let error =
                enforce_incremental_projection_budget(root, &affected_paths, rows, retained_bytes)
                    .err()
                    .ok_or_else(|| io::Error::other("oversized closure reached publication"))?;
            let CliError::RefreshRequired(report) = error else {
                return Err(io::Error::other(format!(
                    "expected typed full-refresh guidance, found {error:?}"
                ))
                .into());
            };
            require_eq(
                &report.reason,
                &IndexRefreshReason::DependencyClosureLimit,
                "incremental budget reason",
            )?;
            require_eq(
                &report.scope,
                &IndexRefreshScope::Full,
                "incremental budget scope",
            )?;
            require_eq(&report.changed, &1, "incremental changed paths")?;
            require_eq(
                &report.sample_paths,
                &vec!["src/lib.rs".to_string()],
                "incremental sample paths",
            )?;
        }
        Ok(())
    }

    #[test]
    fn admitted_resolution_keys_move_once_and_count_at_budget_boundary()
    -> Result<(), Box<dyn Error>> {
        let graph = function_graph("src/lib.rs", 1);
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&graph))?;
        let project = ProjectInstanceId::from_bytes([31; 16])?;
        let admitted_projection = projectatlas_symbols::derive_resolution_keys(
            project,
            packages.package_name(&graph.path),
            &graph,
        )?;
        let admitted_key_bytes = super::resolution_retained_bytes(&admitted_projection);
        let mut admitted = BTreeMap::from([(graph.path.clone(), admitted_projection)]);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let entities = build_entity_projection_with_config(
            project,
            IndexGeneration::new(1),
            &[],
            std::slice::from_ref(&graph),
            &packages,
            &ConfiguredModuleResolution::default(),
            Some(&mut admitted),
            true,
            &control,
        )?;
        require(
            admitted.is_empty(),
            "admitted resolution map retained moved keys",
        )?;
        require(
            entities.retained_bytes >= admitted_key_bytes,
            "entity projection budget omitted live resolution-key bytes",
        )?;

        let mut registry = ProjectResolutionRegistry {
            retained_bytes: super::super::MAX_PUBLICATION_STAGING_BYTES
                .saturating_sub(entities.retained_bytes),
            ..ProjectResolutionRegistry::default()
        };
        require(
            enforce_resolution_staging_budget(&entities, &registry).is_ok(),
            "exact staging budget boundary was rejected",
        )?;
        registry.retained_bytes = registry.retained_bytes.saturating_add(1);
        require(
            enforce_resolution_staging_budget(&entities, &registry).is_err(),
            "staging budget ignored live resolution-key bytes at the boundary",
        )?;
        Ok(())
    }

    #[test]
    fn multi_graph_resolution_peak_moves_each_entry_before_entity_allocation()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([32; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            package_graph("Cargo.toml", "peak-workspace"),
            function_graph("src/target.rs", 64),
            function_graph("src/caller.rs", 64),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let fresh_admitted = || {
            graphs
                .iter()
                .map(|graph| {
                    Ok::<_, Box<dyn Error>>((
                        graph.path.clone(),
                        projectatlas_symbols::derive_resolution_keys(
                            project,
                            packages.package_name(&graph.path),
                            graph,
                        )?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
        };
        let mut admitted = fresh_admitted()?;
        require_eq(
            &admitted.len(),
            &graphs.len(),
            "multi-graph admitted resolution projection count",
        )?;
        let map_bytes = resolution_projection_map_retained_bytes(&admitted);
        require(
            map_bytes > 0,
            "multi-graph projection map had no retained bytes",
        )?;
        let first = build_entity_projection_with_config_limit(
            project,
            generation,
            &[],
            &graphs,
            &packages,
            &ConfiguredModuleResolution::default(),
            Some(&mut admitted),
            true,
            &control,
            super::super::MAX_PUBLICATION_STAGING_BYTES,
        )?;
        require(
            admitted.is_empty(),
            "full multi-graph projection retained admitted map entries",
        )?;
        require_eq(
            &first.projection_removals_before_entities,
            &graphs
                .iter()
                .map(|graph| graph.path.clone())
                .collect::<Vec<_>>(),
            "full projection removal order",
        )?;
        require(
            first.peak_retained_bytes >= map_bytes,
            "full peak accounting omitted the admitted projection map",
        )?;
        let peak = first.peak_retained_bytes;
        let mut at_peak_admitted = fresh_admitted()?;
        let at_peak = build_entity_projection_with_config_limit(
            project,
            generation.checked_next().ok_or("generation overflow")?,
            &[],
            &graphs,
            &packages,
            &ConfiguredModuleResolution::default(),
            Some(&mut at_peak_admitted),
            true,
            &control,
            peak,
        )?;
        require(
            at_peak_admitted.is_empty(),
            "exact-peak multi-graph projection retained admitted map entries",
        )?;
        require_eq(
            &at_peak.projection_removals_before_entities,
            &graphs
                .iter()
                .map(|graph| graph.path.clone())
                .collect::<Vec<_>>(),
            "exact-peak projection removal order",
        )?;
        require(
            at_peak.peak_retained_bytes <= peak,
            "exact-peak projection exceeded the measured full peak unexpectedly",
        )?;
        let below_peak = peak.checked_sub(1).ok_or("multi-graph peak was zero")?;
        require_eq(
            &peak.saturating_sub(below_peak),
            &1,
            "multi-graph below-peak budget was not exactly one byte lower",
        )?;
        let mut below_peak_admitted = fresh_admitted()?;
        let below_peak_result = build_entity_projection_with_config_limit(
            project,
            generation.checked_next().ok_or("generation overflow")?,
            &[],
            &graphs,
            &packages,
            &ConfiguredModuleResolution::default(),
            Some(&mut below_peak_admitted),
            true,
            &control,
            below_peak,
        );
        require(
            matches!(
                &below_peak_result,
                Err(CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::OutputBytes,
                    limit,
                    observed,
                    ..
                })) if *limit == below_peak && *observed == peak
            ),
            "one byte below the measured full peak did not fail at the measured peak",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_direct_and_inbound_keys_are_counted_once_at_budget_boundary()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("incremental-resolution-budget");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let database = root.join(".projectatlas/projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let manifest_path = "Cargo.toml";
        let root_module_path = "src/lib.rs";
        let direct_path = "src/target.rs";
        let inbound_path = "src/caller.rs";
        let manifest_source =
            "[package]\nname = \"dependency-refresh\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let root_module_source = "mod caller;\nmod target;\n";
        let direct_source = "pub fn target() {}\n";
        let inbound_source = "pub fn caller() { target(); }\n";
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join(manifest_path), manifest_source)?;
        fs::write(root.join(root_module_path), root_module_source)?;
        fs::write(root.join(direct_path), direct_source)?;
        fs::write(root.join(inbound_path), inbound_source)?;
        let nodes = vec![
            test_file_node(manifest_path, "cargo-manifest"),
            test_file_node(root_module_path, "rust"),
            test_file_node(direct_path, "rust"),
            test_file_node(inbound_path, "rust"),
        ];
        let graphs = vec![
            extract_symbol_graph(manifest_path, Some("cargo-manifest"), manifest_source),
            extract_symbol_graph(root_module_path, Some("rust"), root_module_source),
            extract_symbol_graph(direct_path, Some("rust"), direct_source),
            extract_symbol_graph(inbound_path, Some("rust"), inbound_source),
        ];
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let full = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &symbol_build_stage_for_graphs(graphs.clone()),
            &control,
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &full,
            &control,
            "incremental-budget-full",
        )?;
        for graph in &graphs {
            store.replace_symbol_graph(graph)?;
        }
        drop(store);
        let store = AtlasStore::open_for_project(&database, &root)?;
        let base_generation = store
            .index_publication()?
            .ok_or("incremental budget full publication is missing")?
            .generation;
        let incremental = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            &[direct_path.to_string()],
            &scan_policy,
            &symbol_build_stage_for_graphs(vec![graphs[2].clone()]),
            &control,
        )?;
        let affected_paths = BTreeSet::from([direct_path.to_string(), inbound_path.to_string()]);
        require(
            matches!(
                &incremental.mutation,
                RepositoryGraphMutation::AffectedPaths(paths)
                    if paths.iter().cloned().collect::<BTreeSet<_>>() == affected_paths
            ),
            "incremental budget fixture did not include its inbound graph",
        )?;
        let expected_generation = base_generation
            .checked_next()
            .ok_or("incremental budget generation overflowed")?;
        for path in [&direct_path, &inbound_path] {
            let derivations = incremental
                .resolution_derivations
                .get(&(path.to_string(), expected_generation))
                .copied();
            require_eq(
                &derivations,
                &Some(1),
                "incremental direct/inbound resolution derivation count",
            )?;
        }
        let peak = incremental.peak_retained_bytes;
        require(peak > 0, "incremental projection measured no retained peak")?;
        let expected_removals = vec![direct_path.to_string(), inbound_path.to_string()];
        require_eq(
            &incremental.projection_removals_before_entities,
            &expected_removals,
            "incremental projection removal order",
        )?;
        let unrelated_manifest_derivations = incremental
            .resolution_derivations
            .get(&(manifest_path.to_string(), expected_generation))
            .copied();
        require_eq(
            &unrelated_manifest_derivations,
            &None,
            "incremental unrelated manifest resolution derivation count",
        )?;
        let unrelated_root_derivations = incremental
            .resolution_derivations
            .get(&(root_module_path.to_string(), expected_generation))
            .copied();
        require_eq(
            &unrelated_root_derivations,
            &None,
            "incremental unrelated root-module resolution derivation count",
        )?;
        let rows = u64::try_from(incremental.entities.len())?;
        let retained_bytes = incremental.retained_bytes;
        drop(incremental);
        let at_peak = stage_incremental_repository_graph_with_test_limit(
            &store,
            &root,
            base_generation,
            &nodes,
            &[direct_path.to_string()],
            &scan_policy,
            &symbol_build_stage_for_graphs(vec![graphs[2].clone()]),
            &control,
            peak,
        )?;
        require_eq(
            &at_peak.projection_removals_before_entities,
            &expected_removals,
            "exact-peak incremental projection removal order",
        )?;
        require(
            at_peak.peak_retained_bytes <= peak,
            "exact-peak incremental projection exceeded its measured peak",
        )?;
        for path in [&direct_path, &inbound_path] {
            require_eq(
                &at_peak
                    .resolution_derivations
                    .get(&(path.to_string(), expected_generation))
                    .copied(),
                &Some(1),
                "exact-peak incremental derivation count",
            )?;
        }
        require_eq(
            &at_peak
                .resolution_derivations
                .get(&(manifest_path.to_string(), expected_generation))
                .copied(),
            &None,
            "exact-peak unrelated manifest derivation count",
        )?;
        require_eq(
            &at_peak
                .resolution_derivations
                .get(&(root_module_path.to_string(), expected_generation))
                .copied(),
            &None,
            "exact-peak unrelated root-module derivation count",
        )?;
        drop(at_peak);
        let below_peak = peak.checked_sub(1).ok_or("incremental peak was zero")?;
        require_eq(
            &peak.saturating_sub(below_peak),
            &1,
            "incremental below-peak budget was not exactly one byte lower",
        )?;
        let below_peak_result = stage_incremental_repository_graph_with_test_limit(
            &store,
            &root,
            base_generation,
            &nodes,
            &[direct_path.to_string()],
            &scan_policy,
            &symbol_build_stage_for_graphs(vec![graphs[2].clone()]),
            &control,
            below_peak,
        );
        require(
            matches!(
                &below_peak_result,
                Err(CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::OutputBytes,
                    limit,
                    observed,
                    ..
                })) if *limit == below_peak && *observed == peak
            ),
            "one byte below the measured incremental peak did not fail at the measured peak",
        )?;
        require(
            retained_bytes > 0,
            "incremental budget fixture did not retain projected key bytes",
        )?;
        require(
            enforce_incremental_projection_budget(&root, &affected_paths, rows, retained_bytes)
                .is_ok(),
            "incremental budget rejected actual direct and inbound projection",
        )?;
        let available = MAX_INCREMENTAL_GRAPH_BYTES.saturating_sub(retained_bytes);
        require(
            enforce_incremental_projection_budget(
                &root,
                &affected_paths,
                rows,
                retained_bytes.saturating_add(available),
            )
            .is_ok(),
            "incremental exact byte boundary was rejected",
        )?;
        require(
            enforce_incremental_projection_budget(
                &root,
                &affected_paths,
                rows,
                retained_bytes.saturating_add(available).saturating_add(1),
            )
            .is_err(),
            "incremental byte boundary ignored one additional byte",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_projection_budget_combines_old_and_new_work() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let affected_paths = BTreeSet::from(["src/lib.rs".to_string()]);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let persisted = RepositoryAffectedSourceFootprint {
            rows: MAX_INCREMENTAL_GRAPH_ROWS,
            retained_bytes: 0,
            truncated: false,
        };
        let staged = StagedRepositoryGraph {
            project: ProjectInstanceId::from_bytes([1; 16])?,
            mutation: RepositoryGraphMutation::AffectedPaths(vec!["src/lib.rs".to_string()]),
            entities: Vec::new(),
            relations: Vec::new(),
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: Vec::new(),
            relation_dependencies: Vec::new(),
            document_unresolved_reasons: Vec::new(),
            identity_rejections: Vec::new(),
            resolution_derivations: BTreeMap::new(),
            peak_retained_bytes: 0,
            projection_removals_before_entities: Vec::new(),
            scan_policy: RootScanPolicy::discover(root, &ScanOptions::default(), &control)?,
            document_target_states: Vec::new(),
            database: None,
            retained_bytes: 0,
        };
        let error =
            enforce_incremental_projection_limits(root, &affected_paths, persisted, &staged)
                .err()
                .ok_or_else(|| io::Error::other("combined old and new work was admitted"))?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "combined budget reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "combined budget scope",
        )?;
        Ok(())
    }

    #[test]
    fn document_projection_memory_budget_includes_reason_validation_state()
    -> Result<(), Box<dyn Error>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let facts: BTreeMap<String, Cow<'static, projectatlas_symbols::MarkdownFacts>> = (0..12)
            .map(|document| {
                let source = (0..1_024)
                    .map(|link| format!("[missing]({document}-{link}.md)"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    format!("docs/source-{document}.md"),
                    Cow::Owned(projectatlas_symbols::extract_markdown_facts(&source)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let candidate_count = facts
            .values()
            .map(|document| document.link_candidates.len())
            .sum::<usize>();
        require_eq(&candidate_count, &12_288, "document candidate fixture size")?;
        let fact_bytes = document_fact_map_retained_bytes(&facts);
        let projection_bytes = document_projection_retained_bytes(&facts, &control)?;
        require(
            projection_bytes >= u64::try_from(candidate_count)? * DOCUMENT_PROJECTION_ROW_BYTES,
            "generated document projection state was under-accounted",
        )?;
        require(
            fact_bytes.saturating_add(projection_bytes) < 512 * 1_024 * 1_024,
            "bounded document projection fixture exceeded the in-memory envelope",
        )?;
        Ok(())
    }

    #[test]
    fn document_projection_estimator_deadline_preserves_generation_and_allows_retry()
    -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 513;
        const CANDIDATES_PER_DOCUMENT: usize = 1_024;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("estimator cancellation project identity is missing")?;
        let publication_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let baseline_graph = function_graph("src/lib.rs", 1);
        let baseline_packages = PackageIndex::from_graphs(std::slice::from_ref(&baseline_graph))?;
        let baseline_entities = build_entity_projection(
            project,
            IndexGeneration::new(1),
            &[],
            std::slice::from_ref(&baseline_graph),
            &baseline_packages,
            true,
            &publication_control,
        )?;
        let baseline_candidates =
            resolution_registry_from_exports(&baseline_entities, &publication_control)?;
        let baseline_staged = finish_projection(
            project,
            IndexGeneration::new(1),
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&baseline_graph),
            baseline_entities,
            &baseline_candidates,
            &publication_control,
        )?;
        let baseline_node = test_file_node("src/lib.rs", "rust");
        {
            let mut publication = store.begin_index_publication("estimator-cancellation")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(std::slice::from_ref(&baseline_node))?;
            publication.finish_scan_replacement()?;
            baseline_staged.apply(&mut publication, &publication_control)?;
            publication.complete()?;
        }
        let publication_before = store
            .index_publication()?
            .ok_or("estimator cancellation baseline publication is missing")?;

        let document_facts: BTreeMap<String, Cow<'_, projectatlas_symbols::MarkdownFacts>> = (0
            ..DOCUMENT_COUNT)
            .map(|document| {
                let path = format!("content/source-{document:03}.md");
                let file_name = path
                    .rsplit_once('/')
                    .map_or(path.as_str(), |(_parent, file_name)| file_name);
                let source = (0..CANDIDATES_PER_DOCUMENT)
                    .map(|index| format!("[self-{index:04}]({file_name})"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    path,
                    Cow::Owned(projectatlas_symbols::extract_markdown_facts(&source)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let candidate_count = document_facts
            .values()
            .map(|facts| facts.link_candidates.len())
            .sum::<usize>();
        require_eq(
            &candidate_count,
            &(DOCUMENT_COUNT * CANDIDATES_PER_DOCUMENT),
            "estimator cancellation candidate count",
        )?;

        let expired_control =
            IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
        let error = document_projection_retained_bytes(&document_facts, &expired_control)
            .err()
            .ok_or("expired estimator unexpectedly traversed all candidates")?;
        require(
            matches!(
                error,
                CliError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::SymbolParsing,
                })
            ),
            "estimator deadline did not return its typed graph-work failure",
        )?;
        require_eq(
            &store.index_publication()?,
            &Some(publication_before.clone()),
            "estimator deadline changed the current generation",
        )?;

        let retry_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let retained_bytes = document_projection_retained_bytes(&document_facts, &retry_control)?;
        require_eq(
            &retained_bytes,
            &0,
            "same-file candidates entered the retained projection estimate",
        )?;
        let retry_graphs = document_facts
            .iter()
            .map(|(path, facts)| facts.symbol_graph(path, Some("markdown")))
            .collect::<Vec<_>>();
        let retry_nodes = document_facts
            .keys()
            .map(|path| test_file_node(path, "markdown"))
            .collect::<Vec<_>>();
        let retry_scan_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &retry_control)?;
        let retry_packages = PackageIndex::from_graphs(&retry_graphs)?;
        let retry_entities = build_entity_projection(
            project,
            IndexGeneration::new(2),
            &retry_nodes,
            &retry_graphs,
            &retry_packages,
            false,
            &retry_control,
        )?;
        let retry_candidates = resolution_registry_from_exports(&retry_entities, &retry_control)?;
        let retried_projection = finish_projection_with_documents(
            project,
            IndexGeneration::new(2),
            RepositoryGraphMutation::Full,
            &retry_graphs,
            &root,
            &retry_nodes,
            &document_facts,
            &GraphIdentityAdmission::default(),
            retry_entities,
            &retry_candidates,
            &retry_scan_policy,
            &retry_control,
        )?;
        require(
            retried_projection.relations.is_empty()
                && retried_projection.document_unresolved_reasons.is_empty(),
            "same-file retry emitted document projection rows",
        )?;
        require_eq(
            &store.index_publication()?,
            &Some(publication_before),
            "estimator retry changed the current generation without publication",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_document_projection_overflow_requests_full_refresh_staging()
    -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 513;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("incremental document budget project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let template = projectatlas_symbols::extract_markdown_facts("[missing](shared.md)");
        let candidate = template
            .link_candidates
            .first()
            .cloned()
            .ok_or("document budget candidate fixture is missing")?;
        let mut template = template;
        while template.link_candidates.len() < MAX_DOCUMENT_LINK_CANDIDATES {
            template.link_candidates.push(candidate.clone());
        }

        let mut nodes = Vec::with_capacity(DOCUMENT_COUNT);
        let mut graphs = Vec::with_capacity(DOCUMENT_COUNT);
        let mut changes = Vec::with_capacity(DOCUMENT_COUNT);
        let mut paths = Vec::with_capacity(DOCUMENT_COUNT);
        for document in 0..DOCUMENT_COUNT {
            let path = format!("content/source-{document:03}.md");
            let graph = template.symbol_graph(&path, Some("markdown"));
            nodes.push(test_file_node(&path, "markdown"));
            paths.push(path.clone());
            graphs.push(graph.clone());
            changes.push(SymbolProjectionChange::Parsed(SymbolParseSuccess {
                path,
                graph,
                markdown_facts: Some(Box::new(template.clone())),
                source_parser: ParserKind::Structural,
                summary: String::new(),
                summary_is_structural: true,
                purpose_suggestion: None,
            }));
        }
        store.replace_scan(&nodes)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entities = build_entity_projection(
            project,
            IndexGeneration::new(1),
            &nodes,
            &graphs,
            &packages,
            false,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let projected_facts = paths
            .iter()
            .map(|path| (path.clone(), Cow::Borrowed(&template)))
            .collect::<BTreeMap<_, _>>();
        let pre_document_bytes = entities
            .retained_bytes
            .saturating_add(candidates.retained_bytes)
            .saturating_add(document_fact_map_retained_bytes(&projected_facts));
        let document_projection_bytes =
            document_projection_retained_bytes(&projected_facts, &control)?;
        require(
            pre_document_bytes < MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            "pre-document incremental state already exceeded the staging budget",
        )?;
        require(
            pre_document_bytes.saturating_add(document_projection_bytes)
                > MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            "document projection did not cross the aggregate staging budget",
        )?;

        let mut symbols = empty_symbol_build_stage();
        symbols.changes = changes;
        let direct_paths = paths.clone();
        let error = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &direct_paths,
            &scan_policy,
            &symbols,
            &control,
        )
        .err()
        .ok_or("document projection overflow was admitted incrementally")?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "document projection overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "document projection overflow scope",
        )?;

        let staged = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require(
            staged.database.is_some(),
            "full-refresh guidance did not select disposable SQLite staging",
        )?;
        Ok(())
    }

    #[test]
    fn resolution_registry_reuses_staged_export_entities() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([2; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"atlas\"\n",
            ),
            extract_symbol_graph("src/lib.rs", Some("rust"), "pub fn run() {}\n"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let registry = resolution_registry_from_exports(&projection, &control)?;
        let registered_bindings = registry
            .candidate_digests_by_key
            .values()
            .map(BTreeSet::len)
            .sum::<usize>();

        require_eq(
            &registry.supplemental_entities_by_digest.len(),
            &0,
            "duplicate registry-owned entity count",
        )?;
        require_eq(
            &registered_bindings,
            &projection.entity_exports.len(),
            "registry key binding count",
        )?;
        require(
            registry
                .candidate_digests_by_key
                .values()
                .flatten()
                .all(|digest| projection.entity_by_digest.contains_key(digest)),
            "resolution key referenced an unstaged entity digest",
        )?;
        let mut bindings_per_entity = BTreeMap::<&str, usize>::new();
        for digest in registry.candidate_digests_by_key.values().flatten() {
            *bindings_per_entity.entry(digest).or_default() += 1;
        }
        require(
            bindings_per_entity.values().any(|count| *count > 1),
            "fixture did not exercise one entity exported under multiple canonical keys",
        )?;
        Ok(())
    }

    #[test]
    fn restart_cleanup_removes_only_inactive_owned_graph_stages() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("restart-cleanup");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let database = atlas_dir.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;

        let owned = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}owned"));
        fs::create_dir(&owned)?;
        let owned_database = owned.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        drop(AtlasStore::create_repository_graph_staging(
            &owned_database,
            &root,
            project,
        )?);
        let owned_payload = owned.join("payload");
        fs::create_dir(&owned_payload)?;
        fs::write(owned_payload.join("row"), "discard")?;
        let owned_link_target = temp.path().join("owned-link-target");
        fs::create_dir(&owned_link_target)?;
        fs::write(owned_link_target.join("sentinel"), "preserve")?;
        let owned_payload_link = owned.join("linked-payload");
        create_directory_link(&owned_link_target, &owned_payload_link)?;
        let interrupted_shell =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}interrupted-shell"));
        fs::create_dir(&interrupted_shell)?;
        let unvalidated_nonempty = atlas_dir.join(format!(
            "{GRAPH_STAGE_DIRECTORY_PREFIX}unvalidated-nonempty"
        ));
        fs::create_dir(&unvalidated_nonempty)?;
        fs::write(unvalidated_nonempty.join("sentinel"), "preserve")?;
        let lookalike = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}lookalike"));
        fs::create_dir(&lookalike)?;
        drop(AtlasStore::open_for_project(
            &lookalike.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
        )?);
        let foreign_root = temp.path().join("foreign-project");
        fs::create_dir(&foreign_root)?;
        let foreign_store =
            AtlasStore::open_for_project(&foreign_root.join("projectatlas.db"), &foreign_root)?;
        let foreign_project = foreign_store
            .project_instance_id()?
            .ok_or("foreign project identity is missing")?;
        let foreign_project_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}foreign-project"));
        fs::create_dir(&foreign_project_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &foreign_project_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
            foreign_project,
        )?);
        let foreign_root_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}foreign-root"));
        fs::create_dir(&foreign_root_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &foreign_root_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &foreign_root,
            project,
        )?);
        let linked_stage_target = temp.path().join("linked-stage-target");
        fs::create_dir(&linked_stage_target)?;
        fs::write(linked_stage_target.join("sentinel"), "preserve")?;
        drop(AtlasStore::create_repository_graph_staging(
            &linked_stage_target.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
            project,
        )?);
        let linked_stage = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}linked"));
        create_directory_link(&linked_stage_target, &linked_stage)?;

        let linked_database_target = temp.path().join("linked-stage-database.db");
        drop(AtlasStore::create_repository_graph_staging(
            &linked_database_target,
            &root,
            project,
        )?);
        let linked_database_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}linked-database"));
        fs::create_dir(&linked_database_stage)?;
        let linked_database = linked_database_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let linked_database_created =
            match create_file_link(&linked_database_target, &linked_database) {
                Ok(()) => true,
                #[cfg(windows)]
                Err(source) if source.raw_os_error() == Some(1314) => false,
                Err(source) => return Err(source.into()),
            };
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let lease = try_graph_stage_lease(&atlas_dir)?
            .ok_or("test could not acquire graph staging lease")?;
        remove_owned_graph_stage_payload(&owned, &owned_database, Some(&control))?;
        require(
            owned_database.is_file() && !owned_payload.exists(),
            "payload cleanup did not retain the ownership database until last",
        )?;
        require(
            fs::symlink_metadata(&owned_payload_link).is_err()
                && owned_link_target.join("sentinel").is_file(),
            "payload cleanup followed or retained a linked child",
        )?;
        cleanup_abandoned_graph_staging(&root, project, &control)?;
        require(
            owned.exists(),
            "restart cleanup removed an actively leased stage",
        )?;
        drop(lease);

        let canceled_control = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled_control.cancel();
        let canceled = cleanup_abandoned_graph_staging(&root, project, &canceled_control)
            .err()
            .ok_or("canceled restart cleanup unexpectedly succeeded")?;
        require(
            matches!(
                canceled,
                CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::Publication
                })
            ),
            "restart cleanup did not preserve typed cancellation",
        )?;
        require(
            owned.exists(),
            "canceled restart cleanup removed an owned stage",
        )?;

        cleanup_abandoned_graph_staging(&root, project, &control)?;
        require(
            !owned.exists(),
            "restart cleanup retained an inactive owned stage",
        )?;
        require(
            !interrupted_shell.exists(),
            "restart cleanup retained an empty interrupted stage shell",
        )?;
        require(
            unvalidated_nonempty.join("sentinel").is_file(),
            "restart cleanup removed a non-empty unvalidated stage",
        )?;
        require(
            lookalike.exists(),
            "restart cleanup removed an unvalidated lookalike stage",
        )?;
        require(
            foreign_project_stage.exists(),
            "restart cleanup removed a valid stage owned by another project",
        )?;
        require(
            foreign_root_stage.exists(),
            "restart cleanup removed a valid stage bound to another root",
        )?;
        require(
            fs::symlink_metadata(&linked_stage).is_ok()
                && linked_stage_target.join("sentinel").is_file(),
            "restart cleanup followed a linked stage directory",
        )?;
        if linked_database_created {
            require(
                fs::symlink_metadata(&linked_database).is_ok() && linked_database_target.is_file(),
                "restart cleanup followed a linked staging database",
            )?;
        }
        Ok(())
    }

    #[test]
    fn restart_cleanup_reclaims_schema_nineteen_owned_graph_stage() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("schema-19-restart-cleanup");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let main_database = atlas_dir.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let store = AtlasStore::open_for_project(&main_database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let prepare_schema_nineteen =
            |stage: &Path, stage_root: &Path, stage_project: ProjectInstanceId| {
                fs::create_dir(stage)?;
                let database = stage.join(GRAPH_STAGE_DATABASE_FILE_NAME);
                drop(AtlasStore::create_repository_graph_staging(
                    &database,
                    stage_root,
                    stage_project,
                )?);
                let connection = Connection::open(database)?;
                connection.execute_batch(
                    "DROP TABLE project_root_identity;
                 DROP TABLE IF EXISTS graph_identity_rejections;
                 UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
                )?;
                Ok::<(), Box<dyn Error>>(())
            };

        let owned = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}schema19-owned"));
        prepare_schema_nineteen(&owned, &root, project)?;
        let owned_database = owned.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let owned_matches =
            AtlasStore::repository_graph_staging_belongs_to(&owned_database, &root, project)?;
        #[cfg(windows)]
        require(
            owned_matches,
            "schema-19 owned staging database was not admitted",
        )?;
        #[cfg(unix)]
        require(
            owned_matches,
            "schema-19 staging database was not admitted by its durable staging ownership",
        )?;
        fs::write(owned.join("large-graph-payload"), b"stale graph payload")?;

        let foreign_root = temp.path().join("schema-19-foreign-root");
        fs::create_dir(&foreign_root)?;
        let foreign = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}schema19-foreign"));
        let foreign_root_project = ProjectInstanceId::from_bytes([9; 16])?;
        prepare_schema_nineteen(&foreign, &foreign_root, foreign_root_project)?;
        let foreign_database = foreign.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        require(
            !AtlasStore::repository_graph_staging_belongs_to(&foreign_database, &root, project)?,
            "schema-19 unrelated-root staging database was admitted",
        )?;

        let schema_eighteen = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}schema18"));
        prepare_schema_nineteen(&schema_eighteen, &root, project)?;
        let schema_eighteen_database = schema_eighteen.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let connection = Connection::open(&schema_eighteen_database)?;
        connection.execute(
            "UPDATE metadata SET value = '18' WHERE key = 'schema_version'",
            [],
        )?;
        require(
            !AtlasStore::repository_graph_staging_belongs_to(
                &schema_eighteen_database,
                &root,
                project,
            )?,
            "schema-18 staging database was admitted as a schema-19 predecessor",
        )?;

        let incomplete_current =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}current-incomplete"));
        fs::create_dir(&incomplete_current)?;
        let incomplete_current_database = incomplete_current.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        drop(AtlasStore::create_repository_graph_staging(
            &incomplete_current_database,
            &root,
            project,
        )?);
        let connection = Connection::open(&incomplete_current_database)?;
        connection.execute_batch(
            "DROP TABLE project_root_identity; DROP TABLE IF EXISTS graph_identity_rejections;",
        )?;
        require(
            !AtlasStore::repository_graph_staging_belongs_to(
                &incomplete_current_database,
                &root,
                project,
            )
            .unwrap_or(false),
            "current staging database without native identity was admitted",
        )?;

        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        cleanup_abandoned_graph_staging(&root, project, &control)?;
        #[cfg(windows)]
        require(
            !owned.exists(),
            "restart cleanup retained an owned schema-19 staging database",
        )?;
        #[cfg(unix)]
        require(
            !owned.exists(),
            "restart cleanup retained an owned schema-19 staging database",
        )?;
        require(
            foreign.exists(),
            "restart cleanup removed an unrelated schema-19 staging database",
        )?;
        require(
            schema_eighteen.exists(),
            "restart cleanup removed a non-predecessor schema-18 staging database",
        )?;
        require(
            incomplete_current.exists(),
            "restart cleanup removed a current staging database without native identity",
        )?;
        Ok(())
    }

    #[test]
    fn restart_cleanup_observes_cancellation_between_owned_stages() -> Result<(), Box<dyn Error>> {
        const STAGE_COUNT: usize = 64;
        const FILES_PER_STAGE: usize = 64;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("restart-cleanup-cancellation");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let store =
            AtlasStore::open_for_project(&atlas_dir.join(GRAPH_STAGE_DATABASE_FILE_NAME), &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let mut stages = Vec::with_capacity(STAGE_COUNT);
        for stage_index in 0..STAGE_COUNT {
            let stage = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}{stage_index:03}"));
            fs::create_dir(&stage)?;
            drop(AtlasStore::create_repository_graph_staging(
                &stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
                &root,
                project,
            )?);
            let payload = stage.join("payload");
            fs::create_dir(&payload)?;
            for file_index in 0..FILES_PER_STAGE {
                fs::write(payload.join(format!("{file_index:03}")), b"x")?;
            }
            stages.push(stage);
        }

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let worker_root = root;
        let worker =
            thread::spawn(move || cleanup_abandoned_graph_staging(&worker_root, project, &control));
        let observation_deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = stages.iter().filter(|stage| stage.exists()).count();
            if remaining < STAGE_COUNT {
                cancellation.cancel();
                break;
            }
            if worker.is_finished() {
                return Err(io::Error::other(
                    "restart cleanup completed before in-flight cancellation was observed",
                )
                .into());
            }
            if Instant::now() >= observation_deadline {
                cancellation.cancel();
                return Err(io::Error::other(
                    "restart cleanup removed no stage within the test deadline",
                )
                .into());
            }
            thread::yield_now();
        }
        let result = worker
            .join()
            .map_err(|_panic| io::Error::other("restart cleanup worker panicked"))?;
        require(
            matches!(
                result,
                Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::Publication
                }))
            ),
            "restart cleanup did not return typed in-flight cancellation",
        )?;
        let remaining = stages.iter().filter(|stage| stage.exists()).count();
        require(
            remaining > 0 && remaining < STAGE_COUNT,
            "in-flight cancellation did not preserve a partial cleanup boundary",
        )
    }

    #[test]
    fn staged_database_owner_retains_incomplete_creation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir(&atlas_dir)?;
        let directory = tempfile::Builder::new()
            .prefix(GRAPH_STAGE_DIRECTORY_PREFIX)
            .tempdir_in(&atlas_dir)?;
        let staging_path = directory.path().to_path_buf();
        fs::write(
            staging_path.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            "incomplete",
        )?;
        fs::write(staging_path.join("payload"), "preserve")?;
        let lease = try_graph_stage_lease(&atlas_dir)?
            .ok_or("test could not acquire graph staging lease")?;
        let owner = super::StagedGraphDatabase {
            store: None,
            directory: Some(directory),
            _lease: lease,
        };

        drop(owner);

        require(
            staging_path.join(GRAPH_STAGE_DATABASE_FILE_NAME).is_file()
                && staging_path.join("payload").is_file(),
            "incomplete staging creation was recursively deleted",
        )
    }

    #[test]
    fn database_staging_publishes_and_removes_its_disposable_store() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("database-staging");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let database = atlas_dir.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )];
        let nodes = vec![test_file_node("src/lib.rs", "rust")];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = finish_projection_in_database(
            &root,
            &nodes,
            project,
            generation,
            &graphs,
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        let staging_path = staged
            .database
            .as_ref()
            .ok_or("database staging was not selected")?
            .directory()?
            .path()
            .to_path_buf();
        let drop_payload = staging_path.join("drop-payload");
        fs::create_dir(&drop_payload)?;
        fs::write(drop_payload.join("row"), "discard")?;
        let drop_link_target = temp.path().join("drop-link-target");
        fs::create_dir(&drop_link_target)?;
        fs::write(drop_link_target.join("sentinel"), "preserve")?;
        create_directory_link(&drop_link_target, &staging_path.join("drop-linked-payload"))?;
        require(
            staging_path.exists(),
            "database staging directory is missing",
        )?;
        {
            let mut publication = store.begin_index_publication("database-staging")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(staged);
        require(
            !staging_path.exists(),
            "database staging directory survived publication",
        )?;
        require(
            drop_link_target.join("sentinel").is_file(),
            "database staging drop followed a linked payload",
        )?;
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let coverage = reader.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/lib.rs"))?,
            },
            8,
        )?;
        require(
            !coverage.truncated,
            "database-staged coverage was truncated",
        )?;
        require(
            !coverage.rows.is_empty(),
            "database-staged graph rows were not published",
        )?;
        Ok(())
    }

    #[test]
    fn staged_identity_rejections_count_toward_normal_and_database_bytes()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("identity-rejection-bytes");
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "pub fn indexed() {}\n")?;
        let database = root.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("identity rejection fixture project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "pub fn indexed() {}\n",
        )];
        let nodes = vec![test_file_node("src/lib.rs", "rust")];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let mut admission = GraphIdentityAdmission::default();
        let rejection_path = format!("src/{}.rs", "long-rejection-path".repeat(64));
        admission.record(
            &rejection_path,
            super::IdentitySpan {
                start_line: 4,
                start_column: 0,
                end_line: 4,
                end_column: 12,
            },
            ParserKind::TreeSitter,
            7,
            &[(
                GraphIdentityField::Symbol,
                GraphIdentityRejectionReason::Oversized,
            )],
            &control,
        )?;
        let rejection_bytes = identity_rejection_keys_retained_bytes(&admission.rejection_keys)?;
        require(
            rejection_bytes > 1,
            "identity rejection fixture did not retain a bounded detail payload",
        )?;
        let build_projection = || {
            let projection = build_entity_projection(
                project, generation, &nodes, &graphs, &packages, true, &control,
            )?;
            let candidates = resolution_registry_from_exports(&projection, &control)?;
            Result::<_, Box<dyn Error>>::Ok((projection, candidates))
        };

        let (projection, candidates) = build_projection()?;
        let baseline = finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            &root,
            &nodes,
            &BTreeMap::new(),
            &GraphIdentityAdmission::default(),
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        let (projection, candidates) = build_projection()?;
        let in_memory = finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            &root,
            &nodes,
            &BTreeMap::new(),
            &admission,
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        require_eq(
            &in_memory.retained_bytes(),
            &baseline.retained_bytes().saturating_add(rejection_bytes),
            "in-memory identity rejection retained bytes",
        )?;
        require_eq(
            &in_memory.identity_rejections.len(),
            &1,
            "in-memory identity rejection detail count",
        )?;
        let parent_prefix = super::super::MAX_PUBLICATION_STAGING_BYTES
            .checked_sub(in_memory.retained_bytes())
            .ok_or("identity rejection fixture exceeded the publication budget")?;
        require(
            super::super::enforce_publication_staging_budget(
                parent_prefix.saturating_add(in_memory.retained_bytes()),
            )
            .is_ok(),
            "parent publication budget rejected exact staged identity bytes",
        )?;
        require(
            super::super::enforce_publication_staging_budget(
                parent_prefix
                    .saturating_add(in_memory.retained_bytes())
                    .saturating_add(1),
            )
            .is_err(),
            "parent publication budget accepted one byte over staged identity bytes",
        )?;
        drop(in_memory);
        drop(baseline);

        let (projection, candidates) = build_projection()?;
        let baseline_database = finish_projection_in_database_with_documents(
            &root,
            &nodes,
            project,
            generation,
            &graphs,
            &BTreeMap::new(),
            &GraphIdentityAdmission::default(),
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        let baseline_database_path_bytes = baseline_database
            .database
            .as_ref()
            .ok_or("database staging baseline was not selected")?
            .directory()?
            .path()
            .join(GRAPH_STAGE_DATABASE_FILE_NAME)
            .as_os_str()
            .as_encoded_bytes()
            .len() as u64;
        require_eq(
            &baseline_database.retained_bytes(),
            &baseline_database_path_bytes,
            "database staging baseline retained bytes",
        )?;
        drop(baseline_database);

        let (projection, candidates) = build_projection()?;
        let database_staged = finish_projection_in_database_with_documents(
            &root,
            &nodes,
            project,
            generation,
            &graphs,
            &BTreeMap::new(),
            &admission,
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        let database_path_bytes = database_staged
            .database
            .as_ref()
            .ok_or("database staging was not selected")?
            .directory()?
            .path()
            .join(GRAPH_STAGE_DATABASE_FILE_NAME)
            .as_os_str()
            .as_encoded_bytes()
            .len() as u64;
        require_eq(
            &database_staged.retained_bytes(),
            &database_path_bytes.saturating_add(rejection_bytes),
            "database staging identity rejection retained bytes",
        )?;
        Ok(())
    }

    #[test]
    fn accepted_relation_families_publish_and_reopen() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("accepted-relation-families");
        for directory in ["src", "tests", "config", "infra", "data"] {
            fs::create_dir_all(root.join(directory))?;
        }
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("relation inventory identity is missing"))?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                concat!(
                    "[package]\nname = \"relation-fixture\"\nversion = \"0.1.0\"\n",
                    "\n[dependencies]\nserde = \"1\"\n",
                ),
            ),
            extract_symbol_graph(
                "src/lib.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "pub struct Router { pub enabled: bool }\n",
                    "impl Router { pub fn install(&self) {} }\n",
                    "pub fn handler() {}\n",
                    "pub fn register() {\n",
                    "    route(\"/health\", handler);\n",
                    "    let _ = std::env::var(\"ATLAS_MODE\").unwrap_or_else(|_| \"super-secret\".into());\n",
                    "    let _ = fs::read_to_string(\"data/input.txt\");\n",
                    "    let _ = fs::write(\"data/output.txt\", \"ok\");\n",
                    "}\n",
                    "fn route(_path: &str, _handler: fn()) {}\n",
                ),
            ),
            extract_symbol_graph(
                "tests/feature_test.rs",
                Some("rust"),
                "fn subject() {}\nfn verifies_subject() { subject(); }\n",
            ),
            extract_symbol_graph(
                "config/appsettings.json",
                Some("json"),
                "{\"token\":\"super-secret\"}\n",
            ),
            extract_symbol_graph(
                "infra/main.tf",
                Some("terraform"),
                "resource \"null_resource\" \"fixture\" {}\n",
            ),
            extract_symbol_graph("data/input.txt", None, "input\n"),
            extract_symbol_graph("data/output.txt", None, ""),
        ];
        let nodes = graphs
            .iter()
            .map(|graph| {
                test_file_node(&graph.path, graph.language.as_deref().unwrap_or("unknown"))
            })
            .collect::<Vec<_>>();
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        {
            let mut publication = store.begin_index_publication("accepted-relation-families")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        for capability in RELATION_FAMILY_CAPABILITIES
            .iter()
            .filter(|capability| capability.state == RelationFamilyState::Active)
        {
            for &family in capability.graph_relations {
                let page = reader.repository_graph_relations(
                    RepositoryGraphRelationQuery::Family { relation: family },
                    128,
                )?;
                require(!page.truncated, &format!("{family:?} page was truncated"))?;
                require(
                    !page.rows.is_empty(),
                    &format!("{family:?} had no reopened persisted relation"),
                )?;
                for relation in page.rows {
                    let occurrences = reader.repository_graph_occurrences(&relation, 32)?;
                    require(
                        !occurrences.rows.is_empty() && !occurrences.truncated,
                        &format!("{family:?} lost exact source occurrences"),
                    )?;
                    require(
                        !format!("{relation:?}").contains("super-secret"),
                        &format!("secret value escaped into persisted {family:?} relation"),
                    )?;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn accepted_relation_families_abstain_without_static_evidence() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([13; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/dynamic.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "struct Client;\n",
                    "impl Client { fn get(&self, _path: &str, _handler: fn()) {} }\n",
                    "fn handler() {}\n",
                    "fn dynamic(client: &Client, route_path: &str, key: &str, file: &str) {\n",
                    "    client.get(\"/health\", handler);\n",
                    "    route(route_path, handler);\n",
                    "    let _ = std::env::var(key);\n",
                    "    let _ = fs::read_to_string(file);\n",
                    "    let _ = fs::write(file, \"super-secret\");\n",
                    "}\n",
                    "fn route(_path: &str, _handler: fn()) {}\n",
                ),
            ),
            extract_symbol_graph(
                "src/escaping.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "fn unsafe_paths() {\n",
                    "    let _ = fs::read_to_string(\"../secret.txt\");\n",
                    "    let _ = fs::write(\"C:/secret.txt\", \"super-secret\");\n",
                    "}\n",
                ),
            ),
            extract_symbol_graph(
                "src/dynamic.js",
                Some("javascript"),
                concat!(
                    "function handler() {}\n",
                    "function middleware() {}\n",
                    "function route(...args) {}\n",
                    "route(\"/health\", handler, middleware);\n",
                ),
            ),
            extract_symbol_graph("docs/main.tf.example", None, "not infrastructure\n"),
            extract_symbol_graph("docs/k8s/README.md", None, "not infrastructure\n"),
            extract_symbol_graph("docs/cloudformation-notes.md", None, "not infrastructure\n"),
            extract_symbol_graph("config/settings.json.bak", None, "super-secret\n"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        for family in [
            ExtendedRelationKind::Tests,
            ExtendedRelationKind::RoutesTo,
            ExtendedRelationKind::Configures,
            ExtendedRelationKind::Deploys,
            ExtendedRelationKind::Reads,
            ExtendedRelationKind::Writes,
        ] {
            require(
                staged
                    .relations
                    .iter()
                    .all(|relation| relation.kind() != GraphRelationKind::Extended(family)),
                &format!("dynamic or lookalike input fabricated {family:?}"),
            )?;
        }
        require(
            staged
                .entities
                .iter()
                .all(|entity| !entity.key().canonical_identity().contains("super-secret")),
            "negative fixture leaked a secret into graph identity",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_identity_preserves_boundaries_and_compacts_stably()
    -> Result<(), Box<dyn Error>> {
        let name = GraphIdentityText::new("leaf")?;
        require_eq(
            &qualified_symbol_identity(&GraphIdentityText::new("outer")?, &name)?,
            &GraphIdentityText::new("outer::leaf")?,
            "shallow qualified identity",
        )?;

        let exact_parent = GraphIdentityText::new(
            "x".repeat(MAX_GRAPH_IDENTITY_BYTES - "::".len() - name.as_str().len()),
        )?;
        let exact = qualified_symbol_identity(&exact_parent, &name)?;
        require_eq(
            &exact.as_str().len(),
            &MAX_GRAPH_IDENTITY_BYTES,
            "exact-boundary qualified identity bytes",
        )?;
        require(
            exact.as_str().ends_with("::leaf"),
            "exact-boundary qualified identity changed its readable suffix",
        )?;

        let first_overbound_parent = GraphIdentityText::new(
            "x".repeat(MAX_GRAPH_IDENTITY_BYTES - "::".len() - name.as_str().len() + 1),
        )?;
        let compact = qualified_symbol_identity(&first_overbound_parent, &name)?;
        require(
            compact.as_str().len() <= MAX_GRAPH_IDENTITY_BYTES,
            "first overbound qualified identity remained oversized",
        )?;
        require(
            compact.as_str().ends_with("::leaf"),
            "compacted qualified identity lost its nearest symbol name",
        )?;
        require_eq(
            &qualified_symbol_identity(&first_overbound_parent, &name)?,
            &compact,
            "repeated compact qualified identity",
        )?;

        let multibyte_parent = GraphIdentityText::new(format!(
            "{}x",
            "é".repeat((MAX_GRAPH_IDENTITY_BYTES - "::".len() - "界".len() - 1) / "é".len())
        ))?;
        let multibyte =
            qualified_symbol_identity(&multibyte_parent, &GraphIdentityText::new("界")?)?;
        require_eq(
            &multibyte.as_str().len(),
            &MAX_GRAPH_IDENTITY_BYTES,
            "multibyte exact-boundary qualified identity bytes",
        )?;
        require(
            multibyte.as_str().ends_with("::界"),
            "multibyte qualified identity changed its readable suffix",
        )?;

        let parent_a = GraphIdentityText::new("a".repeat(MAX_GRAPH_IDENTITY_BYTES))?;
        let parent_b = GraphIdentityText::new("b".repeat(MAX_GRAPH_IDENTITY_BYTES))?;
        let scoped_a = qualified_symbol_identity(&parent_a, &name)?;
        let scoped_b = qualified_symbol_identity(&parent_b, &name)?;
        require(
            scoped_a != scoped_b,
            "distinct deep ancestors with an equal suffix shared one identity",
        )?;
        require(
            scoped_a != qualified_symbol_identity(&parent_a, &GraphIdentityText::new("other")?)?,
            "distinct overbound candidates shared one compact identity",
        )?;
        let (literal_parent, _) = scoped_a
            .as_str()
            .rsplit_once("::")
            .ok_or_else(|| io::Error::other("compact scope omitted its readable suffix"))?;
        require(
            source_symbol_identity(literal_parent.to_string()).is_err(),
            "compact scope namespace remained admissible as an exact source parent",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_parents_reject_invalid_raw_components() -> Result<(), Box<dyn Error>> {
        for (name, parent) in [
            ("bad\nname".to_string(), None),
            ("valid".to_string(), Some("bad\nparent".to_string())),
            (
                format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}literal"),
                Some("parent".to_string()),
            ),
            (
                "valid".to_string(),
                Some(format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}literal")),
            ),
            (
                "x".repeat(MAX_GRAPH_IDENTITY_BYTES + 1),
                Some("parent".to_string()),
            ),
        ] {
            let graph = SymbolGraph {
                path: "src/invalid.rs".to_string(),
                language: Some("rust".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![CodeSymbol {
                    path: "src/invalid.rs".to_string(),
                    language: Some("rust".to_string()),
                    name,
                    kind: SymbolKind::Function,
                    signature: "fn valid()".to_string(),
                    exported: false,
                    documentation: None,
                    line_start: 1,
                    line_end: 1,
                    source_selector: None,
                    parent,
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_string()),
                }],
                relations: Vec::new(),
            };
            require(
                qualified_symbol_parents(&graph).is_err(),
                "invalid raw symbol identity reached qualified derivation",
            )?;
        }
        Ok(())
    }

    #[test]
    fn qualified_symbol_parents_bound_four_thousand_deep_scopes() -> Result<(), Box<dyn Error>> {
        const DEPTH: usize = 4_000;
        let names = (0..DEPTH)
            .map(|index| format!("scope_{index:04}_{}", "x".repeat(229)))
            .collect::<Vec<_>>();
        let graph = SymbolGraph {
            path: "src/deep.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: names
                .iter()
                .enumerate()
                .map(|(index, name)| CodeSymbol {
                    path: "src/deep.rs".to_string(),
                    language: Some("rust".to_string()),
                    name: name.clone(),
                    kind: SymbolKind::Module,
                    signature: name.clone(),
                    exported: false,
                    documentation: None,
                    line_start: index + 1,
                    line_end: DEPTH * 2 - index,
                    source_selector: None,
                    parent: index.checked_sub(1).map(|parent| names[parent].clone()),
                    parser: ParserKind::TreeSitter,
                    detail: Some("mod_item".to_string()),
                })
                .collect(),
            relations: Vec::new(),
        };
        let first = qualified_symbol_parents(&graph)?;
        require_eq(&first.len(), &DEPTH, "deep qualified parent count")?;
        require(
            first.first().is_some_and(Option::is_none),
            "deep root unexpectedly gained a parent",
        )?;
        require(
            first
                .iter()
                .flatten()
                .all(|parent| parent.as_str().len() <= MAX_GRAPH_IDENTITY_BYTES),
            "deep qualification retained an oversized parent",
        )?;
        require(
            first
                .iter()
                .flatten()
                .any(|parent| parent.as_str().starts_with("@projectatlas.scope.v1:")),
            "deep qualification never exercised compact scope identity",
        )?;
        require_eq(
            &qualified_symbol_parents(&graph)?,
            &first,
            "repeated deep qualified parents",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_scopes_produce_distinct_graph_entity_keys() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([3; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        for (path, language, source, expected_parents) in [
            (
                "src/lib.rs",
                "rust",
                concat!(
                    "mod first { struct Runner; impl Runner { fn run(&self) {} } }\n",
                    "mod second { struct Runner; impl Runner { fn run(&self) {} } }\n",
                ),
                ["first::Runner", "second::Runner"],
            ),
            (
                "src/Runner.java",
                "java",
                concat!(
                    "class First { class Runner { void run() {} } }\n",
                    "class Second { class Runner { void run() {} } }\n",
                ),
                ["First::Runner", "Second::Runner"],
            ),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let method_indices = graph
                .symbols
                .iter()
                .enumerate()
                .filter_map(|(index, symbol)| {
                    (symbol.kind == SymbolKind::Method && symbol.name == "run").then_some(index)
                })
                .collect::<Vec<_>>();
            require_eq(&method_indices.len(), &2, "scoped method count")?;
            let first_symbol = &graph.symbols[method_indices[0]];
            let second_symbol = &graph.symbols[method_indices[1]];
            require_eq(
                &first_symbol.parent.as_deref(),
                &Some("Runner"),
                "legacy leaf parent",
            )?;
            require_eq(
                &first_symbol.parent,
                &second_symbol.parent,
                "same legacy leaf parent",
            )?;
            require_eq(
                &first_symbol.signature,
                &second_symbol.signature,
                "same declaration signature",
            )?;

            let graphs = vec![graph];
            let packages = PackageIndex::from_graphs(&graphs)?;
            let projection = build_entity_projection(
                project,
                generation,
                &[],
                &graphs,
                &packages,
                true,
                &control,
            )?;
            let owners = projection
                .owners_by_graph
                .get(path)
                .ok_or("scoped graph owners are missing")?;
            let first = owners.symbol_digests[method_indices[0]]
                .as_ref()
                .and_then(|digest| projection.entity_by_digest.get(digest))
                .ok_or("first scoped method entity is missing")?;
            let second = owners.symbol_digests[method_indices[1]]
                .as_ref()
                .and_then(|digest| projection.entity_by_digest.get(digest))
                .ok_or("second scoped method entity is missing")?;
            let EntitySelector::Symbol {
                symbol: first_selector,
            } = first.selector()
            else {
                return Err("first scoped entity is not a symbol".into());
            };
            let EntitySelector::Symbol {
                symbol: second_selector,
            } = second.selector()
            else {
                return Err("second scoped entity is not a symbol".into());
            };
            require_eq(
                &first_selector
                    .parent
                    .as_ref()
                    .map(GraphIdentityText::as_str),
                &Some(expected_parents[0]),
                "first graph identity parent",
            )?;
            require_eq(
                &second_selector
                    .parent
                    .as_ref()
                    .map(GraphIdentityText::as_str),
                &Some(expected_parents[1]),
                "second graph identity parent",
            )?;
            require(
                first.key() != second.key(),
                "independent semantic scopes shared one graph entity key",
            )?;
        }
        Ok(())
    }

    #[test]
    fn external_classification_follows_effective_semantic_provider() -> Result<(), Box<dyn Error>> {
        for (language, target, system, identity) in [
            ("html", "import fs from \"node:fs\";", "node", "fs"),
            ("svelte", "import url from \"node:url\";", "node", "url"),
            ("vue", "import path from \"node:path\";", "node", "path"),
        ] {
            let case = resolution_case(language, RelationKind::Imports, target, &[]);
            let external = explicit_external_selector(&case.graph, &case.relation)?
                .ok_or("embedded ECMAScript external classification is missing")?;
            require_eq(&external.system.as_str(), &system, "external system")?;
            require_eq(&external.identity.as_str(), &identity, "external identity")?;
        }

        let lock = resolution_case("cargo-lock", RelationKind::DependsOn, "serde-lock", &[]);
        require(
            explicit_external_selector(&lock.graph, &lock.relation)?.is_none(),
            "Cargo.lock was misclassified as an explicit external dependency",
        )?;
        Ok(())
    }

    #[test]
    fn grouped_rust_imports_use_only_their_common_external_module() -> Result<(), Box<dyn Error>> {
        let grouped = resolution_case("rust", RelationKind::Imports, "use std::{fs, io};", &[]);
        require_eq(
            &rust_toolchain_identity(&grouped.relation),
            &Some("std".to_string()),
            "grouped Rust external root",
        )?;
        let nested = resolution_case(
            "rust",
            RelationKind::Imports,
            "use std::fs::{read, write};",
            &[],
        );
        require_eq(
            &rust_toolchain_identity(&nested.relation),
            &Some("std::fs".to_string()),
            "nested grouped Rust external module",
        )?;
        let ordinary = resolution_case("rust", RelationKind::Imports, "use std::fs;", &[]);
        require_eq(
            &rust_toolchain_identity(&ordinary.relation),
            &Some("std::fs".to_string()),
            "ordinary Rust external module",
        )?;
        Ok(())
    }

    #[test]
    fn registry_candidate_merge_deduplicates_exactly_and_observes_cancellation()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([6; 16])?;
        let generation = IndexGeneration::new(1);
        let first_key = test_resolution_key(project, "first-key")?;
        let second_key = test_resolution_key(project, "second-key")?;
        let first = test_symbol_entity(project, generation, "src/first.rs", "first")?;
        let second = test_symbol_entity(project, generation, "src/second.rs", "second")?;
        let mut registry = ProjectResolutionRegistry::default();
        registry.insert_candidate(&first_key, &first)?;
        registry.insert_candidate(&second_key, &first)?;
        registry.insert_candidate(&second_key, &second)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let staged_entities = BTreeMap::new();
        let matches = registry_resolution_matches(
            &[first_key.clone(), second_key.clone()],
            &registry,
            &staged_entities,
            &control,
        )?;
        require_eq(&matches.count, &2, "distinct merged candidate count")?;
        let expected_first = [&first, &second]
            .into_iter()
            .min_by_key(|entity| entity.key().digest())
            .ok_or("candidate fixture is empty")?;
        require_eq(
            &matches.first.map(|entity| entity.key().digest()),
            &Some(expected_first.key().digest()),
            "stable first candidate",
        )?;

        let cancellation = IndexCancellation::new();
        let canceled_control = IndexWorkControl::new(cancellation.clone(), None);
        cancellation.cancel();
        require(
            registry_resolution_matches(
                &[first_key, second_key],
                &registry,
                &staged_entities,
                &canceled_control,
            )
            .is_err(),
            "candidate merge ignored cancellation",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_ambiguous_relations_keep_the_largest_candidate_count() -> Result<(), Box<dyn Error>>
    {
        let project = ProjectInstanceId::from_bytes([15; 16])?;
        let generation = IndexGeneration::new(1);
        let source = test_file_entity(project, generation, "src/duplicate.ts")?;
        let relation = |candidates| -> Result<LogicalRelation, Box<dyn Error>> {
            Ok(LogicalRelation::new(
                &source,
                GraphRelationKind::from_legacy(RelationKind::Contains),
                RelationResolution::Ambiguous {
                    reference: GraphIdentityText::new("declarations")?,
                    candidates: NonZeroU32::new(candidates).ok_or("candidate count was zero")?,
                },
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?)
        };
        let mut relations = BTreeMap::new();
        insert_relation(
            &mut relations,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            0,
        )?;
        insert_relation(
            &mut relations,
            relation(3)?,
            "src/duplicate.ts",
            "logical",
            1,
        )?;
        insert_relation(
            &mut relations,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            2,
        )?;
        let retained = relations
            .into_values()
            .next()
            .ok_or("deduplicated relation was not retained")?;
        require(
            matches!(
                retained.resolution(),
                RelationResolution::Ambiguous { candidates, .. } if candidates.get() == 3
            ),
            "deduplicated ambiguity did not retain the largest candidate count",
        )?;

        let mut conflicting = BTreeMap::new();
        insert_relation(
            &mut conflicting,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            0,
        )?;
        let different_confidence = LogicalRelation::new(
            &source,
            GraphRelationKind::from_legacy(RelationKind::Contains),
            RelationResolution::Ambiguous {
                reference: GraphIdentityText::new("declarations")?,
                candidates: NonZeroU32::new(2).ok_or("candidate count was zero")?,
            },
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        require(
            insert_relation(
                &mut conflicting,
                different_confidence,
                "src/duplicate.ts",
                "logical",
                1,
            )
            .is_err(),
            "a non-ambiguity conflict was merged",
        )?;
        Ok(())
    }

    #[test]
    fn same_file_private_calls_resolve_and_duplicate_declarations_stay_ambiguous()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([5; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let private_graph = extract_symbol_graph(
            "src/private.rs",
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        );
        require(
            private_graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == "helper" && !symbol.exported),
            "fixture helper was not private",
        )?;
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&private_graph))?;
        let projection = build_entity_projection(
            project,
            generation,
            &[],
            std::slice::from_ref(&private_graph),
            &packages,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&private_graph),
            projection,
            &candidates,
            &control,
        )?;
        let helper_call = staged
            .relations
            .iter()
            .find(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Resolved {
                        selector: ReusableTargetSelector::Symbol { symbol },
                        ..
                    } if symbol.name.as_str() == "helper"
                        && symbol.file.as_str() == "src/private.rs"
                )
            })
            .ok_or("private same-file helper call did not resolve")?;
        require_eq(
            &helper_call.kind(),
            &GraphRelationKind::from_legacy(RelationKind::Calls),
            "private same-file relation kind",
        )?;

        let duplicate_graph = SymbolGraph {
            path: "src/duplicate.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_code_symbol("src/duplicate.rs", "caller", Some("Owner"), "fn caller()"),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Owner"),
                    "fn helper(first: u8)",
                ),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Owner"),
                    "fn helper(second: u16)",
                ),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Unrelated"),
                    "fn helper()",
                ),
            ],
            relations: vec![SymbolRelation {
                path: "src/duplicate.rs".to_string(),
                source_name: "caller".to_string(),
                target_name: "helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&duplicate_graph))?;
        let projection = build_entity_projection(
            project,
            generation,
            &[],
            std::slice::from_ref(&duplicate_graph),
            &packages,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&duplicate_graph),
            projection,
            &candidates,
            &control,
        )?;
        require(
            staged.relations.iter().any(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Ambiguous {
                        reference,
                        candidates,
                    } if reference.as_str() == "helper" && candidates.get() == 2
                )
            }),
            "duplicate same-file declarations did not remain ambiguous",
        )?;
        Ok(())
    }

    #[test]
    fn configured_module_targets_reuse_graph_ambiguity_ownership() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([19; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/first/controller.ts",
                Some("typescript"),
                "export function useController() { return 'first'; }\n",
            ),
            extract_symbol_graph(
                "src/second/controller.ts",
                Some("typescript"),
                "export function useController() { return 'second'; }\n",
            ),
            extract_symbol_graph(
                "src/page.ts",
                Some("typescript"),
                "import { useController } from '@/controller';\nexport const value = useController();\n",
            ),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let configured = ConfiguredModuleResolution::new(vec![EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            None,
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/first/*".to_string(), "src/second/*".to_string()],
            )?],
        )?])?;
        let projection = build_entity_projection_with_config(
            project,
            generation,
            &[],
            &graphs,
            &packages,
            &configured,
            None,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        for kind in [RelationKind::Imports, RelationKind::Calls] {
            require(
                staged.relations.iter().any(|relation| {
                    relation.kind() == GraphRelationKind::from_legacy(kind)
                        && matches!(
                            relation.resolution(),
                            RelationResolution::Ambiguous { candidates, .. }
                                if candidates.get() == 2
                        )
                }),
                "configured mapping did not retain all ambiguity candidates",
            )?;
        }
        Ok(())
    }

    #[test]
    fn extracted_provider_matrix_survives_sqlite_publication_and_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("extracted-provider-matrix");
        fs::create_dir_all(&root)?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/lib.rs",
                Some("rust"),
                "use std::{fs, io};\npub fn caller() { private_helper(); }\nfn private_helper() {}\n",
            ),
            extract_symbol_graph(
                "src/config.rs",
                Some("rust"),
                "pub fn load_timeout_millis() -> u64 { 250 }\n",
            ),
            extract_symbol_graph(
                "src/handler.rs",
                Some("rust"),
                "use crate::config;\npub fn health_response() { config::load_timeout_millis(); }\n",
            ),
            extract_symbol_graph(
                "src/router.rs",
                Some("rust"),
                "use crate::handler;\npub fn dispatch(path: &str) -> Option<()> { (path == \"/health\").then(handler::health_response) }\n",
            ),
            extract_symbol_graph(
                "src/app.js",
                Some("javascript"),
                "import path from \"node:path\";\nexport function run() { return path.join('a', 'b'); }\n",
            ),
            extract_symbol_graph(
                "src/app.py",
                Some("python"),
                "import requests\n\ndef run():\n    return requests.get('https://example.test')\n",
            ),
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"matrix-app\"\nversion = \"0.1.0\"\n\n[dependencies]\nduplicate = \"1\"\n",
            ),
            extract_symbol_graph(
                "vendor/first/Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"duplicate\"\nversion = \"1.0.0\"\n",
            ),
            extract_symbol_graph(
                "vendor/second/Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"duplicate\"\nversion = \"2.0.0\"\n",
            ),
            extract_symbol_graph(
                "public/index.html",
                Some("html"),
                "<script type=\"module\">\nimport fs from \"node:fs\";\nexport function boot() { return fs.readFile; }\n</script>\n",
            ),
            extract_symbol_graph(
                "src/Page.svelte",
                Some("svelte"),
                "<script lang=\"ts\">\nimport url from \"node:url\";\nexport function page() { return url.parse('https://example.test'); }\n</script>\n",
            ),
        ];
        for (language, expected_relation) in [
            ("rust", "std"),
            ("javascript", "node:path"),
            ("python", "requests"),
            ("cargo-manifest", "duplicate"),
            ("html", "node:fs"),
            ("svelte", "node:url"),
        ] {
            require(
                graphs.iter().any(|graph| {
                    graph.language.as_deref() == Some(language)
                        && graph
                            .relations
                            .iter()
                            .any(|relation| relation.target_name.contains(expected_relation))
                }),
                &format!("extracted {language} provider relation is missing"),
            )?;
        }
        let nodes = graphs
            .iter()
            .map(|graph| {
                test_file_node(&graph.path, graph.language.as_deref().unwrap_or("unknown"))
            })
            .collect::<Vec<_>>();
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        let source_keys = staged
            .entities
            .iter()
            .map(|entity| entity.key().clone())
            .collect::<Vec<_>>();
        {
            let mut publication = store.begin_index_publication("extracted-provider-matrix")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let mut reopened = Vec::new();
        for source in source_keys {
            let page = reader.repository_graph_relations(
                RepositoryGraphRelationQuery::Outbound { source },
                128,
            )?;
            require(!page.truncated, "extracted provider matrix was truncated")?;
            reopened.extend(page.rows);
        }
        let mut external = BTreeSet::new();
        let mut unresolved = BTreeSet::new();
        let mut ambiguous = BTreeMap::new();
        let mut resolved_symbols = BTreeSet::new();
        for relation in reopened {
            require_eq(
                &relation.generation(),
                &generation,
                "extracted relation generation",
            )?;
            match relation.resolution() {
                RelationResolution::External {
                    external: target, ..
                } => {
                    external.insert((
                        target.system.as_str().to_string(),
                        target.identity.as_str().to_string(),
                    ));
                }
                RelationResolution::Unresolved { reference } => {
                    unresolved.insert(reference.as_str().to_string());
                }
                RelationResolution::Ambiguous {
                    reference,
                    candidates,
                } => {
                    ambiguous.insert(reference.as_str().to_string(), candidates.get());
                }
                RelationResolution::Resolved { selector, .. } => {
                    if let ReusableTargetSelector::Symbol { symbol } = selector {
                        resolved_symbols.insert((
                            symbol.file.as_str().to_string(),
                            symbol.name.as_str().to_string(),
                        ));
                    }
                }
            }
        }
        for expected in [
            ("rust-toolchain".to_string(), "std".to_string()),
            ("node".to_string(), "path".to_string()),
            ("node".to_string(), "fs".to_string()),
            ("node".to_string(), "url".to_string()),
        ] {
            require(
                external.contains(&expected),
                &format!("reopened external target is missing: {expected:?}"),
            )?;
        }
        require(
            unresolved
                .iter()
                .any(|reference| reference.contains("requests")),
            "reopened Python unresolved import is missing",
        )?;
        require_eq(
            &ambiguous.get("duplicate"),
            &Some(&2),
            "reopened Cargo duplicate ambiguity",
        )?;
        require(
            resolved_symbols.contains(&("src/lib.rs".to_string(), "private_helper".to_string())),
            "reopened private Rust helper resolution is missing",
        )?;
        require(
            resolved_symbols.contains(&(
                "src/config.rs".to_string(),
                "load_timeout_millis".to_string(),
            )),
            "reopened Rust module-qualified call resolution is missing",
        )?;
        require(
            resolved_symbols
                .contains(&("src/handler.rs".to_string(), "health_response".to_string())),
            "reopened Rust callback resolution is missing",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn closed_resolution_states_survive_sqlite_publication_and_reopen() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("closed-resolution-states");
        fs::create_dir_all(&root)?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let source = test_file_entity(project, generation, "src/main.rs")?;
        let unique_target = test_symbol_entity(project, generation, "src/unique.rs", "unique")?;
        let first_shared = test_symbol_entity(project, generation, "src/first.rs", "shared")?;
        let second_shared = test_symbol_entity(project, generation, "src/second.rs", "shared")?;
        let local_package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("local-package")?,
                    manifest: RepositoryFilePath::new(Path::new("vendor/local/Cargo.toml"))?,
                },
            },
            generation,
        )?;
        let unique_key = test_resolution_key(project, "unique")?;
        let shared_key = test_resolution_key(project, "shared")?;
        let local_package_key = test_resolution_key(project, "local-package")?;
        let mut registry = ProjectResolutionRegistry::default();
        registry.insert_candidate(&unique_key, &unique_target)?;
        registry.insert_candidate(&shared_key, &first_shared)?;
        registry.insert_candidate(&shared_key, &second_shared)?;
        registry.insert_candidate(&local_package_key, &local_package)?;
        let source_digest = source.key().digest().to_string();
        let owners = GraphOwners {
            file_digest: source_digest.clone(),
            symbol_digests: Vec::new(),
        };
        let staged_entities = BTreeMap::from([(source_digest, source.clone())]);
        let mut external_entities = BTreeMap::new();
        let cases = [
            resolution_case("rust", RelationKind::Calls, "unique", &[&unique_key]),
            resolution_case("rust", RelationKind::Calls, "shared", &[&shared_key]),
            resolution_case("rust", RelationKind::Calls, "missing", &[]),
            resolution_case("rust", RelationKind::Imports, "use std::{fs, io};", &[]),
            resolution_case("cargo-manifest", RelationKind::DependsOn, "serde", &[]),
            resolution_case("cargo-lock", RelationKind::DependsOn, "serde-lock", &[]),
            resolution_case(
                "javascript",
                RelationKind::Imports,
                "import path from \"node:path\";",
                &[],
            ),
            resolution_case(
                "html",
                RelationKind::Imports,
                "import fs from \"node:fs\";",
                &[],
            ),
            resolution_case(
                "svelte",
                RelationKind::Imports,
                "import url from \"node:url\";",
                &[],
            ),
            resolution_case(
                "javascript",
                RelationKind::Imports,
                "import value from \"left-pad\";",
                &[],
            ),
            resolution_case("python", RelationKind::Imports, "import requests", &[]),
            resolution_case(
                "cargo-manifest",
                RelationKind::DependsOn,
                "local-package",
                &[&local_package_key],
            ),
        ];
        let mut relations = Vec::new();
        let mut dependencies = Vec::new();
        for case in &cases {
            let symbol_index = GraphSymbolIndex::new(&case.graph, &control)?;
            let resolution = relation_resolution(
                project,
                generation,
                &case.relation,
                &owners,
                &case.graph,
                &symbol_index,
                &case.keys,
                &registry,
                &staged_entities,
                &mut external_entities,
                &control,
            )?;
            let relation = LogicalRelation::new(
                &source,
                GraphRelationKind::from_legacy(case.relation.kind),
                resolution,
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?;
            for key in &case.keys {
                dependencies.push(RelationDependencyKey::new(
                    relation.key().clone(),
                    key.clone(),
                )?);
            }
            relations.push(relation);
        }
        let external_keys = external_entities
            .values()
            .map(|entity| entity.key().clone())
            .collect::<Vec<_>>();
        let entities = [
            source.clone(),
            unique_target,
            first_shared,
            second_shared,
            local_package,
        ]
        .into_iter()
        .chain(external_entities.into_values())
        .collect::<Vec<_>>();
        let exports = [
            EntityResolutionKey::new(entities[1].key().clone(), unique_key.clone())?,
            EntityResolutionKey::new(entities[2].key().clone(), shared_key.clone())?,
            EntityResolutionKey::new(entities[3].key().clone(), shared_key.clone())?,
            EntityResolutionKey::new(entities[4].key().clone(), local_package_key.clone())?,
        ];
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = StagedRepositoryGraph {
            project,
            mutation: RepositoryGraphMutation::Full,
            entities,
            relations,
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: exports.into(),
            relation_dependencies: dependencies,
            document_unresolved_reasons: Vec::new(),
            identity_rejections: Vec::new(),
            resolution_derivations: BTreeMap::new(),
            peak_retained_bytes: 0,
            projection_removals_before_entities: Vec::new(),
            scan_policy,
            document_target_states: Vec::new(),
            database: None,
            retained_bytes: 0,
        };
        {
            let mut publication = store.begin_index_publication("closed-resolution-states")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&[
                test_file_node("src/main.rs", "rust"),
                test_file_node("src/unique.rs", "rust"),
                test_file_node("src/first.rs", "rust"),
                test_file_node("src/second.rs", "rust"),
                test_file_node("vendor/local/Cargo.toml", "cargo-manifest"),
            ])?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let page = reader.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            32,
        )?;
        require(!page.truncated, "closed resolution proof was truncated")?;
        require_eq(&page.rows.len(), &cases.len(), "reopened relation count")?;
        let mut resolved = BTreeSet::new();
        let mut ambiguous = BTreeMap::new();
        let mut unresolved = BTreeSet::new();
        let mut external = BTreeSet::new();
        for relation in &page.rows {
            require_eq(
                &relation.generation(),
                &generation,
                "reopened relation generation",
            )?;
            match relation.resolution() {
                RelationResolution::Resolved {
                    selector,
                    generation: target_generation,
                    ..
                } => {
                    require_eq(target_generation, &generation, "resolved target generation")?;
                    match selector {
                        ReusableTargetSelector::Symbol { symbol } => {
                            resolved.insert(symbol.name.as_str().to_string());
                        }
                        ReusableTargetSelector::Package { package } => {
                            resolved.insert(package.name.as_str().to_string());
                        }
                        ReusableTargetSelector::Folder { .. }
                        | ReusableTargetSelector::File { .. } => {}
                    }
                }
                RelationResolution::Ambiguous {
                    reference,
                    candidates,
                } => {
                    ambiguous.insert(reference.as_str().to_string(), candidates.get());
                }
                RelationResolution::Unresolved { reference } => {
                    unresolved.insert(reference.as_str().to_string());
                }
                RelationResolution::External {
                    external: selector,
                    generation: target_generation,
                    ..
                } => {
                    require_eq(target_generation, &generation, "external target generation")?;
                    external.insert((
                        selector.system.as_str().to_string(),
                        selector.identity.as_str().to_string(),
                    ));
                }
            }
        }
        require_eq(
            &resolved,
            &BTreeSet::from(["local-package".to_string(), "unique".to_string()]),
            "resolved targets",
        )?;
        require_eq(
            &ambiguous,
            &BTreeMap::from([("shared".to_string(), 2)]),
            "ambiguous targets",
        )?;
        require_eq(
            &unresolved,
            &BTreeSet::from([
                "import requests".to_string(),
                "import value from \"left-pad\";".to_string(),
                "missing".to_string(),
                "serde-lock".to_string(),
            ]),
            "unresolved targets",
        )?;
        require_eq(
            &external,
            &BTreeSet::from([
                ("cargo".to_string(), "serde".to_string()),
                ("node".to_string(), "fs".to_string()),
                ("node".to_string(), "path".to_string()),
                ("node".to_string(), "url".to_string()),
                ("rust-toolchain".to_string(), "std".to_string()),
            ]),
            "external targets",
        )?;
        for key in &external_keys {
            let entity = reader
                .repository_graph_entity(key)?
                .ok_or("reopened external entity is missing")?;
            require(
                matches!(entity.selector(), EntitySelector::External { .. }),
                "external relation target reopened as a local entity",
            )?;
            require_eq(
                &entity.generation(),
                &generation,
                "external entity generation",
            )?;
        }
        let affected = reader.repository_affected_source_paths(
            project,
            &[unique_key, shared_key, local_package_key],
            32,
        )?;
        require(!affected.truncated, "dependency source proof was truncated")?;
        require_eq(
            &affected.rows,
            &vec![RepositoryFilePath::new(Path::new("src/main.rs"))?],
            "reopened dependency source paths",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    struct ResolutionCase {
        graph: SymbolGraph,
        relation: SymbolRelation,
        keys: Vec<CanonicalResolutionKey>,
    }

    fn resolution_case(
        language: &str,
        kind: RelationKind,
        target: &str,
        keys: &[&CanonicalResolutionKey],
    ) -> ResolutionCase {
        let parser = if language.starts_with("cargo-") {
            ParserKind::Manifest
        } else {
            ParserKind::TreeSitter
        };
        ResolutionCase {
            graph: SymbolGraph {
                path: "src/main.rs".to_string(),
                language: Some(language.to_string()),
                parser,
                symbols: Vec::new(),
                relations: Vec::new(),
            },
            relation: SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "src/main.rs".to_string(),
                target_name: target.to_string(),
                kind,
                line: 1,
                context: target.to_string(),
                parser,
            },
            keys: keys.iter().map(|key| (*key).clone()).collect(),
        }
    }

    fn test_resolution_key(
        project: ProjectInstanceId,
        identity: &str,
    ) -> Result<CanonicalResolutionKey, Box<dyn Error>> {
        let provider = GraphIdentityText::new("test-provider")?;
        let language = GraphIdentityText::new("test-language")?;
        let identity = GraphIdentityText::new(identity)?;
        Ok(CanonicalResolutionKey::new(
            project,
            ResolutionKeyDomain::Declaration,
            &provider,
            &language,
            None,
            None,
            None,
            &identity,
        ))
    }

    fn test_file_entity(
        project: ProjectInstanceId,
        generation: IndexGeneration,
        path: &str,
    ) -> Result<GraphEntity, Box<dyn Error>> {
        Ok(GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(path))?,
            },
            generation,
        )?)
    }

    fn test_symbol_entity(
        project: ProjectInstanceId,
        generation: IndexGeneration,
        path: &str,
        name: &str,
    ) -> Result<GraphEntity, Box<dyn Error>> {
        Ok(GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new(path))?,
                    name: GraphIdentityText::new(name)?,
                    kind: SymbolKind::Function,
                    parent: None,
                    signature: GraphIdentityText::new(format!("fn {name}()"))?,
                },
            },
            generation,
        )?)
    }

    fn test_code_symbol(
        path: &str,
        name: &str,
        parent: Option<&str>,
        signature: &str,
    ) -> CodeSymbol {
        CodeSymbol {
            path: path.to_string(),
            language: Some("rust".to_string()),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: signature.to_string(),
            exported: false,
            documentation: None,
            line_start: 1,
            line_end: 1,
            source_selector: None,
            parent: parent.map(ToString::to_string),
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }
    }

    #[test]
    fn source_graph_admission_keeps_valid_rows_and_typed_rejection_coverage()
    -> Result<(), Box<dyn Error>> {
        let valid = test_code_symbol("src/lib.rs", "valid", None, "fn valid()");
        let invalid_name = test_code_symbol("src/lib.rs", "bad\u{0}name", None, "fn bad()");
        let invalid_signature = test_code_symbol("src/lib.rs", "padded", None, " fn padded() ");
        let invalid_parent = test_code_symbol("src/lib.rs", "child", Some(" parent "), "child()");
        let invalid_reserved = test_code_symbol(
            "src/lib.rs",
            &format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}derived"),
            None,
            "fn derived()",
        );
        let invalid_oversized = test_code_symbol(
            "src/lib.rs",
            &"x".repeat(MAX_GRAPH_IDENTITY_BYTES + 1),
            None,
            "fn oversized()",
        );
        let graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                valid,
                invalid_name,
                invalid_signature,
                invalid_parent,
                invalid_reserved,
                invalid_oversized,
            ],
            relations: vec![
                SymbolRelation {
                    path: "src/lib.rs".to_string(),
                    source_name: "valid".to_string(),
                    target_name: "helper".to_string(),
                    kind: RelationKind::Calls,
                    line: 1,
                    context: "helper()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/lib.rs".to_string(),
                    source_name: "valid".to_string(),
                    target_name: "bad\u{0}target".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "bad()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/lib.rs".to_string(),
                    source_name: " valid ".to_string(),
                    target_name: "helper".to_string(),
                    kind: RelationKind::Calls,
                    line: 3,
                    context: "helper()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        };
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (admitted, report) = super::admit_symbol_graph(Cow::Owned(graph), &control)?;
        require_eq(&admitted.symbols.len(), &1, "valid symbol admission")?;
        require_eq(&admitted.relations.len(), &1, "valid relation admission")?;
        require_eq(
            &report.rejected_facts_for("src/lib.rs"),
            &7,
            "rejected fact count",
        )?;
        require_eq(
            &admitted.symbols[0].signature,
            &"fn valid()".to_string(),
            "valid signature preservation",
        )?;

        let coverage = super::coverage_for_graph(
            &admitted,
            IndexGeneration::new(1),
            &report,
            &super::GraphIdentityAdmission::default(),
        )?;
        require_eq(
            &coverage.state(),
            &CoverageState::Partial,
            "admission coverage state",
        )?;
        require_eq(&coverage.covered(), &2, "admission covered count")?;
        require_eq(&coverage.omitted(), &7, "admission omitted count")?;
        let reason = coverage
            .reason()
            .ok_or_else(|| io::Error::other("admission coverage omitted its reason"))?
            .as_str();
        require_eq(
            &reason,
            &PARTIAL_COVERAGE_REASON,
            "admission coverage keeps the stable coarse reason",
        )?;
        require_eq(&report.rejections.len(), &7, "typed rejection detail count")?;
        require(
            report.rejections.iter().all(|rejection| {
                rejection.path.as_str() == "src/lib.rs"
                    && rejection.parser == ParserKind::TreeSitter
                    && rejection.span.start_line() >= 1
                    && rejection.span.end_line() >= rejection.span.start_line()
            }),
            "typed rejection details lost path/parser/span ownership",
        )?;
        require(
            report
                .rejections
                .iter()
                .any(|rejection| rejection.field == GraphIdentityField::RelationTarget),
            "typed rejection details lost relation-target ownership",
        )?;

        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("src"))?;
        let nodes = vec![test_file_node("src/lib.rs", "rust")];
        let project = ProjectInstanceId::from_bytes([71; 16])?;
        let generation = IndexGeneration::new(1);
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&admitted))?;
        let projection = build_entity_projection(
            project,
            generation,
            &nodes,
            std::slice::from_ref(&admitted),
            &packages,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let staged = super::finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&admitted),
            temp.path(),
            &nodes,
            &BTreeMap::new(),
            &report,
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        require_eq(&staged.relations.len(), &1, "valid relation publication")?;
        require(
            staged.entities.iter().any(|entity| {
                matches!(
                    entity.selector(),
                    EntitySelector::Symbol { symbol } if symbol.name.as_str() == "valid"
                )
            }),
            "valid symbol publication was lost with invalid siblings",
        )?;
        require_eq(
            &staged.identity_rejections.len(),
            &7,
            "typed rejection coverage was not staged with valid rows",
        )?;
        Ok(())
    }

    #[test]
    fn invalid_sibling_does_not_fail_symbol_only_coverage() -> Result<(), Box<dyn Error>> {
        let graph = SymbolGraph {
            path: "src/symbol-only.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_code_symbol("src/symbol-only.rs", "valid", None, "fn valid()"),
                test_code_symbol("src/symbol-only.rs", "bad\u{0}name", None, "fn bad()"),
            ],
            relations: Vec::new(),
        };
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (admitted, report) = super::admit_symbol_graph(Cow::Owned(graph), &control)?;
        let coverage = super::coverage_for_graph(
            &admitted,
            IndexGeneration::new(1),
            &report,
            &super::GraphIdentityAdmission::default(),
        )?;
        require_eq(
            &coverage.state(),
            &CoverageState::Partial,
            "valid symbols keep mixed identity coverage partial",
        )?;
        require_eq(
            &coverage.covered(),
            &1,
            "valid symbol is counted as covered",
        )?;
        require_eq(
            &coverage.omitted(),
            &1,
            "invalid sibling is counted as omitted",
        )?;
        require_eq(
            &coverage.reason().map(GraphIdentityText::as_str),
            &Some(PARTIAL_COVERAGE_REASON),
            "symbol-only coverage keeps the coarse reason",
        )?;
        Ok(())
    }

    #[test]
    fn document_target_normalization_is_relative_bounded_and_platform_neutral()
    -> Result<(), Box<dyn Error>> {
        require_eq(
            &normalize_document_target("docs/guide.md", "../src/lib.rs:L12-L20?view=raw#entry")
                .map_err(|reason| io::Error::other(reason.to_string()))?,
            &DocumentTargetIdentity {
                path: "src/lib.rs".to_string(),
                fragment: Some("entry".to_string()),
            },
            "relative document target",
        )?;
        require_eq(
            &normalize_document_target("guide.md", "README")
                .map_err(|reason| io::Error::other(reason.to_string()))?,
            &DocumentTargetIdentity {
                path: "README".to_string(),
                fragment: None,
            },
            "root extensionless target",
        )?;
        require_eq(
            &normalize_document_target("docs/guide.md", "../../../private.txt"),
            &Err(DocumentTargetUnresolvedReason::OutsideRoot),
            "outside-root selector",
        )?;
        require_eq(
            &normalize_document_target("docs/guide.md", "target.md#runtime value"),
            &Err(DocumentTargetUnresolvedReason::NoStaticTarget),
            "non-static fragment refusal",
        )?;
        Ok(())
    }

    #[test]
    fn document_rows_resolve_exact_files_and_headings_with_typed_absence()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join(".gitignore"), "docs/ignored.md\n")?;
        fs::write(root.join("docs/ignored.md"), "ignored")?;
        let source = "[lib](../src/lib.rs)\n[heading](target.md#api)\n[missing heading](target.md#absent)\n[missing](missing.md)\n[ignored](ignored.md)\n[case](../SRC/lib.rs)\n[folder](../src)\n[outside](../../../private.txt)\n[self](guide.md)\n[self heading](guide.md#api)\n";
        let source_facts = projectatlas_symbols::extract_markdown_facts(source);
        let target_facts = projectatlas_symbols::extract_markdown_facts("# API\n");
        let source_graph = source_facts.symbol_graph("docs/guide.md", Some("markdown"));
        let target_graph = target_facts.symbol_graph("docs/target.md", Some("markdown"));
        let source_code_graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let graphs = vec![source_graph, target_graph, source_code_graph];
        let mut nodes = vec![
            test_file_node("docs/guide.md", "markdown"),
            test_file_node("docs/target.md", "markdown"),
            test_file_node("src/lib.rs", "rust"),
        ];
        let mut folder = test_file_node("src", "unknown");
        folder.kind = NodeKind::Folder;
        folder.language = None;
        folder.extension = None;
        nodes.push(folder);
        let project = ProjectInstanceId::from_bytes([31; 16])?;
        let generation = IndexGeneration::new(4);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let owners = projection
            .owners_by_graph
            .remove("docs/guide.md")
            .ok_or_else(|| io::Error::other("document owners were not projected"))?;
        let scan_policy = RootScanPolicy::discover(root, &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(root, &nodes, &scan_policy)?;
        let rows = project_document_rows(
            project,
            generation,
            &graphs[0],
            &source_facts,
            &owners,
            &index,
            &candidates,
            &projection.entity_by_digest,
            &control,
        )?;
        require_eq(&rows.relations.len(), &9, "document relation count")?;
        require_eq(&rows.occurrences.len(), &9, "document occurrence count")?;
        require_eq(
            &rows
                .relations
                .iter()
                .filter(|relation| {
                    matches!(relation.resolution(), RelationResolution::Resolved { .. })
                })
                .count(),
            &2,
            "resolved document targets",
        )?;
        let reasons = rows
            .document_unresolved_reasons
            .iter()
            .map(|(_key, reason)| *reason)
            .collect::<BTreeSet<_>>();
        require_eq(
            &reasons,
            &BTreeSet::from([
                DocumentTargetUnresolvedReason::Missing,
                DocumentTargetUnresolvedReason::Ignored,
                DocumentTargetUnresolvedReason::OutsideRoot,
                DocumentTargetUnresolvedReason::CaseConflict,
                DocumentTargetUnresolvedReason::Unsupported,
            ]),
            "typed unresolved reasons",
        )?;
        require(
            rows.relations.iter().any(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Resolved {
                        selector: projectatlas_core::graph::ReusableTargetSelector::Symbol {
                            symbol
                        },
                        ..
                    } if symbol.kind == SymbolKind::Heading && symbol.signature.as_str() == "api"
                )
            }),
            "heading fragment did not resolve to its heading entity",
        )?;
        Ok(())
    }

    #[test]
    fn markdown_identity_admission_keeps_valid_document_siblings() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join("docs"))?;
        let guide_source = "# Guide\n\n[invalid](<target.md\u{1}>)\n[valid](target.md#target)\n";
        fs::write(root.join("docs/guide.md"), guide_source)?;
        fs::write(root.join("docs/target.md"), "# Target\n")?;
        let source_facts = projectatlas_symbols::extract_markdown_facts(guide_source);
        require_eq(
            &source_facts.link_candidates.len(),
            &2,
            "parser Markdown sibling candidate count",
        )?;
        require_eq(
            &source_facts.link_candidates[0].selector,
            &"target.md\u{1}".to_string(),
            "parser Markdown control selector",
        )?;
        let source_graph = source_facts.symbol_graph("docs/guide.md", Some("markdown"));
        let target_facts = projectatlas_symbols::extract_markdown_facts("# Target\n");
        let target_graph = target_facts.symbol_graph("docs/target.md", Some("markdown"));
        let nodes = vec![
            test_file_node("docs/guide.md", "markdown"),
            test_file_node("docs/target.md", "markdown"),
        ];
        let control = super::super::standalone_index_work_control();
        let mut symbols = empty_symbol_build_stage();
        symbols.report.candidates = 2;
        symbols.report.parsed = 2;
        symbols.report.summaries = 2;
        symbols.changes = vec![
            SymbolProjectionChange::Parsed(SymbolParseSuccess {
                path: "docs/guide.md".to_string(),
                graph: source_graph.clone(),
                markdown_facts: Some(Box::new(source_facts)),
                source_parser: ParserKind::Structural,
                summary: "Guide".to_string(),
                summary_is_structural: true,
                purpose_suggestion: None,
            }),
            SymbolProjectionChange::Parsed(SymbolParseSuccess {
                path: "docs/target.md".to_string(),
                graph: target_graph.clone(),
                markdown_facts: Some(Box::new(target_facts)),
                source_parser: ParserKind::Structural,
                summary: "Target".to_string(),
                summary_is_structural: true,
                purpose_suggestion: None,
            }),
        ];
        symbols.identity_admission = super::admit_symbol_build_stage(&mut symbols, &control)?;
        let admitted_counts = symbols
            .changes
            .iter()
            .filter_map(|change| match change {
                SymbolProjectionChange::Parsed(parsed) => {
                    Some((parsed.graph.symbols.len(), parsed.graph.relations.len()))
                }
                SymbolProjectionChange::Clear { .. } => None,
            })
            .fold(
                (0, 0),
                |(symbols, relations), (next_symbols, next_relations)| {
                    (symbols + next_symbols, relations + next_relations)
                },
            );
        require_eq(
            &(symbols.report.symbols, symbols.report.relations),
            &admitted_counts,
            "post-admission symbol report counts",
        )?;
        require_eq(
            &symbols.identity_admission.rejections.len(),
            &1,
            "Markdown rejection detail count",
        )?;
        let rejection = symbols
            .identity_admission
            .rejections
            .first()
            .ok_or_else(|| io::Error::other("Markdown rejection detail is missing"))?;
        require_eq(
            &rejection.path.as_str(),
            &"docs/guide.md",
            "Markdown rejection path",
        )?;
        require_eq(
            &rejection.parser,
            &ParserKind::Structural,
            "Markdown rejection parser",
        )?;
        require_eq(
            &rejection.field,
            &GraphIdentityField::RelationTarget,
            "Markdown rejection field",
        )?;
        require_eq(
            &rejection.reason,
            &GraphIdentityRejectionReason::ControlCharacters,
            "Markdown rejection reason",
        )?;
        require_eq(
            &rejection.span.start_line(),
            &3,
            "Markdown rejection start line",
        )?;
        require_eq(
            &rejection.span.start_column(),
            &0,
            "Markdown rejection start column",
        )?;
        require_eq(
            &rejection.span.end_line(),
            &3,
            "Markdown rejection end line",
        )?;
        require_eq(
            &rejection.span.end_column(),
            &23,
            "Markdown rejection end column",
        )?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&nodes)?;
        store.replace_symbol_graph(&source_graph)?;
        store.replace_symbol_graph(&target_graph)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require_eq(
            &staged.identity_rejections.len(),
            &1,
            "staged Markdown rejection detail count",
        )?;
        require_eq(
            &staged.relations.len(),
            &1,
            "staged valid Markdown relation count",
        )?;
        require(
            staged.coverage.iter().any(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path } if path.as_str() == "docs/guide.md"
                ) && coverage.state() == CoverageState::Complete
                    && coverage.covered() == 1
                    && coverage.omitted() == 0
            }),
            "invalid Markdown selector changed valid-sibling coverage semantics",
        )?;
        require(
            staged.relations.iter().any(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Resolved {
                        selector: projectatlas_core::graph::ReusableTargetSelector::Symbol {
                            symbol
                        },
                        ..
                    } if symbol.file.as_str() == "docs/target.md"
                        && symbol.signature.as_str() == "target"
                )
            }),
            "staged valid Markdown sibling was not resolved",
        )?;
        publish_full_staged_graph(&mut store, &nodes, &staged, &control, "markdown-admission")?;
        drop(store);
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("Markdown admission project identity is missing")?;
        let paths = nodes
            .iter()
            .map(|node| RepositoryNodePath::new(Path::new(&node.path)))
            .collect::<Result<Vec<_>, _>>()?;
        let persisted_rejections =
            store.repository_graph_identity_rejections(project, &paths, 16, None)?;
        require_eq(
            &persisted_rejections.len(),
            &1,
            "persisted Markdown rejection detail count",
        )?;
        let persisted_rejection = persisted_rejections
            .first()
            .ok_or("persisted Markdown rejection detail is missing")?;
        require_eq(
            &persisted_rejection.span.start_line(),
            &3,
            "persisted Markdown rejection start line",
        )?;
        require_eq(
            &persisted_rejection.span.start_column(),
            &0,
            "persisted Markdown rejection start column",
        )?;
        require_eq(
            &persisted_rejection.span.end_line(),
            &3,
            "persisted Markdown rejection end line",
        )?;
        require_eq(
            &persisted_rejection.span.end_column(),
            &23,
            "persisted Markdown rejection end column",
        )?;
        let persisted_wire = serde_json::to_string(&persisted_rejections)?;
        require(
            !persisted_wire.contains("\\u0001") && !persisted_wire.contains("target.md"),
            "persisted Markdown rejection retained the invalid selector",
        )?;
        let persisted_relations = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            },
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require_eq(
            &persisted_relations.rows.len(),
            &1,
            "reopened valid Markdown relation count",
        )?;
        require(
            persisted_relations.rows.iter().any(|relation| {
                matches!(
                    relation.relation.resolution(),
                    RelationResolution::Resolved {
                        selector: projectatlas_core::graph::ReusableTargetSelector::Symbol {
                            symbol
                        },
                        ..
                    } if symbol.file.as_str() == "docs/target.md"
                        && symbol.signature.as_str() == "target"
                )
            }),
            "reopened valid Markdown sibling was not resolved",
        )?;

        let base_generation = store
            .index_publication()?
            .ok_or("Markdown admission publication is missing")?
            .generation;
        let mut incremental_symbols = symbol_build_stage_for_markdown(
            source_graph.clone(),
            projectatlas_symbols::extract_markdown_facts(guide_source),
        );
        incremental_symbols.identity_admission =
            super::admit_symbol_build_stage(&mut incremental_symbols, &control)?;
        let incremental_stage = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            &["docs/guide.md".to_string()],
            &scan_policy,
            &incremental_symbols,
            &control,
        )?;
        require_eq(
            &incremental_stage.identity_rejections.len(),
            &1,
            "incremental Markdown rejection detail count",
        )?;
        require_eq(
            &incremental_stage.relations.len(),
            &1,
            "incremental valid Markdown relation count",
        )?;
        let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled.cancel();
        {
            let mut publication = store.begin_index_publication("markdown-admission-cancel")?;
            let error = incremental_stage.apply(&mut publication, &canceled).err();
            require(
                matches!(
                    error,
                    Some(CliError::IndexWork(IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::Publication
                    }))
                ),
                "incremental Markdown cancellation was not observed",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(base_generation),
            "publication after canceled Markdown refresh",
        )?;
        {
            let mut publication =
                store.begin_index_publication("markdown-admission-incremental")?;
            incremental_stage.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        let incremental_publication = store
            .index_publication()?
            .ok_or("incremental Markdown publication is missing")?;
        require_eq(
            &incremental_publication.generation,
            &base_generation
                .checked_next()
                .ok_or("Markdown generation overflowed")?,
            "incremental Markdown generation",
        )?;
        drop(store);
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let reopened_incremental =
            store.repository_graph_identity_rejections(project, &paths, 16, None)?;
        require_eq(
            &reopened_incremental,
            &persisted_rejections,
            "reopened incremental Markdown rejection details",
        )?;

        let fault_generation = store
            .index_publication()?
            .ok_or("incremental Markdown publication disappeared")?
            .generation;
        let mut fault_symbols = symbol_build_stage_for_markdown(
            source_graph.clone(),
            projectatlas_symbols::extract_markdown_facts(guide_source),
        );
        fault_symbols.identity_admission =
            super::admit_symbol_build_stage(&mut fault_symbols, &control)?;
        let mut fault_stage = stage_incremental_repository_graph(
            &store,
            &root,
            fault_generation,
            &nodes,
            &["docs/guide.md".to_string()],
            &scan_policy,
            &fault_symbols,
            &control,
        )?;
        fault_stage.identity_rejections.resize(
            usize::try_from(GraphLimits::MAX_ROWS)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
            fault_stage.identity_rejections[0].clone(),
        );
        {
            let mut publication = store.begin_index_publication("markdown-admission-fault")?;
            require(
                fault_stage.apply(&mut publication, &control).is_err(),
                "late Markdown rejection-detail fault did not fail",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(fault_generation),
            "generation after late Markdown rejection-detail fault",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &paths, 16, None)?,
            &reopened_incremental,
            "previous Markdown generation after late fault",
        )?;
        let retry_symbols = symbol_build_stage_for_markdown(
            source_graph,
            projectatlas_symbols::extract_markdown_facts(guide_source),
        );
        let retry_stage = stage_incremental_repository_graph(
            &store,
            &root,
            fault_generation,
            &nodes,
            &["docs/guide.md".to_string()],
            &scan_policy,
            &retry_symbols,
            &control,
        )?;
        {
            let mut publication = store.begin_index_publication("markdown-admission-retry")?;
            retry_stage.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        require_eq(
            &store.repository_graph_identity_rejections(project, &paths, 16, None)?,
            &reopened_incremental,
            "deterministic Markdown retry",
        )?;
        Ok(())
    }

    #[test]
    fn document_rows_use_file_identity_and_deduplicate_across_headings()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("src"))?;
        fs::write(temp.path().join("src/lib.rs"), "pub fn entry() {}\n")?;
        let source = "# First\n[target](../src/lib.rs)\n[second](guide.md#second)\n[self](guide.md#first)\n\n# Second\n[target](../src/lib.rs)\n[target](../src/lib.rs)\n[first](guide.md#first)\n[self](guide.md#second)\n";
        let facts = projectatlas_symbols::extract_markdown_facts(source);
        let enclosing_heading_bytes = facts
            .link_candidates
            .iter()
            .map(|candidate| candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64)
            .sum::<u64>();
        let mut facts_without_heading_owners = facts.clone();
        for candidate in &mut facts_without_heading_owners.link_candidates {
            candidate.enclosing_heading = None;
        }
        let retained_with_heading_owners = document_fact_map_retained_bytes(&BTreeMap::from([(
            "docs/guide.md".to_string(),
            Cow::Owned(facts.clone()),
        )]));
        let retained_without_heading_owners =
            document_fact_map_retained_bytes(&BTreeMap::from([(
                "docs/guide.md".to_string(),
                Cow::Owned(facts_without_heading_owners),
            )]));
        require_eq(
            &retained_with_heading_owners.saturating_sub(retained_without_heading_owners),
            &enclosing_heading_bytes,
            "document registry enclosing-heading bytes",
        )?;
        let source_graph = facts.symbol_graph("docs/guide.md", Some("markdown"));
        let target_graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let graphs = vec![source_graph, target_graph];
        let nodes = vec![
            test_file_node("docs/guide.md", "markdown"),
            test_file_node("src/lib.rs", "rust"),
        ];
        let project = ProjectInstanceId::from_bytes([33; 16])?;
        let generation = IndexGeneration::new(5);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let owners = projection
            .owners_by_graph
            .remove("docs/guide.md")
            .ok_or_else(|| io::Error::other("document owners were not projected"))?;
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        let rows = project_document_rows(
            project,
            generation,
            &graphs[0],
            &facts,
            &owners,
            &index,
            &candidates,
            &projection.entity_by_digest,
            &control,
        )?;
        require_eq(&rows.relations.len(), &3, "file-owned relation count")?;
        require_eq(&rows.occurrences.len(), &7, "distinct link occurrences")?;
        require(
            rows.relations.iter().all(|relation| {
                projection
                    .entity_by_digest
                    .get(relation.source().digest())
                    .is_some_and(|entity| {
                        matches!(
                            entity.selector(),
                            EntitySelector::File { path } if path.as_str() == "docs/guide.md"
                        )
                    })
            }),
            "document relations were not owned by the document file",
        )?;
        let heading_targets = rows
            .relations
            .iter()
            .filter_map(|relation| {
                let RelationResolution::Resolved {
                    selector: projectatlas_core::graph::ReusableTargetSelector::Symbol { symbol },
                    ..
                } = relation.resolution()
                else {
                    return None;
                };
                (symbol.kind == SymbolKind::Heading).then(|| symbol.signature.as_str().to_string())
            })
            .collect::<BTreeSet<_>>();
        require_eq(
            &heading_targets,
            &BTreeSet::from(["first".to_string(), "second".to_string()]),
            "file-owned heading targets",
        )?;
        let source_target_occurrences = rows
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.file().as_str() == "docs/guide.md")
            .count();
        require_eq(
            &source_target_occurrences,
            &7,
            "file-owned relation occurrences",
        )?;
        Ok(())
    }

    #[test]
    fn complete_document_without_static_candidates_reports_no_candidates()
    -> Result<(), Box<dyn Error>> {
        let facts = projectatlas_symbols::extract_markdown_facts(
            "# Overview\n\nLong prose without a repository reference.\n",
        );
        let coverage = document_coverage("docs/overview.md", &facts, IndexGeneration::new(6))?;
        require_eq(
            &coverage.state(),
            &CoverageState::NoCandidates,
            "empty document relation coverage",
        )?;
        require_eq(&coverage.total(), &0, "empty document relation total")?;
        Ok(())
    }

    #[test]
    fn partial_markdown_evidence_limit_maps_to_intermediate_bytes_coverage()
    -> Result<(), Box<dyn Error>> {
        let label = "l".repeat(MAX_MARKDOWN_LABEL_BYTES);
        let selector = format!(
            "src/{}.rs",
            "s".repeat(MAX_DOCUMENT_SELECTOR_BYTES - "src/".len() - ".rs".len())
        );
        let evidence_bytes = label.len() + selector.len();
        let source = format!("[{label}]({selector})\n")
            .repeat(MAX_MARKDOWN_EVIDENCE_BYTES / evidence_bytes + 1);
        let facts = projectatlas_symbols::extract_markdown_facts(&source);
        require(
            facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::EvidenceBytes),
            "real Markdown extraction did not reach its evidence-byte limit",
        )?;
        require(
            !facts.link_candidates.is_empty(),
            "evidence-limited Markdown extraction lost every valid candidate",
        )?;

        let coverage = document_coverage("docs/limited.md", &facts, IndexGeneration::new(7))?;
        require_eq(
            &coverage.state(),
            &CoverageState::Partial,
            "evidence-limited document coverage state",
        )?;
        require_eq(
            &coverage.reached_limit(),
            &Some(GraphLimitKind::IntermediateBytes),
            "evidence-limited document graph limit",
        )?;
        Ok(())
    }

    #[test]
    fn document_cycles_emit_only_canonical_bounded_edges() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("docs"))?;
        fs::write(temp.path().join("docs/a.md"), "# A\n\n[b](b.md#b)\n")?;
        fs::write(temp.path().join("docs/b.md"), "# B\n\n[a](a.md#a)\n")?;
        let facts = [
            projectatlas_symbols::extract_markdown_facts("# A\n\n[b](b.md#b)\n"),
            projectatlas_symbols::extract_markdown_facts("# B\n\n[a](a.md#a)\n"),
        ];
        let graphs = vec![
            facts[0].symbol_graph("docs/a.md", Some("markdown")),
            facts[1].symbol_graph("docs/b.md", Some("markdown")),
        ];
        let nodes = vec![
            test_file_node("docs/a.md", "markdown"),
            test_file_node("docs/b.md", "markdown"),
        ];
        let project = ProjectInstanceId::from_bytes([35; 16])?;
        let generation = IndexGeneration::new(1);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        let mut relations = Vec::new();
        for (graph, facts) in graphs.iter().zip(&facts) {
            let owners = projection
                .owners_by_graph
                .remove(&graph.path)
                .ok_or_else(|| io::Error::other("document owners were not projected"))?;
            relations.extend(
                project_document_rows(
                    project,
                    generation,
                    graph,
                    facts,
                    &owners,
                    &index,
                    &candidates,
                    &projection.entity_by_digest,
                    &control,
                )?
                .relations,
            );
        }
        require_eq(&relations.len(), &2, "document cycle relation count")?;
        require(
            relations.iter().all(|relation| {
                relation.kind() == GraphRelationKind::Extended(ExtendedRelationKind::Documents)
                    && matches!(
                        relation.resolution(),
                        RelationResolution::Resolved {
                            selector: ReusableTargetSelector::Symbol { symbol },
                            ..
                        } if symbol.kind == SymbolKind::Heading
                    )
            }),
            "document cycle emitted a non-canonical or unresolved edge",
        )?;
        Ok(())
    }

    #[test]
    fn document_casefold_collisions_refuse_exact_winners_and_share_invalidation_keys()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let nodes = vec![
            test_file_node("src/lib.rs", "rust"),
            test_file_node("SRC/lib.rs", "rust"),
        ];
        let control = super::super::standalone_index_work_control();
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        require_eq(
            &index.unresolved_reason("src/lib.rs")?,
            &Some(DocumentTargetUnresolvedReason::CaseConflict),
            "exact case-collision target",
        )?;
        let project = ProjectInstanceId::from_bytes([32; 16])?;
        require_eq(
            &document_casefold_resolution_key(project, "src/lib.rs")?,
            &document_casefold_resolution_key(project, "SRC/lib.rs")?,
            "casefold invalidation key",
        )?;
        Ok(())
    }

    #[test]
    fn document_target_state_change_refuses_stale_publication() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = super::super::standalone_index_work_control();
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &[], &scan_policy)?;
        let actual = index
            .unresolved_reason("docs/target.md")?
            .ok_or_else(|| std::io::Error::other("absent target unexpectedly resolved"))?;
        let stale = if actual == DocumentTargetUnresolvedReason::Missing {
            DocumentTargetUnresolvedReason::Ignored
        } else {
            DocumentTargetUnresolvedReason::Missing
        };
        let document_target_states = vec![("docs/target.md".to_string(), stale)];
        drop(index);
        let staged = StagedRepositoryGraph {
            project: ProjectInstanceId::from_bytes([34; 16])?,
            mutation: RepositoryGraphMutation::Full,
            entities: Vec::new(),
            relations: Vec::new(),
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: Vec::new(),
            relation_dependencies: Vec::new(),
            document_unresolved_reasons: Vec::new(),
            identity_rejections: Vec::new(),
            resolution_derivations: BTreeMap::new(),
            peak_retained_bytes: 0,
            projection_removals_before_entities: Vec::new(),
            scan_policy,
            document_target_states,
            database: None,
            retained_bytes: 0,
        };
        require(
            matches!(
                staged.revalidate_document_targets(temp.path()),
                Err(CliError::RefreshRequired(_))
            ),
            "changed non-indexed target state reached publication",
        )?;
        Ok(())
    }

    fn test_file_node(path: &str, language: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: path
                .rsplit_once('/')
                .map(|(parent, _name)| parent.to_string()),
            extension: Path::new(path)
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy())),
            language: Some(language.to_string()),
            size_bytes: Some(1),
            mtime_ns: Some(1),
            content_hash: Some(format!("hash:{path}")),
        }
    }

    #[test]
    fn large_document_projection_publishes_in_memory_staging_and_incremental()
    -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 10;
        const LINKS_PER_DOCUMENT: usize = 1_024;
        const INCREMENTAL_LINKS: usize = 1_024;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join("content/real-target"))?;
        fs::write(root.join("content/resolved.md"), "# Resolved\n")?;
        let symlink_available = match create_directory_link(
            &root.join("content/real-target"),
            &root.join("content/alias"),
        ) {
            Ok(()) => true,
            Err(source) if cfg!(windows) && source.raw_os_error() == Some(1314) => {
                fs::create_dir(root.join("content/alias"))?;
                false
            }
            Err(source) => return Err(source.into()),
        };
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("large document fixture project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let mut nodes = Vec::with_capacity(DOCUMENT_COUNT + 2);
        let mut docs_node = test_file_node("content", "unknown");
        docs_node.kind = NodeKind::Folder;
        docs_node.language = None;
        docs_node.extension = None;
        nodes.push(docs_node);
        let mut graphs = Vec::with_capacity(DOCUMENT_COUNT + 1);
        let mut document_facts: BTreeMap<
            String,
            Cow<'static, projectatlas_symbols::MarkdownFacts>,
        > = BTreeMap::new();
        for document in 0..DOCUMENT_COUNT {
            let path = format!("content/source-{document:02}.md");
            let mut links = Vec::with_capacity(LINKS_PER_DOCUMENT);
            if document == 0 {
                links.push("[resolved](resolved.md)".to_string());
                links.push("[symlink](alias)".to_string());
            }
            while links.len() < LINKS_PER_DOCUMENT {
                let link = links.len();
                links.push(format!(
                    "[missing-{document:02}-{link:04}](missing-{document:02}-{link:04}.md)"
                ));
            }
            let facts = projectatlas_symbols::extract_markdown_facts(&links.join("\n"));
            require_eq(
                &facts.link_candidates.len(),
                &LINKS_PER_DOCUMENT,
                "large document candidate count",
            )?;
            graphs.push(facts.symbol_graph(&path, Some("markdown")));
            document_facts.insert(path.clone(), Cow::Owned(facts));
            nodes.push(test_file_node(&path, "markdown"));
        }
        let resolved_facts = projectatlas_symbols::extract_markdown_facts("# Resolved\n");
        graphs.push(resolved_facts.symbol_graph("content/resolved.md", Some("markdown")));
        nodes.push(test_file_node("content/resolved.md", "markdown"));
        let candidate_count = document_facts
            .values()
            .map(|facts| facts.link_candidates.len())
            .sum::<usize>();
        require_eq(
            &candidate_count,
            &(DOCUMENT_COUNT * LINKS_PER_DOCUMENT),
            "large document projection candidate total",
        )?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let packages = PackageIndex::from_graphs(&graphs)?;

        let generation = IndexGeneration::new(1);
        let entities = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let in_memory = finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            &root,
            &nodes,
            &document_facts,
            &GraphIdentityAdmission::default(),
            entities,
            &candidates,
            &scan_policy,
            &control,
        )?;
        require(
            in_memory.database.is_none(),
            "normal projection unexpectedly selected disposable staging",
        )?;
        require(
            in_memory.document_unresolved_reasons.len() > GraphLimits::MAX_ROWS as usize,
            "normal projection did not retain a multi-ceiling reason vector",
        )?;
        require_eq(
            &in_memory.relations.len(),
            &candidate_count,
            "normal projection relation count",
        )?;
        require(
            in_memory.relations.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::Resolved { .. })
            }),
            "normal projection lost the resolved document result",
        )?;
        let symlink_relation = in_memory
            .relations
            .iter()
            .find(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Unresolved { reference }
                        if reference.as_str() == "alias"
                )
            })
            .ok_or("normal projection lost the symlink document result")?;
        let symlink_reason = in_memory
            .document_unresolved_reasons
            .iter()
            .find(|(key, _reason)| key == symlink_relation.key())
            .map(|(_key, reason)| *reason);
        require_eq(
            &symlink_reason,
            &Some(DocumentTargetUnresolvedReason::Unsupported),
            "symlink document result did not retain its closed reason",
        )?;
        require(
            !symlink_available || root.join("content/alias").exists(),
            "symlink document fixture disappeared before publication",
        )?;
        {
            let mut publication = store.begin_index_publication("large-document-projection")?;
            publication.upsert_scan_node_batch(&nodes)?;
            in_memory.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        let published_page = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            },
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require(
            published_page.truncated && published_page.rows.len() == GraphLimits::MAX_ROWS as usize,
            "normal projection publication did not expose a bounded multi-page result",
        )?;

        let incremental_path = "content/source-00.md".to_string();
        let incremental_source = (0..INCREMENTAL_LINKS)
            .map(|link| format!("[incremental-{link:04}](incremental-{link:04}.md)"))
            .collect::<Vec<_>>()
            .join("\n");
        let incremental_facts = projectatlas_symbols::extract_markdown_facts(&incremental_source);
        let incremental_graph = incremental_facts.symbol_graph(&incremental_path, Some("markdown"));
        let incremental_nodes = vec![test_file_node(&incremental_path, "markdown")];
        let incremental_packages =
            PackageIndex::from_graphs(std::slice::from_ref(&incremental_graph))?;
        let incremental_entities = build_entity_projection(
            project,
            IndexGeneration::new(2),
            &incremental_nodes,
            std::slice::from_ref(&incremental_graph),
            &incremental_packages,
            false,
            &control,
        )?;
        let incremental_candidates =
            resolution_registry_from_exports(&incremental_entities, &control)?;
        let incremental_staged = finish_projection_with_documents(
            project,
            IndexGeneration::new(2),
            RepositoryGraphMutation::AffectedPaths(vec![incremental_path.clone()]),
            std::slice::from_ref(&incremental_graph),
            &root,
            &nodes,
            &BTreeMap::from([(incremental_path.clone(), Cow::Owned(incremental_facts))]),
            &GraphIdentityAdmission::default(),
            incremental_entities,
            &incremental_candidates,
            &scan_policy,
            &control,
        )?;
        require(
            incremental_staged.document_unresolved_reasons.len() <= GraphLimits::MAX_ROWS as usize,
            "incremental projection exceeded its aggregate row budget",
        )?;
        enforce_incremental_projection_limits(
            &root,
            &BTreeSet::from([incremental_path]),
            RepositoryAffectedSourceFootprint {
                rows: 0,
                retained_bytes: 0,
                truncated: false,
            },
            &incremental_staged,
        )?;
        {
            let mut publication =
                store.begin_index_projection_refresh("large-document-projection")?;
            incremental_staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        require_eq(
            &store.repository_graph_generation()?,
            &Some(IndexGeneration::new(2)),
            "incremental publication generation",
        )?;

        let staging_generation = IndexGeneration::new(3);
        let staging_entities = build_entity_projection(
            project,
            staging_generation,
            &nodes,
            &graphs,
            &packages,
            true,
            &control,
        )?;
        let staging_candidates = resolution_registry_from_exports(&staging_entities, &control)?;
        let staged = finish_projection_in_database_with_documents(
            &root,
            &nodes,
            project,
            staging_generation,
            &graphs,
            &document_facts,
            &GraphIdentityAdmission::default(),
            staging_entities,
            &staging_candidates,
            &scan_policy,
            &control,
        )?;
        let database_stage = staged
            .database
            .as_ref()
            .ok_or("large projection did not select disposable staging")?;
        let staged_database_bytes = fs::metadata(
            database_stage
                .directory()?
                .path()
                .join(GRAPH_STAGE_DATABASE_FILE_NAME),
        )?
        .len();
        require(
            staged_database_bytes > 0,
            "disposable staging database retained no durable bytes",
        )?;
        {
            let mut publication =
                store.begin_index_projection_refresh("large-document-projection")?;
            publication.upsert_scan_node_batch(&nodes)?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        let staged_page = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            },
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require(
            staged_page.truncated && staged_page.rows.len() == GraphLimits::MAX_ROWS as usize,
            "disposable staging publication did not expose a bounded multi-page result",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_document_admission_uses_emitted_rows() -> Result<(), Box<dyn Error>> {
        const DISCARDED_DOCUMENT_COUNT: usize = 513;
        const EMITTED_DOCUMENT_COUNT: usize = 10;
        const CANDIDATES_PER_DOCUMENT: usize = 1_024;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join("content"))?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let paths = (0..DISCARDED_DOCUMENT_COUNT)
            .map(|index| format!("content/links-{index:03}.md"))
            .collect::<Vec<_>>();
        let mut nodes = paths
            .iter()
            .map(|path| test_file_node(path, "markdown"))
            .collect::<Vec<_>>();
        store.replace_scan(&nodes)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let symbols = empty_symbol_build_stage();

        for path in &paths {
            let file_name = path
                .rsplit_once('/')
                .map(|(_parent, file_name)| file_name)
                .ok_or("same-file document fixture path has no parent")?;
            let self_source = (0..CANDIDATES_PER_DOCUMENT)
                .map(|index| format!("[self-{index:05}]({file_name})"))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join(path), &self_source)?;
            let self_facts = projectatlas_symbols::extract_markdown_facts(&self_source);
            require_eq(
                &self_facts.link_candidates.len(),
                &CANDIDATES_PER_DOCUMENT,
                "same-file raw candidates per document",
            )?;
            store.replace_symbol_graph(&self_facts.symbol_graph(path, Some("markdown")))?;
        }
        for (node, path) in nodes.iter_mut().zip(&paths) {
            let bytes = fs::read(root.join(path))?;
            node.content_hash = Some(blake3::hash(&bytes).to_hex().to_string());
        }
        store.replace_scan(&nodes)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let incremental = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &paths,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require(
            matches!(
                &incremental.mutation,
                RepositoryGraphMutation::AffectedPaths(affected) if affected == &paths
            ),
            "same-file candidates were not retained as an incremental projection",
        )?;
        require(
            incremental.relations.is_empty() && incremental.document_unresolved_reasons.is_empty(),
            "same-file candidates emitted document rows despite the no-fragment filter",
        )?;
        require(
            store.repository_graph_generation()?.is_none(),
            "incremental admission test unexpectedly published a generation",
        )?;

        let emitted_paths = paths
            .iter()
            .take(EMITTED_DOCUMENT_COUNT)
            .cloned()
            .collect::<Vec<_>>();
        for path in &emitted_paths {
            let unsupported_source = (0..CANDIDATES_PER_DOCUMENT)
                .map(|index| format!("[external-{index:05}](target-{index:05}.md#invalid?)"))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join(path), &unsupported_source)?;
            let unsupported_facts =
                projectatlas_symbols::extract_markdown_facts(&unsupported_source);
            require_eq(
                &unsupported_facts.link_candidates.len(),
                &CANDIDATES_PER_DOCUMENT,
                "unsupported raw candidates per document",
            )?;
            store.replace_symbol_graph(&unsupported_facts.symbol_graph(path, Some("markdown")))?;
        }
        for (node, path) in nodes.iter_mut().zip(&paths) {
            let bytes = fs::read(root.join(path))?;
            node.content_hash = Some(blake3::hash(&bytes).to_hex().to_string());
        }
        store.replace_scan(&nodes)?;
        let error = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &emitted_paths,
            &scan_policy,
            &symbols,
            &control,
        )
        .err()
        .ok_or("emitted document rows over the ceiling were admitted incrementally")?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "emitted-row overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "emitted-row overflow scope",
        )?;
        require(
            store.repository_graph_generation()?.is_none(),
            "emitted-row overflow changed the current generation",
        )?;
        Ok(())
    }

    #[test]
    fn reopened_source_parse_success_does_not_promote_fallback_graph_facts()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open(&database)?;
        let graph = SymbolGraph {
            path: "src/optional.lang".to_string(),
            language: Some("optional-language".to_string()),
            parser: ParserKind::Fallback,
            symbols: vec![CodeSymbol {
                path: "src/optional.lang".to_string(),
                language: Some("optional-language".to_string()),
                name: "entry".to_string(),
                kind: SymbolKind::Function,
                signature: "entry()".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Fallback,
                detail: None,
            }],
            relations: vec![SymbolRelation {
                path: "src/optional.lang".to_string(),
                source_name: "entry".to_string(),
                target_name: "helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper()".to_string(),
                parser: ParserKind::Fallback,
            }],
        };
        store.replace_symbol_graph_with_metadata(
            &graph,
            &SourceParseMetadata {
                path: graph.path.clone(),
                language: graph.language.clone(),
                parser: ParserKind::TreeSitter,
                symbol_count: graph.symbols.len(),
                relation_count: graph.relations.len(),
            },
        )?;
        drop(store);

        let reader = AtlasStore::open_read_only(&database)?;
        let graphs = reader.load_symbol_graphs_for_paths(std::slice::from_ref(&graph.path))?;
        require_eq(&graphs, &vec![graph], "reopened fact graph")?;
        let project = ProjectInstanceId::from_bytes([7; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entities =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            entities,
            &candidates,
            &control,
        )?;
        require_eq(&staged.relations.len(), &1, "normalized relation count")?;
        require_eq(
            &staged.relations[0].confidence(),
            &ConfidenceClass::Low,
            "fallback relation confidence after reopen",
        )?;
        require_eq(
            &staged.relations[0].completeness(),
            &Completeness::Partial,
            "fallback relation completeness after reopen",
        )?;
        require(
            staged
                .relations
                .iter()
                .any(|relation| relation.kind() == GraphRelationKind::Legacy(RelationKind::Calls)),
            "reopened graph lost its legacy call relation",
        )?;
        require_eq(&staged.coverage.len(), &1, "normalized coverage count")?;
        require_eq(
            &staged.coverage[0].state(),
            &CoverageState::Partial,
            "fallback coverage after reopen",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn partial_php_graph_publishes_existing_partial_coverage() -> Result<(), Box<dyn Error>> {
        let graph = extract_symbol_graph(
            "src/dynamic.php",
            Some("php"),
            "<?php function run(): void { $callable(); helper(); }",
        );
        require_eq(
            &graph.parser,
            &ParserKind::Fallback,
            "partial PHP fact parser",
        )?;
        let coverage = coverage_for_graph(&graph, IndexGeneration::new(1))?;
        require_eq(
            &coverage.state(),
            &CoverageState::Partial,
            "partial PHP coverage state",
        )?;
        require_eq(&coverage.covered(), &1, "partial PHP covered relations")?;
        require_eq(&coverage.omitted(), &1, "partial PHP omitted relations")?;
        require(
            coverage.reason().is_some(),
            "partial PHP coverage must disclose its reason",
        )?;
        Ok(())
    }

    #[test]
    fn mixed_php_graph_publishes_complete_coverage() -> Result<(), Box<dyn Error>> {
        for (path, source, symbol_name) in [
            (
                "src/inline-output.php",
                "//x<?php function marker(): void { helper(); }",
                "marker",
            ),
            (
                "src/inline-echo.php",
                "#output<?= $value ?><?php function after_echo(): void { helper(); }",
                "after_echo",
            ),
        ] {
            let graph = extract_symbol_graph(path, Some("php"), source);
            require_eq(
                &graph.parser,
                &ParserKind::TreeSitter,
                "mixed PHP fact parser",
            )?;
            require(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == symbol_name),
                "mixed PHP declaration is missing before coverage projection",
            )?;
            let coverage = coverage_for_graph(&graph, IndexGeneration::new(1))?;
            require_eq(
                &coverage.state(),
                &CoverageState::Complete,
                "mixed PHP coverage state",
            )?;
            require(
                coverage.covered() > 0,
                "mixed PHP complete coverage must retain static relations",
            )?;
            require_eq(&coverage.omitted(), &0, "mixed PHP omitted relations")?;
            require(
                coverage.reason().is_none(),
                "complete mixed PHP coverage must not disclose a partial reason",
            )?;
        }
        Ok(())
    }

    #[test]
    fn persisted_closure_overflow_preserves_the_complete_generation() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("persisted-closure-overflow");
        fs::create_dir_all(&root)?;
        let mut store = AtlasStore::open_for_project(&root.join("projectatlas.db"), &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![function_graph("src/lib.rs", 1)];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entity_projection = build_entity_projection(
            project,
            IndexGeneration::new(1),
            &[],
            &graphs,
            &packages,
            true,
            &control,
        )?;
        let dependency_keys = entity_projection
            .keys_by_graph
            .values()
            .flat_map(projectatlas_symbols::ResolutionKeyProjection::relation_keys)
            .flat_map(|relation| relation.keys().iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let candidates = resolution_registry_from_exports(&entity_projection, &control)?;
        let staged = finish_projection(
            project,
            IndexGeneration::new(1),
            RepositoryGraphMutation::Full,
            &graphs,
            entity_projection,
            &candidates,
            &control,
        )?;
        let file_key = staged
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or("staged file entity is missing")?
            .key()
            .clone();
        {
            let mut publication = store.begin_index_publication("closure-overflow")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&[Node {
                path: "src/lib.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(1),
                mtime_ns: Some(1),
                content_hash: Some("initial".to_string()),
            }])?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }

        store.replace_symbol_graph(&function_graph(
            "src/lib.rs",
            usize::try_from(MAX_INCREMENTAL_GRAPH_ROWS).unwrap_or(usize::MAX),
        ))?;
        let publication_before = store
            .index_publication()?
            .ok_or("complete publication is missing")?;
        let entity_before = store
            .repository_graph_entity(&file_key)?
            .ok_or("published file entity is missing")?;
        let exports_before =
            store.repository_export_keys_for_paths(project, &["src/lib.rs".to_string()], 100)?;
        let dependencies_before =
            store.repository_affected_source_paths(project, &dependency_keys, 100)?;
        let empty_symbols = empty_symbol_build_stage();
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;

        let error = stage_incremental_repository_graph(
            &store,
            &root,
            publication_before.generation,
            &[],
            &["src/lib.rs".to_string()],
            &scan_policy,
            &empty_symbols,
            &control,
        )
        .err()
        .ok_or_else(|| io::Error::other("oversized closure acquired the publication writer"))?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "persisted overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "persisted overflow scope",
        )?;
        require_eq(
            &store.index_publication()?,
            &Some(publication_before),
            "publication after overflow",
        )?;
        require_eq(
            &store.repository_graph_entity(&file_key)?,
            &Some(entity_before),
            "file entity after overflow",
        )?;
        require_eq(
            &store.repository_export_keys_for_paths(project, &["src/lib.rs".to_string()], 100)?,
            &exports_before,
            "export keys after overflow",
        )?;
        require_eq(
            &store.repository_affected_source_paths(project, &dependency_keys, 100)?,
            &dependencies_before,
            "dependency owners after overflow",
        )?;
        Ok(())
    }

    #[test]
    fn semantic_provider_key_rejection_keeps_valid_exports_and_relations()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("semantic-resolution-key-admission");
        fs::create_dir_all(&root)?;
        let long_path = |folder: &str, component: char, file: &str| {
            let components = (0..20)
                .map(|_| std::iter::repeat_n(component, 200).collect::<String>())
                .collect::<Vec<_>>()
                .join("/");
            format!("src/{folder}/{components}/{file}")
        };
        let path = long_path("first", 'd', "page.rs");
        let sibling_path = long_path("second", 'e', "sibling.rs");
        let nodes = vec![
            test_file_node(&path, "rust"),
            test_file_node(&sibling_path, "rust"),
        ];
        let project_database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&project_database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("semantic resolution project identity is missing")?;
        let initial_graphs = vec![
            semantic_resolution_key_graph(&path, true),
            semantic_resolution_key_graph(&sibling_path, false),
        ];
        let initial_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let scan_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &initial_control)?;
        let initial_stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &symbol_build_stage_for_graphs(initial_graphs.clone()),
            &initial_control,
        )?;
        require(
            initial_stage
                .identity_rejections
                .iter()
                .filter(|row| row.field == GraphIdentityField::ResolutionKey)
                .count()
                >= 2,
            "semantic provider full-stage did not retain every invalid resolution-key fact",
        )?;
        let initial_coverage = initial_stage
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path: coverage_path } if coverage_path.as_str() == path
                )
            })
            .ok_or("semantic provider coverage row is missing")?;
        require_eq(
            &initial_coverage.state(),
            &CoverageState::Partial,
            "semantic provider valid-symbol coverage state",
        )?;
        require(
            initial_coverage.covered() > 0,
            "semantic provider valid symbols were not counted as covered",
        )?;
        let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled.cancel();
        {
            let mut publication = store.begin_index_publication("semantic-resolution-cancel")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            let error = initial_stage.apply(&mut publication, &canceled).err();
            require(
                matches!(
                    error,
                    Some(CliError::IndexWork(IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::Publication
                    }))
                ),
                "semantic provider cancellation was not observed after admission",
            )?;
        }
        require_eq(
            &store.index_publication()?,
            &None,
            "publication after semantic provider cancellation",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &initial_stage,
            &initial_control,
            "semantic-resolution-initial",
        )?;
        let base_generation = store
            .index_publication()?
            .ok_or("semantic resolution initial publication is missing")?
            .generation;
        let rejection_paths = nodes
            .iter()
            .map(|node| RepositoryNodePath::new(Path::new(&node.path)))
            .collect::<Result<Vec<_>, _>>()?;
        let rejections =
            store.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        let rejections = rejections
            .into_iter()
            .filter(|row| row.field == GraphIdentityField::ResolutionKey)
            .collect::<Vec<_>>();
        require(
            rejections.len() >= 2,
            "semantic provider full-stage rejection detail count",
        )?;
        require(
            rejections.iter().all(|row| {
                row.path.as_str() == path
                    && row.reason == GraphIdentityRejectionReason::Oversized
                    && row.parser == ParserKind::TreeSitter
            }),
            "semantic provider rejection provenance was not exact",
        )?;
        require(
            rejections.iter().any(|row| row.span.start_line() == 4)
                && rejections.iter().any(|row| row.span.start_line() == 5),
            "semantic provider did not retain distinct invalid symbol spans",
        )?;
        let rejection_wire = serde_json::to_string(&rejections)?;
        require(
            !rejection_wire.contains("LeakedIdentity"),
            "semantic provider rejection retained raw identity text",
        )?;

        let file_entities = store.repository_graph_entities_by_path(
            project,
            &RepositoryNodePath::new(Path::new(&path))?,
            64,
        )?;
        require(
            file_entities.rows.iter().any(|entity| {
                matches!(
                    entity.selector(),
                    EntitySelector::Symbol { symbol } if symbol.name.as_str() == "page_helper"
                )
            }),
            "valid semantic sibling export was dropped",
        )?;
        let sibling_entities = store.repository_graph_entities_by_path(
            project,
            &RepositoryNodePath::new(Path::new(&sibling_path))?,
            64,
        )?;
        require(
            sibling_entities.rows.iter().any(|entity| {
                matches!(
                    entity.selector(),
                    EntitySelector::Symbol { symbol } if symbol.name.as_str() == "sibling_helper"
                )
            }),
            "valid semantic sibling-folder export was dropped",
        )?;
        let calls = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Calls),
            },
            32,
        )?;
        require(
            calls
                .rows
                .iter()
                .any(|relation| relation.resolution().resolved_target().is_some()),
            "valid semantic sibling relation was not resolved",
        )?;
        let imports = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Imports),
            },
            32,
        )?;
        require(
            !imports.rows.is_empty(),
            "valid semantic import relation was dropped beside invalid imports",
        )?;
        let exports =
            store.repository_export_keys_for_paths(project, std::slice::from_ref(&path), 128)?;
        require(
            exports.rows.len() > 2,
            "valid semantic provider resolution exports were dropped",
        )?;
        let sibling_exports = store.repository_export_keys_for_paths(
            project,
            std::slice::from_ref(&sibling_path),
            128,
        )?;
        require(
            sibling_exports.rows.len() > 2,
            "valid semantic sibling-folder resolution exports were dropped",
        )?;
        let sibling_exports_before_incremental = sibling_exports.rows;

        drop(store);
        let mut store = AtlasStore::open_for_project(&project_database, &root)?;
        require_eq(
            &store.project_instance_id()?,
            &Some(project),
            "semantic provider project identity after SQLite reopen",
        )?;
        let reopened_sibling_exports = store.repository_export_keys_for_paths(
            project,
            std::slice::from_ref(&sibling_path),
            128,
        )?;
        require_eq(
            &reopened_sibling_exports.rows,
            &sibling_exports_before_incremental,
            "semantic provider valid exports after SQLite reopen",
        )?;

        let invalid_sibling_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let invalid_sibling_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &invalid_sibling_control)?;
        let invalid_sibling_stage = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            std::slice::from_ref(&sibling_path),
            &invalid_sibling_policy,
            &symbol_build_stage_for_graphs(vec![semantic_resolution_key_graph(
                &sibling_path,
                true,
            )]),
            &invalid_sibling_control,
        )?;
        let incremental_generation = base_generation
            .checked_next()
            .ok_or("semantic resolution incremental generation overflowed")?;
        let direct_derivations = invalid_sibling_stage
            .resolution_derivations
            .get(&(sibling_path.clone(), incremental_generation))
            .copied();
        require_eq(
            &direct_derivations,
            &Some(1),
            "semantic provider direct resolution derivation count",
        )?;
        require(
            invalid_sibling_stage
                .identity_rejections
                .iter()
                .filter(|row| row.field == GraphIdentityField::ResolutionKey)
                .count()
                >= 2,
            "semantic provider incremental resolution-key rejection",
        )?;
        {
            let mut publication =
                store.begin_index_publication("semantic-resolution-incremental-invalid")?;
            invalid_sibling_stage.apply(&mut publication, &invalid_sibling_control)?;
            publication.complete()?;
        }
        let incremented_generation = store
            .index_publication()?
            .ok_or("semantic resolution incremental publication is missing")?
            .generation;
        let with_two_rejections =
            store.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        require(
            with_two_rejections.len() >= 4,
            "semantic provider incremental rejection detail count",
        )?;
        require(
            with_two_rejections
                .iter()
                .any(|row| row.path.as_str() == sibling_path),
            "semantic provider incremental rejection was not added for the changed path",
        )?;
        let invalid_sibling_exports = store.repository_export_keys_for_paths(
            project,
            std::slice::from_ref(&sibling_path),
            128,
        )?;
        require(
            sibling_exports_before_incremental
                .iter()
                .all(|key| invalid_sibling_exports.rows.contains(key)),
            "semantic provider valid exports were lost beside incremental rejection",
        )?;
        let invalid_sibling_calls = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Calls),
            },
            32,
        )?;
        require(
            invalid_sibling_calls
                .rows
                .iter()
                .any(|relation| relation.resolution().resolved_target().is_some()),
            "semantic provider valid resolved relation was lost beside incremental rejection",
        )?;

        let repaired_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let repaired_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &repaired_control)?;
        let repaired_stage = stage_incremental_repository_graph(
            &store,
            &root,
            incremented_generation,
            &nodes,
            std::slice::from_ref(&sibling_path),
            &repaired_policy,
            &symbol_build_stage_for_graphs(vec![semantic_resolution_key_graph(
                &sibling_path,
                false,
            )]),
            &repaired_control,
        )?;
        {
            let mut publication =
                store.begin_index_publication("semantic-resolution-incremental-repair")?;
            repaired_stage.apply(&mut publication, &repaired_control)?;
            publication.complete()?;
        }
        let repaired_generation = store
            .index_publication()?
            .ok_or("semantic resolution repair publication is missing")?
            .generation;
        let repaired_rejections =
            store.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        require(
            repaired_rejections.len() >= 2,
            "semantic provider repaired rejection detail count",
        )?;
        require(
            repaired_rejections
                .iter()
                .all(|row| row.path.as_str() == path),
            "semantic provider repair removed or replaced an unrelated path detail",
        )?;
        let repaired_sibling_exports = store.repository_export_keys_for_paths(
            project,
            std::slice::from_ref(&sibling_path),
            128,
        )?;
        require_eq(
            &repaired_sibling_exports.rows,
            &sibling_exports_before_incremental,
            "semantic provider repair did not restore valid exports",
        )?;

        let fault_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let fault_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &fault_control)?;
        let mut fault_stage = stage_full_repository_graph(
            &store,
            &root,
            repaired_generation,
            &nodes,
            &fault_policy,
            &symbol_build_stage_for_graphs(initial_graphs.clone()),
            &fault_control,
        )?;
        fault_stage.identity_rejections.resize(
            usize::try_from(GraphLimits::MAX_ROWS)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
            fault_stage.identity_rejections[0].clone(),
        );
        {
            let mut publication = store.begin_index_publication("semantic-resolution-fault")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            require(
                fault_stage.apply(&mut publication, &fault_control).is_err(),
                "semantic provider late rejection-detail fault did not fail",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(repaired_generation),
            "semantic provider generation after late rejection-detail fault",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?,
            &repaired_rejections,
            "semantic provider prior generation after fault",
        )?;

        let retry_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let retry_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &retry_control)?;
        let retry_stage = stage_full_repository_graph(
            &store,
            &root,
            repaired_generation,
            &nodes,
            &retry_policy,
            &symbol_build_stage_for_graphs(initial_graphs),
            &retry_control,
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &retry_stage,
            &retry_control,
            "semantic-resolution-retry",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?,
            &rejections,
            "semantic provider deterministic retry",
        )?;
        Ok(())
    }

    #[test]
    fn full_stage_admits_siblings_and_reopens_typed_rejections_with_cancel_retry()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("full-identity-admission");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("tests"))?;
        fs::write(
            root.join("src/one.rs"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )?;
        fs::write(
            root.join("tests/two.rs"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("full admission project identity is missing")?;
        let nodes = vec![
            test_file_node("src/one.rs", "rust"),
            test_file_node("tests/two.rs", "rust"),
        ];
        let mut first_graph =
            identity_sibling_graph("src/one.rs", GraphIdentityField::RelationTarget);
        first_graph.relations.push(SymbolRelation {
            path: "src/one.rs".to_string(),
            source_name: "bad\u{0}source".to_string(),
            target_name: "bad\u{0}target".to_string(),
            kind: RelationKind::Calls,
            line: 4,
            context: "identity admission dual-field fixture".to_string(),
            parser: ParserKind::TreeSitter,
        });
        let graphs = vec![
            first_graph,
            identity_sibling_graph("tests/two.rs", GraphIdentityField::RelationSource),
        ];
        let symbols = symbol_build_stage_for_graphs(graphs);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require_eq(
            &staged.identity_rejections.len(),
            &4,
            "full-stage typed rejection count",
        )?;
        let staged_dual_rejections = staged
            .identity_rejections
            .iter()
            .filter(|row| {
                row.path.as_str() == "src/one.rs"
                    && row.span.start_line() == 4
                    && row.reason == GraphIdentityRejectionReason::ControlCharacters
            })
            .collect::<Vec<_>>();
        require_eq(
            &staged_dual_rejections.len(),
            &2,
            "full-stage dual-field rejection details",
        )?;
        require(
            staged_dual_rejections
                .iter()
                .any(|row| row.field == GraphIdentityField::RelationSource)
                && staged_dual_rejections
                    .iter()
                    .any(|row| row.field == GraphIdentityField::RelationTarget),
            "full-stage dual-field rejection lost source or target provenance",
        )?;
        require(
            staged_dual_rejections
                .iter()
                .map(|row| row.fact_index)
                .all(|fact_index| fact_index == staged_dual_rejections[0].fact_index),
            "full-stage dual-field rejection lost parser fact identity",
        )?;
        let staged_first_coverage = staged
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path } if path.as_str() == "src/one.rs"
                )
            })
            .ok_or("full-stage first coverage row is missing")?;
        require_eq(
            &staged_first_coverage.omitted(),
            &2,
            "full-stage dual-field rejection counted once per parser fact",
        )?;
        let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled.cancel();
        {
            let mut publication = store.begin_index_publication("full-identity-cancel")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            let error = staged.apply(&mut publication, &canceled).err();
            require(
                matches!(
                    error,
                    Some(CliError::IndexWork(IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::Publication
                    }))
                ),
                "full-stage cancellation was not observed after admission",
            )?;
        }
        require_eq(
            &store.index_publication()?,
            &None,
            "publication after canceled full-stage admission",
        )?;

        publish_full_staged_graph(&mut store, &nodes, &staged, &control, "full-identity")?;
        let first_publication = store
            .index_publication()?
            .ok_or("full identity publication is missing")?;
        require_eq(
            &first_publication.generation,
            &IndexGeneration::new(1),
            "first full identity generation",
        )?;
        drop(store);

        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        let rejection_paths = nodes
            .iter()
            .map(|node| RepositoryNodePath::new(Path::new(&node.path)))
            .collect::<Result<Vec<_>, _>>()?;
        let reopened_rejections =
            reopened.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        require_eq(
            &reopened_rejections.len(),
            &4,
            "reopened full-stage typed rejection count",
        )?;
        let reopened_dual_rejections = reopened_rejections
            .iter()
            .filter(|row| {
                row.path.as_str() == "src/one.rs"
                    && row.span.start_line() == 4
                    && row.reason == GraphIdentityRejectionReason::ControlCharacters
            })
            .collect::<Vec<_>>();
        require_eq(
            &reopened_dual_rejections.len(),
            &2,
            "reopened dual-field rejection details",
        )?;
        require(
            reopened_dual_rejections
                .iter()
                .any(|row| row.field == GraphIdentityField::RelationSource)
                && reopened_dual_rejections
                    .iter()
                    .any(|row| row.field == GraphIdentityField::RelationTarget),
            "reopened dual-field rejection lost source or target provenance",
        )?;
        require(
            reopened_dual_rejections
                .iter()
                .map(|row| row.fact_index)
                .all(|fact_index| fact_index == reopened_dual_rejections[0].fact_index),
            "reopened dual-field rejection lost parser fact identity",
        )?;
        let target_rejection = reopened_rejections
            .iter()
            .find(|row| {
                row.field == GraphIdentityField::RelationTarget
                    && row.path.as_str() == "src/one.rs"
                    && row.span.start_line() == 3
            })
            .ok_or("relation-target rejection is missing")?;
        require_eq(
            &target_rejection.path.as_str(),
            &"src/one.rs",
            "relation-target rejection path",
        )?;
        require_eq(
            &target_rejection.parser,
            &ParserKind::TreeSitter,
            "relation-target rejection parser",
        )?;
        require_eq(
            &target_rejection.reason,
            &GraphIdentityRejectionReason::ControlCharacters,
            "relation-target rejection reason",
        )?;
        require_eq(
            &target_rejection.span.start_line(),
            &3,
            "relation-target rejection start line",
        )?;
        require_eq(
            &target_rejection.span.end_line(),
            &3,
            "relation-target rejection end line",
        )?;
        let source_rejection = reopened_rejections
            .iter()
            .find(|row| {
                row.field == GraphIdentityField::RelationSource
                    && row.path.as_str() == "tests/two.rs"
            })
            .ok_or("relation-source rejection is missing")?;
        require_eq(
            &source_rejection.path.as_str(),
            &"tests/two.rs",
            "relation-source rejection path",
        )?;
        require_eq(
            &source_rejection.parser,
            &ParserKind::TreeSitter,
            "relation-source rejection parser",
        )?;
        require_eq(
            &source_rejection.reason,
            &GraphIdentityRejectionReason::ControlCharacters,
            "relation-source rejection reason",
        )?;
        require_eq(
            &source_rejection.span.start_line(),
            &3,
            "relation-source rejection start line",
        )?;
        require_eq(
            &source_rejection.span.end_line(),
            &3,
            "relation-source rejection end line",
        )?;
        let wire = serde_json::to_string(&reopened_rejections)?;
        require(
            !wire.contains("bad") && !wire.contains("target\u{0}"),
            "full-stage typed rejection retained raw invalid identity material",
        )?;
        for path in &rejection_paths {
            let entities = reopened.repository_graph_entities_by_path(project, path, 64)?;
            require(
                entities.rows.iter().any(|entity| {
                    matches!(
                        entity.selector(),
                        EntitySelector::Symbol { symbol } if symbol.name.as_str() == "caller"
                    )
                }),
                "valid sibling symbol was not navigable after SQLite reopen",
            )?;
        }
        let calls = reopened.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Calls),
            },
            16,
        )?;
        require_eq(
            &calls.rows.len(),
            &2,
            "valid sibling call relations after SQLite reopen",
        )?;
        reopened.finish_index_read_snapshot()?;

        let mut writer = AtlasStore::open_for_project(&database, &root)?;
        let fault_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let fault_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &fault_control)?;
        let mut fault_stage = stage_full_repository_graph(
            &writer,
            &root,
            first_publication.generation,
            &nodes,
            &fault_policy,
            &symbols,
            &fault_control,
        )?;
        fault_stage.identity_rejections.resize(
            usize::try_from(GraphLimits::MAX_ROWS)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
            fault_stage.identity_rejections[0].clone(),
        );
        {
            let mut publication = writer.begin_index_publication("full-identity-fault")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            require(
                fault_stage.apply(&mut publication, &fault_control).is_err(),
                "oversized rejection detail did not fault after graph replacement",
            )?;
        }
        require_eq(
            &writer
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(first_publication.generation),
            "generation after late rejection-detail fault",
        )?;
        let retained =
            writer.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        require_eq(
            &retained,
            &reopened_rejections,
            "prior complete typed rejection generation after fault",
        )?;
        let retry_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let retry_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &retry_control)?;
        let retry_stage = stage_full_repository_graph(
            &writer,
            &root,
            first_publication.generation,
            &nodes,
            &retry_policy,
            &symbols,
            &retry_control,
        )?;
        publish_full_staged_graph(
            &mut writer,
            &nodes,
            &retry_stage,
            &retry_control,
            "full-identity-retry",
        )?;
        let retried =
            writer.repository_graph_identity_rejections(project, &rejection_paths, 16, None)?;
        require_eq(
            &retried,
            &reopened_rejections,
            "deterministic full-stage retry",
        )?;
        Ok(())
    }

    #[test]
    fn full_stage_reuse_carries_persisted_rejection_coverage_and_replaces_it()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("full-reuse-identity-admission");
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src/reused.rs"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("full reuse project identity is missing")?;
        let path = "src/reused.rs";
        let nodes = vec![test_file_node(path, "rust")];
        let graph = identity_sibling_graph(path, GraphIdentityField::RelationTarget);
        let first_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let first_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &first_control)?;
        let mut first_symbols = symbol_build_stage_for_graphs(vec![graph]);
        first_symbols.identity_admission =
            super::admit_symbol_build_stage(&mut first_symbols, &first_control)?;
        let first_graph = first_symbols
            .changes
            .iter()
            .find_map(|change| match change {
                SymbolProjectionChange::Parsed(parsed) => Some(parsed.graph.clone()),
                SymbolProjectionChange::Clear { .. } => None,
            })
            .ok_or("full reuse parsed graph is missing")?;
        let first_stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &first_policy,
            &first_symbols,
            &first_control,
        )?;
        require_eq(
            &first_stage.identity_rejections.len(),
            &1,
            "full reuse initial rejection detail count",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &first_stage,
            &first_control,
            "full-reuse-initial",
        )?;
        // This is the same persistence sink used by apply_symbol_build_stage;
        // the next full stage intentionally receives no parser changes.
        store.replace_symbol_graph(&first_graph)?;
        let first_generation = store
            .index_publication()?
            .ok_or("full reuse initial publication is missing")?
            .generation;
        drop(store);

        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let reuse_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let reuse_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &reuse_control)?;
        let reused_stage = stage_full_repository_graph(
            &store,
            &root,
            first_generation,
            &nodes,
            &reuse_policy,
            &empty_symbol_build_stage(),
            &reuse_control,
        )?;
        require_eq(
            &reused_stage.identity_rejections,
            &first_stage.identity_rejections,
            "full reuse retained exact persisted rejection details",
        )?;
        let reused_coverage = reused_stage
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path: coverage_path } if coverage_path.as_str() == path
                ) && coverage.relation().is_none()
            })
            .ok_or("full reuse coverage row is missing")?;
        require_eq(
            &reused_coverage.state(),
            &CoverageState::Partial,
            "full reuse retained partial coverage",
        )?;
        require_eq(
            &reused_coverage.omitted(),
            &1,
            "full reuse retained rejection omission count",
        )?;
        require(
            reused_stage
                .relations
                .iter()
                .any(|relation| relation.resolution().resolved_target().is_some()),
            "full reuse dropped the valid sibling relation",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &reused_stage,
            &reuse_control,
            "full-reuse-republish",
        )?;
        let persisted_paths = vec![RepositoryNodePath::new(Path::new(path))?];
        let persisted =
            store.repository_graph_identity_rejections(project, &persisted_paths, 16, None)?;
        require_eq(
            &persisted,
            &first_stage.identity_rejections,
            "full reuse persisted exact rejection details",
        )?;

        let cancel_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let canceled_stage = stage_full_repository_graph(
            &store,
            &root,
            first_generation
                .checked_next()
                .ok_or("full reuse generation overflow")?,
            &nodes,
            &reuse_policy,
            &empty_symbol_build_stage(),
            &cancel_control,
        )?;
        cancel_control.cancel();
        {
            let mut publication = store.begin_index_publication("full-reuse-cancel")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            require(
                canceled_stage
                    .apply(&mut publication, &cancel_control)
                    .is_err(),
                "full reuse cancellation was ignored",
            )?;
        }
        let retained_generation = store
            .index_publication()?
            .ok_or("full reuse republished generation is missing")?
            .generation;
        require_eq(
            &retained_generation,
            &first_generation
                .checked_next()
                .ok_or("full reuse generation overflow")?,
            "full reuse cancellation changed the current generation",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &persisted_paths, 16, None)?,
            &persisted,
            "full reuse cancellation changed rejection details",
        )?;

        let fault_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let fault_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &fault_control)?;
        let mut fault_stage = stage_full_repository_graph(
            &store,
            &root,
            retained_generation,
            &nodes,
            &fault_policy,
            &empty_symbol_build_stage(),
            &fault_control,
        )?;
        fault_stage.identity_rejections.resize(
            usize::try_from(GraphLimits::MAX_ROWS)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
            fault_stage.identity_rejections[0].clone(),
        );
        {
            let mut publication = store.begin_index_publication("full-reuse-fault")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            require(
                fault_stage.apply(&mut publication, &fault_control).is_err(),
                "full reuse late rejection-detail fault was ignored",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .ok_or("full reuse fault lost publication")?
                .generation,
            &retained_generation,
            "full reuse late fault changed current generation",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &persisted_paths, 16, None)?,
            &persisted,
            "full reuse late fault changed rejection details",
        )?;
        let retry_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let retry_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &retry_control)?;
        let retry_stage = stage_full_repository_graph(
            &store,
            &root,
            retained_generation,
            &nodes,
            &retry_policy,
            &empty_symbol_build_stage(),
            &retry_control,
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &retry_stage,
            &retry_control,
            "full-reuse-retry",
        )?;
        require_eq(
            &store.repository_graph_identity_rejections(project, &persisted_paths, 16, None)?,
            &persisted,
            "full reuse retry changed rejection details",
        )?;
        let retained_generation = store
            .index_publication()?
            .ok_or("full reuse retry publication is missing")?
            .generation;

        let valid_graph = extract_symbol_graph(
            path,
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        );
        let valid_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let valid_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &valid_control)?;
        let valid_stage = stage_full_repository_graph(
            &store,
            &root,
            retained_generation,
            &nodes,
            &valid_policy,
            &symbol_build_stage_for_graphs(vec![valid_graph]),
            &valid_control,
        )?;
        require(
            valid_stage.identity_rejections.is_empty(),
            "full reuse changed graph retained stale rejection detail",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &valid_stage,
            &valid_control,
            "full-reuse-repair",
        )?;
        require(
            store
                .repository_graph_identity_rejections(project, &persisted_paths, 16, None)?
                .is_empty(),
            "full reuse repaired graph retained stale rejection detail",
        )?;
        Ok(())
    }

    #[test]
    fn full_stage_reuse_preserves_capped_fallback_omission_count_without_details()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("full-reuse-fallback-identity-admission");
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src/reused.rs"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("fallback reuse project identity is missing")?;
        let path = "src/reused.rs";
        let nodes = vec![test_file_node(path, "rust")];
        let mut graph = identity_sibling_graph(path, GraphIdentityField::RelationTarget);
        graph.parser = ParserKind::Fallback;
        for symbol in &mut graph.symbols {
            symbol.parser = ParserKind::Fallback;
        }
        for relation in &mut graph.relations {
            relation.parser = ParserKind::Fallback;
        }
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let mut symbols = symbol_build_stage_for_graphs(vec![graph]);
        symbols.identity_admission = super::admit_symbol_build_stage(&mut symbols, &control)?;
        let persisted_graph = symbols
            .changes
            .iter()
            .find_map(|change| match change {
                SymbolProjectionChange::Parsed(parsed) => Some(parsed.graph.clone()),
                SymbolProjectionChange::Clear { .. } => None,
            })
            .ok_or("fallback sanitized graph is missing")?;
        let mut stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &policy,
            &symbols,
            &control,
        )?;
        require_eq(
            &stage.identity_rejections.len(),
            &1,
            "fallback initial typed rejection detail count",
        )?;
        let coverage = stage
            .coverage
            .iter_mut()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path: coverage_path } if coverage_path.as_str() == path
                ) && coverage.relation().is_none()
            })
            .ok_or("fallback initial coverage row is missing")?;
        let scope = coverage.scope().clone();
        let state = coverage.state();
        let covered = coverage.covered();
        let generation = coverage.generation();
        let reason = coverage.reason().cloned();
        let reached_limit = coverage.reached_limit();
        *coverage = CoverageRecord::new(
            scope,
            None,
            state,
            covered,
            2,
            generation,
            reason,
            reached_limit,
        )?;
        // Simulate a capped publication: the persisted coverage count remains
        // authoritative even when no typed detail row survived the ceiling.
        stage.identity_rejections.clear();
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &stage,
            &control,
            "fallback-reuse-initial",
        )?;
        store.replace_symbol_graph(&persisted_graph)?;
        let base_generation = store
            .index_publication()?
            .ok_or("fallback initial publication is missing")?
            .generation;
        let persisted_paths = vec![RepositoryNodePath::new(Path::new(path))?];
        require(
            store
                .repository_graph_identity_rejections(project, &persisted_paths, 16, None)?
                .is_empty(),
            "fallback capped publication retained an unexpected detail row",
        )?;

        let reuse_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let reuse_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &reuse_control)?;
        let reused = stage_full_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            &reuse_policy,
            &empty_symbol_build_stage(),
            &reuse_control,
        )?;
        require(
            reused.identity_rejections.is_empty(),
            "fallback reuse fabricated a detail row after cap",
        )?;
        let reused_coverage = reused
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path: coverage_path } if coverage_path.as_str() == path
                ) && coverage.relation().is_none()
            })
            .ok_or("fallback reused coverage row is missing")?;
        require_eq(
            &reused_coverage.omitted(),
            &2,
            "fallback reused persisted omission count",
        )?;
        require_eq(
            &reused_coverage.state(),
            &CoverageState::Partial,
            "fallback reused coverage state",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_reuse_hydrates_unchanged_affected_dependent_rejections()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp
            .path()
            .join("incremental-reuse-dependent-identity-admission");
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/provider.rs"), "pub fn changed() {}\n")?;
        fs::write(
            root.join("src/consumer.rs"),
            "pub fn caller() { changed(); }\n",
        )?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("incremental dependent project identity is missing")?;
        let provider_path = "src/provider.rs";
        let consumer_path = "src/consumer.rs";
        let nodes = vec![
            test_file_node(provider_path, "rust"),
            test_file_node(consumer_path, "rust"),
        ];
        let provider = extract_symbol_graph(provider_path, Some("rust"), "pub fn changed() {}\n");
        let mut consumer = extract_symbol_graph(
            consumer_path,
            Some("rust"),
            "pub fn caller() { changed(); }\n",
        );
        consumer.relations.push(SymbolRelation {
            path: consumer_path.to_string(),
            source_name: "caller".to_string(),
            target_name: "bad\0target".to_string(),
            kind: RelationKind::Calls,
            line: 3,
            context: "incremental unchanged dependent fixture".to_string(),
            parser: ParserKind::TreeSitter,
        });
        let mut initial_symbols = symbol_build_stage_for_graphs(vec![provider, consumer]);
        let initial_control = IndexWorkControl::new(IndexCancellation::new(), None);
        initial_symbols.identity_admission =
            super::admit_symbol_build_stage(&mut initial_symbols, &initial_control)?;
        let persisted_graphs = initial_symbols
            .changes
            .iter()
            .filter_map(|change| match change {
                SymbolProjectionChange::Parsed(parsed) => Some(parsed.graph.clone()),
                SymbolProjectionChange::Clear { .. } => None,
            })
            .collect::<Vec<_>>();
        let policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &initial_control)?;
        let initial_stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &policy,
            &initial_symbols,
            &initial_control,
        )?;
        require(
            initial_stage
                .identity_rejections
                .iter()
                .any(|rejection| rejection.path.as_str() == consumer_path),
            "initial dependent rejection was not admitted",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &initial_stage,
            &initial_control,
            "incremental-dependent-initial",
        )?;
        for graph in &persisted_graphs {
            store.replace_symbol_graph(graph)?;
        }
        let base_generation = store
            .index_publication()?
            .ok_or("incremental dependent initial publication is missing")?
            .generation;
        let consumer_key = RepositoryNodePath::new(Path::new(consumer_path))?;
        let initial_rows = store.repository_graph_identity_rejections(
            project,
            std::slice::from_ref(&consumer_key),
            16,
            None,
        )?;
        require_eq(
            &initial_rows.len(),
            &1,
            "initial unchanged dependent rejection rows",
        )?;

        let replacement =
            extract_symbol_graph(provider_path, Some("rust"), "pub fn replacement() {}\n");
        let replacement_symbols = symbol_build_stage_for_graphs(vec![replacement]);
        let incremental_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let incremental_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &incremental_control)?;
        let incremental_stage = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            std::slice::from_ref(&provider_path.to_string()),
            &incremental_policy,
            &replacement_symbols,
            &incremental_control,
        )?;
        require(
            incremental_stage
                .identity_rejections
                .iter()
                .any(|rejection| rejection.path.as_str() == consumer_path),
            "incremental affected dependent did not hydrate rejection details",
        )?;
        let staged_coverage = incremental_stage
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path } if path.as_str() == consumer_path
                ) && coverage.relation().is_none()
            })
            .ok_or("incremental affected dependent coverage is missing")?;
        require_eq(
            &staged_coverage.omitted(),
            &1,
            "incremental affected dependent omission count",
        )?;
        {
            let mut publication = store.begin_index_publication("incremental-dependent-reuse")?;
            incremental_stage.apply(&mut publication, &incremental_control)?;
            publication.complete()?;
        }
        require_eq(
            &store.repository_graph_identity_rejections(
                project,
                std::slice::from_ref(&consumer_key),
                16,
                None,
            )?,
            &initial_rows,
            "incremental affected dependent persisted rejection rows",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_reuse_reconciles_markdown_and_semantic_rejections_once()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        let semantic_components = (0..20)
            .map(|_| "s".repeat(200))
            .collect::<Vec<_>>()
            .join("/");
        let semantic_path = format!("src/semantic/{semantic_components}/graph.rs");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::create_dir_all(
            root.join(&semantic_path)
                .parent()
                .ok_or("semantic fixture parent is missing")?,
        )?;
        let guide_source = "[invalid](<../src/worker.rs#bad\u{1}>)\n[worker](../src/worker.rs)\n";
        fs::write(root.join("src/worker.rs"), "pub fn worker() {}\n")?;
        fs::write(root.join(&semantic_path), "use crate::worker;\n")?;
        fs::write(root.join("docs/guide.md"), guide_source)?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("combined reuse project identity is missing")?;
        let mut nodes = vec![
            test_file_node("src/worker.rs", "rust"),
            test_file_node(&semantic_path, "rust"),
            test_file_node("docs/guide.md", "markdown"),
        ];
        let worker_graph =
            extract_symbol_graph("src/worker.rs", Some("rust"), "pub fn worker() {}\n");
        let semantic_graph = semantic_resolution_key_graph(&semantic_path, true);
        let guide_facts = projectatlas_symbols::extract_markdown_facts(guide_source);
        let guide_graph = guide_facts.symbol_graph("docs/guide.md", Some("markdown"));
        let guide_bytes = guide_source.as_bytes();
        nodes[2].size_bytes = Some(u64::try_from(guide_bytes.len())?);
        nodes[2].content_hash = Some(blake3::hash(guide_bytes).to_hex().to_string());
        let mut initial_symbols = symbol_build_stage_for_graphs(vec![worker_graph, semantic_graph]);
        initial_symbols.report.candidates = initial_symbols.report.candidates.saturating_add(1);
        initial_symbols.report.parsed = initial_symbols.report.parsed.saturating_add(1);
        initial_symbols.report.summaries = initial_symbols.report.summaries.saturating_add(1);
        initial_symbols
            .changes
            .push(SymbolProjectionChange::Parsed(SymbolParseSuccess {
                path: "docs/guide.md".to_string(),
                graph: guide_graph,
                markdown_facts: Some(Box::new(guide_facts)),
                source_parser: ParserKind::Structural,
                summary: "combined reuse Markdown fixture".to_string(),
                summary_is_structural: true,
                purpose_suggestion: None,
            }));
        let initial_control = IndexWorkControl::new(IndexCancellation::new(), None);
        initial_symbols.identity_admission =
            super::admit_symbol_build_stage(&mut initial_symbols, &initial_control)?;
        let scan_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &initial_control)?;
        let initial_stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &nodes,
            &scan_policy,
            &initial_symbols,
            &initial_control,
        )?;
        require(
            initial_stage
                .identity_rejections
                .iter()
                .any(|row| row.path.as_str() == "docs/guide.md"),
            "combined reuse fixture did not retain Markdown rejection",
        )?;
        require(
            initial_stage.identity_rejections.iter().any(|row| {
                row.path.as_str() == semantic_path && row.field == GraphIdentityField::ResolutionKey
            }),
            "combined reuse fixture did not retain semantic rejection",
        )?;
        publish_full_staged_graph(
            &mut store,
            &nodes,
            &initial_stage,
            &initial_control,
            "combined-reuse-initial",
        )?;
        for change in &initial_symbols.changes {
            if let SymbolProjectionChange::Parsed(parsed) = change {
                store.replace_symbol_graph(&parsed.graph)?;
            }
        }
        let base_generation = store
            .index_publication()?
            .ok_or("combined reuse initial publication is missing")?
            .generation;
        let paths = nodes
            .iter()
            .map(|node| RepositoryNodePath::new(Path::new(&node.path)))
            .collect::<Result<Vec<_>, _>>()?;
        let initial_rows = store.repository_graph_identity_rejections(
            project,
            &paths,
            GraphLimits::MAX_ROWS,
            None,
        )?;
        let replacement =
            extract_symbol_graph("src/worker.rs", Some("rust"), "pub fn replacement() {}\n");
        let replacement_symbols = symbol_build_stage_for_graphs(vec![replacement]);
        let incremental_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let incremental_stage = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &nodes,
            &["src/worker.rs".to_string()],
            &scan_policy,
            &replacement_symbols,
            &incremental_control,
        )?;
        let guide_coverage = incremental_stage
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path } if path.as_str() == "docs/guide.md"
                ) && coverage.relation().is_none()
            })
            .ok_or("combined reuse Markdown coverage is missing")?;
        let semantic_coverage = incremental_stage
            .coverage
            .iter()
            .find(|coverage| {
                matches!(
                    coverage.scope(),
                    CoverageScope::Path { path } if path.as_str() == semantic_path
                ) && coverage.relation().is_none()
            })
            .ok_or("combined reuse semantic coverage is missing")?;
        let initial_guide_omitted = initial_rows
            .iter()
            .filter(|row| row.path.as_str() == "docs/guide.md")
            .count();
        let initial_semantic_omitted = initial_rows
            .iter()
            .filter(|row| row.path.as_str() == semantic_path)
            .count();
        require_eq(
            &guide_coverage.omitted(),
            &u64::try_from(initial_guide_omitted)?,
            "reused Markdown omission count",
        )?;
        require_eq(
            &semantic_coverage.omitted(),
            &u64::try_from(initial_semantic_omitted)?,
            "reused semantic omission count",
        )?;
        require_eq(
            &incremental_stage
                .identity_rejections
                .iter()
                .filter(|row| row.path.as_str() == "docs/guide.md")
                .count(),
            &initial_guide_omitted,
            "reused Markdown detail count",
        )?;
        require_eq(
            &incremental_stage
                .identity_rejections
                .iter()
                .filter(|row| row.path.as_str() == semantic_path)
                .count(),
            &initial_semantic_omitted,
            "reused semantic detail count",
        )?;
        let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled.cancel();
        {
            let mut publication = store.begin_index_publication("combined-reuse-cancel")?;
            require(
                incremental_stage
                    .apply(&mut publication, &canceled)
                    .is_err(),
                "combined reuse cancellation reached publication",
            )?;
        }
        require_eq(
            &store
                .index_publication()?
                .map(|publication| publication.generation),
            &Some(base_generation),
            "combined reuse cancellation changed generation",
        )?;
        {
            let mut publication = store.begin_index_publication("combined-reuse-incremental")?;
            incremental_stage.apply(&mut publication, &incremental_control)?;
            publication.complete()?;
        }
        let persisted_rows = store.repository_graph_identity_rejections(
            project,
            &paths,
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require_eq(
            &persisted_rows,
            &initial_rows,
            "reused Markdown and semantic persisted rows",
        )?;
        let retry_generation = store
            .index_publication()?
            .ok_or("combined reuse incremental publication is missing")?
            .generation;
        let retry_stage = stage_incremental_repository_graph(
            &store,
            &root,
            retry_generation,
            &nodes,
            &["src/worker.rs".to_string()],
            &scan_policy,
            &replacement_symbols,
            &incremental_control,
        )?;
        require_eq(
            &retry_stage
                .identity_rejections
                .iter()
                .filter(|row| row.path.as_str() == "docs/guide.md")
                .count(),
            &initial_guide_omitted,
            "reused Markdown deterministic retry detail count",
        )?;
        require_eq(
            &retry_stage
                .identity_rejections
                .iter()
                .filter(|row| row.path.as_str() == semantic_path)
                .count(),
            &initial_semantic_omitted,
            "reused semantic deterministic retry detail count",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_stage_repairs_and_removes_only_affected_rejection_paths()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("incremental-identity-admission");
        for directory in ["src", "tests", "docs"] {
            fs::create_dir_all(root.join(directory))?;
        }
        for path in ["src/one.rs", "tests/two.rs", "docs/three.rs"] {
            fs::write(
                root.join(path),
                "pub fn caller() { helper(); }\nfn helper() {}\n",
            )?;
        }
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("incremental admission project identity is missing")?;
        let initial_nodes = [
            test_file_node("src/one.rs", "rust"),
            test_file_node("tests/two.rs", "rust"),
            test_file_node("docs/three.rs", "rust"),
        ];
        let initial_graphs = vec![
            identity_sibling_graph("src/one.rs", GraphIdentityField::RelationTarget),
            identity_sibling_graph("tests/two.rs", GraphIdentityField::RelationTarget),
            identity_sibling_graph("docs/three.rs", GraphIdentityField::RelationTarget),
        ];
        let initial_symbols = symbol_build_stage_for_graphs(initial_graphs);
        let initial_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let initial_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &initial_control)?;
        let initial_stage = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::ZERO,
            &initial_nodes,
            &initial_policy,
            &initial_symbols,
            &initial_control,
        )?;
        publish_full_staged_graph(
            &mut store,
            &initial_nodes,
            &initial_stage,
            &initial_control,
            "incremental-identity-initial",
        )?;
        let base_generation = store
            .index_publication()?
            .ok_or("incremental initial publication is missing")?
            .generation;
        let src_path = RepositoryNodePath::new(Path::new("src/one.rs"))?;
        let removed_path = RepositoryNodePath::new(Path::new("tests/two.rs"))?;
        let retained_path = RepositoryNodePath::new(Path::new("docs/three.rs"))?;
        let initial_rows = store.repository_graph_identity_rejections(
            project,
            &[
                src_path.clone(),
                removed_path.clone(),
                retained_path.clone(),
            ],
            16,
            None,
        )?;
        require_eq(
            &initial_rows.len(),
            &3,
            "initial incremental rejection rows",
        )?;

        let repaired_graph = extract_symbol_graph(
            "src/one.rs",
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        );
        let repaired_symbols = symbol_build_stage_for_graphs(vec![repaired_graph]);
        let expected_nodes = vec![initial_nodes[0].clone(), initial_nodes[2].clone()];
        let incremental_control = IndexWorkControl::new(IndexCancellation::new(), None);
        let incremental_policy =
            RootScanPolicy::discover(&root, &ScanOptions::default(), &incremental_control)?;
        let incremental_stage = stage_incremental_repository_graph(
            &store,
            &root,
            base_generation,
            &expected_nodes,
            &["src/one.rs".to_string(), "tests/two.rs".to_string()],
            &incremental_policy,
            &repaired_symbols,
            &incremental_control,
        )?;
        {
            let mut publication = store.begin_index_publication("incremental-identity-repair")?;
            incremental_stage.apply(&mut publication, &incremental_control)?;
            publication.complete()?;
        }
        let repaired_rows = store.repository_graph_identity_rejections(
            project,
            &[
                src_path.clone(),
                removed_path.clone(),
                retained_path.clone(),
            ],
            16,
            None,
        )?;
        require(
            repaired_rows
                .iter()
                .all(|row| row.path != src_path && row.path != removed_path),
            "incremental repair/removal retained affected rejection detail",
        )?;
        require(
            repaired_rows.iter().any(|row| row.path == retained_path),
            "incremental repair removed unrelated rejection detail",
        )?;
        require_eq(
            &repaired_rows.len(),
            &1,
            "incremental unaffected rejection rows",
        )?;
        Ok(())
    }

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: actual={actual:?}, expected={expected:?}"
            ))
            .into())
        }
    }

    fn publish_full_staged_graph(
        store: &mut AtlasStore,
        nodes: &[Node],
        staged: &StagedRepositoryGraph,
        control: &IndexWorkControl,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        let mut publication = store.begin_index_publication(label)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(nodes)?;
        publication.finish_scan_replacement()?;
        staged.apply(&mut publication, control)?;
        publication.complete()?;
        Ok(())
    }

    fn symbol_build_stage_for_markdown(
        graph: SymbolGraph,
        markdown_facts: projectatlas_symbols::MarkdownFacts,
    ) -> SymbolBuildStage {
        let mut stage = empty_symbol_build_stage();
        stage.report.candidates = 1;
        stage.report.parsed = 1;
        stage.report.summaries = 1;
        stage.changes = vec![SymbolProjectionChange::Parsed(SymbolParseSuccess {
            path: graph.path.clone(),
            source_parser: ParserKind::Structural,
            graph,
            markdown_facts: Some(Box::new(markdown_facts)),
            summary: "markdown identity admission fixture".to_string(),
            summary_is_structural: true,
            purpose_suggestion: None,
        })];
        stage
    }

    fn symbol_build_stage_for_graphs(graphs: Vec<SymbolGraph>) -> SymbolBuildStage {
        let mut stage = empty_symbol_build_stage();
        stage.report.candidates = graphs.len();
        stage.report.parsed = graphs.len();
        stage.report.symbols = graphs.iter().map(|graph| graph.symbols.len()).sum();
        stage.report.relations = graphs.iter().map(|graph| graph.relations.len()).sum();
        stage.changes = graphs
            .into_iter()
            .map(|graph| {
                let path = graph.path.clone();
                let source_parser = graph.parser;
                SymbolProjectionChange::Parsed(SymbolParseSuccess {
                    path,
                    graph,
                    markdown_facts: None,
                    source_parser,
                    summary: "identity admission fixture".to_string(),
                    summary_is_structural: false,
                    purpose_suggestion: None,
                })
            })
            .collect();
        stage
    }

    fn identity_sibling_graph(path: &str, invalid_field: GraphIdentityField) -> SymbolGraph {
        let mut graph = extract_symbol_graph(
            path,
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        );
        graph.relations.push(SymbolRelation {
            path: path.to_string(),
            source_name: if invalid_field == GraphIdentityField::RelationSource {
                "bad\0source".to_string()
            } else {
                "caller".to_string()
            },
            target_name: if invalid_field == GraphIdentityField::RelationTarget {
                "bad\0target".to_string()
            } else {
                "helper".to_string()
            },
            kind: RelationKind::Calls,
            line: 3,
            context: "identity admission fixture".to_string(),
            parser: ParserKind::TreeSitter,
        });
        graph
    }

    fn semantic_resolution_key_graph(path: &str, include_invalid: bool) -> SymbolGraph {
        let helper = if path.contains("sibling") {
            "sibling_helper"
        } else {
            "page_helper"
        };
        let parent = "LeakedIdentity".repeat(16);
        let invalid_methods = if include_invalid {
            format!(
                "pub struct {parent};\nimpl {parent} {{\n    pub fn first(&self) {{}}\n    pub fn second(&self) {{}}\n}}\n"
            )
        } else {
            String::new()
        };
        extract_symbol_graph(
            path,
            Some("rust"),
            &format!(
                "use crate::worker;\n{invalid_methods}pub fn caller() {{ {helper}(); worker(); }}\npub fn {helper}() {{}}\n"
            ),
        )
    }

    fn empty_symbol_build_stage() -> SymbolBuildStage {
        SymbolBuildStage {
            report: SymbolBuildReport {
                candidates: 0,
                parsed: 0,
                unchanged: 0,
                too_large: 0,
                binary_or_non_utf8: 0,
                timed_out: 0,
                max_workers: 1,
                timeout_seconds: None,
                symbols: 0,
                relations: 0,
                summaries: 0,
                purpose_suggestions: 0,
            },
            changes: Vec::new(),
            retained_bytes: 0,
            identity_admission: GraphIdentityAdmission::default(),
        }
    }

    fn function_graph(path: &str, symbol_count: usize) -> SymbolGraph {
        SymbolGraph {
            path: path.to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: (0..symbol_count)
                .map(|index| CodeSymbol {
                    path: path.to_string(),
                    language: Some("rust".to_string()),
                    name: format!("symbol_{index}"),
                    kind: SymbolKind::Function,
                    signature: format!("fn symbol_{index}()"),
                    exported: index == 0,
                    documentation: None,
                    line_start: index + 1,
                    line_end: index + 1,
                    source_selector: None,
                    parent: None,
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_string()),
                })
                .collect(),
            relations: vec![SymbolRelation {
                path: path.to_string(),
                source_name: "symbol_0".to_string(),
                target_name: "dependency".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "dependency()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        }
    }

    fn package_graph(path: &str, name: &str) -> SymbolGraph {
        SymbolGraph {
            path: path.to_string(),
            language: Some("cargo-manifest".to_string()),
            parser: ParserKind::Manifest,
            symbols: vec![CodeSymbol {
                path: path.to_string(),
                language: Some("cargo-manifest".to_string()),
                name: name.to_string(),
                kind: SymbolKind::Package,
                signature: format!("name = \"{name}\""),
                exported: true,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Manifest,
                detail: Some("cargo-package".to_string()),
            }],
            relations: Vec::new(),
        }
    }
}
