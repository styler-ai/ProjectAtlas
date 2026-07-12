//! Dev-only lexical and `SQLite` architecture evaluation over scanned corpus rows.

use regex::RegexBuilder;
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

/// Versioned controlled lexical edge cases compiled into the evaluator.
pub(super) const LEXICAL_FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/contracts/lexical-search-edge-cases-v1.json");

/// Runtime-produced source table required by both experiments.
const SOURCE_TABLE: &str = "file_texts";
/// Output-owned FTS candidate table.
const FTS_TABLE: &str = "evaluation_file_text_fts";
/// Candidate tokenizer under evaluation.
const FTS_TOKENIZER: &str = "trigram";
/// Exact result ordering used by both full and candidate verification.
const EXACT_ORDER: &str = "path-line-column-text";
/// WAL checkpoint mode used before storage observations.
const CHECKPOINT_MODE: &str = "truncate";
/// Deterministic percentile estimator.
const PERCENTILE_METHOD: &str = "nearest-rank-ceiling";
/// Journal mode used by output-owned evaluation databases.
const JOURNAL_MODE: &str = "wal";
/// Injected constraint failure identity.
const FAILURE_INJECTION: &str = "derived-row-slot-check-after-inactive-replacement-before-flip";
/// Explicit raw-page-writer architecture disposition.
const RAW_PAGE_WRITES: &str = "rejected";
/// Exact physical-slot publication topology.
const SLOT_MODE: &str = "two-slot-atomic-publication";
/// Manifest field that owns the global deterministic evaluation seed.
pub(super) const GLOBAL_SEED_REFERENCE: &str = "experiment_design.rng.seed_hex";
/// Stable identifier for the domain-separated digest ordering used by evaluations.
pub(super) const ORDERING_ALGORITHM_ID: &str = "sha256-domain-separated-ordering";
/// Version of the deterministic ordering contract.
pub(super) const ORDERING_ALGORITHM_VERSION: &str = "2";
/// Domain tag that makes ordering digests independent from other SHA-256 uses.
const ORDERING_DOMAIN_TAG: &[u8] = b"projectatlas.evaluation-order.v2";

/// Failures at the isolated architecture-evaluation boundary.
#[derive(Debug, Error)]
pub(super) enum ArchitectureEvaluationError {
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A regular-expression fixture was invalid.
    #[error(transparent)]
    Regex(#[from] regex::Error),
    /// A `SQLite` operation failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON fixture or manifest decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A preregistered invariant failed.
    #[error("architecture evaluation policy failed: {0}")]
    Policy(String),
}

/// Immutable identity used to derive deterministic evaluation order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct ArchitectureSampleContext {
    /// Manifest field that supplied the global seed.
    global_seed_reference: String,
    /// Exact registered seed value.
    global_seed_hex: String,
    /// Stable corpus/operation/sample identity.
    stable_cell_identity: String,
    /// Zero-based outer repetition.
    repetition: u64,
    /// Decoded seed bytes used by SHA-256 ordering.
    #[serde(skip)]
    global_seed: [u8; 32],
}

impl ArchitectureSampleContext {
    /// Construct a validated deterministic sample identity.
    pub(super) fn new(
        global_seed_reference: impl Into<String>,
        global_seed_hex: impl Into<String>,
        stable_cell_identity: impl Into<String>,
        repetition: usize,
    ) -> Result<Self, ArchitectureEvaluationError> {
        let global_seed_reference = global_seed_reference.into();
        let global_seed_hex = global_seed_hex.into();
        let stable_cell_identity = stable_cell_identity.into();
        require(
            global_seed_reference == GLOBAL_SEED_REFERENCE,
            "global evaluation seed reference drifted",
        )?;
        require(
            !stable_cell_identity.trim().is_empty(),
            "architecture sample cell identity is empty",
        )?;
        let global_seed = decode_seed(&global_seed_hex)?;
        let repetition = u64::try_from(repetition).map_err(|source| {
            ArchitectureEvaluationError::Policy(format!(
                "architecture sample repetition exceeds the fixed u64 encoding: {source}"
            ))
        })?;
        Ok(Self {
            global_seed_reference,
            global_seed_hex,
            stable_cell_identity,
            repetition,
            global_seed,
        })
    }

    /// Return one domain-separated SHA-256 digest for deterministic ordering.
    fn ordering_digest(&self, identity: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(ORDERING_DOMAIN_TAG);
        hash_order_field(&mut hasher, &self.global_seed);
        hash_order_field(&mut hasher, self.stable_cell_identity.as_bytes());
        hash_order_field(&mut hasher, &self.repetition.to_le_bytes());
        hash_order_field(&mut hasher, identity.as_bytes());
        hasher.finalize().into()
    }

    /// Select a deterministic low-bit order for one paired observation.
    fn pair_order(&self, pair_identity: &str) -> PairOrder {
        let digest = self.ordering_digest(pair_identity);
        if digest[31] & 1 == 0 {
            PairOrder::WithoutFtsThenWithFts
        } else {
            PairOrder::WithFtsThenWithoutFts
        }
    }
}

/// Deterministic execution order for one without/with-FTS pair.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PairOrder {
    /// Measure the exact scan before the FTS candidate path.
    WithoutFtsThenWithFts,
    /// Measure the FTS candidate path before the exact scan.
    WithFtsThenWithoutFts,
}

/// Boolean evidence check serialized without losing its typed role.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct CheckStatus(bool);

impl CheckStatus {
    /// Construct a typed check from its observed condition.
    const fn from_condition(condition: bool) -> Self {
        Self(condition)
    }

    /// Whether the observed check passed.
    const fn passed(self) -> bool {
        self.0
    }
}

/// Registered dev-only architecture experiments.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchitectureEvaluationPlan {
    /// Plan schema version.
    schema_version: u32,
    /// Controlled lexical fixture binding.
    lexical_fixture: LexicalFixtureBinding,
    /// FTS differential plan.
    fts_differential: FtsDifferentialPlan,
    /// `SQLite` strategy matrix.
    sqlite_strategy: SqliteStrategyPlan,
}

impl ArchitectureEvaluationPlan {
    /// Validate every closed architecture-evaluation choice before a sample runs.
    pub(super) fn validate(
        &self,
        fixture_sha256: &str,
        fixture_bytes: usize,
        registered_warmups: usize,
    ) -> Result<(), ArchitectureEvaluationError> {
        require(self.schema_version == 1, "plan schema version drifted")?;
        require(
            self.lexical_fixture.path == "fixtures/contracts/lexical-search-edge-cases-v1.json"
                && self.lexical_fixture.fixture_id == "projectatlas.lexical-edge-cases.v1"
                && self.lexical_fixture.sha256 == fixture_sha256
                && self.lexical_fixture.bytes == fixture_bytes,
            "lexical fixture binding drifted",
        )?;
        self.fts_differential.validate(registered_warmups)?;
        self.sqlite_strategy.validate(registered_warmups)
    }

    /// Return the FTS registered repetition count.
    pub(super) const fn fts_repetitions(&self) -> usize {
        self.fts_differential.repetitions
    }

    /// Return the `SQLite` registered repetition count.
    pub(super) const fn sqlite_repetitions(&self) -> usize {
        self.sqlite_strategy.repetitions
    }
}

/// Exact fixture file identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LexicalFixtureBinding {
    /// Repository-relative fixture path.
    path: String,
    /// Stable fixture identifier.
    fixture_id: String,
    /// SHA-256 over exact fixture bytes.
    sha256: String,
    /// Exact fixture byte count.
    bytes: usize,
}

/// Registered corpus query for FTS candidate comparison.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusQuerySpec {
    /// Stable query identifier.
    id: String,
    /// Literal pattern.
    pattern: String,
    /// Exact verifier case policy.
    case_sensitive: bool,
    /// Required non-vacuous baseline matches.
    minimum_matches: usize,
}

/// Registered FTS differential choices.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FtsDifferentialPlan {
    /// Existing scanned source table.
    source_table: String,
    /// Output-owned candidate table.
    fts_table: String,
    /// FTS tokenizer under evaluation.
    tokenizer: String,
    /// Fixed candidate-eligible corpus queries.
    corpus_queries: Vec<CorpusQuerySpec>,
    /// Exact result ordering contract.
    exact_order: String,
    /// Query classes that must bypass FTS.
    forced_fallback_classes: Vec<LexicalClass>,
    /// Timed requests retained inside each outer sample.
    timed_query_iterations: usize,
    /// Deterministic percentile estimator.
    percentile_method: String,
    /// Registered outer warmups.
    warmups: usize,
    /// Registered outer measurements.
    repetitions: usize,
    /// Deterministic update probe.
    update_marker: String,
    /// WAL checkpoint mode before byte capture.
    checkpoint_mode: String,
    /// Whether an empty required class fails.
    zero_positive_classes_are_failure: bool,
    /// Whether any omitted failure fails the result.
    dropped_failures_are_failure: bool,
}

impl FtsDifferentialPlan {
    /// Validate the closed FTS experiment before touching `SQLite`.
    fn validate(&self, registered_warmups: usize) -> Result<(), ArchitectureEvaluationError> {
        let required_fallbacks = BTreeSet::from([
            LexicalClass::Punctuation,
            LexicalClass::ShortString,
            LexicalClass::UnicodeIdentifier,
            LexicalClass::Regex,
            LexicalClass::Fuzzy,
            LexicalClass::TokenizerIncompatible,
        ]);
        let actual_fallbacks = self
            .forced_fallback_classes
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        require(
            self.source_table == SOURCE_TABLE
                && self.fts_table == FTS_TABLE
                && self.tokenizer == FTS_TOKENIZER
                && self.exact_order == EXACT_ORDER
                && self.checkpoint_mode == CHECKPOINT_MODE
                && self.warmups == registered_warmups
                && self.repetitions == 15
                && self.timed_query_iterations > 0
                && self.percentile_method == PERCENTILE_METHOD
                && !self.update_marker.is_empty()
                && self.zero_positive_classes_are_failure
                && self.dropped_failures_are_failure
                && actual_fallbacks == required_fallbacks
                && !self.corpus_queries.is_empty()
                && self.corpus_queries.iter().all(|query| {
                    !query.id.is_empty()
                        && query.pattern.chars().count() >= 3
                        && query
                            .pattern
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric() || character == '_')
                        && query.minimum_matches > 0
                }),
            "FTS differential plan drifted",
        )
    }
}

/// Registered row count for each committed pre-publication load transaction.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadTransactionSpec {
    /// Stable cell label.
    id: String,
    /// Rows per committed load transaction, or every row in one transaction.
    rows: Option<usize>,
}

/// Index maintenance timing under evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IndexSet {
    /// Maintain the secondary index during row loading.
    Maintained,
    /// Create the secondary index after rows are loaded.
    PostLoad,
}

impl IndexSet {
    /// Stable evidence identifier.
    const fn id(self) -> &'static str {
        match self {
            Self::Maintained => "maintained",
            Self::PostLoad => "post-load",
        }
    }
}

/// Candidate import path under evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ImportMode {
    /// Prepared transactions into a same-database local staging table.
    PreparedBatches,
    /// A separate staging database imported through `ATTACH` and `INSERT SELECT`.
    StagingAttachInsertSelect,
}

impl ImportMode {
    /// Stable evidence identifier.
    const fn id(self) -> &'static str {
        match self {
            Self::PreparedBatches => "prepared-batches",
            Self::StagingAttachInsertSelect => "staging-attach-insert-select",
        }
    }
}

/// Closed policy for selecting one correctness-preserving `SQLite` matrix cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum SqliteCellSelectionPolicy {
    /// Prefer total-strategy throughput, then the lowest stable cell identifier.
    #[serde(
        rename = "highest-correctness-preserving-total-strategy-rows-per-second-then-lowest-stable-cell-id"
    )]
    HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId,
}

impl SqliteCellSelectionPolicy {
    /// Select the highest-throughput eligible cell with a stable lexical tie-break.
    fn select_cell(self, cells: &[SqliteStrategyCell]) -> Option<String> {
        match self {
            Self::HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId => {
                cells
                    .iter()
                    .filter(|cell| cell.passed())
                    .filter_map(|cell| {
                        cell.rows_per_second
                            .map(|rate| (rate, cell.cell_id.as_str()))
                    })
                    .min_by_key(|(rate, stable_cell_id)| {
                        (std::cmp::Reverse(*rate), *stable_cell_id)
                    })
                    .map(|(_rate, stable_cell_id)| stable_cell_id.to_owned())
            }
        }
    }
}

/// Registered `SQLite` matrix choices.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SqliteStrategyPlan {
    /// Existing scanned source table.
    source_table: String,
    /// Committed transaction sizes used while preparing either load source.
    load_transactions: Vec<LoadTransactionSpec>,
    /// Secondary index timing variants.
    index_sets: Vec<IndexSet>,
    /// Direct and staging import variants.
    import_modes: Vec<ImportMode>,
    /// Exact Cartesian cell count.
    required_cells: usize,
    /// Exact physical slot count.
    slot_count: usize,
    /// `SQLite` journal mode.
    journal_mode: String,
    /// WAL checkpoint mode.
    checkpoint_mode: String,
    /// Failure injected before publication.
    failure_injection: String,
    /// Authored fixture key outside derived slots.
    authored_key: String,
    /// Authored fixture value outside derived slots.
    authored_value: String,
    /// Explicit disposition of raw page writing.
    raw_page_writes: String,
    /// Registered outer warmups.
    warmups: usize,
    /// Registered outer measurements.
    repetitions: usize,
    /// Deterministic correctness-preserving cell selection policy.
    selection_policy: SqliteCellSelectionPolicy,
    /// Whether omitted cell failures fail the aggregate.
    dropped_failures_are_failure: bool,
}

impl SqliteStrategyPlan {
    /// Validate the exact twelve-cell storage experiment.
    fn validate(&self, registered_warmups: usize) -> Result<(), ArchitectureEvaluationError> {
        let load_transaction_rows = self
            .load_transactions
            .iter()
            .map(|size| (size.id.as_str(), size.rows))
            .collect::<Vec<_>>();
        let index_sets = self.index_sets.iter().copied().collect::<BTreeSet<_>>();
        let import_modes = self.import_modes.iter().copied().collect::<BTreeSet<_>>();
        require(
            self.source_table == SOURCE_TABLE
                && load_transaction_rows
                    == [
                        ("rows-256", Some(256)),
                        ("rows-2048", Some(2048)),
                        ("all-rows", None),
                    ]
                && index_sets == BTreeSet::from([IndexSet::Maintained, IndexSet::PostLoad])
                && import_modes
                    == BTreeSet::from([
                        ImportMode::PreparedBatches,
                        ImportMode::StagingAttachInsertSelect,
                    ])
                && self.required_cells == 12
                && self.slot_count == 2
                && self.journal_mode == JOURNAL_MODE
                && self.checkpoint_mode == CHECKPOINT_MODE
                && self.failure_injection == FAILURE_INJECTION
                && !self.authored_key.is_empty()
                && !self.authored_value.is_empty()
                && self.raw_page_writes == RAW_PAGE_WRITES
                && self.warmups == registered_warmups
                && self.repetitions == 15
                && self.selection_policy
                    == SqliteCellSelectionPolicy::HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId
                && self.dropped_failures_are_failure,
            "SQLite strategy plan drifted",
        )
    }
}

/// One concrete matrix cell in its deterministic execution plan.
struct SqliteCellPlan<'a> {
    /// Registered pre-publication load transaction.
    load: &'a LoadTransactionSpec,
    /// Secondary-index timing.
    index_set: IndexSet,
    /// Load-source implementation.
    import_mode: ImportMode,
    /// Stable serialized cell identity.
    id: String,
    /// Deterministic seeded ordering key.
    order_key: [u8; 32],
}

/// Materialize and seed-order the exact Cartesian `SQLite` matrix.
fn sqlite_execution_plan<'a>(
    plan: &'a SqliteStrategyPlan,
    sample: &ArchitectureSampleContext,
) -> Vec<SqliteCellPlan<'a>> {
    let mut cells = plan
        .load_transactions
        .iter()
        .flat_map(|load| {
            plan.index_sets.iter().copied().flat_map(move |index_set| {
                plan.import_modes.iter().copied().map(move |import_mode| {
                    let id = cell_id(load, index_set, import_mode);
                    SqliteCellPlan {
                        load,
                        index_set,
                        import_mode,
                        order_key: sample.ordering_digest(&format!("sqlite-cell:{id}")),
                        id,
                    }
                })
            })
        })
        .collect::<Vec<_>>();
    cells.sort_by(|left, right| {
        left.order_key
            .cmp(&right.order_key)
            .then_with(|| left.id.cmp(&right.id))
    });
    cells
}

/// One source row copied from the scanned `file_texts` table.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SourceTextRow {
    /// Repository-relative path.
    path: String,
    /// UTF-8 source bytes persisted by `ProjectAtlas`.
    content: String,
}

/// Exact line-level verifier result.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LexicalMatch {
    /// Repository-relative or controlled-fixture path.
    path: String,
    /// One-based line.
    line: usize,
    /// One-based byte column.
    column: usize,
    /// Exact matched source line.
    text: String,
}

/// Versioned controlled lexical fixture.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LexicalFixture {
    /// Fixture schema version.
    schema_version: u32,
    /// Stable fixture identifier.
    fixture_id: String,
    /// Controlled rows, separate from corpus-derived FTS rows.
    rows: Vec<FixtureRow>,
    /// Fallback and edge-class cases.
    cases: Vec<FixtureCase>,
}

/// One controlled lexical row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRow {
    /// Synthetic path.
    path: String,
    /// Synthetic source content.
    content: String,
}

/// Closed lexical query mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LexicalMode {
    /// Exact substring matching.
    Literal,
    /// Regular-expression matching.
    Regex,
    /// Ordered subsequence matching.
    Fuzzy,
}

/// Closed controlled edge classes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum LexicalClass {
    /// Punctuation-only or punctuation-sensitive literal.
    Punctuation,
    /// Literal shorter than the trigram boundary.
    ShortString,
    /// Unicode-sensitive identifier.
    UnicodeIdentifier,
    /// Regular expression.
    Regex,
    /// Fuzzy subsequence.
    Fuzzy,
    /// Literal incompatible with safe tokenizer narrowing.
    TokenizerIncompatible,
}

/// Whether FTS candidate narrowing is allowed for a case.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum CandidatePolicy {
    /// The exact path must scan every row.
    ForceFallback,
}

/// One controlled lexical expectation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    /// Stable case identifier.
    id: String,
    /// Correctness class.
    class: LexicalClass,
    /// Query mode.
    mode: LexicalMode,
    /// Query pattern.
    pattern: String,
    /// Exact verifier case policy.
    case_sensitive: bool,
    /// Required candidate policy.
    candidate_policy: CandidatePolicy,
    /// Exact distinct matching paths.
    expected_paths: Vec<String>,
}

/// Main database and live WAL/SHM byte evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub(super) struct SqliteFileBytes {
    /// Main database bytes.
    main: u64,
    /// WAL bytes.
    wal: u64,
    /// Shared-memory sidecar bytes.
    shm: u64,
    /// Saturating sum of the observed files.
    total: u64,
}

/// One retained paired request observation in its deterministic execution order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct FtsRequestObservation {
    /// Registered corpus query identity.
    query_id: String,
    /// Zero-based request iteration inside this outer sample.
    iteration: usize,
    /// Deterministic without/with-FTS execution order.
    order: PairOrder,
    /// Exact full-scan duration.
    without_fts_ns: u64,
    /// FTS candidate plus exact-verification duration.
    with_fts_ns: u64,
}

/// Paired update timing retained with its deterministic execution order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct FtsUpdateObservation {
    /// Deterministic without/with-FTS execution order.
    order: PairOrder,
    /// Source-table update duration with FTS disabled.
    without_fts_ns: u64,
    /// Equivalent source-table and FTS update duration.
    with_fts_ns: u64,
}

/// Checkpointed storage for equivalent databases without and with FTS.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
struct FtsStorageBytes {
    /// Source-table database without FTS.
    without_fts: SqliteFileBytes,
    /// Equivalent source-table database with FTS enabled.
    with_fts: SqliteFileBytes,
}

/// Differential FTS result for one corpus sample.
#[derive(Debug, Serialize)]
pub(super) struct FtsDifferentialResult {
    /// Deterministic identity and seed binding for this sample.
    sample_context: ArchitectureSampleContext,
    /// Controlled fixture identity.
    fixture_id: String,
    /// Real scanned corpus rows indexed into FTS.
    corpus_rows: usize,
    /// Controlled rows evaluated separately from FTS.
    controlled_edge_rows: usize,
    /// Controlled cases retained.
    controlled_edge_cases: usize,
    /// Whether FTS5 with trigram support was exercised.
    fts5_available: bool,
    /// Candidate-eligible corpus queries executed.
    candidate_queries: usize,
    /// Forced fallback cases executed.
    forced_fallback_queries: usize,
    /// Positive exact corpus matches across candidate queries.
    baseline_positive_matches: usize,
    /// Whether every exact baseline path appeared in its candidate set.
    candidate_superset_complete: CheckStatus,
    /// Whether exact final rows and order matched the full baseline.
    exact_rows_and_order_equal: CheckStatus,
    /// Literal candidate correctness.
    literal_complete: CheckStatus,
    /// Punctuation fallback correctness.
    punctuation_complete: CheckStatus,
    /// Short-string fallback correctness.
    short_string_complete: CheckStatus,
    /// Unicode fallback correctness.
    unicode_complete: CheckStatus,
    /// Regex fallback correctness.
    regex_fallback_complete: CheckStatus,
    /// Fuzzy fallback correctness.
    fuzzy_fallback_complete: CheckStatus,
    /// Tokenizer-incompatible fallback correctness.
    tokenizer_fallback_complete: CheckStatus,
    /// Whether the deterministic FTS update was restored exactly.
    update_restored: CheckStatus,
    /// Paired forward-update costs; verification and restoration are untimed.
    update_observation: FtsUpdateObservation,
    /// Every raw paired request timing in execution order.
    request_observations: Vec<FtsRequestObservation>,
    /// Equivalent storage without and with FTS after truncating checkpoints.
    checkpointed_storage_bytes: FtsStorageBytes,
    /// Incremental checkpointed bytes introduced by enabling FTS.
    index_bytes: u64,
    /// Full-scan exact verifier p50.
    baseline_p50_ns: u64,
    /// Full-scan exact verifier p95.
    baseline_p95_ns: u64,
    /// FTS candidate plus exact verifier p50.
    candidate_p50_ns: u64,
    /// FTS candidate plus exact verifier p95.
    candidate_p95_ns: u64,
    /// Retained failures; never dropped from the aggregate.
    failures: Vec<String>,
    /// Whether every preregistered correctness and non-vacuity gate passed.
    eligible: CheckStatus,
}

impl FtsDifferentialResult {
    /// Whether the outer sample may be treated as successful.
    pub(super) const fn is_eligible(&self) -> bool {
        self.eligible.passed()
    }
}

/// Status of one retained `SQLite` matrix cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CellStatus {
    /// Every correctness and rollback invariant passed.
    Passed,
    /// The cell failed and retained its diagnostic.
    Failed,
}

/// Storage boundary used by one pre-publication load source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LoadStorageScope {
    /// Incremental bytes added to the main database by a local staging table.
    LocalMainDatabaseDelta,
    /// Total bytes occupied by an external staging database.
    ExternalDatabaseTotal,
}

/// One retained `SQLite` matrix cell.
#[derive(Debug, Serialize)]
struct SqliteStrategyCell {
    /// Passed or failed state.
    status: CellStatus,
    /// Stable matrix cell identity.
    cell_id: String,
    /// Registered pre-publication load-transaction identity.
    load_transaction: String,
    /// Effective rows per committed load transaction.
    load_transaction_rows: usize,
    /// Secondary-index timing.
    index_set: IndexSet,
    /// Candidate import implementation.
    import_mode: ImportMode,
    /// Storage boundary used for pre-publication load rows.
    load_storage_scope: LoadStorageScope,
    /// Exact publication topology.
    slot_mode: &'static str,
    /// Corpus-derived source rows.
    source_rows: usize,
    /// Pre-publication load construction and attachment time.
    prepublication_load_ns: Option<u64>,
    /// Committed transactions used to build the load source.
    load_transactions_committed: Option<usize>,
    /// Inactive-slot replacement, reconciliation, and flip time.
    publication_ns: Option<u64>,
    /// Staging preparation plus publication time.
    strategy_ns: Option<u64>,
    /// Integer source rows per second over total strategy time.
    rows_per_second: Option<u64>,
    /// WAL bytes immediately before the truncating checkpoint.
    wal_bytes_before_checkpoint: Option<u64>,
    /// Main/WAL/SHM bytes after checkpoint.
    checkpointed_storage_bytes: Option<SqliteFileBytes>,
    /// Incremental local or total external pre-publication load bytes.
    prepublication_load_bytes: Option<u64>,
    /// Load-source cleanup duration after successful publication.
    load_cleanup_ns: Option<u64>,
    /// Main/WAL/SHM bytes after old-slot cleanup.
    retained_bytes: Option<u64>,
    /// Old-slot cleanup and checkpoint time.
    cleanup_ns: Option<u64>,
    /// Exact inactive-slot rows matched the source rows.
    reconciliation_ok: CheckStatus,
    /// The inactive slot became active through one metadata flip.
    publication_ok: CheckStatus,
    /// The injected constraint failure rolled back.
    rollback_ok: CheckStatus,
    /// Authored data outside slots was unchanged.
    authored_rows_preserved: CheckStatus,
    /// The injected failure did not change the active slot/epoch.
    active_slot_preserved_on_failure: CheckStatus,
    /// The injected failure restored the prior inactive rollback slot.
    inactive_slot_preserved_on_failure: CheckStatus,
    /// Successful publication retained the previously active rollback slot.
    retained_rollback_slot_preserved: CheckStatus,
    /// Retained bounded diagnostic for a failed cell.
    error: Option<String>,
}

impl SqliteStrategyCell {
    /// Build a failed cell without discarding its matrix identity.
    fn failed(
        load: &LoadTransactionSpec,
        effective_rows: usize,
        index_set: IndexSet,
        import_mode: ImportMode,
        source_rows: usize,
        error: &ArchitectureEvaluationError,
    ) -> Self {
        Self {
            status: CellStatus::Failed,
            cell_id: cell_id(load, index_set, import_mode),
            load_transaction: load.id.clone(),
            load_transaction_rows: effective_rows,
            index_set,
            import_mode,
            load_storage_scope: load_storage_scope(import_mode),
            slot_mode: SLOT_MODE,
            source_rows,
            prepublication_load_ns: None,
            load_transactions_committed: None,
            publication_ns: None,
            strategy_ns: None,
            rows_per_second: None,
            wal_bytes_before_checkpoint: None,
            checkpointed_storage_bytes: None,
            prepublication_load_bytes: None,
            load_cleanup_ns: None,
            retained_bytes: None,
            cleanup_ns: None,
            reconciliation_ok: CheckStatus::from_condition(false),
            publication_ok: CheckStatus::from_condition(false),
            rollback_ok: CheckStatus::from_condition(false),
            authored_rows_preserved: CheckStatus::from_condition(false),
            active_slot_preserved_on_failure: CheckStatus::from_condition(false),
            inactive_slot_preserved_on_failure: CheckStatus::from_condition(false),
            retained_rollback_slot_preserved: CheckStatus::from_condition(false),
            error: Some(truncate_error(&error.to_string())),
        }
    }

    /// Whether the cell is eligible for selection.
    fn passed(&self) -> bool {
        let timing_is_complete = matches!(
            (
                self.prepublication_load_ns,
                self.load_transactions_committed,
                self.publication_ns,
                self.strategy_ns,
                self.rows_per_second,
                self.load_cleanup_ns,
                self.cleanup_ns,
            ),
            (
                Some(load),
                Some(load_transactions),
                Some(publication),
                Some(strategy),
                Some(rate),
                Some(load_cleanup),
                Some(cleanup),
            )
                if load > 0
                    && publication > 0
                    && strategy == load.saturating_add(publication)
                    && strategy > 0
                    && rate > 0
                    && self.load_transaction_rows > 0
                    && load_transactions
                        == self.source_rows.div_ceil(self.load_transaction_rows)
                    && load_cleanup > 0
                    && cleanup > 0
        );
        let storage_is_complete = matches!(
            (
                self.wal_bytes_before_checkpoint,
                self.checkpointed_storage_bytes,
                self.prepublication_load_bytes,
                self.retained_bytes,
            ),
            (Some(wal), Some(checkpointed), Some(load_bytes), Some(retained))
                if wal > 0
                    && checkpointed.main > 0
                    && checkpointed.wal == 0
                    && checkpointed.total >= checkpointed.main
                    && retained > 0
                    && load_bytes > 0
        );
        matches!(self.status, CellStatus::Passed)
            && self.error.is_none()
            && timing_is_complete
            && storage_is_complete
            && self.reconciliation_ok.passed()
            && self.publication_ok.passed()
            && self.rollback_ok.passed()
            && self.authored_rows_preserved.passed()
            && self.active_slot_preserved_on_failure.passed()
            && self.inactive_slot_preserved_on_failure.passed()
            && self.retained_rollback_slot_preserved.passed()
    }
}

/// `SQLite` strategy aggregate for one corpus sample.
#[derive(Debug, Serialize)]
pub(super) struct SqliteStrategyResult {
    /// Deterministic identity and seed binding for this sample.
    sample_context: ArchitectureSampleContext,
    /// Deterministic execution order of every retained cell.
    execution_order: Vec<String>,
    /// Real scanned source rows supplied to every matrix cell.
    source_rows: usize,
    /// Required Cartesian matrix size.
    required_cells: usize,
    /// Cells retained, including failures.
    retained_cells: usize,
    /// Cells satisfying every correctness and rollback gate.
    successful_cells: usize,
    /// Full compact cell evidence.
    cells: Vec<SqliteStrategyCell>,
    /// Fastest eligible cell, never selected from a failed cell.
    selected_cell: Option<String>,
    /// Whether every injected failure rolled back.
    all_rollbacks_ok: CheckStatus,
    /// Whether every cell preserved authored data.
    all_authored_rows_preserved: CheckStatus,
    /// Whether every injected failure preserved the active slot and epoch.
    all_active_slots_preserved_on_failure: CheckStatus,
    /// Retained cell failures.
    failures: Vec<String>,
    /// Whether all preregistered cells and invariants passed.
    eligible: CheckStatus,
}

impl SqliteStrategyResult {
    /// Whether the outer sample may be treated as successful.
    pub(super) const fn is_eligible(&self) -> bool {
        self.eligible.passed()
    }
}

/// Closed serialized metrics for the two registered architecture experiments.
#[derive(Debug, Serialize)]
#[serde(tag = "result_kind", rename_all = "kebab-case")]
pub(super) enum ArchitectureMetrics {
    /// FTS5 candidate selector with exact-verifier differential evidence.
    FtsResult {
        /// Typed compact differential aggregate.
        #[serde(flatten)]
        result: FtsDifferentialResult,
    },
    /// Prepared/staging/two-slot `SQLite` matrix evidence.
    SqliteStrategyResult {
        /// Typed compact matrix aggregate.
        #[serde(flatten)]
        result: SqliteStrategyResult,
    },
}

/// Run the preregistered FTS candidate-versus-exact differential.
pub(super) fn run_fts_differential(
    source_db: &Path,
    work_directory: &Path,
    plan: &ArchitectureEvaluationPlan,
    sample: &ArchitectureSampleContext,
) -> Result<FtsDifferentialResult, ArchitectureEvaluationError> {
    fs::create_dir(work_directory)?;
    let rows = load_source_rows(source_db, &work_directory.join("source-snapshot.db"))?;
    require(!rows.is_empty(), "source file_texts table is empty")?;
    let fixture: LexicalFixture = serde_json::from_slice(LEXICAL_FIXTURE_BYTES)?;
    validate_fixture(&fixture, &plan.lexical_fixture.fixture_id)?;
    let controlled_rows = fixture
        .rows
        .iter()
        .map(|row| SourceTextRow {
            path: row.path.clone(),
            content: row.content.clone(),
        })
        .collect::<Vec<_>>();
    let mut edge = evaluate_controlled_cases(&fixture, &controlled_rows)?;
    let baseline_database_path = work_directory.join("lexical-baseline.db");
    let fts_database_path = work_directory.join("fts-candidate.db");
    let mut failures = std::mem::take(&mut edge.failures);
    let mut fts5_available = false;
    let mut baseline_positive_matches = 0_usize;
    let mut candidate_superset_complete = false;
    let mut exact_rows_and_order_equal = false;
    let mut update_restored = false;
    let mut update_observation = FtsUpdateObservation {
        order: sample.pair_order("fts-update"),
        without_fts_ns: 0,
        with_fts_ns: 0,
    };
    let mut checkpointed_storage_bytes = FtsStorageBytes::default();
    let mut index_bytes = 0_u64;
    let mut request_observations = Vec::new();

    let databases = build_baseline_database(&baseline_database_path, &rows).and_then(|baseline| {
        build_fts_database(&fts_database_path, &rows).map(|fts| (baseline, fts))
    });
    match databases {
        Ok((mut baseline_connection, mut fts_connection)) => {
            fts5_available = true;
            let mut all_supersets = true;
            let mut all_equal = true;
            for query in &plan.fts_differential.corpus_queries {
                let baseline = exact_matches(
                    &rows,
                    LexicalMode::Literal,
                    &query.pattern,
                    query.case_sensitive,
                )?;
                baseline_positive_matches =
                    baseline_positive_matches.saturating_add(baseline.len());
                if baseline.len() < query.minimum_matches {
                    failures.push(format!(
                        "corpus query {} produced {} matches below minimum {}",
                        query.id,
                        baseline.len(),
                        query.minimum_matches
                    ));
                }
                let candidates = fts_candidate_rows(&fts_connection, &query.pattern)?;
                let candidate_paths = candidates
                    .iter()
                    .map(|row| row.path.as_str())
                    .collect::<BTreeSet<_>>();
                let baseline_paths = baseline
                    .iter()
                    .map(|row| row.path.as_str())
                    .collect::<BTreeSet<_>>();
                let superset = baseline_paths.is_subset(&candidate_paths);
                let verified = exact_matches(
                    &candidates,
                    LexicalMode::Literal,
                    &query.pattern,
                    query.case_sensitive,
                )?;
                let equal = verified == baseline;
                all_supersets &= superset;
                all_equal &= equal;
                if !superset || !equal {
                    failures.push(format!(
                        "corpus query {} failed candidate-superset or exact-order equivalence",
                        query.id
                    ));
                }
                for iteration in 0..plan.fts_differential.timed_query_iterations {
                    let pair_identity = format!("fts-query:{}:{iteration}", query.id);
                    let order = sample.pair_order(&pair_identity);
                    let (measured_baseline, without_fts_ns, measured_verified, with_fts_ns) =
                        match order {
                            PairOrder::WithoutFtsThenWithFts => {
                                let (baseline, baseline_ns) = timed_exact_scan(&rows, query)?;
                                let (candidate, candidate_ns) =
                                    timed_fts_candidate_scan(&fts_connection, query)?;
                                (baseline, baseline_ns, candidate, candidate_ns)
                            }
                            PairOrder::WithFtsThenWithoutFts => {
                                let (candidate, candidate_ns) =
                                    timed_fts_candidate_scan(&fts_connection, query)?;
                                let (baseline, baseline_ns) = timed_exact_scan(&rows, query)?;
                                (baseline, baseline_ns, candidate, candidate_ns)
                            }
                        };
                    if measured_verified != measured_baseline {
                        failures.push(format!(
                            "timed corpus query {} changed exact rows or order",
                            query.id
                        ));
                    }
                    request_observations.push(FtsRequestObservation {
                        query_id: query.id.clone(),
                        iteration,
                        order,
                        without_fts_ns,
                        with_fts_ns,
                    });
                }
            }
            candidate_superset_complete = all_supersets;
            exact_rows_and_order_equal = all_equal;
            let update = evaluate_fts_update(
                &mut baseline_connection,
                &mut fts_connection,
                &rows,
                &plan.fts_differential.update_marker,
                update_observation.order,
            )?;
            update_restored = update.restored;
            update_observation = update.observation;
            if !update_restored {
                failures.push("FTS update probe was not restored exactly".into());
            }
            checkpoint_wal(&baseline_connection)?;
            checkpoint_wal(&fts_connection)?;
            checkpointed_storage_bytes = FtsStorageBytes {
                without_fts: sqlite_file_bytes(&baseline_database_path)?,
                with_fts: sqlite_file_bytes(&fts_database_path)?,
            };
            if let Some(incremental_bytes) = checkpointed_storage_bytes
                .with_fts
                .total
                .checked_sub(checkpointed_storage_bytes.without_fts.total)
                .filter(|bytes| *bytes > 0)
            {
                index_bytes = incremental_bytes;
            } else {
                failures.push("FTS-enabled storage did not exceed the lexical baseline".into());
            }
            drop(baseline_connection);
            drop(fts_connection);
        }
        Err(error) => failures.push(format!("lexical/FTS5 setup failed: {error}")),
    }
    cleanup_sqlite_files(&baseline_database_path)?;
    cleanup_sqlite_files(&fts_database_path)?;

    let literal_complete =
        baseline_positive_matches > 0 && candidate_superset_complete && exact_rows_and_order_equal;
    let expected_timed_samples = plan
        .fts_differential
        .corpus_queries
        .len()
        .saturating_mul(plan.fts_differential.timed_query_iterations);
    let baseline_durations = request_observations
        .iter()
        .map(|observation| observation.without_fts_ns)
        .collect::<Vec<_>>();
    let candidate_durations = request_observations
        .iter()
        .map(|observation| observation.with_fts_ns)
        .collect::<Vec<_>>();
    let baseline_p50_ns = percentile(&baseline_durations, 50);
    let baseline_p95_ns = percentile(&baseline_durations, 95);
    let candidate_p50_ns = percentile(&candidate_durations, 50);
    let candidate_p95_ns = percentile(&candidate_durations, 95);
    let observed_request_ids = request_observations
        .iter()
        .map(|observation| (observation.query_id.as_str(), observation.iteration))
        .collect::<BTreeSet<_>>();
    let expected_request_ids = plan
        .fts_differential
        .corpus_queries
        .iter()
        .flat_map(|query| {
            (0..plan.fts_differential.timed_query_iterations)
                .map(move |iteration| (query.id.as_str(), iteration))
        })
        .collect::<BTreeSet<_>>();
    let observed_orders = request_observations
        .iter()
        .map(|observation| observation.order)
        .collect::<BTreeSet<_>>();
    let ordering_evidence_complete = request_observations.iter().all(|observation| {
        sample.pair_order(&format!(
            "fts-query:{}:{}",
            observation.query_id, observation.iteration
        )) == observation.order
    }) && (expected_timed_samples < 2
        || observed_orders.len() == 2);
    let latency_evidence_complete = expected_timed_samples > 0
        && request_observations.len() == expected_timed_samples
        && observed_request_ids == expected_request_ids
        && ordering_evidence_complete
        && request_observations
            .iter()
            .all(|observation| observation.without_fts_ns > 0 && observation.with_fts_ns > 0)
        && baseline_p50_ns > 0
        && baseline_p95_ns >= baseline_p50_ns
        && candidate_p50_ns > 0
        && candidate_p95_ns >= candidate_p50_ns;
    let eligible = fts5_available
        && literal_complete
        && edge.passed(LexicalClass::Punctuation)
        && edge.passed(LexicalClass::ShortString)
        && edge.passed(LexicalClass::UnicodeIdentifier)
        && edge.passed(LexicalClass::Regex)
        && edge.passed(LexicalClass::Fuzzy)
        && edge.passed(LexicalClass::TokenizerIncompatible)
        && update_restored
        && update_observation.without_fts_ns > 0
        && update_observation.with_fts_ns > 0
        && checkpointed_storage_bytes.without_fts.main > 0
        && checkpointed_storage_bytes.with_fts.main > 0
        && index_bytes > 0
        && latency_evidence_complete
        && failures.is_empty();
    Ok(FtsDifferentialResult {
        sample_context: sample.clone(),
        fixture_id: fixture.fixture_id,
        corpus_rows: rows.len(),
        controlled_edge_rows: controlled_rows.len(),
        controlled_edge_cases: fixture.cases.len(),
        fts5_available,
        candidate_queries: plan.fts_differential.corpus_queries.len(),
        forced_fallback_queries: edge.fallback_queries,
        baseline_positive_matches,
        candidate_superset_complete: CheckStatus::from_condition(candidate_superset_complete),
        exact_rows_and_order_equal: CheckStatus::from_condition(exact_rows_and_order_equal),
        literal_complete: CheckStatus::from_condition(literal_complete),
        punctuation_complete: CheckStatus::from_condition(edge.passed(LexicalClass::Punctuation)),
        short_string_complete: CheckStatus::from_condition(edge.passed(LexicalClass::ShortString)),
        unicode_complete: CheckStatus::from_condition(edge.passed(LexicalClass::UnicodeIdentifier)),
        regex_fallback_complete: CheckStatus::from_condition(edge.passed(LexicalClass::Regex)),
        fuzzy_fallback_complete: CheckStatus::from_condition(edge.passed(LexicalClass::Fuzzy)),
        tokenizer_fallback_complete: CheckStatus::from_condition(
            edge.passed(LexicalClass::TokenizerIncompatible),
        ),
        update_restored: CheckStatus::from_condition(update_restored),
        update_observation,
        request_observations,
        checkpointed_storage_bytes,
        index_bytes,
        baseline_p50_ns,
        baseline_p95_ns,
        candidate_p50_ns,
        candidate_p95_ns,
        failures,
        eligible: CheckStatus::from_condition(eligible),
    })
}

/// Run the preregistered prepared/staging/two-slot `SQLite` matrix.
pub(super) fn run_sqlite_strategy(
    source_db: &Path,
    work_directory: &Path,
    plan: &ArchitectureEvaluationPlan,
    sample: &ArchitectureSampleContext,
) -> Result<SqliteStrategyResult, ArchitectureEvaluationError> {
    fs::create_dir(work_directory)?;
    let rows = load_source_rows(source_db, &work_directory.join("source-snapshot.db"))?;
    require(!rows.is_empty(), "source file_texts table is empty")?;
    let execution_plan = sqlite_execution_plan(&plan.sqlite_strategy, sample);
    let execution_order = execution_plan
        .iter()
        .map(|cell| cell.id.clone())
        .collect::<Vec<_>>();
    let mut cells = Vec::with_capacity(plan.sqlite_strategy.required_cells);
    for planned in execution_plan {
        let effective_rows = planned.load.rows.unwrap_or(rows.len()).max(1);
        let database_path = work_directory.join(format!("{}.db", planned.id));
        let staging_path = work_directory.join(format!("{}.staging.db", planned.id));
        let cell = match run_sqlite_cell(
            &database_path,
            &staging_path,
            &rows,
            planned.load,
            planned.index_set,
            planned.import_mode,
            &plan.sqlite_strategy,
        ) {
            Ok(cell) => cell,
            Err(error) => SqliteStrategyCell::failed(
                planned.load,
                effective_rows,
                planned.index_set,
                planned.import_mode,
                rows.len(),
                &error,
            ),
        };
        let cleanup_result =
            cleanup_sqlite_files(&database_path).and_then(|()| cleanup_sqlite_files(&staging_path));
        let cell = match cleanup_result {
            Ok(()) => cell,
            Err(error) if cell.passed() => SqliteStrategyCell::failed(
                planned.load,
                effective_rows,
                planned.index_set,
                planned.import_mode,
                rows.len(),
                &error,
            ),
            Err(_error) => cell,
        };
        cells.push(cell);
    }
    let successful_cells = cells.iter().filter(|cell| cell.passed()).count();
    let selected_cell = plan.sqlite_strategy.selection_policy.select_cell(&cells);
    let all_rollbacks_ok = !cells.is_empty() && cells.iter().all(|cell| cell.rollback_ok.passed());
    let all_authored_rows_preserved = !cells.is_empty()
        && cells
            .iter()
            .all(|cell| cell.authored_rows_preserved.passed());
    let all_active_slots_preserved_on_failure = !cells.is_empty()
        && cells
            .iter()
            .all(|cell| cell.active_slot_preserved_on_failure.passed());
    let failures = cells
        .iter()
        .filter_map(|cell| {
            cell.error
                .as_ref()
                .map(|error| format!("{}: {error}", cell.cell_id))
        })
        .collect::<Vec<_>>();
    let eligible = cells.len() == plan.sqlite_strategy.required_cells
        && successful_cells == plan.sqlite_strategy.required_cells
        && selected_cell.is_some()
        && all_rollbacks_ok
        && all_authored_rows_preserved
        && all_active_slots_preserved_on_failure
        && failures.is_empty();
    Ok(SqliteStrategyResult {
        sample_context: sample.clone(),
        execution_order,
        source_rows: rows.len(),
        required_cells: plan.sqlite_strategy.required_cells,
        retained_cells: cells.len(),
        successful_cells,
        cells,
        selected_cell,
        all_rollbacks_ok: CheckStatus::from_condition(all_rollbacks_ok),
        all_authored_rows_preserved: CheckStatus::from_condition(all_authored_rows_preserved),
        all_active_slots_preserved_on_failure: CheckStatus::from_condition(
            all_active_slots_preserved_on_failure,
        ),
        failures,
        eligible: CheckStatus::from_condition(eligible),
    })
}

/// Load exact corpus rows through an output-owned `SQLite` snapshot.
fn load_source_rows(
    source_db: &Path,
    snapshot_db: &Path,
) -> Result<Vec<SourceTextRow>, ArchitectureEvaluationError> {
    materialize_source_snapshot(source_db, snapshot_db)?;
    let rows = (|| {
        let connection = Connection::open(snapshot_db)?;
        connection.pragma_update(None, "query_only", true)?;
        let mut statement =
            connection.prepare("SELECT path, content FROM file_texts ORDER BY path")?;
        let rows = statement
            .query_map([], |row| {
                Ok(SourceTextRow {
                    path: row.get(0)?,
                    content: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            rows.windows(2).all(|pair| pair[0].path < pair[1].path)
                && rows.iter().all(|row| !row.path.is_empty()),
            "source file_texts paths are empty, duplicated, or unordered",
        )?;
        Ok(rows)
    })();
    let cleanup = cleanup_sqlite_files(snapshot_db);
    match (rows, cleanup) {
        (Ok(rows), Ok(())) => Ok(rows),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(read_error), Err(cleanup_error)) => Err(ArchitectureEvaluationError::Policy(format!(
            "source snapshot read failed: {read_error}; cleanup also failed: {cleanup_error}"
        ))),
    }
}

/// Copy durable `SQLite` state into an output-owned database before opening it.
fn materialize_source_snapshot(
    source_db: &Path,
    snapshot_db: &Path,
) -> Result<(), ArchitectureEvaluationError> {
    cleanup_sqlite_files(snapshot_db)?;
    for (source, destination) in [
        (source_db.to_owned(), snapshot_db.to_owned()),
        (
            sidecar_path(source_db, "-wal")?,
            sidecar_path(snapshot_db, "-wal")?,
        ),
        (
            sidecar_path(source_db, "-journal")?,
            sidecar_path(snapshot_db, "-journal")?,
        ),
    ] {
        match fs::metadata(&source) {
            Ok(metadata) if metadata.is_file() => {
                let copied = fs::copy(&source, &destination)?;
                require(
                    copied == metadata.len(),
                    "SQLite snapshot copy was incomplete",
                )?;
            }
            Ok(_metadata) => {
                return Err(ArchitectureEvaluationError::Policy(
                    "SQLite snapshot source is not a file".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && source != source_db => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Validate the versioned controlled lexical fixture.
fn validate_fixture(
    fixture: &LexicalFixture,
    expected_id: &str,
) -> Result<(), ArchitectureEvaluationError> {
    let paths = fixture
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<BTreeSet<_>>();
    let case_ids = fixture
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    require(
        fixture.schema_version == 1
            && fixture.fixture_id == expected_id
            && !fixture.rows.is_empty()
            && paths.len() == fixture.rows.len()
            && !fixture.cases.is_empty()
            && case_ids.len() == fixture.cases.len()
            && fixture
                .rows
                .iter()
                .all(|row| !row.path.is_empty() && !row.content.is_empty())
            && fixture.cases.iter().all(|case| {
                !case.id.is_empty()
                    && !case.pattern.is_empty()
                    && !case.expected_paths.is_empty()
                    && case.candidate_policy == CandidatePolicy::ForceFallback
            }),
        "controlled lexical fixture is empty or drifted",
    )
}

/// Aggregate controlled fallback class results.
struct ControlledCaseResult {
    /// Retained fallback count.
    fallback_queries: usize,
    /// Classes with non-vacuous exact expected results.
    passed_classes: BTreeSet<LexicalClass>,
    /// Retained failures.
    failures: Vec<String>,
}

impl ControlledCaseResult {
    /// Whether one required fallback class passed.
    fn passed(&self, class: LexicalClass) -> bool {
        self.passed_classes.contains(&class)
    }
}

/// Run every controlled edge case through the exact fallback matcher.
fn evaluate_controlled_cases(
    fixture: &LexicalFixture,
    rows: &[SourceTextRow],
) -> Result<ControlledCaseResult, ArchitectureEvaluationError> {
    let mut passed_classes = BTreeSet::new();
    let mut failures = Vec::new();
    for case in &fixture.cases {
        let matches = exact_matches(rows, case.mode, &case.pattern, case.case_sensitive)?;
        let paths = matches
            .iter()
            .map(|result| result.path.as_str())
            .collect::<BTreeSet<_>>();
        let expected = case
            .expected_paths
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if !matches.is_empty() && paths == expected {
            passed_classes.insert(case.class);
        } else {
            failures.push(format!(
                "controlled case {} returned paths {paths:?}, expected {expected:?}",
                case.id
            ));
        }
    }
    Ok(ControlledCaseResult {
        fallback_queries: fixture.cases.len(),
        passed_classes,
        failures,
    })
}

/// Run the authoritative exact matcher over a deterministic row order.
fn exact_matches(
    rows: &[SourceTextRow],
    mode: LexicalMode,
    pattern: &str,
    case_sensitive: bool,
) -> Result<Vec<LexicalMatch>, ArchitectureEvaluationError> {
    let regex = if mode == LexicalMode::Regex {
        Some(
            RegexBuilder::new(pattern)
                .case_insensitive(!case_sensitive)
                .build()?,
        )
    } else {
        None
    };
    let mut matches = Vec::new();
    for row in rows {
        for (line_index, line) in row.content.lines().enumerate() {
            match mode {
                LexicalMode::Literal => {
                    let columns = literal_columns(line, pattern, case_sensitive);
                    for column in columns {
                        matches.push(LexicalMatch {
                            path: row.path.clone(),
                            line: line_index.saturating_add(1),
                            column: column.saturating_add(1),
                            text: line.to_owned(),
                        });
                    }
                }
                LexicalMode::Regex => {
                    let matcher = regex.as_ref().ok_or_else(|| {
                        ArchitectureEvaluationError::Policy(
                            "regex matcher was not initialized".into(),
                        )
                    })?;
                    for found in matcher.find_iter(line) {
                        matches.push(LexicalMatch {
                            path: row.path.clone(),
                            line: line_index.saturating_add(1),
                            column: found.start().saturating_add(1),
                            text: line.to_owned(),
                        });
                    }
                }
                LexicalMode::Fuzzy => {
                    if fuzzy_subsequence(line, pattern, case_sensitive) {
                        matches.push(LexicalMatch {
                            path: row.path.clone(),
                            line: line_index.saturating_add(1),
                            column: 1,
                            text: line.to_owned(),
                        });
                    }
                }
            }
        }
    }
    matches.sort();
    Ok(matches)
}

/// Return zero-based byte columns for one literal query.
fn literal_columns(line: &str, pattern: &str, case_sensitive: bool) -> Vec<usize> {
    if case_sensitive {
        return line
            .match_indices(pattern)
            .map(|(column, _)| column)
            .collect();
    }
    line.to_lowercase()
        .match_indices(&pattern.to_lowercase())
        .map(|(column, _)| column)
        .collect()
}

/// Return whether a pattern is an ordered character subsequence.
fn fuzzy_subsequence(text: &str, pattern: &str, case_sensitive: bool) -> bool {
    let normalized_text;
    let normalized_pattern;
    let (text, pattern) = if case_sensitive {
        (text, pattern)
    } else {
        normalized_text = text.to_lowercase();
        normalized_pattern = pattern.to_lowercase();
        (normalized_text.as_str(), normalized_pattern.as_str())
    };
    let mut pattern_characters = pattern.chars();
    let mut current = pattern_characters.next();
    for character in text.chars() {
        if current == Some(character) {
            current = pattern_characters.next();
            if current.is_none() {
                return true;
            }
        }
    }
    false
}

/// Create the exact-search storage used when FTS is disabled.
fn build_baseline_database(
    database_path: &Path,
    rows: &[SourceTextRow],
) -> Result<Connection, ArchitectureEvaluationError> {
    let mut connection = Connection::open(database_path)?;
    connection.pragma_update(None, "journal_mode", JOURNAL_MODE)?;
    connection.pragma_update(None, "synchronous", "normal")?;
    connection.execute_batch(
        "CREATE TABLE evaluation_file_text_source(
             path TEXT PRIMARY KEY,
             content TEXT NOT NULL
         );",
    )?;
    let transaction = connection.transaction()?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO evaluation_file_text_source(path, content) VALUES(?1, ?2)")?;
        for row in rows {
            insert.execute(params![row.path, row.content])?;
        }
    }
    transaction.commit()?;
    Ok(connection)
}

/// Create an equivalent source database plus a contentful FTS5 trigram index.
fn build_fts_database(
    database_path: &Path,
    rows: &[SourceTextRow],
) -> Result<Connection, ArchitectureEvaluationError> {
    let mut connection = build_baseline_database(database_path, rows)?;
    connection.execute_batch(
        "CREATE VIRTUAL TABLE evaluation_file_text_fts
         USING fts5(path UNINDEXED, content, tokenize='trigram');",
    )?;
    let transaction = connection.transaction()?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO evaluation_file_text_fts(path, content) VALUES(?1, ?2)")?;
        for row in rows {
            insert.execute(params![row.path, row.content])?;
        }
    }
    transaction.commit()?;
    Ok(connection)
}

/// Select candidate rows through FTS without treating them as final matches.
fn fts_candidate_rows(
    connection: &Connection,
    pattern: &str,
) -> Result<Vec<SourceTextRow>, ArchitectureEvaluationError> {
    let query = format!("\"{}\"", pattern.replace('"', "\"\""));
    let mut statement = connection.prepare(
        "SELECT source.path, source.content
         FROM evaluation_file_text_fts AS candidate
         JOIN evaluation_file_text_source AS source ON source.path = candidate.path
         WHERE evaluation_file_text_fts MATCH ?1
         ORDER BY source.path",
    )?;
    let rows = statement
        .query_map([query], |row| {
            Ok(SourceTextRow {
                path: row.get(0)?,
                content: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Measure one exact full scan without FTS candidate narrowing.
fn timed_exact_scan(
    rows: &[SourceTextRow],
    query: &CorpusQuerySpec,
) -> Result<(Vec<LexicalMatch>, u64), ArchitectureEvaluationError> {
    let started = Instant::now();
    let matches = exact_matches(
        rows,
        LexicalMode::Literal,
        &query.pattern,
        query.case_sensitive,
    )?;
    Ok((matches, elapsed_ns(started)))
}

/// Measure FTS candidate selection followed by the same exact verifier.
fn timed_fts_candidate_scan(
    connection: &Connection,
    query: &CorpusQuerySpec,
) -> Result<(Vec<LexicalMatch>, u64), ArchitectureEvaluationError> {
    let started = Instant::now();
    let candidates = fts_candidate_rows(connection, &query.pattern)?;
    let matches = exact_matches(
        &candidates,
        LexicalMode::Literal,
        &query.pattern,
        query.case_sensitive,
    )?;
    Ok((matches, elapsed_ns(started)))
}

/// Load authoritative source rows from one evaluation database.
fn evaluation_source_rows(
    connection: &Connection,
) -> Result<Vec<SourceTextRow>, ArchitectureEvaluationError> {
    let mut statement = connection.prepare(
        "SELECT path, content
         FROM evaluation_file_text_source
         ORDER BY path",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(SourceTextRow {
                path: row.get(0)?,
                content: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Read the contentful FTS table in canonical logical order.
fn fts_logical_rows(
    connection: &Connection,
) -> Result<Vec<SourceTextRow>, ArchitectureEvaluationError> {
    let mut statement = connection.prepare(
        "SELECT path, content
         FROM evaluation_file_text_fts
         ORDER BY path",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(SourceTextRow {
                path: row.get(0)?,
                content: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Prove the FTS logical contents and internal index match the expected rows.
fn fts_contents_are_exact(
    connection: &Connection,
    expected: &[SourceTextRow],
) -> Result<bool, ArchitectureEvaluationError> {
    let logical_rows = fts_logical_rows(connection)?;
    let integrity_ok = connection
        .execute(
            "INSERT INTO evaluation_file_text_fts(evaluation_file_text_fts)
             VALUES('integrity-check')",
            [],
        )
        .is_ok();
    Ok(integrity_ok && logical_rows == expected)
}

/// Transactionally update one authoritative source row.
fn update_source_row(
    connection: &mut Connection,
    row: &SourceTextRow,
    content: &str,
) -> Result<bool, ArchitectureEvaluationError> {
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE evaluation_file_text_source SET content = ?1 WHERE path = ?2",
        params![content, row.path],
    )?;
    transaction.commit()?;
    Ok(changed == 1)
}

/// Transactionally update the same source row and its FTS candidate entry.
fn update_source_row_with_fts(
    connection: &mut Connection,
    row: &SourceTextRow,
    content: &str,
) -> Result<bool, ArchitectureEvaluationError> {
    let transaction = connection.transaction()?;
    let source_changed = transaction.execute(
        "UPDATE evaluation_file_text_source SET content = ?1 WHERE path = ?2",
        params![content, row.path],
    )?;
    let fts_changed = transaction.execute(
        "UPDATE evaluation_file_text_fts SET content = ?1 WHERE path = ?2",
        params![content, row.path],
    )?;
    transaction.commit()?;
    Ok(source_changed == 1 && fts_changed == 1)
}

/// Compare and restore one identical source update without and with FTS.
fn evaluate_fts_update(
    baseline_connection: &mut Connection,
    fts_connection: &mut Connection,
    rows: &[SourceTextRow],
    marker: &str,
    order: PairOrder,
) -> Result<FtsUpdateResult, ArchitectureEvaluationError> {
    let row = rows
        .first()
        .ok_or_else(|| ArchitectureEvaluationError::Policy("FTS source rows are empty".into()))?;
    require(
        exact_matches(
            &evaluation_source_rows(baseline_connection)?,
            LexicalMode::Literal,
            marker,
            true,
        )?
        .is_empty()
            && fts_candidate_rows(fts_connection, marker)?.is_empty(),
        "FTS update marker already exists",
    )?;
    require(
        evaluation_source_rows(baseline_connection)? == rows
            && evaluation_source_rows(fts_connection)? == rows
            && fts_contents_are_exact(fts_connection, rows)?,
        "FTS update stores differ before the paired update",
    )?;
    let fts_rows_before = fts_logical_rows(fts_connection)?;
    let mut changed = row.content.clone();
    changed.push('\n');
    changed.push_str(marker);
    changed.push('\n');

    let (baseline_changed, without_fts_ns, fts_changed, with_fts_ns) = match order {
        PairOrder::WithoutFtsThenWithFts => {
            let started = Instant::now();
            let baseline_changed = update_source_row(baseline_connection, row, &changed)?;
            let without_fts_ns = elapsed_ns(started);
            let started = Instant::now();
            let fts_changed = update_source_row_with_fts(fts_connection, row, &changed)?;
            let with_fts_ns = elapsed_ns(started);
            (baseline_changed, without_fts_ns, fts_changed, with_fts_ns)
        }
        PairOrder::WithFtsThenWithoutFts => {
            let started = Instant::now();
            let fts_changed = update_source_row_with_fts(fts_connection, row, &changed)?;
            let with_fts_ns = elapsed_ns(started);
            let started = Instant::now();
            let baseline_changed = update_source_row(baseline_connection, row, &changed)?;
            let without_fts_ns = elapsed_ns(started);
            (baseline_changed, without_fts_ns, fts_changed, with_fts_ns)
        }
    };
    let baseline_visible = exact_matches(
        &evaluation_source_rows(baseline_connection)?,
        LexicalMode::Literal,
        marker,
        true,
    )?
    .iter()
    .any(|found| found.path == row.path);

    let fts_visible = exact_matches(
        &fts_candidate_rows(fts_connection, marker)?,
        LexicalMode::Literal,
        marker,
        true,
    )?
    .iter()
    .any(|found| found.path == row.path);

    let baseline_restored = update_source_row(baseline_connection, row, &row.content)?;
    let fts_restored = update_source_row_with_fts(fts_connection, row, &row.content)?;
    let baseline_rows = evaluation_source_rows(baseline_connection)?;
    let fts_rows = evaluation_source_rows(fts_connection)?;
    let baseline_absent =
        exact_matches(&baseline_rows, LexicalMode::Literal, marker, true)?.is_empty();
    let fts_absent = fts_candidate_rows(fts_connection, marker)?.is_empty();
    let fts_exact = fts_logical_rows(fts_connection)? == fts_rows_before
        && fts_contents_are_exact(fts_connection, rows)?;
    Ok(FtsUpdateResult {
        observation: FtsUpdateObservation {
            order,
            without_fts_ns,
            with_fts_ns,
        },
        restored: baseline_changed
            && fts_changed
            && baseline_visible
            && fts_visible
            && baseline_restored
            && fts_restored
            && baseline_absent
            && fts_absent
            && baseline_rows == rows
            && fts_rows == rows
            && fts_exact,
    })
}

/// Paired update timing and restoration state.
struct FtsUpdateResult {
    /// Forward update costs for equivalent databases.
    observation: FtsUpdateObservation,
    /// Whether visibility and absence checks passed.
    restored: bool,
}

/// Run one complete `SQLite` matrix cell.
fn run_sqlite_cell(
    database_path: &Path,
    staging_path: &Path,
    rows: &[SourceTextRow],
    load: &LoadTransactionSpec,
    index_set: IndexSet,
    import_mode: ImportMode,
    plan: &SqliteStrategyPlan,
) -> Result<SqliteStrategyCell, ArchitectureEvaluationError> {
    let effective_rows = load.rows.unwrap_or(rows.len()).max(1);
    let mut retained_inactive_rows = vec![rows.first().cloned().ok_or_else(|| {
        ArchitectureEvaluationError::Policy("SQLite strategy cell has no source row".into())
    })?];
    retained_inactive_rows[0]
        .content
        .push_str("\n// retained inactive-slot rollback fixture\n");
    let mut connection = Connection::open(database_path)?;
    connection.pragma_update(None, "journal_mode", JOURNAL_MODE)?;
    connection.pragma_update(None, "synchronous", "normal")?;
    create_strategy_schema(&connection)?;
    connection.execute(
        "INSERT INTO authored_records(authored_key, authored_value) VALUES(?1, ?2)",
        params![plan.authored_key, plan.authored_value],
    )?;
    connection.execute(
        "INSERT INTO publication_state(singleton, active_slot, active_epoch)
         VALUES(1, 0, 1)",
        [],
    )?;
    if index_set == IndexSet::Maintained {
        create_strategy_index(&connection)?;
    }
    insert_slot_rows_for_setup(&mut connection, rows, 0)?;
    insert_slot_rows_for_setup(&mut connection, &retained_inactive_rows, 1)?;

    let load_evidence = prepare_prepublication_load(
        &mut connection,
        database_path,
        staging_path,
        rows,
        effective_rows,
        import_mode,
    )?;
    let rollback = verify_publication_rollback(
        &mut connection,
        rows,
        &retained_inactive_rows,
        index_set,
        import_mode,
        plan,
    )?;

    let publication_started = Instant::now();
    let publication = connection.transaction()?;
    replace_inactive_slot(&publication, import_mode)?;
    if index_set == IndexSet::PostLoad {
        create_strategy_index(&publication)?;
    }
    let imported = load_slot_rows(&publication, 1)?;
    let reconciliation_ok = imported == rows;
    require(reconciliation_ok, "inactive slot differs from corpus rows")?;
    let flipped = publication.execute(
        "UPDATE publication_state
         SET active_slot = 1, active_epoch = 2
         WHERE singleton = 1 AND active_slot = 0 AND active_epoch = 1",
        [],
    )?;
    require(flipped == 1, "atomic publication metadata flip failed")?;
    publication.commit()?;
    let publication_ns = elapsed_ns(publication_started);
    let strategy_ns = load_evidence.duration_ns.saturating_add(publication_ns);
    let published = storage_snapshot(&connection, plan)?;
    let active_rows = load_slot_rows(&connection, 1)?;
    let retained_previous_rows = load_slot_rows(&connection, 0)?;
    let retained_rollback_slot_preserved = retained_previous_rows == rows;
    let publication_ok = published.active_slot == 1
        && published.active_epoch == 2
        && active_rows == rows
        && retained_rollback_slot_preserved;
    let authored_rows_preserved = rollback.authored_rows_preserved.passed()
        && published.authored_value == plan.authored_value;
    require(
        publication_ok && authored_rows_preserved,
        "publication did not retain exact slots or authored data",
    )?;
    let wal_bytes_before_checkpoint = sqlite_file_bytes(database_path)?.wal;
    checkpoint_wal(&connection)?;
    let checkpointed_storage_bytes = sqlite_file_bytes(database_path)?;
    let load_cleanup_started = Instant::now();
    cleanup_prepublication_load(&mut connection, staging_path, import_mode)?;
    let load_cleanup_ns = elapsed_ns(load_cleanup_started);
    let cleanup_started = Instant::now();
    let cleanup = connection.transaction()?;
    cleanup.execute("DELETE FROM derived_rows WHERE slot = 0", [])?;
    cleanup.commit()?;
    checkpoint_wal(&connection)?;
    let cleanup_ns = elapsed_ns(cleanup_started);
    let after_cleanup = storage_snapshot(&connection, plan)?;
    let retained_bytes = sqlite_file_bytes(database_path)?.total;
    require(
        after_cleanup.active_slot == 1
            && after_cleanup.active_epoch == 2
            && after_cleanup.authored_value == plan.authored_value
            && load_slot_rows(&connection, 0)?.is_empty()
            && load_slot_rows(&connection, 1)? == rows,
        "old-slot cleanup damaged active or authored data",
    )?;
    let rows_per_second = rows_per_second(rows.len(), strategy_ns);
    drop(connection);
    Ok(SqliteStrategyCell {
        status: CellStatus::Passed,
        cell_id: cell_id(load, index_set, import_mode),
        load_transaction: load.id.clone(),
        load_transaction_rows: effective_rows,
        index_set,
        import_mode,
        load_storage_scope: load_evidence.storage_scope,
        slot_mode: SLOT_MODE,
        source_rows: rows.len(),
        prepublication_load_ns: Some(load_evidence.duration_ns),
        load_transactions_committed: Some(load_evidence.transactions_committed),
        publication_ns: Some(publication_ns),
        strategy_ns: Some(strategy_ns),
        rows_per_second: Some(rows_per_second),
        wal_bytes_before_checkpoint: Some(wal_bytes_before_checkpoint),
        checkpointed_storage_bytes: Some(checkpointed_storage_bytes),
        prepublication_load_bytes: Some(load_evidence.bytes),
        load_cleanup_ns: Some(load_cleanup_ns),
        retained_bytes: Some(retained_bytes),
        cleanup_ns: Some(cleanup_ns),
        reconciliation_ok: CheckStatus::from_condition(reconciliation_ok),
        publication_ok: CheckStatus::from_condition(publication_ok),
        rollback_ok: rollback.rollback_ok,
        authored_rows_preserved: CheckStatus::from_condition(authored_rows_preserved),
        active_slot_preserved_on_failure: CheckStatus::from_condition(
            rollback.active_slot_preserved.passed(),
        ),
        inactive_slot_preserved_on_failure: CheckStatus::from_condition(
            rollback.inactive_slot_preserved.passed(),
        ),
        retained_rollback_slot_preserved: CheckStatus::from_condition(
            retained_rollback_slot_preserved,
        ),
        error: None,
    })
}

/// Create the concrete authored/publication/derived evaluation schema.
fn create_strategy_schema(connection: &Connection) -> Result<(), ArchitectureEvaluationError> {
    connection.execute_batch(
        "CREATE TABLE authored_records(
             authored_key TEXT PRIMARY KEY,
             authored_value TEXT NOT NULL
         );
         CREATE TABLE publication_state(
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             active_slot INTEGER NOT NULL CHECK(active_slot IN (0, 1)),
             active_epoch INTEGER NOT NULL CHECK(active_epoch > 0)
         );
         CREATE TABLE derived_rows(
             slot INTEGER NOT NULL CHECK(slot IN (0, 1)),
             path TEXT NOT NULL,
             byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
             content TEXT NOT NULL,
             PRIMARY KEY(slot, path)
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// Add the measured secondary derived-row index.
fn create_strategy_index(connection: &Connection) -> Result<(), ArchitectureEvaluationError> {
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_evaluation_derived_path_slot
         ON derived_rows(path, slot, byte_count);",
    )?;
    Ok(())
}

/// Insert fixture rows in one setup transaction outside the measured strategy.
fn insert_slot_rows_for_setup(
    connection: &mut Connection,
    rows: &[SourceTextRow],
    slot: i64,
) -> Result<(), ArchitectureEvaluationError> {
    let transaction = connection.transaction()?;
    {
        let mut insert = transaction.prepare(
            "INSERT INTO derived_rows(slot, path, byte_count, content)
             VALUES(?1, ?2, ?3, ?4)",
        )?;
        for row in rows {
            insert.execute(params![
                slot,
                row.path,
                saturating_i64(row.content.len()),
                row.content
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// Evidence produced while building one pre-publication load source.
struct PrepublicationLoadEvidence {
    /// Load construction plus attachment duration.
    duration_ns: u64,
    /// Actual row-load transactions committed.
    transactions_committed: usize,
    /// Incremental local or total external load-source bytes.
    bytes: u64,
    /// Storage boundary represented by `bytes`.
    storage_scope: LoadStorageScope,
}

/// Build either load source with real committed transaction boundaries.
fn prepare_prepublication_load(
    connection: &mut Connection,
    database_path: &Path,
    staging_path: &Path,
    rows: &[SourceTextRow],
    load_transaction_rows: usize,
    import_mode: ImportMode,
) -> Result<PrepublicationLoadEvidence, ArchitectureEvaluationError> {
    checkpoint_wal(connection)?;
    let main_bytes_before = sqlite_file_bytes(database_path)?.total;
    let started = Instant::now();
    let (transactions_committed, bytes, storage_scope) = match import_mode {
        ImportMode::PreparedBatches => {
            connection.execute_batch(
                "CREATE TABLE evaluation_local_stage(
                     path TEXT PRIMARY KEY,
                     byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
                     content TEXT NOT NULL
                 ) WITHOUT ROWID;",
            )?;
            let transactions_committed = insert_stage_transactions(
                connection,
                rows,
                load_transaction_rows,
                "INSERT INTO evaluation_local_stage(path, byte_count, content)
                 VALUES(?1, ?2, ?3)",
            )?;
            checkpoint_wal(connection)?;
            let bytes = sqlite_file_bytes(database_path)?
                .total
                .checked_sub(main_bytes_before)
                .filter(|bytes| *bytes > 0)
                .ok_or_else(|| {
                    ArchitectureEvaluationError::Policy(
                        "local pre-publication load added no measured storage".into(),
                    )
                })?;
            (
                transactions_committed,
                bytes,
                LoadStorageScope::LocalMainDatabaseDelta,
            )
        }
        ImportMode::StagingAttachInsertSelect => {
            let build = build_staging_database(staging_path, rows, load_transaction_rows)?;
            connection.execute(
                "ATTACH DATABASE ?1 AS evaluation_stage",
                [path_text(staging_path)?],
            )?;
            (
                build.transactions_committed,
                build.bytes,
                LoadStorageScope::ExternalDatabaseTotal,
            )
        }
    };
    let expected_transactions = rows.len().div_ceil(load_transaction_rows.max(1));
    require(
        transactions_committed == expected_transactions && transactions_committed > 0,
        "pre-publication load transaction count drifted",
    )?;
    Ok(PrepublicationLoadEvidence {
        duration_ns: elapsed_ns(started),
        transactions_committed,
        bytes,
        storage_scope,
    })
}

/// Replace the inactive slot without crossing the publication transaction.
fn replace_inactive_slot(
    transaction: &Transaction<'_>,
    import_mode: ImportMode,
) -> Result<(), ArchitectureEvaluationError> {
    transaction.execute("DELETE FROM derived_rows WHERE slot = 1", [])?;
    match import_mode {
        ImportMode::PreparedBatches => {
            transaction.execute(
                "INSERT INTO derived_rows(slot, path, byte_count, content)
                 SELECT 1, path, byte_count, content
                 FROM evaluation_local_stage
                 ORDER BY path",
                [],
            )?;
        }
        ImportMode::StagingAttachInsertSelect => {
            transaction.execute(
                "INSERT INTO derived_rows(slot, path, byte_count, content)
                 SELECT 1, path, byte_count, content
                 FROM evaluation_stage.staged_rows
                 ORDER BY path",
                [],
            )?;
        }
    }
    Ok(())
}

/// Insert staged rows through actual committed transactions and return their count.
fn insert_stage_transactions(
    connection: &mut Connection,
    rows: &[SourceTextRow],
    transaction_rows: usize,
    insert_sql: &str,
) -> Result<usize, ArchitectureEvaluationError> {
    let mut committed = 0_usize;
    for chunk in rows.chunks(transaction_rows.max(1)) {
        let transaction = connection.transaction()?;
        {
            let mut insert = transaction.prepare(insert_sql)?;
            for row in chunk {
                insert.execute(params![
                    row.path,
                    saturating_i64(row.content.len()),
                    row.content
                ])?;
            }
        }
        transaction.commit()?;
        committed = committed.saturating_add(1);
    }
    Ok(committed)
}

/// External staging database build result.
struct StagingDatabaseBuild {
    /// Total database and sidecar bytes after checkpoint.
    bytes: u64,
    /// Actual staged-row transactions committed.
    transactions_committed: usize,
}

/// Build a separate staging database using committed load transactions.
fn build_staging_database(
    path: &Path,
    rows: &[SourceTextRow],
    transaction_rows: usize,
) -> Result<StagingDatabaseBuild, ArchitectureEvaluationError> {
    let mut connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", JOURNAL_MODE)?;
    connection.pragma_update(None, "synchronous", "normal")?;
    connection.execute_batch(
        "CREATE TABLE staged_rows(
             path TEXT PRIMARY KEY,
             byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
             content TEXT NOT NULL
         ) WITHOUT ROWID;",
    )?;
    let transactions_committed = insert_stage_transactions(
        &mut connection,
        rows,
        transaction_rows,
        "INSERT INTO staged_rows(path, byte_count, content) VALUES(?1, ?2, ?3)",
    )?;
    checkpoint_wal(&connection)?;
    let bytes = sqlite_file_bytes(path)?.total;
    drop(connection);
    Ok(StagingDatabaseBuild {
        bytes,
        transactions_committed,
    })
}

/// Remove the successful publication's pre-publication load source.
fn cleanup_prepublication_load(
    connection: &mut Connection,
    staging_path: &Path,
    import_mode: ImportMode,
) -> Result<(), ArchitectureEvaluationError> {
    match import_mode {
        ImportMode::PreparedBatches => {
            let transaction = connection.transaction()?;
            transaction.execute_batch("DROP TABLE evaluation_local_stage;")?;
            transaction.commit()?;
            checkpoint_wal(connection)?;
        }
        ImportMode::StagingAttachInsertSelect => {
            connection.execute_batch("DETACH DATABASE evaluation_stage;")?;
            cleanup_sqlite_files(staging_path)?;
        }
    }
    Ok(())
}

/// Observed state after a publication transaction is dropped without commit.
struct PublicationRollbackEvidence {
    /// Replacement work was rolled back after the injected failure.
    rollback_ok: CheckStatus,
    /// Active metadata and rows were unchanged.
    active_slot_preserved: CheckStatus,
    /// The previous inactive rollback slot was restored.
    inactive_slot_preserved: CheckStatus,
    /// Authored data outside both slots was unchanged.
    authored_rows_preserved: CheckStatus,
}

/// Prove dropped publication work restores both slots, metadata, and authored data.
fn verify_publication_rollback(
    connection: &mut Connection,
    rows: &[SourceTextRow],
    retained_inactive_rows: &[SourceTextRow],
    index_set: IndexSet,
    import_mode: ImportMode,
    plan: &SqliteStrategyPlan,
) -> Result<PublicationRollbackEvidence, ArchitectureEvaluationError> {
    require(
        plan.failure_injection == FAILURE_INJECTION,
        "failure injection drifted",
    )?;
    let before = storage_snapshot(connection, plan)?;
    let active_rows_before = load_slot_rows(connection, before.active_slot)?;
    let inactive_rows_before = load_slot_rows(connection, 1)?;
    require(
        inactive_rows_before == retained_inactive_rows,
        "inactive rollback fixture drifted before failure injection",
    )?;

    let injection_failed = {
        let transaction = connection.transaction()?;
        replace_inactive_slot(&transaction, import_mode)?;
        if index_set == IndexSet::PostLoad {
            create_strategy_index(&transaction)?;
        }
        require(
            load_slot_rows(&transaction, 1)? == rows,
            "failure probe did not replace the inactive slot",
        )?;
        let failed = transaction
            .execute(
                "INSERT INTO derived_rows(slot, path, byte_count, content)
                 VALUES(2, ?1, 0, '')",
                ["injected-invalid-slot"],
            )
            .is_err();
        drop(transaction);
        failed
    };

    let after = storage_snapshot(connection, plan)?;
    let active_slot_preserved = before.active_slot == after.active_slot
        && before.active_epoch == after.active_epoch
        && load_slot_rows(connection, after.active_slot)? == active_rows_before;
    let inactive_slot_preserved = load_slot_rows(connection, 1)? == inactive_rows_before;
    let authored_rows_preserved = before.authored_value == after.authored_value;
    let rollback_ok = injection_failed
        && active_slot_preserved
        && inactive_slot_preserved
        && authored_rows_preserved;
    require(
        rollback_ok,
        "dropped publication transaction changed slot, metadata, or authored state",
    )?;
    Ok(PublicationRollbackEvidence {
        rollback_ok: CheckStatus::from_condition(rollback_ok),
        active_slot_preserved: CheckStatus::from_condition(active_slot_preserved),
        inactive_slot_preserved: CheckStatus::from_condition(inactive_slot_preserved),
        authored_rows_preserved: CheckStatus::from_condition(authored_rows_preserved),
    })
}

/// Load one slot in canonical path/content order.
fn load_slot_rows(
    connection: &Connection,
    slot: i64,
) -> Result<Vec<SourceTextRow>, ArchitectureEvaluationError> {
    let mut statement = connection
        .prepare("SELECT path, content FROM derived_rows WHERE slot = ?1 ORDER BY path")?;
    let rows = statement
        .query_map([slot], |row| {
            Ok(SourceTextRow {
                path: row.get(0)?,
                content: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Minimal state used to prove publication failure isolation.
struct StorageSnapshot {
    /// Active physical slot.
    active_slot: i64,
    /// Active structural epoch.
    active_epoch: i64,
    /// Authored fixture value.
    authored_value: String,
}

/// Read publication metadata and authored state together.
fn storage_snapshot(
    connection: &Connection,
    plan: &SqliteStrategyPlan,
) -> Result<StorageSnapshot, ArchitectureEvaluationError> {
    let (active_slot, active_epoch) = connection.query_row(
        "SELECT active_slot, active_epoch FROM publication_state WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let authored_value = connection.query_row(
        "SELECT authored_value FROM authored_records WHERE authored_key = ?1",
        [&plan.authored_key],
        |row| row.get(0),
    )?;
    Ok(StorageSnapshot {
        active_slot,
        active_epoch,
        authored_value,
    })
}

/// Return the stable Cartesian matrix cell identifier.
fn cell_id(load: &LoadTransactionSpec, index_set: IndexSet, import_mode: ImportMode) -> String {
    format!("{}-{}-{}", load.id, index_set.id(), import_mode.id())
}

/// Return the storage accounting boundary for one load-source implementation.
const fn load_storage_scope(import_mode: ImportMode) -> LoadStorageScope {
    match import_mode {
        ImportMode::PreparedBatches => LoadStorageScope::LocalMainDatabaseDelta,
        ImportMode::StagingAttachInsertSelect => LoadStorageScope::ExternalDatabaseTotal,
    }
}

/// Run a truncating WAL checkpoint and reject busy/incomplete results.
fn checkpoint_wal(connection: &Connection) -> Result<(), ArchitectureEvaluationError> {
    let (busy, _log, _checkpointed): (i64, i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    require(busy == 0, "WAL checkpoint reported busy")
}

/// Capture main/WAL/SHM byte counts for one `SQLite` database.
fn sqlite_file_bytes(path: &Path) -> Result<SqliteFileBytes, ArchitectureEvaluationError> {
    let main = file_bytes(path)?;
    let wal = file_bytes(&sidecar_path(path, "-wal")?)?;
    let shm = file_bytes(&sidecar_path(path, "-shm")?)?;
    Ok(SqliteFileBytes {
        main,
        wal,
        shm,
        total: main.saturating_add(wal).saturating_add(shm),
    })
}

/// Remove output-owned main/WAL/SHM files.
fn cleanup_sqlite_files(path: &Path) -> Result<(), ArchitectureEvaluationError> {
    for candidate in [
        path.to_owned(),
        sidecar_path(path, "-wal")?,
        sidecar_path(path, "-shm")?,
        sidecar_path(path, "-journal")?,
    ] {
        match fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Return zero for an absent file and reject non-files.
fn file_bytes(path: &Path) -> Result<u64, ArchitectureEvaluationError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(_metadata) => Err(ArchitectureEvaluationError::Policy(
            "SQLite evidence path is not a file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

/// Append a `SQLite` sidecar suffix without lossy path conversion.
fn sidecar_path(path: &Path, suffix: &str) -> Result<PathBuf, ArchitectureEvaluationError> {
    let name = path
        .file_name()
        .ok_or_else(|| ArchitectureEvaluationError::Policy("SQLite path has no filename".into()))?;
    let mut sidecar = name.to_os_string();
    sidecar.push(suffix);
    Ok(path.with_file_name(sidecar))
}

/// Return a UTF-8 path accepted by `SQLite` ATTACH.
fn path_text(path: &Path) -> Result<&str, ArchitectureEvaluationError> {
    path.to_str()
        .ok_or_else(|| ArchitectureEvaluationError::Policy("SQLite path is not UTF-8".into()))
}

/// Decode the exact 256-bit seed accepted by deterministic ordering.
fn decode_seed(value: &str) -> Result<[u8; 32], ArchitectureEvaluationError> {
    let bytes = value.as_bytes();
    require(
        bytes.len() == 64,
        "global evaluation seed is not 256-bit hex",
    )?;
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let high = hex_nibble(bytes[index.saturating_mul(2)])?;
        let low = hex_nibble(bytes[index.saturating_mul(2).saturating_add(1)])?;
        *output = high << 4 | low;
    }
    Ok(decoded)
}

/// Decode one ASCII hexadecimal nibble without accepting locale-specific text.
fn hex_nibble(value: u8) -> Result<u8, ArchitectureEvaluationError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(ArchitectureEvaluationError::Policy(
            "global evaluation seed contains non-hex bytes".into(),
        )),
    }
}

/// Hash one length-prefixed field so concatenated order identities are unambiguous.
fn hash_order_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(value);
}

/// Convert elapsed monotonic time to a saturating nanosecond count.
fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Compute a deterministic nearest-rank percentile.
fn percentile(values: &[u64], percentile: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let rank = percentile.saturating_mul(ordered.len()).saturating_add(99) / 100;
    let index = rank.saturating_sub(1).min(ordered.len().saturating_sub(1));
    ordered.get(index).copied().unwrap_or(0)
}

/// Compute integer rows per second without floating-point drift.
fn rows_per_second(rows: usize, duration_ns: u64) -> u64 {
    if duration_ns == 0 {
        return u64::MAX;
    }
    u64::try_from(rows)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000_000_000)
        / duration_ns
}

/// Convert usize to `SQLite` INTEGER with saturation.
fn saturating_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Bound retained per-cell diagnostics.
fn truncate_error(value: &str) -> String {
    value.chars().take(2048).collect()
}

/// Convert a failed invariant into a typed policy error.
fn require(condition: bool, message: &str) -> Result<(), ArchitectureEvaluationError> {
    if condition {
        Ok(())
    } else {
        Err(ArchitectureEvaluationError::Policy(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal manifest view used by focused module tests.
    #[derive(Deserialize)]
    struct ManifestPlan {
        /// Registered deterministic experiment policy.
        experiment_design: TestExperimentDesign,
        /// Architecture-evaluation plan.
        architecture_evaluations: ArchitectureEvaluationPlan,
        /// Registered architecture result fields.
        result_schema: ResultSchemaPlan,
    }

    /// Exact manifest field inventories asserted against serialized metrics.
    #[derive(Deserialize)]
    struct ResultSchemaPlan {
        /// Serialized deterministic sample context fields.
        architecture_sample_context: Vec<String>,
        /// FTS aggregate fields.
        fts_result_metrics: Vec<String>,
        /// One raw FTS request observation.
        fts_request_observation: Vec<String>,
        /// One ordered FTS update observation.
        fts_update_observation: Vec<String>,
        /// `SQLite` aggregate fields.
        sqlite_strategy_result_metrics: Vec<String>,
        /// `SQLite` cell fields.
        sqlite_strategy_cell: Vec<String>,
    }

    /// Minimal registered experiment design used by evaluator tests.
    #[derive(Deserialize)]
    struct TestExperimentDesign {
        /// Registered deterministic RNG.
        rng: TestRng,
    }

    /// Exact global seed value used by ordering tests.
    #[derive(Deserialize)]
    struct TestRng {
        /// Registered 256-bit seed.
        seed_hex: String,
    }

    /// The FTS differential uses real file_text rows and every controlled fallback class.
    #[test]
    fn fts_differential_is_complete_and_nonvacuous() -> Result<(), ArchitectureEvaluationError> {
        let temp = tempfile::tempdir()?;
        let source_db = temp.path().join("source.db");
        create_source_database(&source_db)?;
        let manifest = test_manifest()?;
        let sample = test_sample(&manifest, "test-corpus:fts-differential:measurement", 0)?;
        let result = run_fts_differential(
            &source_db,
            &temp.path().join("fts-evaluation"),
            &manifest.architecture_evaluations,
            &sample,
        )?;
        assert!(result.is_eligible());
        assert_eq!(result.sample_context, sample);
        assert!(result.corpus_rows > 0);
        assert!(result.baseline_positive_matches > 0);
        assert_eq!(result.forced_fallback_queries, 6);
        assert!(result.candidate_superset_complete.passed());
        assert!(result.exact_rows_and_order_equal.passed());
        assert!(result.update_restored.passed());
        assert!(result.update_observation.without_fts_ns > 0);
        assert!(result.update_observation.with_fts_ns > 0);
        assert_eq!(
            result.update_observation.order,
            sample.pair_order("fts-update")
        );
        let expected_observations = manifest
            .architecture_evaluations
            .fts_differential
            .corpus_queries
            .len()
            .saturating_mul(
                manifest
                    .architecture_evaluations
                    .fts_differential
                    .timed_query_iterations,
            );
        assert_eq!(result.request_observations.len(), expected_observations);
        assert_eq!(
            result
                .request_observations
                .iter()
                .map(|observation| observation.order)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                PairOrder::WithoutFtsThenWithFts,
                PairOrder::WithFtsThenWithoutFts,
            ])
        );
        for observation in &result.request_observations {
            assert!(observation.without_fts_ns > 0);
            assert!(observation.with_fts_ns > 0);
            assert_eq!(
                observation.order,
                sample.pair_order(&format!(
                    "fts-query:{}:{}",
                    observation.query_id, observation.iteration
                ))
            );
        }
        assert!(result.checkpointed_storage_bytes.without_fts.main > 0);
        assert!(
            result.checkpointed_storage_bytes.with_fts.total
                > result.checkpointed_storage_bytes.without_fts.total
        );
        assert_eq!(
            result.index_bytes,
            result
                .checkpointed_storage_bytes
                .with_fts
                .total
                .checked_sub(result.checkpointed_storage_bytes.without_fts.total)
                .expect("FTS storage must exceed the lexical baseline")
        );
        assert!(result.index_bytes < result.checkpointed_storage_bytes.with_fts.total);
        assert!(result.baseline_p50_ns > 0);
        assert!(result.baseline_p95_ns >= result.baseline_p50_ns);
        assert!(result.candidate_p50_ns > 0);
        assert!(result.candidate_p95_ns >= result.candidate_p50_ns);
        let metrics = serde_json::to_value(ArchitectureMetrics::FtsResult { result })?;
        assert_object_fields(&metrics, &manifest.result_schema.fts_result_metrics)?;
        assert_object_fields(
            &metrics["sample_context"],
            &manifest.result_schema.architecture_sample_context,
        )?;
        assert_object_fields(
            &metrics["update_observation"],
            &manifest.result_schema.fts_update_observation,
        )?;
        let observations = metrics["request_observations"].as_array().ok_or_else(|| {
            ArchitectureEvaluationError::Policy(
                "serialized FTS request observations are missing".into(),
            )
        })?;
        assert_eq!(observations.len(), expected_observations);
        for observation in observations {
            assert_object_fields(observation, &manifest.result_schema.fts_request_observation)?;
        }
        assert_object_fields(
            &metrics["checkpointed_storage_bytes"],
            &["without_fts".to_owned(), "with_fts".to_owned()],
        )?;
        let storage_fields = [
            "main".to_owned(),
            "wal".to_owned(),
            "shm".to_owned(),
            "total".to_owned(),
        ];
        assert_object_fields(
            &metrics["checkpointed_storage_bytes"]["without_fts"],
            &storage_fields,
        )?;
        assert_object_fields(
            &metrics["checkpointed_storage_bytes"]["with_fts"],
            &storage_fields,
        )?;
        Ok(())
    }

    /// Every `SQLite` matrix cell publishes atomically and retains rollback evidence.
    #[test]
    fn sqlite_strategy_matrix_preserves_slots_and_authored_data()
    -> Result<(), ArchitectureEvaluationError> {
        let temp = tempfile::tempdir()?;
        let source_db = temp.path().join("source.db");
        create_source_database(&source_db)?;
        let manifest = test_manifest()?;
        let sample = test_sample(&manifest, "test-corpus:sqlite-strategy:measurement", 0)?;
        let result = run_sqlite_strategy(
            &source_db,
            &temp.path().join("sqlite-evaluation"),
            &manifest.architecture_evaluations,
            &sample,
        )?;
        assert!(result.is_eligible());
        assert_eq!(result.sample_context, sample);
        assert_eq!(
            result.execution_order,
            result
                .cells
                .iter()
                .map(|cell| cell.cell_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.execution_order,
            sqlite_execution_plan(&manifest.architecture_evaluations.sqlite_strategy, &sample)
                .into_iter()
                .map(|cell| cell.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(result.required_cells, 12);
        assert_eq!(result.retained_cells, 12);
        assert_eq!(result.successful_cells, 12);
        assert!(result.all_rollbacks_ok.passed());
        assert!(result.all_authored_rows_preserved.passed());
        assert!(result.all_active_slots_preserved_on_failure.passed());
        assert!(result.selected_cell.is_some());
        assert!(result.source_rows > 2_048);
        let cell_ids = result
            .cells
            .iter()
            .map(|cell| cell.cell_id.clone())
            .collect::<BTreeSet<_>>();
        let expected_cell_ids = ["rows-256", "rows-2048", "all-rows"]
            .into_iter()
            .flat_map(|size| {
                [IndexSet::Maintained, IndexSet::PostLoad]
                    .into_iter()
                    .flat_map(move |index_set| {
                        [
                            ImportMode::PreparedBatches,
                            ImportMode::StagingAttachInsertSelect,
                        ]
                        .into_iter()
                        .map(move |import_mode| {
                            format!("{size}-{}-{}", index_set.id(), import_mode.id())
                        })
                    })
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(cell_ids, expected_cell_ids);
        let load_transactions = result
            .cells
            .iter()
            .map(|cell| (cell.load_transaction.as_str(), cell.load_transaction_rows))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            load_transactions,
            BTreeSet::from([
                ("rows-256", 256),
                ("rows-2048", 2_048),
                ("all-rows", result.source_rows),
            ])
        );
        assert_eq!(
            result
                .cells
                .iter()
                .map(|cell| cell.index_set)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([IndexSet::Maintained, IndexSet::PostLoad])
        );
        assert_eq!(
            result
                .cells
                .iter()
                .map(|cell| cell.import_mode)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                ImportMode::PreparedBatches,
                ImportMode::StagingAttachInsertSelect,
            ])
        );
        for cell in &result.cells {
            assert!(cell.passed());
            assert_eq!(cell.status, CellStatus::Passed);
            assert_eq!(cell.slot_mode, SLOT_MODE);
            assert_eq!(cell.source_rows, result.source_rows);
            assert_eq!(
                cell.load_storage_scope,
                load_storage_scope(cell.import_mode)
            );
            let prepublication_load_ns = cell.prepublication_load_ns.ok_or_else(|| {
                ArchitectureEvaluationError::Policy("pre-publication load timing is missing".into())
            })?;
            let publication_ns = cell.publication_ns.ok_or_else(|| {
                ArchitectureEvaluationError::Policy("publication timing is missing".into())
            })?;
            let strategy_ns = cell.strategy_ns.ok_or_else(|| {
                ArchitectureEvaluationError::Policy("strategy timing is missing".into())
            })?;
            assert!(publication_ns > 0);
            assert!(prepublication_load_ns > 0);
            assert_eq!(
                strategy_ns,
                prepublication_load_ns.saturating_add(publication_ns)
            );
            assert_eq!(
                cell.load_transactions_committed,
                Some(result.source_rows.div_ceil(cell.load_transaction_rows))
            );
            assert!(cell.rows_per_second.is_some_and(|rate| rate > 0));
            assert!(
                cell.wal_bytes_before_checkpoint
                    .is_some_and(|bytes| bytes > 0)
            );
            let checkpointed = cell.checkpointed_storage_bytes.ok_or_else(|| {
                ArchitectureEvaluationError::Policy("checkpointed storage is missing".into())
            })?;
            assert!(checkpointed.main > 0);
            assert_eq!(checkpointed.wal, 0);
            assert_eq!(
                checkpointed.total,
                checkpointed
                    .main
                    .saturating_add(checkpointed.wal)
                    .saturating_add(checkpointed.shm)
            );
            let load_bytes = cell.prepublication_load_bytes.ok_or_else(|| {
                ArchitectureEvaluationError::Policy(
                    "pre-publication load storage is missing".into(),
                )
            })?;
            assert!(load_bytes > 0);
            assert!(cell.load_cleanup_ns.is_some_and(|duration| duration > 0));
            assert!(cell.retained_bytes.is_some_and(|bytes| bytes > 0));
            assert!(cell.cleanup_ns.is_some_and(|duration| duration > 0));
            assert!(cell.reconciliation_ok.passed());
            assert!(cell.publication_ok.passed());
            assert!(cell.rollback_ok.passed());
            assert!(cell.authored_rows_preserved.passed());
            assert!(cell.active_slot_preserved_on_failure.passed());
            assert!(cell.inactive_slot_preserved_on_failure.passed());
            assert!(cell.retained_rollback_slot_preserved.passed());
            assert!(cell.error.is_none());
        }
        assert!(
            result
                .selected_cell
                .as_ref()
                .is_some_and(|selected| cell_ids.contains(selected))
        );
        let metrics = serde_json::to_value(ArchitectureMetrics::SqliteStrategyResult { result })?;
        assert_object_fields(
            &metrics,
            &manifest.result_schema.sqlite_strategy_result_metrics,
        )?;
        assert_object_fields(
            &metrics["sample_context"],
            &manifest.result_schema.architecture_sample_context,
        )?;
        let cells = metrics
            .get("cells")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                ArchitectureEvaluationError::Policy("serialized SQLite cells are missing".into())
            })?;
        for cell in cells {
            assert_object_fields(cell, &manifest.result_schema.sqlite_strategy_cell)?;
        }
        Ok(())
    }

    /// The manifest accepts only the stable closed `SQLite` selection policy schema.
    #[test]
    fn sqlite_selection_policy_rejects_manifest_and_schema_mutations()
    -> Result<(), ArchitectureEvaluationError> {
        let manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json"
        ))?;
        let plan = architecture_plan_from_manifest_value(&manifest)?;
        require(
            plan.sqlite_strategy.selection_policy
                == SqliteCellSelectionPolicy::HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId,
            "registered SQLite selection policy drifted",
        )?;

        let mut arbitrary_policy = manifest.clone();
        let policy_value = arbitrary_policy
            .pointer_mut("/architecture_evaluations/sqlite_strategy/selection_policy")
            .ok_or_else(|| {
                ArchitectureEvaluationError::Policy(
                    "SQLite selection policy field is missing".into(),
                )
            })?;
        *policy_value = serde_json::Value::String("fastest-nonempty-description".into());
        require(
            architecture_plan_from_manifest_value(&arbitrary_policy).is_err(),
            "arbitrary SQLite selection policy was accepted",
        )?;

        let mut missing_policy = manifest.clone();
        let removed =
            sqlite_strategy_manifest_object_mut(&mut missing_policy)?.remove("selection_policy");
        require(
            removed.is_some() && architecture_plan_from_manifest_value(&missing_policy).is_err(),
            "missing SQLite selection policy was accepted",
        )?;

        let mut unknown_field = manifest;
        sqlite_strategy_manifest_object_mut(&mut unknown_field)?.insert(
            "selection_rule".into(),
            serde_json::Value::String("legacy free-form rule".into()),
        );
        require(
            architecture_plan_from_manifest_value(&unknown_field).is_err(),
            "unknown SQLite selection schema field was accepted",
        )
    }

    /// Selection excludes faster invalid cells and chooses the highest eligible throughput.
    #[test]
    fn sqlite_selection_policy_preserves_correctness_before_throughput()
    -> Result<(), ArchitectureEvaluationError> {
        let policy = SqliteCellSelectionPolicy::HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId;
        let mut invalid_fastest = controlled_selection_cell("invalid-fastest", 300);
        invalid_fastest.reconciliation_ok = CheckStatus::from_condition(false);
        let cells = [
            controlled_selection_cell("eligible-slower", 100),
            invalid_fastest,
            controlled_selection_cell("eligible-fastest", 200),
        ];
        require(
            policy.select_cell(&cells).as_deref() == Some("eligible-fastest"),
            "SQLite selection did not prioritize correctness-preserving throughput",
        )
    }

    /// Equal throughput is resolved by the lowest stable cell identifier in any input order.
    #[test]
    fn sqlite_selection_policy_uses_stable_cell_id_tie_break()
    -> Result<(), ArchitectureEvaluationError> {
        let policy = SqliteCellSelectionPolicy::HighestCorrectnessPreservingTotalStrategyThroughputThenLowestStableCellId;
        let forward = [
            controlled_selection_cell("zeta-cell", 200),
            controlled_selection_cell("alpha-cell", 200),
        ];
        let reverse = [
            controlled_selection_cell("alpha-cell", 200),
            controlled_selection_cell("zeta-cell", 200),
        ];
        require(
            policy.select_cell(&forward).as_deref() == Some("alpha-cell")
                && policy.select_cell(&reverse).as_deref() == Some("alpha-cell"),
            "SQLite selection tie-break depends on cell order",
        )
    }

    /// Seeded low-bit ordering is deterministic and exercises both pair orders.
    #[test]
    fn sample_context_ordering_is_deterministic_and_not_fixed()
    -> Result<(), ArchitectureEvaluationError> {
        let manifest = test_manifest()?;
        let orders = (0..64)
            .map(|repetition| {
                test_sample(
                    &manifest,
                    "test-corpus:fts-differential:measurement",
                    repetition,
                )
                .map(|sample| sample.pair_order("fts-query:ascii-trigram-pub:0"))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        assert_eq!(
            orders,
            BTreeSet::from([
                PairOrder::WithoutFtsThenWithFts,
                PairOrder::WithFtsThenWithoutFts,
            ])
        );
        let sample = test_sample(&manifest, "stable-cell", 7)?;
        assert_eq!(
            sample.ordering_digest("pair"),
            decode_seed("71d2c2cbb72d6970d4ccccbefc591b301bf7c86d02cbc99ec82c4d1ee2700101")?,
            "the versioned fixed-width ordering encoding drifted"
        );
        assert_eq!(
            sample.pair_order("pair"),
            PairOrder::WithFtsThenWithoutFts,
            "the golden digest must retain its registered low-bit order"
        );
        assert_eq!(
            sample.pair_order("pair"),
            sample.pair_order("pair"),
            "the same sample and pair must retain one order"
        );
        Ok(())
    }

    /// Exact FTS restoration rejects logical corruption outside the update marker.
    #[test]
    fn fts_exact_contents_reject_unrelated_corruption() -> Result<(), ArchitectureEvaluationError> {
        let temp = tempfile::tempdir()?;
        let rows = vec![
            SourceTextRow {
                path: "src/lib.rs".into(),
                content: "pub fn value() {}\n".into(),
            },
            SourceTextRow {
                path: "src/other.rs".into(),
                content: "pub struct Other;\n".into(),
            },
        ];
        let connection = build_fts_database(&temp.path().join("fts.db"), &rows)?;
        assert!(fts_contents_are_exact(&connection, &rows)?);
        connection.execute(
            "DELETE FROM evaluation_file_text_fts WHERE path = ?1",
            ["src/other.rs"],
        )?;
        assert!(!fts_contents_are_exact(&connection, &rows)?);
        Ok(())
    }

    /// Empty indexed files remain valid and immutable reads leave WAL state untouched.
    #[test]
    fn source_rows_accept_empty_content_without_mutating_wal_state()
    -> Result<(), ArchitectureEvaluationError> {
        let temp = tempfile::tempdir()?;
        let source_db = temp.path().join("source with spaces.db");
        let mut writer = Connection::open(&source_db)?;
        writer.pragma_update(None, "journal_mode", JOURNAL_MODE)?;
        writer.pragma_update(None, "wal_autocheckpoint", 0)?;
        writer.execute_batch(
            "CREATE TABLE file_texts(path TEXT PRIMARY KEY, content TEXT NOT NULL);",
        )?;
        checkpoint_wal(&writer)?;
        let transaction = writer.transaction()?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO file_texts(path, content) VALUES(?1, ?2)")?;
            insert.execute(params!["src/empty.rs", ""])?;
            insert.execute(params!["src/lib.rs", "pub fn value() {}\n"])?;
        }
        transaction.commit()?;
        let before = sqlite_source_contents(&source_db)?;

        let rows = load_source_rows(&source_db, &temp.path().join("source-snapshot.db"))?;

        let after = sqlite_source_contents(&source_db)?;
        assert_eq!(before, after);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.content.is_empty()));
        Ok(())
    }

    /// Parse the exact registered architecture contract compiled into the example.
    fn test_manifest() -> Result<ManifestPlan, ArchitectureEvaluationError> {
        Ok(serde_json::from_slice(include_bytes!(
            "../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json"
        ))?)
    }

    /// Decode the exact architecture subtree from a mutable manifest fixture.
    fn architecture_plan_from_manifest_value(
        manifest: &serde_json::Value,
    ) -> Result<ArchitectureEvaluationPlan, ArchitectureEvaluationError> {
        let architecture_plan = manifest
            .get("architecture_evaluations")
            .cloned()
            .ok_or_else(|| {
                ArchitectureEvaluationError::Policy(
                    "manifest architecture evaluation plan is missing".into(),
                )
            })?;
        Ok(serde_json::from_value(architecture_plan)?)
    }

    /// Return the exact mutable `SQLite` strategy schema object.
    fn sqlite_strategy_manifest_object_mut(
        manifest: &mut serde_json::Value,
    ) -> Result<&mut serde_json::Map<String, serde_json::Value>, ArchitectureEvaluationError> {
        manifest
            .pointer_mut("/architecture_evaluations/sqlite_strategy")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| {
                ArchitectureEvaluationError::Policy(
                    "manifest SQLite strategy plan is missing".into(),
                )
            })
    }

    /// Build one controlled cell satisfying every correctness and evidence invariant.
    fn controlled_selection_cell(cell_id: &str, rate: u64) -> SqliteStrategyCell {
        SqliteStrategyCell {
            status: CellStatus::Passed,
            cell_id: cell_id.into(),
            load_transaction: "controlled-load".into(),
            load_transaction_rows: 1,
            index_set: IndexSet::Maintained,
            import_mode: ImportMode::PreparedBatches,
            load_storage_scope: LoadStorageScope::LocalMainDatabaseDelta,
            slot_mode: SLOT_MODE,
            source_rows: 1,
            prepublication_load_ns: Some(1),
            load_transactions_committed: Some(1),
            publication_ns: Some(1),
            strategy_ns: Some(2),
            rows_per_second: Some(rate),
            wal_bytes_before_checkpoint: Some(1),
            checkpointed_storage_bytes: Some(SqliteFileBytes {
                main: 1,
                wal: 0,
                shm: 0,
                total: 1,
            }),
            prepublication_load_bytes: Some(1),
            load_cleanup_ns: Some(1),
            retained_bytes: Some(1),
            cleanup_ns: Some(1),
            reconciliation_ok: CheckStatus::from_condition(true),
            publication_ok: CheckStatus::from_condition(true),
            rollback_ok: CheckStatus::from_condition(true),
            authored_rows_preserved: CheckStatus::from_condition(true),
            active_slot_preserved_on_failure: CheckStatus::from_condition(true),
            inactive_slot_preserved_on_failure: CheckStatus::from_condition(true),
            retained_rollback_slot_preserved: CheckStatus::from_condition(true),
            error: None,
        }
    }

    /// Build one sample context from the manifest-owned global seed.
    fn test_sample(
        manifest: &ManifestPlan,
        stable_cell_identity: &str,
        repetition: usize,
    ) -> Result<ArchitectureSampleContext, ArchitectureEvaluationError> {
        ArchitectureSampleContext::new(
            GLOBAL_SEED_REFERENCE,
            manifest.experiment_design.rng.seed_hex.clone(),
            stable_cell_identity,
            repetition,
        )
    }

    /// Compare one serialized object with its exact manifest field inventory.
    fn assert_object_fields(
        value: &serde_json::Value,
        expected: &[String],
    ) -> Result<(), ArchitectureEvaluationError> {
        let object = value.as_object().ok_or_else(|| {
            ArchitectureEvaluationError::Policy("serialized metric is not an object".into())
        })?;
        let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        let expected = expected.iter().map(String::as_str).collect::<BTreeSet<_>>();
        require(
            actual == expected && expected.len() == object.len(),
            "serialized metric fields differ from the manifest",
        )
    }

    /// Capture exact source database and sidecar bytes for immutability assertions.
    fn sqlite_source_contents(
        path: &Path,
    ) -> Result<Vec<Option<Vec<u8>>>, ArchitectureEvaluationError> {
        [
            path.to_owned(),
            sidecar_path(path, "-wal")?,
            sidecar_path(path, "-shm")?,
        ]
        .into_iter()
        .map(|candidate| match fs::read(candidate) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        })
        .collect()
    }

    /// Create a non-vacuous stand-in exceeding every fixed batch boundary.
    fn create_source_database(path: &Path) -> Result<(), ArchitectureEvaluationError> {
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE file_texts(path TEXT PRIMARY KEY, content TEXT NOT NULL);",
        )?;
        let transaction = connection.transaction()?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO file_texts(path, content) VALUES(?1, ?2)")?;
            insert.execute(params!["src/lib.rs", "pub fn value() -> u8 { 1 }\n"])?;
            insert.execute(params!["src/main.rs", "pub fn main() {}\n"])?;
            insert.execute(params!["src/other.rs", "struct Other;\n"])?;
            insert.execute(params!["src/empty.rs", ""])?;
            for index in 0..2_049 {
                insert.execute(params![
                    format!("src/generated/{index:04}.rs"),
                    format!("pub fn generated_{index}() -> usize {{ {index} }}\n")
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
