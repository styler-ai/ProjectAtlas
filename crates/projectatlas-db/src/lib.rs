//! Purpose: Persist `ProjectAtlas` 3 indexes in `SQLite`.

use projectatlas_core::health::{HealthFinding, Severity, finding_id};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SourceParseMetadata, SymbolGraph, SymbolKind,
    SymbolRelation,
};
use projectatlas_core::telemetry::{
    TokenBucketOverview, TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow,
    UsageEvent, default_estimate_method, default_token_accuracy, default_token_model,
    default_token_provider, default_token_trace, default_tokenizer_backend,
};
use projectatlas_core::{
    AGENT_REVIEWED_SOURCE_VALUES, HIGH_IMPACT_FILE_NAMES, HIGH_IMPACT_PATH_PREFIXES,
    HIGH_IMPACT_PATH_SEGMENTS, IndexedNode, Node, NodeKind, Overview, Purpose, PurposeSource,
    PurposeStatus, normalize_native_path_display, normalize_repo_path_prefix,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::TryFromIntError;
use std::path::Path;
use thiserror::Error;

/// Current `SQLite` schema version supported by this crate.
const SCHEMA_VERSION: i64 = 8;
/// Maximum persisted text for denormalized symbol-name search summaries.
const MAX_SYMBOL_SEARCH_SUMMARY_CHARS: usize = 16_000;

/// Database-layer error type.
#[derive(Debug, Error)]
pub enum DbError {
    /// `SQLite` operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Schema version is not supported.
    #[error("unsupported schema version {found}, expected {expected}")]
    SchemaVersion {
        /// Version found in database.
        found: i64,
        /// Expected version.
        expected: i64,
    },
    /// Invalid enum value read from the database.
    #[error("invalid {field} value in database: {value}")]
    InvalidEnum {
        /// Field name.
        field: &'static str,
        /// Invalid value.
        value: String,
    },
    /// Count value from `SQLite` could not fit in `usize`.
    #[error("invalid count for {field}: {value}")]
    InvalidCount {
        /// Count field name.
        field: &'static str,
        /// Invalid database count.
        value: i64,
        /// Source conversion error.
        source: TryFromIntError,
    },
    /// A caller supplied a path that is not in the current index.
    #[error("path {path:?} is not indexed; run scan, fix the path, or choose an indexed path")]
    PathNotIndexed {
        /// Repository-relative path.
        path: String,
    },
    /// A caller attempted to resolve a health finding that is not currently active.
    #[error(
        "health finding {finding_id:?} with category {category:?} and path {path:?} is not active; run health-check and use an exact finding id/path/category"
    )]
    HealthFindingNotActive {
        /// Requested finding id.
        finding_id: String,
        /// Requested category.
        category: String,
        /// Requested primary path.
        path: String,
    },
}

/// Convenient result alias for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// `SQLite`-backed `ProjectAtlas` index store.
pub struct AtlasStore {
    /// Active database connection for index reads and writes.
    connection: Connection,
}

/// UTF-8 source text persisted for indexed search.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexedFileText {
    /// Repository-relative file path using forward slashes.
    pub path: String,
    /// BLAKE3 content hash from the scanned file node.
    pub content_hash: Option<String>,
    /// UTF-8 byte count stored for telemetry.
    pub byte_count: usize,
    /// Number of text lines stored for context extraction.
    pub line_count: usize,
    /// Full UTF-8 source text used by indexed search.
    pub content: String,
}

/// Agent-approved resolution for a deterministic health finding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthResolution {
    /// Stable health finding id.
    pub finding_id: String,
    /// Finding category.
    pub category: String,
    /// Primary path.
    pub path: String,
    /// Related path, when any.
    pub related_path: Option<String>,
    /// Agent rationale for suppressing future repeats.
    pub rationale: String,
}

/// Bounded health query used by agent-facing adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthQuery {
    /// Pagination start index after filters are applied.
    pub start_index: usize,
    /// Maximum findings to return.
    pub limit: usize,
    /// Optional finding category filter.
    pub category: Option<String>,
    /// Optional severity filter.
    pub severity: Option<Severity>,
    /// Optional repository-relative path prefix filter.
    pub path_prefix: Option<String>,
    /// Return counts without finding rows.
    pub summary_only: bool,
    /// Health and purpose-curation scope.
    pub scope: HealthScope,
}

/// Scope controls for bounded health and purpose-curation queries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthScope {
    /// Include all indexed paths.
    All,
    /// Include only source files and folders with source descendants.
    SourceOnly,
    /// Include all folders plus high-impact files.
    PurposeDefault,
    /// Include all folders, high-impact files, and non-source files.
    PurposeWithAssets,
    /// Include all folders, high-impact files, and all source files.
    PurposeWithSourceFiles,
    /// Include every indexed file and folder.
    PurposeStrict,
}

impl HealthScope {
    /// Scope matching unfiltered health output.
    pub fn all() -> Self {
        Self::All
    }

    /// Scope restricted to source-relevant paths.
    pub fn source_only() -> Self {
        Self::SourceOnly
    }

    /// Default agent purpose curation scope: folders plus high-impact files.
    pub fn purpose_default() -> Self {
        Self::PurposeDefault
    }

    /// Purpose curation scope including non-source asset files.
    pub fn purpose_with_assets() -> Self {
        Self::PurposeWithAssets
    }

    /// Purpose curation scope including all source files.
    pub fn purpose_with_source_files() -> Self {
        Self::PurposeWithSourceFiles
    }

    /// Strict purpose curation scope including every indexed path.
    pub fn purpose_strict() -> Self {
        Self::PurposeStrict
    }

    /// Whether this scope should be reported as source-focused in agent payloads.
    pub fn is_source_focused(self) -> bool {
        self.source_only_filter()
    }

    /// Whether this scope uses the folder-first high-impact purpose queue.
    pub fn is_purpose_queue(self) -> bool {
        self.high_impact_queue()
    }

    /// Whether source relevance should be applied before queue-specific filters.
    fn source_only_filter(self) -> bool {
        matches!(
            self,
            Self::SourceOnly | Self::PurposeDefault | Self::PurposeWithSourceFiles
        )
    }

    /// Whether the scope should use folder-first purpose queue selection.
    fn high_impact_queue(self) -> bool {
        matches!(
            self,
            Self::PurposeDefault
                | Self::PurposeWithAssets
                | Self::PurposeWithSourceFiles
                | Self::PurposeStrict
        )
    }

    /// Whether non-source asset files should be included in queue selection.
    fn include_assets(self) -> bool {
        matches!(self, Self::PurposeWithAssets)
    }

    /// Whether all source files should be included in queue selection.
    fn include_source_files(self) -> bool {
        matches!(self, Self::PurposeWithSourceFiles | Self::PurposeStrict)
    }

    /// Whether all files should be included in queue selection.
    fn include_all_files(self) -> bool {
        matches!(self, Self::PurposeStrict)
    }
}

/// Bounded health findings page returned by the database layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthFindingsPage {
    /// Findings after filters are applied.
    pub total: usize,
    /// Findings before filters are applied, after resolved findings are removed.
    pub unfiltered_total: usize,
    /// Findings returned in this page.
    pub returned: usize,
    /// Pagination start index used for this page.
    pub start_index: usize,
    /// Maximum findings requested for this page.
    pub limit: usize,
    /// Returned health finding rows.
    pub findings: Vec<HealthFinding>,
}

/// Static metadata for one purpose lifecycle health category.
#[derive(Clone, Copy, Debug)]
struct PurposeHealthSpec {
    /// Stored purpose status that emits this health category.
    status: &'static str,
    /// Health finding category for the lifecycle status.
    category: &'static str,
    /// Health finding message for every row in this lifecycle category.
    message: &'static str,
    /// Agent recommendation for resolving this lifecycle category.
    recommendation: &'static str,
}

/// Purpose lifecycle health categories that can be paged directly in `SQLite`.
const PURPOSE_HEALTH_SPECS: [PurposeHealthSpec; 3] = [
    PurposeHealthSpec {
        status: "missing",
        category: "missing-purpose",
        message: "Path is indexed but has no approved purpose.",
        recommendation: "Set an agent-reviewed one-line purpose in the ProjectAtlas index.",
    },
    PurposeHealthSpec {
        status: "suggested",
        category: "suggested-purpose-review",
        message: "Path has a generated purpose suggestion but no agent-approved purpose.",
        recommendation: "Inspect enough context and approve or correct the purpose in SQLite.",
    },
    PurposeHealthSpec {
        status: "stale",
        category: "stale-purpose",
        message: "Path changed after its purpose was approved.",
        recommendation: "Inspect current context and approve or correct the one-line purpose.",
    },
];
/// Health category for approved purpose rows that still need agent review.
const AGENT_REVIEW_REQUIRED_CATEGORY: &str = "purpose-agent-review-required";
/// Health message for approved purpose rows that still need agent review.
const AGENT_REVIEW_REQUIRED_MESSAGE: &str =
    "Purpose is approved but has not been reviewed by an agent.";
/// Health recommendation for approved purpose rows that still need agent review.
const AGENT_REVIEW_REQUIRED_RECOMMENDATION: &str =
    "Inspect current context and approve or correct the purpose with purpose set.";

/// Folder names treated as repeated temporary/generated-output buckets.
const TEMP_FOLDER_BUCKETS: [&str; 6] = ["tmp", "temp", "cache", "generated", "out", "output"];

impl AtlasStore {
    /// Open or create an index store.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` setup or schema validation fails.
    pub fn open(path: &Path) -> DbResult<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let store = Self { connection };
        store.initialize_schema()?;
        Ok(store)
    }

    /// Open an in-memory store for tests.
    ///
    /// # Errors
    ///
    /// Returns an error if schema setup fails.
    pub fn in_memory() -> DbResult<Self> {
        let store = Self {
            connection: Connection::open_in_memory()?,
        };
        store.initialize_schema()?;
        Ok(store)
    }

    /// Initialize schema.
    ///
    /// # Errors
    ///
    /// Returns an error if schema creation or validation fails.
    pub fn initialize_schema(&self) -> DbResult<()> {
        self.reset_legacy_summary_schema()?;
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS nodes (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                kind TEXT NOT NULL,
                parent_path TEXT,
                extension TEXT,
                language TEXT,
                size_bytes INTEGER,
                mtime_ns INTEGER,
                content_hash TEXT,
                exists_now INTEGER NOT NULL DEFAULT 1,
                first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                last_indexed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS purposes (
                node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
                purpose TEXT,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_by TEXT
            );

            CREATE TABLE IF NOT EXISTS summaries (
                id INTEGER PRIMARY KEY,
                node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
                summary_level TEXT NOT NULL DEFAULT 'node',
                subject TEXT NOT NULL DEFAULT '',
                summary TEXT,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(node_id, summary_level, subject)
            );

            CREATE TABLE IF NOT EXISTS usage_events (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                command TEXT NOT NULL,
                path TEXT,
                query TEXT,
                estimated_tokens_without_projectatlas INTEGER,
                estimated_tokens_with_projectatlas INTEGER,
                estimated_tokens_saved INTEGER,
                token_savings_bucket TEXT NOT NULL DEFAULT 'navigation_avoidance',
                provider TEXT NOT NULL DEFAULT 'heuristic',
                model TEXT NOT NULL DEFAULT 'unknown',
                tokenizer_backend TEXT NOT NULL DEFAULT 'chars_div_4',
                accuracy TEXT NOT NULL DEFAULT 'heuristic_estimate',
                baseline_kind TEXT NOT NULL DEFAULT 'selected_candidates',
                confidence TEXT NOT NULL DEFAULT 'inferred',
                calculation_trace TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)',
                accounting_layer TEXT NOT NULL DEFAULT 'modeled_avoidance',
                estimate_method TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4',
                denominator_kind TEXT NOT NULL DEFAULT 'selected_candidates',
                baseline_identity TEXT NOT NULL DEFAULT '',
                baseline_fingerprint TEXT NOT NULL DEFAULT '',
                dedupe_scope TEXT NOT NULL DEFAULT 'session',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                language TEXT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                exported INTEGER NOT NULL DEFAULT 0,
                documentation TEXT,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                parent TEXT,
                parser TEXT NOT NULL,
                detail TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS source_parse_metadata (
                path TEXT PRIMARY KEY,
                language TEXT,
                parser TEXT NOT NULL,
                symbol_count INTEGER NOT NULL,
                relation_count INTEGER NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS symbol_relations (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                source_name TEXT NOT NULL,
                target_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                context TEXT NOT NULL,
                parser TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS health_resolutions (
                finding_id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                path TEXT NOT NULL,
                related_path TEXT,
                rationale TEXT NOT NULL,
                resolved_by TEXT NOT NULL DEFAULT 'agent',
                resolved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS file_texts (
                path TEXT PRIMARY KEY,
                content_hash TEXT,
                byte_count INTEGER NOT NULL,
                line_count INTEGER NOT NULL,
                content TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
            CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_path);
            CREATE INDEX IF NOT EXISTS idx_purposes_status ON purposes(status);
            CREATE INDEX IF NOT EXISTS idx_summaries_level ON summaries(summary_level);
            CREATE INDEX IF NOT EXISTS idx_summaries_summary ON summaries(summary);
            CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_events(session_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
            CREATE INDEX IF NOT EXISTS idx_source_parse_metadata_parser ON source_parse_metadata(parser);
            CREATE INDEX IF NOT EXISTS idx_symbol_relations_path ON symbol_relations(path);
            CREATE INDEX IF NOT EXISTS idx_symbol_relations_target ON symbol_relations(target_name);
            CREATE INDEX IF NOT EXISTS idx_health_resolutions_category ON health_resolutions(category);
            CREATE INDEX IF NOT EXISTS idx_file_texts_hash ON file_texts(content_hash);
            ",
        )?;
        self.ensure_symbol_metadata_columns()?;
        self.ensure_usage_event_metadata_columns()?;
        self.connection.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage_events(created_at);
            CREATE INDEX IF NOT EXISTS idx_usage_session_created_at ON usage_events(session_id, created_at);
            ",
        )?;
        let stored = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match stored {
            Some(value) => {
                let found = value.parse::<i64>().map_or(-1, |parsed| parsed);
                if (1..SCHEMA_VERSION).contains(&found) {
                    self.connection.execute(
                        "UPDATE metadata SET value = ?1 WHERE key = 'schema_version'",
                        [SCHEMA_VERSION.to_string()],
                    )?;
                } else if found != SCHEMA_VERSION {
                    return Err(DbError::SchemaVersion {
                        found,
                        expected: SCHEMA_VERSION,
                    });
                }
            }
            None => {
                self.connection.execute(
                    "INSERT INTO metadata(key, value) VALUES('schema_version', ?1)",
                    [SCHEMA_VERSION.to_string()],
                )?;
            }
        }
        Ok(())
    }

    /// Add usage telemetry metadata columns to older databases.
    fn ensure_usage_event_metadata_columns(&self) -> DbResult<()> {
        let mut statement = self.connection.prepare("PRAGMA table_info(usage_events)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = Vec::new();
        for row in rows {
            columns.push(row?);
        }
        self.ensure_usage_event_column(
            &columns,
            "token_savings_bucket",
            "TEXT NOT NULL DEFAULT 'navigation_avoidance'",
        )?;
        self.ensure_usage_event_column(&columns, "provider", "TEXT NOT NULL DEFAULT 'heuristic'")?;
        self.ensure_usage_event_column(&columns, "model", "TEXT NOT NULL DEFAULT 'unknown'")?;
        self.ensure_usage_event_column(
            &columns,
            "tokenizer_backend",
            "TEXT NOT NULL DEFAULT 'chars_div_4'",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "accuracy",
            "TEXT NOT NULL DEFAULT 'heuristic_estimate'",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "baseline_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        )?;
        self.ensure_usage_event_column(&columns, "confidence", "TEXT NOT NULL DEFAULT 'inferred'")?;
        self.ensure_usage_event_column(
            &columns,
            "calculation_trace",
            "TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "accounting_layer",
            "TEXT NOT NULL DEFAULT 'modeled_avoidance'",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "estimate_method",
            "TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4'",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "denominator_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        )?;
        self.ensure_usage_event_column(&columns, "baseline_identity", "TEXT NOT NULL DEFAULT ''")?;
        self.ensure_usage_event_column(
            &columns,
            "baseline_fingerprint",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        self.ensure_usage_event_column(
            &columns,
            "dedupe_scope",
            "TEXT NOT NULL DEFAULT 'session'",
        )?;
        self.ensure_usage_event_column(&columns, "created_at", "TEXT")?;
        self.connection.execute(
            "
            UPDATE usage_events
            SET accounting_layer = 'observed_delta',
                denominator_kind = 'full_file',
                dedupe_scope = 'event'
            WHERE token_savings_bucket = 'full_file_compression'
              AND (
                accounting_layer != 'observed_delta'
                OR denominator_kind != 'full_file'
                OR dedupe_scope != 'event'
              )
            ",
            [],
        )?;
        self.connection.execute(
            "UPDATE usage_events SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL OR created_at = ''",
            [],
        )?;
        Ok(())
    }

    /// Add one usage event metadata column when it is absent.
    fn ensure_usage_event_column(
        &self,
        columns: &[String],
        name: &str,
        definition: &str,
    ) -> DbResult<()> {
        if columns.iter().any(|column| column == name) {
            return Ok(());
        }
        self.connection.execute(
            &format!("ALTER TABLE usage_events ADD COLUMN {name} {definition}"),
            [],
        )?;
        Ok(())
    }

    /// Add optional symbol metadata columns to older databases.
    fn ensure_symbol_metadata_columns(&self) -> DbResult<()> {
        let mut statement = self.connection.prepare("PRAGMA table_info(symbols)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = Vec::new();
        for row in rows {
            columns.push(row?);
        }
        if !columns.iter().any(|column| column == "exported") {
            self.connection.execute(
                "ALTER TABLE symbols ADD COLUMN exported INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.iter().any(|column| column == "documentation") {
            self.connection
                .execute("ALTER TABLE symbols ADD COLUMN documentation TEXT", [])?;
        }
        Ok(())
    }

    /// Drop an in-progress generated summary table that lacks multi-level keys.
    fn reset_legacy_summary_schema(&self) -> DbResult<()> {
        let exists = self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'summaries'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(());
        }
        let mut statement = self.connection.prepare("PRAGMA table_info(summaries)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut has_subject = false;
        for column in columns {
            if column? == "subject" {
                has_subject = true;
                break;
            }
        }
        if !has_subject {
            self.connection.execute("DROP TABLE summaries", [])?;
        }
        Ok(())
    }

    /// Upsert a full scan result and mark previously seen missing paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_scan(&mut self, nodes: &[Node]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("UPDATE nodes SET exists_now = 0", [])?;
        for node in nodes {
            upsert_node(&transaction, node)?;
        }
        transaction.execute(
            "DELETE FROM symbol_relations WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM symbols WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM source_parse_metadata WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM file_texts WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Upsert a partial scan result without marking unrelated paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn upsert_scan_nodes(&mut self, nodes: &[Node]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        for node in nodes {
            upsert_node(&transaction, node)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Mark paths and their descendants absent after filesystem delete events.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn mark_paths_absent(&mut self, paths: &[String]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        for path in paths {
            if path == "." || path.is_empty() {
                continue;
            }
            let descendant_pattern = sqlite_descendant_pattern(path);
            transaction.execute(
                "UPDATE nodes SET exists_now = 0 WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
                params![path, descendant_pattern],
            )?;
            transaction.execute(
                "DELETE FROM symbol_relations WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
                params![path, descendant_pattern],
            )?;
            transaction.execute(
                "DELETE FROM symbols WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
                params![path, descendant_pattern],
            )?;
            transaction.execute(
                "DELETE FROM source_parse_metadata WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
                params![path, descendant_pattern],
            )?;
            transaction.execute(
                "DELETE FROM file_texts WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
                params![path, descendant_pattern],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Replace indexed text for scanned file paths.
    ///
    /// `paths` should contain every file path considered by the scan batch.
    /// Existing indexed text for those paths is cleared first so binary,
    /// deleted, or no-longer-UTF-8 files cannot leave stale searchable content.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_file_texts_for_paths(
        &mut self,
        paths: &[String],
        texts: &[IndexedFileText],
    ) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        for path in paths {
            transaction.execute("DELETE FROM file_texts WHERE path = ?1", [path])?;
        }
        for text in texts {
            upsert_file_text(&transaction, text)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Load one indexed text row by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_text(&self, path: &str) -> DbResult<Option<IndexedFileText>> {
        let mut statement = self.connection.prepare(
            "
            SELECT path, content_hash, byte_count, line_count, content
            FROM file_texts
            WHERE path = ?1
            ",
        )?;
        let mut rows = statement.query([path])?;
        rows.next()?.map(file_text_from_row).transpose()
    }

    /// Load indexed text rows for search.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Regex and fuzzy searches pass `None` and still use the persisted text
    /// index instead of reopening source files from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_texts_for_search(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
    ) -> DbResult<Vec<IndexedFileText>> {
        let mut texts = Vec::new();
        self.visit_file_texts_for_search(literal_pattern, case_sensitive, |text| {
            texts.push(text);
            Ok(true)
        })?;
        Ok(texts)
    }

    /// Visit indexed text rows for search without materializing all rows.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Returning `false` from `visitor` stops iteration early.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_texts_for_search<F>(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(IndexedFileText) -> DbResult<bool>,
    {
        if let Some(pattern) = literal_pattern.filter(|pattern| !pattern.is_empty()) {
            if case_sensitive {
                let mut statement = self.connection.prepare(
                    "
                    SELECT path, content_hash, byte_count, line_count, content
                    FROM file_texts
                    WHERE instr(content, ?1) > 0
                    ORDER BY path
                    ",
                )?;
                let mut rows = statement.query([pattern])?;
                while let Some(row) = rows.next()? {
                    if !visitor(file_text_from_row(row)?)? {
                        return Ok(());
                    }
                }
            } else {
                let pattern = pattern.to_ascii_lowercase();
                let mut statement = self.connection.prepare(
                    "
                    SELECT path, content_hash, byte_count, line_count, content
                    FROM file_texts
                    WHERE instr(lower(content), ?1) > 0
                    ORDER BY path
                    ",
                )?;
                let mut rows = statement.query([pattern])?;
                while let Some(row) = rows.next()? {
                    if !visitor(file_text_from_row(row)?)? {
                        return Ok(());
                    }
                }
            }
        } else {
            let mut statement = self.connection.prepare(
                "
                SELECT path, content_hash, byte_count, line_count, content
                FROM file_texts
                ORDER BY path
                ",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                if !visitor(file_text_from_row(row)?)? {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Count files with persisted UTF-8 text for indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_count(&self) -> DbResult<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM file_texts", [], |row| {
                row.get::<_, i64>(0)
            })?;
        count_to_usize("file_texts", count)
    }

    /// Sum persisted UTF-8 source bytes used by indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_byte_count(&self) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COALESCE(SUM(byte_count), 0) FROM file_texts",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        count_to_usize("file_text_bytes", count)
    }

    /// Persist the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn set_project_root(&self, root: &Path) -> DbResult<()> {
        let value = normalize_metadata_path(root);
        self.connection.execute(
            "
            INSERT INTO metadata(key, value)
            VALUES('project_root', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            ",
            [value],
        )?;
        Ok(())
    }

    /// Load the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn project_root(&self) -> DbResult<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'project_root'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    /// Replace the symbol graph for a file path.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_symbol_graph(&mut self, graph: &SymbolGraph) -> DbResult<()> {
        let metadata = SourceParseMetadata::from_graph(graph);
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM symbols WHERE path = ?1", [&graph.path])?;
        transaction.execute(
            "DELETE FROM symbol_relations WHERE path = ?1",
            [&graph.path],
        )?;
        transaction.execute(
            "
            INSERT INTO source_parse_metadata(
                path,
                language,
                parser,
                symbol_count,
                relation_count,
                updated_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
            ON CONFLICT(path) DO UPDATE SET
                language = excluded.language,
                parser = excluded.parser,
                symbol_count = excluded.symbol_count,
                relation_count = excluded.relation_count,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                metadata.path,
                metadata.language.as_deref(),
                metadata.parser.to_string(),
                usize_to_i64(metadata.symbol_count),
                usize_to_i64(metadata.relation_count),
            ],
        )?;
        let node_id = transaction
            .query_row(
                "SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1",
                [&graph.path],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        for symbol in &graph.symbols {
            transaction.execute(
                "
                INSERT INTO symbols(
                    path,
                    language,
                    name,
                    kind,
                    signature,
                    exported,
                    documentation,
                    line_start,
                    line_end,
                    parent,
                    parser,
                    detail
                )
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ",
                params![
                    symbol.path,
                    symbol.language.as_deref(),
                    symbol.name,
                    symbol.kind.to_string(),
                    symbol.signature,
                    symbol.exported,
                    symbol.documentation.as_deref(),
                    usize_to_i64(symbol.line_start),
                    usize_to_i64(symbol.line_end),
                    symbol.parent.as_deref(),
                    symbol.parser.to_string(),
                    symbol.detail.as_deref(),
                ],
            )?;
        }
        for relation in &graph.relations {
            transaction.execute(
                "
                INSERT INTO symbol_relations(
                    path,
                    source_name,
                    target_name,
                    kind,
                    line,
                    context,
                    parser
                )
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    relation.path,
                    relation.source_name,
                    relation.target_name,
                    relation.kind.to_string(),
                    usize_to_i64(relation.line),
                    relation.context,
                    relation.parser.to_string(),
                ],
            )?;
        }
        if let Some(node_id) = node_id {
            replace_symbol_search_summary(
                &transaction,
                node_id,
                symbol_search_summary(graph).as_deref(),
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Clear source-derived intelligence for one live file path.
    ///
    /// This removes symbols, relations, and the node-level content summary so
    /// skipped or failed parser work cannot leave stale source facts visible.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_source_index_for_path(&self, path: &str) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection
            .execute("DELETE FROM symbols WHERE path = ?1", [path])?;
        self.connection
            .execute("DELETE FROM symbol_relations WHERE path = ?1", [path])?;
        self.connection
            .execute("DELETE FROM source_parse_metadata WHERE path = ?1", [path])?;
        self.connection.execute(
            "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND (
                    (summary_level = 'node' AND subject = '')
                    OR (summary_level = 'search' AND subject = 'symbols')
                  )
            ",
            [node_id],
        )?;
        Ok(())
    }

    /// Clear symbols and relations for one live file path while preserving node summaries.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn clear_symbol_graph_for_path(&self, path: &str) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection
            .execute("DELETE FROM symbols WHERE path = ?1", [path])?;
        self.connection
            .execute("DELETE FROM symbol_relations WHERE path = ?1", [path])?;
        self.connection
            .execute("DELETE FROM source_parse_metadata WHERE path = ?1", [path])?;
        self.connection.execute(
            "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'search'
              AND subject = 'symbols'
            ",
            [node_id],
        )?;
        Ok(())
    }

    /// Persist an observed one-line summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_node_summary(&self, path: &str, summary: &str) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection.execute(
            "
            INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
            VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
                summary_level = 'node',
                subject = '',
                summary = excluded.summary,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![node_id, summary],
        )?;
        Ok(())
    }

    /// Remove the observed node-level summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_node_summary(&self, path: &str) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection.execute(
            "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'node'
              AND subject = ''
            ",
            [node_id],
        )?;
        Ok(())
    }

    /// Load symbols filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE path = ?1 AND (name LIKE ?2 OR signature LIKE ?2 OR documentation LIKE ?2)
                ORDER BY path, line_start, name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE path = ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE name LIKE ?1 OR signature LIKE ?1 OR documentation LIKE ?1 OR path LIKE ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                ORDER BY path, line_start, name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_kinds(
        &self,
        file: &str,
        kinds: &[SymbolKind],
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let max_rows = usize_to_i64(limit.max(1));
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE path = ?1 AND kind IN ({placeholders})
            ORDER BY path, line_start, name
            LIMIT {max_rows}
            "
        );
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        self.query_symbols(&sql, params_from_iter(values.iter()))
    }

    /// Count symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbols_by_kinds(&self, file: &str, kinds: &[SymbolKind]) -> DbResult<usize> {
        if kinds.is_empty() {
            return Ok(0);
        }
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql =
            format!("SELECT COUNT(*) FROM symbols WHERE path = ?1 AND kind IN ({placeholders})");
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values.iter()), |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count indexed symbols grouped by exact name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_name_counts(&self, names: &[String]) -> DbResult<HashMap<String, usize>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "SELECT name, COUNT(*) FROM symbols WHERE name IN ({placeholders}) GROUP BY name"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(names.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = HashMap::new();
        for row in rows {
            let (name, count) = row?;
            counts.insert(name, i64_to_usize(count));
        }
        Ok(counts)
    }

    /// Load symbols with exact names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_names(&self, names: &[String]) -> DbResult<Vec<CodeSymbol>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE name IN ({placeholders})
            ORDER BY path, line_start, name
            "
        );
        self.query_symbols(&sql, params_from_iter(names.iter()))
    }

    /// Load exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_exported_symbol_names_for_path(
        &self,
        file: &str,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT name
            FROM symbols
            WHERE path = ?1 AND exported = 1
            ORDER BY name
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![file, max_rows], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }

    /// Count exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn exported_symbol_count_for_path(&self, file: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT name) FROM symbols WHERE path = ?1 AND exported = 1",
            [file],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load one symbol by exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_by_name(&self, file: &str, name: &str) -> DbResult<Option<CodeSymbol>> {
        let mut symbols = self.load_symbols(Some(file), Some(name), 100)?;
        symbols.retain(|symbol| symbol.name == name);
        Ok(symbols.into_iter().next())
    }

    /// Load all symbols with an exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_exact_file_and_name(
        &self,
        file: &str,
        name: &str,
    ) -> DbResult<Vec<CodeSymbol>> {
        self.query_symbols(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE path = ?1 AND name = ?2
            ORDER BY line_start, line_end, kind, parent
            ",
            params![file, name],
        )
    }

    /// Load one existing node with purpose state by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_node_by_path(&self, path: &str) -> DbResult<Option<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1 AND n.path = ?1
            ",
        )?;
        let row = statement
            .query_row([path], |row| {
                let kind_value: String = row.get(1)?;
                let source_value: String = row.get(9)?;
                let status_value: String = row.get(10)?;
                Ok((
                    row.get::<_, String>(0)?,
                    kind_value,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<u64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    source_value,
                    status_value,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .optional()?;
        row.map(indexed_node_from_parts).transpose()
    }

    /// Load existing nodes for exact repository paths.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes_by_paths(&self, paths: &[String]) -> DbResult<Vec<IndexedNode>> {
        let mut unique_paths = paths.to_vec();
        unique_paths.sort();
        unique_paths.dedup();
        let mut nodes = Vec::new();
        for path in unique_paths {
            if let Some(node) = self.load_node_by_path(&path)? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    /// Load symbol relations filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1 AND (source_name LIKE ?2 OR target_name LIKE ?2 OR context LIKE ?2)
                ORDER BY path, line, source_name, target_name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE source_name LIKE ?1 OR target_name LIKE ?1 OR context LIKE ?1 OR path LIKE ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                ORDER BY path, line, source_name, target_name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        self.query_relations(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY path, line, source_name, target_name
            LIMIT ?3
            ",
            params![file, kind.to_string(), max_rows],
        )
    }

    /// Count symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT target_name
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY target_name
            LIMIT ?3
            ",
        )?;
        let rows = statement.query_map(params![file, kind.to_string(), max_rows], |row| {
            row.get::<_, String>(0)
        })?;
        let mut targets = Vec::new();
        for row in rows {
            targets.push(row?);
        }
        Ok(targets)
    }

    /// Count distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT target_name) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load call relations targeting any of the requested symbol names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_call_relations_to_targets(
        &self,
        target_names: &[String],
        limit_per_target: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        if target_names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, target_names.len());
        let limit_placeholder = target_names.len() + 1;
        let sql = format!(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM (
                SELECT
                    path,
                    source_name,
                    target_name,
                    kind,
                    line,
                    context,
                    parser,
                    ROW_NUMBER() OVER (
                        PARTITION BY target_name
                        ORDER BY path, line, source_name, target_name
                    ) AS target_row
                FROM symbol_relations
                WHERE kind = 'calls' AND target_name IN ({placeholders})
            )
            WHERE target_row <= ?{limit_placeholder}
            ORDER BY path, line, source_name, target_name
            "
        );
        let mut values = target_names
            .iter()
            .map(|target| Value::Text(target.clone()))
            .collect::<Vec<_>>();
        values.push(Value::Integer(usize_to_i64(limit_per_target.max(1))));
        let mut relations = self.query_relations(&sql, params_from_iter(values.iter()))?;
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.kind == right.kind
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Load import relations whose persisted target text mentions any term.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_import_relations_matching_targets(
        &self,
        terms: &[String],
        limit_per_term: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let mut unique_terms = terms.to_vec();
        unique_terms.sort();
        unique_terms.dedup();
        let mut relations = Vec::new();
        for term in unique_terms.iter().filter(|term| !term.trim().is_empty()) {
            let mut term_relations = self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE kind = 'imports' AND target_name LIKE ?1 ESCAPE '\\'
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![
                    sqlite_like_pattern(term),
                    usize_to_i64(limit_per_term.max(1))
                ],
            )?;
            relations.append(&mut term_relations);
        }
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.kind == right.kind
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Count persisted symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count(&self) -> DbResult<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbol relations.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_relation_count(&self) -> DbResult<usize> {
        let count =
            self.connection
                .query_row("SELECT COUNT(*) FROM symbol_relations", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        count_to_usize("symbol_relations", count)
    }

    /// Count persisted symbols for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count_for_path(&self, path: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbols for a batch of file paths.
    ///
    /// Paths without symbols are omitted from the returned map.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_counts_for_paths(&self, paths: &[String]) -> DbResult<HashMap<String, usize>> {
        let mut counts = HashMap::new();
        for chunk in paths.chunks(900) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT path, COUNT(*) FROM symbols WHERE path IN ({placeholders}) GROUP BY path"
            );
            let mut statement = self.connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                let path = row.get::<_, String>(0)?;
                let count = row.get::<_, i64>(1)?;
                Ok((path, i64_to_usize(count)))
            })?;
            for row in rows {
                let (path, count) = row?;
                counts.insert(path, count);
            }
        }
        Ok(counts)
    }

    /// Return distinct parser strategies that produced symbols for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_parser_kinds_for_path(&self, path: &str) -> DbResult<Vec<ParserKind>> {
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT parser
            FROM symbols
            WHERE path = ?1
            ORDER BY parser
            ",
        )?;
        let rows = statement.query_map([path], |row| {
            Ok(ParserKind::from_db(&row.get::<_, String>(0)?))
        })?;
        let mut parsers = Vec::new();
        for row in rows {
            parsers.push(row?);
        }
        Ok(parsers)
    }

    /// Load file-level parser metadata for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_source_parse_metadata(&self, path: &str) -> DbResult<Option<SourceParseMetadata>> {
        self.connection
            .query_row(
                "
                SELECT path, language, parser, symbol_count, relation_count
                FROM source_parse_metadata
                WHERE path = ?1
                ",
                [path],
                |row| {
                    let symbol_count = row.get::<_, i64>(3)?;
                    let relation_count = row.get::<_, i64>(4)?;
                    Ok(SourceParseMetadata {
                        path: row.get(0)?,
                        language: row.get(1)?,
                        parser: ParserKind::from_db(&row.get::<_, String>(2)?),
                        symbol_count: i64_to_usize(symbol_count),
                        relation_count: i64_to_usize(relation_count),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Load the maximum indexed symbol end line for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn max_symbol_end_line_for_path(&self, path: &str) -> DbResult<usize> {
        let line = self.connection.query_row(
            "SELECT COALESCE(MAX(line_end), 0) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(line))
    }

    /// Query symbols with a caller-provided statement and parameters.
    fn query_symbols<P>(&self, sql: &str, params: P) -> DbResult<Vec<CodeSymbol>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            Ok(CodeSymbol {
                path: row.get(0)?,
                language: row.get(1)?,
                name: row.get(2)?,
                kind: SymbolKind::from_db(&row.get::<_, String>(3)?),
                signature: row.get(4)?,
                line_start: i64_to_usize(row.get::<_, i64>(5)?),
                line_end: i64_to_usize(row.get::<_, i64>(6)?),
                parent: row.get(7)?,
                parser: ParserKind::from_db(&row.get::<_, String>(8)?),
                detail: row.get(9)?,
                exported: row.get::<_, i64>(10)? != 0,
                documentation: row.get(11)?,
            })
        })?;
        let mut symbols = Vec::new();
        for row in rows {
            symbols.push(row?);
        }
        Ok(symbols)
    }

    /// Query relations with a caller-provided statement and parameters.
    fn query_relations<P>(&self, sql: &str, params: P) -> DbResult<Vec<SymbolRelation>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            let kind_value: String = row.get(3)?;
            let relation_kind = RelationKind::from_db(&kind_value).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(format!(
                        "invalid relation kind {kind_value}"
                    ))),
                )
            })?;
            Ok(SymbolRelation {
                path: row.get(0)?,
                source_name: row.get(1)?,
                target_name: row.get(2)?,
                kind: relation_kind,
                line: i64_to_usize(row.get::<_, i64>(4)?),
                context: row.get(5)?,
                parser: ParserKind::from_db(&row.get::<_, String>(6)?),
            })
        })?;
        let mut relations = Vec::new();
        for row in rows {
            relations.push(row?);
        }
        Ok(relations)
    }

    /// Persist a purpose for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_purpose(&self, path: &str, purpose: &str, source: PurposeSource) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection.execute(
            "
            INSERT INTO purposes(node_id, purpose, source, status, updated_at)
            VALUES(?1, ?2, ?3, 'approved', CURRENT_TIMESTAMP)
            ON CONFLICT(node_id) DO UPDATE SET
                purpose = excluded.purpose,
                source = excluded.source,
                status = 'approved',
                updated_at = CURRENT_TIMESTAMP
            ",
            params![node_id, purpose, source.to_string()],
        )?;
        Ok(())
    }

    /// Persist a non-approved purpose suggestion for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_suggested_purpose(&self, path: &str, purpose: &str) -> DbResult<()> {
        let node_id = self.node_id_for_path(path)?;
        self.connection.execute(
            "
            INSERT INTO purposes(node_id, purpose, source, status, updated_at)
            VALUES(?1, ?2, 'generated', 'suggested', CURRENT_TIMESTAMP)
            ON CONFLICT(node_id) DO UPDATE SET
                purpose = excluded.purpose,
                source = 'generated',
                status = 'suggested',
                updated_at = CURRENT_TIMESTAMP
            ",
            params![node_id, purpose],
        )?;
        Ok(())
    }

    /// Load a node id for a repository path.
    fn node_id_for_path(&self, path: &str) -> DbResult<i64> {
        self.connection
            .query_row(
                "SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1",
                [path],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| DbError::PathNotIndexed {
                path: path.to_string(),
            })
    }

    /// Load existing nodes with purpose state.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes(&self) -> DbResult<Vec<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let kind_value: String = row.get(1)?;
            let source_value: String = row.get(9)?;
            let status_value: String = row.get(10)?;
            Ok((
                row.get::<_, String>(0)?,
                kind_value,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<u64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                source_value,
                status_value,
                row.get::<_, Option<String>>(11)?,
            ))
        })?;
        let mut nodes = Vec::new();
        for row in rows {
            let (
                path,
                kind_value,
                parent_path,
                extension,
                language,
                size_bytes,
                mtime_ns,
                content_hash,
                purpose,
                source_value,
                status_value,
                summary,
            ) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            let source = parse_source(&source_value)?;
            let status =
                PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
                    field: "status",
                    value: status_value,
                })?;
            nodes.push(IndexedNode {
                node: Node {
                    path: path.clone(),
                    kind,
                    parent_path,
                    extension,
                    language,
                    size_bytes,
                    mtime_ns,
                    content_hash,
                },
                purpose: Purpose {
                    path,
                    purpose,
                    source,
                    status,
                },
                summary,
            });
        }
        Ok(nodes)
    }

    /// Load a bounded ranked node list directly from `SQLite`.
    ///
    /// This is the hot path for agent orientation commands. It keeps large
    /// repositories from materializing every indexed path just to answer a
    /// top-N folder or file query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_ranked_nodes(
        &self,
        query: &str,
        kind: NodeKind,
        folder: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> DbResult<Vec<IndexedNode>> {
        let terms = normalize_query_terms(query);
        let score_expression = ranked_score_expression(terms.len());
        let mut sql = format!(
            "
            SELECT path, kind, parent_path, extension, language, size_bytes, mtime_ns,
                   content_hash, purpose, source, status, summary
            FROM (
                SELECT
                    n.path,
                    n.kind,
                    n.parent_path,
                    n.extension,
                    n.language,
                    n.size_bytes,
                    n.mtime_ns,
                    n.content_hash,
                    p.purpose,
                    p.source,
                    p.status,
                    s.summary,
                    {score_expression} AS score
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                LEFT JOIN summaries s ON s.node_id = n.id
                    AND s.summary_level = 'node'
                    AND s.subject = ''
                LEFT JOIN summaries symbol_summaries ON symbol_summaries.node_id = n.id
                    AND symbol_summaries.summary_level = 'search'
                    AND symbol_summaries.subject = 'symbols'
                WHERE n.exists_now = 1
                  AND n.kind = ?
            "
        );
        let mut values = Vec::new();
        for term in &terms {
            let pattern = sqlite_like_pattern(term);
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern));
        }
        values.push(Value::from(kind.to_string()));
        if kind == NodeKind::File
            && let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".")
        {
            sql.push_str(" AND (n.parent_path = ? OR n.parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(
            "
            )
            WHERE score > 0
            ORDER BY score DESC, path
            LIMIT ?
            OFFSET ?
            ",
        );
        values.push(Value::from(usize_to_i64(limit.max(1))));
        values.push(Value::from(usize_to_i64(offset)));

        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(indexed_node_from_sql_row(row)?);
        }
        Ok(nodes)
    }

    /// Sum indexed source bytes represented by file nodes.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or the aggregate cannot fit in `usize`.
    pub fn source_file_byte_count(&self, folder: Option<&str>) -> DbResult<usize> {
        let mut sql = String::from(
            "
            SELECT COALESCE(SUM(COALESCE(size_bytes, 0)), 0)
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("source_file_bytes", count)
    }

    /// Visit indexed file paths and source sizes for exact token baselines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_token_estimates<F>(
        &self,
        folder: Option<&str>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(String, Option<u64>) -> DbResult<bool>,
    {
        let mut sql = String::from(
            "
            SELECT path, size_bytes
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(" ORDER BY path");
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        while let Some(row) = rows.next()? {
            if !visitor(row.get::<_, String>(0)?, row.get::<_, Option<u64>>(1)?)? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Build unresolved health findings without loading the full node table.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings(
        &self,
        resolved_ids: &[String],
    ) -> DbResult<Vec<HealthFinding>> {
        let mut findings = Vec::new();
        self.visit_unresolved_health_findings(resolved_ids, |finding| {
            findings.push(finding);
            Ok(true)
        })?;
        Ok(findings)
    }

    /// Build a bounded unresolved health findings page.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings_page(
        &self,
        resolved_ids: &[String],
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        let mut unfiltered_total = 0_usize;
        let mut total = 0_usize;
        let mut findings = Vec::new();

        for spec in PURPOSE_HEALTH_SPECS {
            unfiltered_total +=
                self.count_purpose_status_findings(spec, None, resolved_ids, HealthScope::all())?;
        }

        let scope = query.scope;
        if scope.high_impact_queue() && query.category.is_none() {
            let matching_count = if query
                .severity
                .is_none_or(|severity| severity == Severity::Warning)
            {
                self.count_purpose_lifecycle_findings(
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                )?
            } else {
                0
            };
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_purpose_lifecycle_findings_page(
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        } else {
            for spec in PURPOSE_HEALTH_SPECS {
                if !purpose_health_spec_matches_query(spec, query) {
                    continue;
                }

                let matching_count = self.count_purpose_status_findings(
                    spec,
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                )?;
                if !query.summary_only
                    && findings.len() < query.limit
                    && total + matching_count > query.start_index
                {
                    let local_start = query.start_index.saturating_sub(total);
                    let local_limit = query.limit - findings.len();
                    findings.extend(self.load_purpose_status_findings_page(
                        spec,
                        query.path_prefix.as_deref(),
                        resolved_ids,
                        scope,
                        local_start,
                        local_limit,
                    )?);
                }
                total += matching_count;
            }
        }

        for category in [
            AGENT_REVIEW_REQUIRED_CATEGORY,
            "duplicate-purpose",
            "repeated-temporary-folder",
        ] {
            let unfiltered_scope = if category == AGENT_REVIEW_REQUIRED_CATEGORY {
                HealthScope::purpose_strict()
            } else {
                HealthScope::all()
            };
            let unfiltered_count = self.count_structural_health_findings(
                category,
                None,
                resolved_ids,
                unfiltered_scope,
            )?;
            unfiltered_total += unfiltered_count;
            if !health_category_matches_query(category, Severity::Warning, query) {
                continue;
            }
            let matching_count = self.count_structural_health_findings(
                category,
                query.path_prefix.as_deref(),
                resolved_ids,
                scope,
            )?;
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_structural_health_findings_page(
                    category,
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        }
        Ok(HealthFindingsPage {
            total,
            unfiltered_total,
            returned: findings.len(),
            start_index: query.start_index,
            limit: query.limit,
            findings,
        })
    }

    /// Visit unresolved health findings without materializing the full table.
    fn visit_unresolved_health_findings<F>(
        &self,
        resolved_ids: &[String],
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let resolved = resolved_ids.iter().cloned().collect::<HashSet<_>>();
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[0], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[1], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[2], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_agent_review_required_findings(&resolved, &mut visitor)? {
            return Ok(());
        }
        self.visit_structural_health_findings(&resolved, &mut visitor)
    }

    /// Visit structural health findings that are not simple purpose statuses.
    fn visit_structural_health_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        if !self.visit_duplicate_purpose_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_repeated_temp_folder_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        Ok(())
    }

    /// Build findings for one purpose lifecycle status.
    fn visit_purpose_status_findings<F>(
        &self,
        spec: PurposeHealthSpec,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let mut statement = self.connection.prepare(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = ?1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([spec.status], |row| row.get::<_, String>(0))?;
        for row in rows {
            let path = row?;
            let finding = HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_lifecycle_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) =
            purpose_lifecycle_where_clause(path_prefix, resolved_ids, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_lifecycle_count", count)
    }

    /// Load one globally ordered purpose lifecycle page directly from `SQLite`.
    fn load_purpose_lifecycle_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_lifecycle_where_clause(path_prefix, resolved_ids, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = purpose_default_queue_order_expression("n", "p");
        let sql = format!(
            "
            SELECT n.path, p.status
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, status) = row?;
            let spec = purpose_health_spec_for_status(&status)?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_status_findings(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) =
            purpose_status_where_clause(spec, path_prefix, resolved_ids, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_status_count", count)
    }

    /// Load one bounded unresolved purpose lifecycle page directly from `SQLite`.
    fn load_purpose_status_findings_page(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_status_where_clause(spec, path_prefix, resolved_ids, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = if scope.high_impact_queue() {
            purpose_default_queue_order_expression("n", "p")
        } else {
            "n.path".to_string()
        };
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            let path = row?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved structural health findings directly in `SQLite`.
    fn count_structural_health_findings(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        match category {
            AGENT_REVIEW_REQUIRED_CATEGORY => {
                self.count_agent_review_required_findings(path_prefix, resolved_ids, scope)
            }
            "duplicate-purpose" => {
                self.count_duplicate_purpose_findings(path_prefix, resolved_ids, scope)
            }
            "repeated-temporary-folder" => {
                self.count_repeated_temp_folder_findings(path_prefix, resolved_ids, scope)
            }
            _ => Ok(0),
        }
    }

    /// Load a bounded unresolved structural health page directly from `SQLite`.
    fn load_structural_health_findings_page(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        match category {
            AGENT_REVIEW_REQUIRED_CATEGORY => self.load_agent_review_required_findings_page(
                path_prefix,
                resolved_ids,
                scope,
                start_index,
                limit,
            ),
            "duplicate-purpose" => self.load_duplicate_purpose_findings_page(
                path_prefix,
                resolved_ids,
                scope,
                start_index,
                limit,
            ),
            "repeated-temporary-folder" => self.load_repeated_temp_folder_findings_page(
                path_prefix,
                resolved_ids,
                scope,
                start_index,
                limit,
            ),
            _ => Ok(Vec::new()),
        }
    }

    /// Visit approved navigation-critical purposes that still need agent review.
    fn visit_agent_review_required_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let high_impact = high_impact_file_path_expression("lower(n.path)");
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = 'approved'
              AND p.source NOT IN ({reviewed_sources})
              AND (n.kind = 'folder' OR (n.kind = 'file' AND {high_impact}))
            ORDER BY CASE WHEN n.kind = 'folder' THEN 0 ELSE 1 END, n.path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let finding = agent_review_required_finding(row?);
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count approved navigation-critical purposes that still need agent review.
    fn count_agent_review_required_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            AGENT_REVIEW_REQUIRED_CATEGORY,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_agent_review_required_count", count)
    }

    /// Load approved navigation-critical purposes that still need agent review.
    fn load_agent_review_required_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            AGENT_REVIEW_REQUIRED_CATEGORY,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT path
            FROM findings
            {where_clause}
            ORDER BY CASE WHEN kind = 'folder' THEN 0 ELSE 1 END, path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            findings.push(agent_review_required_finding(row?));
        }
        Ok(findings)
    }

    /// Count duplicate-purpose findings directly in `SQLite`.
    fn count_duplicate_purpose_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            "duplicate-purpose",
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_duplicate_purpose_count", count)
    }

    /// Load a bounded duplicate-purpose findings page directly in `SQLite`.
    fn load_duplicate_purpose_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            "duplicate-purpose",
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, kind, related_path
            FROM findings
            {where_clause}
            ORDER BY kind, lower(purpose), path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, kind_value, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            findings.push(HealthFinding {
                id: finding_id("duplicate-purpose", &path, Some(&related_path)),
                severity: Severity::Warning,
                category: "duplicate-purpose".to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation:
                    "Review whether these paths duplicate responsibility or need clearer purposes."
                        .to_string(),
            });
        }
        Ok(findings)
    }

    /// Count repeated temporary-folder findings directly in `SQLite`.
    fn count_repeated_temp_folder_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let mut total = 0_usize;
        for bucket in TEMP_FOLDER_BUCKETS {
            total += self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolved_ids,
                scope,
            )?;
        }
        Ok(total)
    }

    /// Count one repeated temporary-folder bucket directly in `SQLite`.
    fn count_repeated_temp_folder_bucket_findings(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            "repeated-temporary-folder",
            path_prefix,
            resolved_ids,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_repeated_temp_count", count)
    }

    /// Load a bounded repeated temporary-folder findings page directly in `SQLite`.
    fn load_repeated_temp_folder_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut total = 0_usize;
        let mut findings = Vec::new();
        for bucket in TEMP_FOLDER_BUCKETS {
            let matching_count = self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolved_ids,
                scope,
            )?;
            if findings.len() < limit && total + matching_count > start_index {
                let local_start = start_index.saturating_sub(total);
                let local_limit = limit - findings.len();
                findings.extend(self.load_repeated_temp_folder_bucket_findings_page(
                    bucket,
                    path_prefix,
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
            if findings.len() >= limit {
                break;
            }
        }
        Ok(findings)
    }

    /// Load one repeated temporary-folder bucket directly in `SQLite`.
    fn load_repeated_temp_folder_bucket_findings_page(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            "repeated-temporary-folder",
            path_prefix,
            resolved_ids,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY n.path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, related_path
            FROM findings
            {where_clause}
            ORDER BY path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, related_path) = row?;
            findings.push(HealthFinding {
                id: finding_id("repeated-temporary-folder", &path, Some(&related_path)),
                severity: Severity::Warning,
                category: "repeated-temporary-folder".to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                recommendation:
                    "Consolidate temporary/generated output roots or add an allowlist rationale."
                        .to_string(),
            });
        }
        Ok(findings)
    }

    /// Visit duplicate-purpose health findings through grouped SQL candidates.
    fn visit_duplicate_purpose_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       p.purpose,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.purpose IS NOT NULL
            )
            SELECT path, kind, purpose, related_path
            FROM duplicate_rows
            WHERE duplicate_count > 1
              AND duplicate_rank > 1
            ORDER BY kind, lower(purpose), path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (path, kind_value, _purpose, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value.clone(),
            })?;
            let finding = HealthFinding {
                id: finding_id("duplicate-purpose", &path, Some(&related_path)),
                severity: Severity::Warning,
                category: "duplicate-purpose".to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation:
                    "Review whether these paths duplicate responsibility or need clearer purposes."
                        .to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Visit repeated temporary/generated folder findings.
    fn visit_repeated_temp_folder_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        for bucket in TEMP_FOLDER_BUCKETS {
            let exact = bucket.to_string();
            let suffix = format!("%/{bucket}");
            let mut statement = self.connection.prepare(
                "
                SELECT path
                FROM nodes
                WHERE exists_now = 1
                  AND kind = 'folder'
                  AND (lower(path) = ?1 OR lower(path) LIKE ?2)
                ORDER BY path
                ",
            )?;
            let rows =
                statement.query_map(params![exact, suffix], |row| row.get::<_, String>(0))?;
            let mut first_path = None;
            for row in rows {
                let path = row?;
                let Some(first_path) = first_path.as_ref() else {
                    first_path = Some(path);
                    continue;
                };
                let finding = HealthFinding {
                    id: finding_id(
                        "repeated-temporary-folder",
                        &path,
                        Some(first_path.as_str()),
                    ),
                    severity: Severity::Warning,
                    category: "repeated-temporary-folder".to_string(),
                    path,
                    related_path: Some(first_path.clone()),
                    message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                    recommendation:
                        "Consolidate temporary/generated output roots or add an allowlist rationale."
                            .to_string(),
                };
                if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Compute an overview from the current index.
    ///
    /// # Errors
    ///
    /// Returns an error if the aggregate query fails or a count is invalid.
    pub fn overview(&self) -> DbResult<Overview> {
        let counts = self.connection.query_row(
            "
            SELECT
                COALESCE(SUM(CASE WHEN n.kind = 'file' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN n.kind = 'folder' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = 'missing' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = 'stale' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = 'approved' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = 'suggested' THEN 1 ELSE 0 END), 0)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        Ok(Overview {
            files: count_to_usize("files", counts.0)?,
            folders: count_to_usize("folders", counts.1)?,
            missing_purposes: count_to_usize("missing_purposes", counts.2)?,
            stale_purposes: count_to_usize("stale_purposes", counts.3)?,
            approved_purposes: count_to_usize("approved_purposes", counts.4)?,
            suggested_purposes: count_to_usize("suggested_purposes", counts.5)?,
        })
    }

    /// Record a usage event.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn record_usage(&self, event: &UsageEvent) -> DbResult<()> {
        self.connection.execute(
            "
            INSERT INTO usage_events(
                session_id,
                command,
                path,
                query,
                estimated_tokens_without_projectatlas,
                estimated_tokens_with_projectatlas,
                estimated_tokens_saved,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                calculation_trace,
                accounting_layer,
                estimate_method,
                denominator_kind,
                baseline_identity,
                baseline_fingerprint,
                dedupe_scope,
                created_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, CURRENT_TIMESTAMP)
            ",
            params![
                event.session_id,
                event.command,
                event.path,
                event.query,
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
                event.estimated_tokens_saved,
                event.token_savings_bucket,
                event.provider,
                event.model,
                event.tokenizer_backend,
                event.accuracy,
                event.baseline_kind,
                event.confidence,
                event.calculation_trace,
                event.accounting_layer,
                event.estimate_method,
                event.denominator_kind,
                event.baseline_identity,
                event.baseline_fingerprint,
                event.dedupe_scope
            ],
        )?;
        Ok(())
    }

    /// Load usage events.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub fn usage_events(&self, session_id: Option<&str>) -> DbResult<Vec<UsageEvent>> {
        let sql = if session_id.is_some() {
            "
            SELECT session_id, command, path, query, estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas, estimated_tokens_saved,
                   token_savings_bucket, provider, model, tokenizer_backend,
                   accuracy, baseline_kind, confidence, calculation_trace,
                   accounting_layer, estimate_method, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE session_id = ?1
            ORDER BY id
            "
        } else {
            "
            SELECT session_id, command, path, query, estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas, estimated_tokens_saved,
                   token_savings_bucket, provider, model, tokenizer_backend,
                   accuracy, baseline_kind, confidence, calculation_trace,
                   accounting_layer, estimate_method, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            ORDER BY id
            "
        };
        let mut statement = self.connection.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(UsageEvent {
                session_id: row.get(0)?,
                command: row.get(1)?,
                path: row.get(2)?,
                query: row.get(3)?,
                estimated_tokens_without_projectatlas: row.get(4)?,
                estimated_tokens_with_projectatlas: row.get(5)?,
                estimated_tokens_saved: row.get(6)?,
                token_savings_bucket: row.get(7)?,
                provider: row.get(8)?,
                model: row.get(9)?,
                tokenizer_backend: row.get(10)?,
                accuracy: row.get(11)?,
                baseline_kind: row.get(12)?,
                confidence: row.get(13)?,
                calculation_trace: row.get(14)?,
                accounting_layer: row.get(15)?,
                estimate_method: row.get(16)?,
                denominator_kind: row.get(17)?,
                baseline_identity: row.get(18)?,
                baseline_fingerprint: row.get(19)?,
                dedupe_scope: row.get(20)?,
            })
        };
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Load the narrow raw-event fields required for token accounting dedupe.
    fn token_accounting_events(&self, session_id: Option<&str>) -> DbResult<Vec<UsageEvent>> {
        let sql = if session_id.is_some() {
            "
            SELECT session_id, command, path, query,
                   estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas,
                   token_savings_bucket, baseline_kind, confidence,
                   accounting_layer, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            ORDER BY id
            "
        } else {
            "
            SELECT session_id, command, path, query,
                   estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas,
                   token_savings_bucket, baseline_kind, confidence,
                   accounting_layer, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            ORDER BY id
            "
        };
        let mut statement = self.connection.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(UsageEvent {
                session_id: row.get(0)?,
                command: row.get(1)?,
                path: row.get(2)?,
                query: row.get(3)?,
                estimated_tokens_without_projectatlas: row.get(4)?,
                estimated_tokens_with_projectatlas: row.get(5)?,
                estimated_tokens_saved: None,
                token_savings_bucket: row.get(6)?,
                provider: default_token_provider(),
                model: default_token_model(),
                tokenizer_backend: default_tokenizer_backend(),
                accuracy: default_token_accuracy(),
                baseline_kind: row.get(7)?,
                confidence: row.get(8)?,
                calculation_trace: default_token_trace(),
                accounting_layer: row.get(9)?,
                estimate_method: default_estimate_method(),
                denominator_kind: row.get(10)?,
                baseline_identity: row.get(11)?,
                baseline_fingerprint: row.get(12)?,
                dedupe_scope: row.get(13)?,
            })
        };
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Build a token overview.
    ///
    /// # Errors
    ///
    /// Returns an error if loading events fails.
    pub fn token_overview(&self, session_id: Option<&str>) -> DbResult<TokenOverview> {
        let sql = if session_id.is_some() {
            "
            SELECT
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer,
                     estimate_method, denominator_kind, dedupe_scope
            ORDER BY token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
        } else {
            "
            SELECT
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer,
                     estimate_method, denominator_kind, dedupe_scope
            ORDER BY token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
        };
        let mapper = |row: &rusqlite::Row<'_>| {
            let calls = row.get::<_, i64>(11)?.max(0) as u128;
            Ok(TokenBucketOverview::from_totals(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                calls,
                token_total_from_sql("estimated_tokens_without_projectatlas", row.get(12)?),
                token_total_from_sql("estimated_tokens_with_projectatlas", row.get(13)?),
            ))
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut buckets = Vec::new();
        for row in rows {
            buckets.push(row?);
        }
        let mut overview = TokenOverview::from_buckets(buckets);
        overview.apply_accounting_from_events(&self.token_accounting_events(session_id)?);
        Ok(overview)
    }

    /// Build token trend aggregates grouped by day, week, month, or year.
    ///
    /// # Errors
    ///
    /// Returns an error if the window is unsupported or loading events fails.
    pub fn token_trends(
        &self,
        session_id: Option<&str>,
        window: TokenTrendWindow,
    ) -> DbResult<TokenTrendReport> {
        let period_expr = token_trend_period_expression(window);
        let sql = if session_id.is_some() {
            format!(
                "
            SELECT
                {period_expr} AS period,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY period, token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer, estimate_method,
                     denominator_kind, dedupe_scope
            ORDER BY period, token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
            )
        } else {
            format!(
                "
            SELECT
                {period_expr} AS period,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY period, token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer, estimate_method,
                     denominator_kind, dedupe_scope
            ORDER BY period, token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
            )
        };
        let mapper = |row: &rusqlite::Row<'_>| {
            let period = row.get::<_, String>(0)?;
            let calls = row.get::<_, i64>(12)?.max(0) as u128;
            let bucket = TokenBucketOverview::from_totals(
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                calls,
                token_total_from_sql("estimated_tokens_without_projectatlas", row.get(13)?),
                token_total_from_sql("estimated_tokens_with_projectatlas", row.get(14)?),
            );
            Ok((period, bucket))
        };
        let mut statement = self.connection.prepare(&sql)?;
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut buckets_by_period = BTreeMap::<String, Vec<TokenBucketOverview>>::new();
        for row in rows {
            let (period, bucket) = row?;
            buckets_by_period.entry(period).or_default().push(bucket);
        }
        let periods = buckets_by_period
            .into_iter()
            .map(|(period, buckets)| TokenTrendPeriod::from_buckets(period, buckets))
            .collect();
        Ok(TokenTrendReport::new(
            session_id.map(ToString::to_string),
            window,
            periods,
        ))
    }

    /// Mark a deterministic health finding as agent-resolved.
    ///
    /// # Errors
    ///
    /// Returns an error if the finding is not active or persistence fails.
    pub fn resolve_health_finding(&self, resolution: &HealthResolution) -> DbResult<()> {
        let resolved_ids = self.resolved_health_ids()?;
        if !self.active_health_finding_matches(&resolved_ids, resolution)? {
            return Err(DbError::HealthFindingNotActive {
                finding_id: resolution.finding_id.clone(),
                category: resolution.category.clone(),
                path: resolution.path.clone(),
            });
        }
        self.connection.execute(
            "
            INSERT INTO health_resolutions(
                finding_id,
                category,
                path,
                related_path,
                rationale,
                resolved_by,
                resolved_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, 'agent', CURRENT_TIMESTAMP)
            ON CONFLICT(finding_id) DO UPDATE SET
                category = excluded.category,
                path = excluded.path,
                related_path = excluded.related_path,
                rationale = excluded.rationale,
                resolved_by = 'agent',
                resolved_at = CURRENT_TIMESTAMP
            ",
            params![
                resolution.finding_id,
                resolution.category,
                resolution.path,
                resolution.related_path,
                resolution.rationale,
            ],
        )?;
        Ok(())
    }

    /// Return whether the visible SQL health surface contains the exact finding.
    fn active_health_finding_matches(
        &self,
        resolved_ids: &[String],
        resolution: &HealthResolution,
    ) -> DbResult<bool> {
        const PAGE_SIZE: usize = 256;
        let mut start_index = 0_usize;
        loop {
            let page = self.unresolved_health_findings_page(
                resolved_ids,
                &HealthQuery {
                    start_index,
                    limit: PAGE_SIZE,
                    category: Some(resolution.category.clone()),
                    severity: Some(Severity::Warning),
                    path_prefix: Some(resolution.path.clone()),
                    summary_only: false,
                    scope: HealthScope::all(),
                },
            )?;
            if page.findings.iter().any(|finding| {
                finding.id == resolution.finding_id
                    && finding.category == resolution.category
                    && finding.path == resolution.path
                    && finding.related_path == resolution.related_path
            }) {
                return Ok(true);
            }
            if page.returned == 0 || start_index + page.returned >= page.total {
                return Ok(false);
            }
            start_index += page.returned;
        }
    }

    /// Load resolved health finding ids.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn resolved_health_ids(&self) -> DbResult<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT finding_id FROM health_resolutions ORDER BY finding_id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}

/// Normalize a filesystem path stored in `SQLite` metadata.
fn normalize_metadata_path(path: &Path) -> String {
    normalize_native_path_display(path)
}

/// Upsert one scanned node into an existing transaction.
fn upsert_node(transaction: &Transaction<'_>, node: &Node) -> DbResult<()> {
    let existing = transaction
        .query_row(
            "
            SELECT n.content_hash, p.status
            FROM nodes n
            LEFT JOIN purposes p ON p.node_id = n.id
            WHERE n.path = ?1
            ",
            [&node.path],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    let content_changed = existing.as_ref().is_some_and(|(old_hash, _)| {
        node.kind == NodeKind::File
            && old_hash.is_some()
            && node.content_hash.is_some()
            && old_hash != &node.content_hash
    });
    let should_mark_stale = content_changed
        && existing.as_ref().and_then(|(_, status)| status.as_deref()) == Some("approved");
    transaction.execute(
        "
        INSERT INTO nodes(path, kind, parent_path, extension, language, size_bytes, mtime_ns, content_hash, exists_now)
        VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
        ON CONFLICT(path) DO UPDATE SET
            kind = excluded.kind,
            parent_path = excluded.parent_path,
            extension = excluded.extension,
            language = excluded.language,
            size_bytes = excluded.size_bytes,
            mtime_ns = excluded.mtime_ns,
            content_hash = excluded.content_hash,
            exists_now = 1,
            last_seen_at = CURRENT_TIMESTAMP,
            last_indexed_at = CURRENT_TIMESTAMP
        ",
        params![
            node.path,
            node.kind.to_string(),
            node.parent_path,
            node.extension,
            node.language,
            node.size_bytes,
            node.mtime_ns,
            node.content_hash
        ],
    )?;
    let node_id = transaction.query_row(
        "SELECT id FROM nodes WHERE path = ?1",
        [&node.path],
        |row| row.get::<_, i64>(0),
    )?;
    transaction.execute(
        "
        INSERT INTO purposes(node_id, purpose, source, status)
        VALUES(?1, NULL, 'missing', 'missing')
        ON CONFLICT(node_id) DO NOTHING
        ",
        [node_id],
    )?;
    let summary = generate_node_summary(node);
    transaction.execute(
        "
        INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
        VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
        ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
            summary = CASE WHEN ?3 THEN excluded.summary ELSE summaries.summary END,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![node_id, summary, content_changed],
    )?;
    if should_mark_stale {
        transaction.execute(
            "
            UPDATE purposes
            SET status = 'stale',
                updated_at = CURRENT_TIMESTAMP
            WHERE node_id = ?1
            ",
            [node_id],
        )?;
    }
    Ok(())
}

/// Upsert one persisted UTF-8 source-text row for indexed search.
fn upsert_file_text(transaction: &Transaction<'_>, text: &IndexedFileText) -> DbResult<()> {
    transaction.execute(
        "
        INSERT INTO file_texts(path, content_hash, byte_count, line_count, content, updated_at)
        VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
        ON CONFLICT(path) DO UPDATE SET
            content_hash = excluded.content_hash,
            byte_count = excluded.byte_count,
            line_count = excluded.line_count,
            content = excluded.content,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![
            text.path,
            text.content_hash.as_deref(),
            usize_to_i64(text.byte_count),
            usize_to_i64(text.line_count),
            text.content
        ],
    )?;
    Ok(())
}

/// Read one persisted indexed text row.
fn file_text_from_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedFileText> {
    let byte_count = count_to_usize("file_texts.byte_count", row.get::<_, i64>(2)?)?;
    let line_count = count_to_usize("file_texts.line_count", row.get::<_, i64>(3)?)?;
    Ok(IndexedFileText {
        path: row.get(0)?,
        content_hash: row.get(1)?,
        byte_count,
        line_count,
        content: row.get(4)?,
    })
}

/// Build an indexed node from the standard node select column order.
fn indexed_node_from_sql_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedNode> {
    let kind_value: String = row.get(1)?;
    let source_value: String = row.get(9)?;
    let status_value: String = row.get(10)?;
    indexed_node_from_parts((
        row.get::<_, String>(0)?,
        kind_value,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, Option<String>>(3)?,
        row.get::<_, Option<String>>(4)?,
        row.get::<_, Option<u64>>(5)?,
        row.get::<_, Option<i64>>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
        source_value,
        status_value,
        row.get::<_, Option<String>>(11)?,
    ))
}

/// Build an indexed node from database row parts.
fn indexed_node_from_parts(
    row: (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<u64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
    ),
) -> DbResult<IndexedNode> {
    let (
        path,
        kind_value,
        parent_path,
        extension,
        language,
        size_bytes,
        mtime_ns,
        content_hash,
        purpose,
        source_value,
        status_value,
        summary,
    ) = row;
    let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
        field: "kind",
        value: kind_value,
    })?;
    let source = parse_source(&source_value)?;
    let status = PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
        field: "status",
        value: status_value,
    })?;
    Ok(IndexedNode {
        node: Node {
            path: path.clone(),
            kind,
            parent_path,
            extension,
            language,
            size_bytes,
            mtime_ns,
            content_hash,
        },
        purpose: Purpose {
            path,
            purpose,
            source,
            status,
        },
        summary,
    })
}

/// Split a user query into lowercase terms for SQL ranking.
fn normalize_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Build the SQL score expression for ranked node lookup.
fn ranked_score_expression(term_count: usize) -> String {
    if term_count == 0 {
        return "1".to_string();
    }
    (0..term_count)
        .map(|_| {
            "(CASE WHEN lower(n.path) LIKE ? ESCAPE '\\' THEN 20 ELSE 0 END \
             + CASE WHEN lower(COALESCE(p.purpose, '')) LIKE ? ESCAPE '\\' THEN 30 ELSE 0 END \
             + CASE WHEN lower(COALESCE(s.summary, '')) LIKE ? ESCAPE '\\' THEN 10 ELSE 0 END \
             + CASE WHEN lower(COALESCE(symbol_summaries.summary, '')) LIKE ? ESCAPE '\\' THEN 25 ELSE 0 END)"
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Convert a normalized term into a `SQLite` LIKE pattern.
fn sqlite_like_pattern(term: &str) -> String {
    format!("%{}%", sqlite_like_escape(term))
}

/// Build a `SQLite` LIKE descendant pattern for a repository path prefix.
fn sqlite_descendant_pattern(path: &str) -> String {
    format!("{}/%", sqlite_like_escape(path))
}

/// Escape user or path text for `SQLite` LIKE patterns with backslash escaping.
fn sqlite_like_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Replace the denormalized symbol-name search summary for one file node.
fn replace_symbol_search_summary(
    transaction: &Transaction<'_>,
    node_id: i64,
    summary: Option<&str>,
) -> DbResult<()> {
    if let Some(summary) = summary {
        transaction.execute(
            "
            INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
            VALUES(?1, 'search', 'symbols', ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
                summary = excluded.summary,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![node_id, summary],
        )?;
    } else {
        transaction.execute(
            "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'search'
              AND subject = 'symbols'
            ",
            [node_id],
        )?;
    }
    Ok(())
}

/// Build a bounded search-only summary from symbol names.
fn symbol_search_summary(graph: &SymbolGraph) -> Option<String> {
    let mut names = graph
        .symbols
        .iter()
        .filter(|symbol| !matches!(symbol.kind, SymbolKind::Import | SymbolKind::Unknown))
        .map(|symbol| symbol.name.trim())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return None;
    }
    let summary = format!("symbols {}", names.join(" "));
    Some(truncate_summary_chars(
        &summary,
        MAX_SYMBOL_SEARCH_SUMMARY_CHARS,
    ))
}

/// Truncate a summary at a valid UTF-8 boundary.
fn truncate_summary_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

/// Parse a stored purpose source value into the domain enum.
fn parse_source(value: &str) -> DbResult<PurposeSource> {
    let source = match value {
        "missing" => PurposeSource::Missing,
        "imported" => PurposeSource::Imported,
        "generated" => PurposeSource::Generated,
        // Older databases could contain `human`; ProjectAtlas now treats
        // explicit approval as agent-owned and serializes new writes as `agent`.
        "agent" | "human" => PurposeSource::Agent,
        _ => {
            return Err(DbError::InvalidEnum {
                field: "source",
                value: value.to_string(),
            });
        }
    };
    Ok(source)
}

/// Convert an aggregate database count into a platform `usize`.
fn count_to_usize(field: &'static str, value: i64) -> DbResult<usize> {
    usize::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Convert a `SQLite` REAL aggregate token total to a saturating wide integer.
fn token_total_from_sql(_field: &'static str, value: f64) -> u128 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= u128::MAX as f64 {
        u128::MAX
    } else {
        value.round() as u128
    }
}

/// Return the `SQLite` period expression for one token trend window.
fn token_trend_period_expression(window: TokenTrendWindow) -> &'static str {
    match window {
        TokenTrendWindow::Day => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 10)",
        TokenTrendWindow::Week => "strftime('%Y-W%W', COALESCE(created_at, CURRENT_TIMESTAMP))",
        TokenTrendWindow::Month => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 7)",
        TokenTrendWindow::Year => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 4)",
    }
}

/// Convert a usize to i64 with saturation for database storage.
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Convert a non-negative i64 to usize for database reads.
fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

/// Wrap a query string for a SQL LIKE expression.
fn like_query(query: &str) -> String {
    format!("%{query}%")
}

/// Build numbered SQL placeholders starting at a caller-selected index.
fn numbered_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Generate the durable node-level content summary.
fn generate_node_summary(node: &Node) -> String {
    match node.kind {
        NodeKind::Folder => format!("Folder for {}", path_label(&node.path)),
        NodeKind::File => file_summary(node),
    }
}

/// Generate a one-line observed file summary from scan metadata.
fn file_summary(node: &Node) -> String {
    let language = node
        .language
        .as_deref()
        .or(node.extension.as_deref())
        .unwrap_or("unknown");
    let size = node.size_bytes.map_or_else(
        || "unknown size".to_string(),
        |bytes| format!("{bytes} bytes"),
    );
    format!("{language} file, {size}")
}

/// Return a readable label for a repository-relative path.
fn path_label(path: &str) -> String {
    if path == "." {
        return "repository root".to_string();
    }
    path.rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

/// Emit a health finding when it has not already been resolved.
fn emit_unresolved_finding<F>(
    finding: HealthFinding,
    resolved_ids: &HashSet<String>,
    visitor: &mut F,
) -> DbResult<bool>
where
    F: FnMut(HealthFinding) -> DbResult<bool>,
{
    if resolved_ids.contains(&finding.id) {
        return Ok(true);
    }
    visitor(finding)
}

/// Return whether a purpose-status source can match a bounded health query.
fn purpose_health_spec_matches_query(spec: PurposeHealthSpec, query: &HealthQuery) -> bool {
    health_category_matches_query(spec.category, Severity::Warning, query)
}

/// Return whether a health category/severity can match a bounded query.
fn health_category_matches_query(category: &str, severity: Severity, query: &HealthQuery) -> bool {
    query
        .category
        .as_deref()
        .is_none_or(|requested| category.eq_ignore_ascii_case(requested))
        && query.severity.is_none_or(|requested| severity == requested)
}

/// Return purpose health metadata for a stored purpose status.
fn purpose_health_spec_for_status(status: &str) -> DbResult<PurposeHealthSpec> {
    PURPOSE_HEALTH_SPECS
        .iter()
        .copied()
        .find(|spec| spec.status == status)
        .ok_or_else(|| DbError::InvalidEnum {
            field: "status",
            value: status.to_string(),
        })
}

/// Build the health finding for an approved purpose that still needs agent review.
fn agent_review_required_finding(path: String) -> HealthFinding {
    HealthFinding {
        id: finding_id(AGENT_REVIEW_REQUIRED_CATEGORY, &path, None),
        severity: Severity::Warning,
        category: AGENT_REVIEW_REQUIRED_CATEGORY.to_string(),
        path,
        related_path: None,
        message: AGENT_REVIEW_REQUIRED_MESSAGE.to_string(),
        recommendation: AGENT_REVIEW_REQUIRED_RECOMMENDATION.to_string(),
    }
}

/// Build the shared SQL filter for globally ordered purpose lifecycle findings.
fn purpose_lifecycle_where_clause(
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let statuses = PURPOSE_HEALTH_SPECS
        .iter()
        .map(|spec| format!("'{}'", spec.status))
        .collect::<Vec<_>>()
        .join(", ");
    let mut clauses = vec![
        "n.exists_now = 1".to_string(),
        format!("p.status IN ({statuses})"),
    ];
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    for spec in PURPOSE_HEALTH_SPECS {
        let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
        if !resolved_paths.is_empty() {
            clauses.push(format!(
                "NOT (p.status = '{}' AND n.path IN ({}))",
                spec.status,
                numbered_placeholders(values.len() + 1, resolved_paths.len())
            ));
            values.extend(resolved_paths.into_iter().map(Value::from));
        }
    }

    (clauses.join(" AND "), values)
}

/// Build the shared SQL filter for purpose lifecycle health findings.
fn purpose_status_where_clause(
    spec: PurposeHealthSpec,
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let mut clauses = vec!["n.exists_now = 1".to_string(), "p.status = ?1".to_string()];
    let mut values = vec![Value::from(spec.status.to_string())];

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
    if !resolved_paths.is_empty() {
        clauses.push(format!(
            "n.path NOT IN ({})",
            numbered_placeholders(values.len() + 1, resolved_paths.len())
        ));
        values.extend(resolved_paths.into_iter().map(Value::from));
    }

    (clauses.join(" AND "), values)
}

/// Build a structural-health SQL filter over `findings` CTE columns.
fn structural_finding_where_clause(
    category: &str,
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
    first_placeholder: usize,
) -> (String, Vec<Value>) {
    let mut placeholder = first_placeholder;
    let mut clauses = Vec::new();
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push("source_relevant = 1".to_string());
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_finding_expression(scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "((path = ?{path_exact} OR path LIKE ?{path_descendant} ESCAPE '\\') \
              OR (related_path = ?{related_exact} OR related_path LIKE ?{related_descendant} ESCAPE '\\'))",
            path_exact = placeholder,
            path_descendant = placeholder + 1,
            related_exact = placeholder + 2,
            related_descendant = placeholder + 3
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        placeholder += 4;
    }

    let resolved_ids = resolved_ids_for_category(resolved_ids, category);
    if !resolved_ids.is_empty() {
        clauses.push(format!(
            "('{category}:' || path || ':' || related_path) NOT IN ({})",
            numbered_placeholders(placeholder, resolved_ids.len())
        ));
        values.extend(resolved_ids.into_iter().map(Value::from));
    }

    if clauses.is_empty() {
        (String::new(), values)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), values)
    }
}

/// SQL expression for approved purposes that need agent review at the requested scope.
fn purpose_review_candidate_expression(node_alias: &str, scope: HealthScope) -> String {
    let scope = match scope {
        HealthScope::All => HealthScope::PurposeStrict,
        other => other,
    };
    purpose_default_queue_node_expression(node_alias, "p", scope)
}

/// SQL expression for paths that belong in the default purpose queue.
fn purpose_default_queue_node_expression(
    node_alias: &str,
    purpose_alias: &str,
    scope: HealthScope,
) -> String {
    let asset_clause = if scope.include_assets() {
        format!(
            " OR ({node_alias}.kind = 'file' AND NOT ({}))",
            source_relevant_node_expression(node_alias)
        )
    } else {
        String::new()
    };
    let source_file_clause = if scope.include_source_files() {
        format!(" OR ({node_alias}.kind = 'file' AND COALESCE({node_alias}.language, '') <> '')")
    } else {
        String::new()
    };
    let all_file_clause = if scope.include_all_files() {
        format!(" OR {node_alias}.kind = 'file'")
    } else {
        String::new()
    };
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "({node_alias}.kind = 'folder' \
          OR ({node_alias}.kind = 'file' \
              AND {purpose_alias}.status = 'stale' \
              AND {purpose_alias}.source IN ({stale_queue_sources})) \
          OR ({node_alias}.kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)")),
    )
}

/// SQL expression for finding CTE columns that belong in the default purpose queue.
fn purpose_default_queue_finding_expression(scope: HealthScope) -> String {
    let asset_clause = if scope.include_assets() {
        " OR (kind = 'file' AND COALESCE(language, '') = '')"
    } else {
        ""
    };
    let source_file_clause = if scope.include_source_files() {
        " OR (kind = 'file' AND COALESCE(language, '') <> '')"
    } else {
        ""
    };
    let all_file_clause = if scope.include_all_files() {
        " OR kind = 'file'"
    } else {
        ""
    };
    format!(
        "(kind = 'folder' OR (kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression("lower(path)")
    )
}

/// SQL ORDER BY expression that keeps folder-purpose work ahead of file cleanup.
fn purpose_default_queue_order_expression(node_alias: &str, purpose_alias: &str) -> String {
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "CASE \
            WHEN {node_alias}.kind = 'folder' THEN 0 \
            WHEN {node_alias}.kind = 'file' \
                AND {purpose_alias}.status = 'stale' \
                AND {purpose_alias}.source IN ({stale_queue_sources}) THEN 1 \
            WHEN {node_alias}.kind = 'file' AND {} THEN 2 \
            ELSE 3 \
        END, {node_alias}.path",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)"))
    )
}

/// Purpose sources whose stale file purposes stay in the default queue.
const STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES: &[&str] = &["agent", "human", "imported"];

/// Return whether `source_only` should run before queue-specific folder/file selection.
fn source_filter_applies_before_queue(scope: HealthScope) -> bool {
    scope.source_only_filter() && !scope.high_impact_queue()
}

/// Render trusted static strings as SQL string literals.
fn sql_string_literals(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// SQL expression mirroring the path-based high-impact file heuristic.
fn high_impact_file_path_expression(lower_path: &str) -> String {
    let name_matches = HIGH_IMPACT_FILE_NAMES
        .iter()
        .map(|name| format!("{lower_path} = '{name}' OR {lower_path} LIKE '%/{name}'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let prefix_matches = HIGH_IMPACT_PATH_PREFIXES
        .iter()
        .map(|prefix| format!("{lower_path} LIKE '{prefix}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let segment_matches = HIGH_IMPACT_PATH_SEGMENTS
        .iter()
        .map(|segment| format!("{lower_path} LIKE '%{segment}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({name_matches} OR {prefix_matches} OR {segment_matches})")
}

/// Return a SQL expression that treats source files and folders with source descendants as source-relevant.
fn source_relevant_node_expression(alias: &str) -> String {
    format!(
        "(({alias}.kind = 'file' AND COALESCE({alias}.language, '') <> '') \
          OR ({alias}.kind = 'folder' AND EXISTS (\
              SELECT 1 FROM nodes source_child \
              WHERE source_child.exists_now = 1 \
                AND source_child.kind = 'file' \
                AND COALESCE(source_child.language, '') <> '' \
                AND (\
                    {alias}.path = '.' \
                    OR source_child.parent_path = {alias}.path \
                    OR substr(source_child.parent_path, 1, length({alias}.path) + 1) = {alias}.path || '/'\
                )\
          )))"
    )
}

/// Extract resolved primary paths for lifecycle categories without related paths.
fn resolved_purpose_paths(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter_map(|id| {
            id.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(':'))
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

/// Extract resolved full ids for categories that include related paths.
fn resolved_ids_for_category(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::telemetry::{
        TOKEN_ACCOUNTING_MODELED_AVOIDANCE, TOKEN_ACCURACY_HEURISTIC,
        TOKEN_BASELINE_SELECTED_CANDIDATES, TOKEN_BUCKET_FULL_FILE_COMPRESSION,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE, TOKEN_CONFIDENCE_INFERRED, TOKEN_DEDUPE_SCOPE_EVENT,
        usage_from_estimates, usage_from_estimates_with_accounting, usage_from_text,
    };
    use projectatlas_core::{NodeKind, normalized_parent};
    use std::error::Error;
    use std::fmt::Debug;
    use std::io;

    #[test]
    fn stores_nodes_and_overview() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        let overview = store.overview()?;
        require_eq(&overview.files, &1, "file count")?;
        require_eq(&overview.missing_purposes, &1, "missing purpose count")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.purpose,
            &None,
            "purpose remains separate from summary",
        )?;
        require_eq(
            &nodes[0].summary,
            &Some("rust file, 12 bytes".to_string()),
            "node-level summary",
        )?;
        let loaded = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("indexed node was not found by path"))?;
        require_eq(
            &loaded.node.path,
            &"src/main.rs".to_string(),
            "targeted path lookup",
        )?;
        require_eq(
            &store.load_node_by_path("src/missing.rs")?.is_none(),
            &true,
            "missing targeted path lookup",
        )?;
        Ok(())
    }

    #[test]
    fn records_token_overview() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let mut session_event = usage_from_estimates(
            "session",
            "outline",
            Some("src/main.rs".to_string()),
            None,
            100,
            20,
        );
        session_event.estimated_tokens_saved = Some(1);
        store.record_usage(&session_event)?;
        let mut unknown_event = usage_from_estimates("session", "unknown", None, None, 0, 0);
        unknown_event.estimated_tokens_without_projectatlas = None;
        unknown_event.estimated_tokens_with_projectatlas = None;
        unknown_event.estimated_tokens_saved = None;
        store.record_usage(&unknown_event)?;
        store.record_usage(&usage_from_estimates(
            "other-session",
            "outline",
            Some("src/lib.rs".to_string()),
            None,
            200,
            50,
        ))?;
        let overview = store.token_overview(Some("session"))?;
        require_eq(&overview.calls, &1, "usage call count")?;
        require_eq(&overview.estimated_saved, &80, "saved token count")?;
        require_eq(&overview.buckets.len(), &1, "usage bucket count")?;
        require_eq(
            &overview.buckets[0].accuracy,
            &TOKEN_ACCURACY_HEURISTIC.to_string(),
            "usage bucket accuracy",
        )?;
        let all_sessions = store.token_overview(None)?;
        require_eq(&all_sessions.calls, &2, "all-session usage call count")?;
        require_eq(
            &all_sessions.estimated_without_projectatlas,
            &300,
            "all-session baseline tokens",
        )?;
        require_eq(
            &all_sessions.estimated_with_projectatlas,
            &70,
            "all-session atlas tokens",
        )?;
        require_eq(
            &all_sessions.estimated_saved,
            &230,
            "all-session saved tokens",
        )?;

        store.record_usage(&usage_from_text(
            "bucketed",
            "summary",
            Some("src/main.rs".to_string()),
            None,
            "abcdefghijkl",
            "abcd",
        ))?;
        store.record_usage(&usage_from_estimates(
            "bucketed", "folders", None, None, 100, 20,
        ))?;
        let bucketed = store.token_overview(Some("bucketed"))?;
        require_eq(&bucketed.buckets.len(), &2, "bucketed overview count")?;
        require_eq(
            &bucketed.buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "source compression bucket",
        )?;
        require_eq(
            &bucketed.buckets[1].token_savings_bucket,
            &TOKEN_BUCKET_NAVIGATION_AVOIDANCE.to_string(),
            "navigation bucket",
        )?;

        store.record_usage(&usage_from_text(
            "deduped",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            "abcdabcd",
            "ab",
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            40,
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            30,
        ))?;
        let deduped = store.token_overview(Some("deduped"))?;
        require_eq(
            &deduped.legacy_gross_estimated_saved,
            &731,
            "legacy gross saved tokens remains available",
        )?;
        require_eq(
            &deduped.measured_tokens_saved,
            &1,
            "measured saved tokens remain separate",
        )?;
        require_eq(
            &deduped.gross_modeled_tokens_avoided,
            &730,
            "gross modeled avoided tokens remains available",
        )?;
        require_eq(
            &deduped.deduped_modeled_tokens_avoided,
            &330,
            "modeled avoided tokens are deduped by baseline",
        )?;
        require_eq(
            &deduped.tokens_avoided,
            &331,
            "headline avoided tokens use measured plus deduped modeled",
        )?;

        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            40,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            30,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        let event_scoped = store.token_overview(Some("event-scoped"))?;
        require_eq(
            &event_scoped.gross_modeled_tokens_avoided,
            &730,
            "event-scoped gross modeled avoided tokens",
        )?;
        require_eq(
            &event_scoped.deduped_modeled_tokens_avoided,
            &730,
            "event-scoped modeled events are not collapsed",
        )?;
        require_eq(
            &event_scoped.repeated_baselines_deduped,
            &0,
            "event-scoped modeled events do not count as deduped repeats",
        )?;

        let mut negative_event = usage_from_estimates("negative", "outline", None, None, 20, 50);
        negative_event.estimated_tokens_saved = Some(999);
        store.record_usage(&negative_event)?;
        let negative = store.token_overview(Some("negative"))?;
        require_eq(&negative.calls, &1, "negative session call count")?;
        require_eq(
            &negative.estimated_saved,
            &-30,
            "negative session recomputed delta",
        )?;
        require_eq(
            &negative.savings_rate,
            &Some(-1.5),
            "negative session savings rate",
        )?;

        let mut zero_event = usage_from_estimates("zero-baseline", "outline", None, None, 0, 12);
        zero_event.estimated_tokens_saved = Some(999);
        store.record_usage(&zero_event)?;
        let zero_baseline = store.token_overview(Some("zero-baseline"))?;
        require_eq(&zero_baseline.calls, &1, "zero baseline call count")?;
        require_eq(
            &zero_baseline.estimated_saved,
            &-12,
            "zero baseline recomputed delta",
        )?;
        require_eq(
            &zero_baseline.savings_rate,
            &None,
            "zero baseline savings rate",
        )?;

        for index in 0..3 {
            store.connection.execute(
                "
                INSERT INTO usage_events(
                    session_id,
                    command,
                    path,
                    query,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                )
                VALUES(?1, 'large', NULL, NULL, ?2, 0, ?2)
                ",
                params![format!("large-{index}"), i64::MAX,],
            )?;
        }
        let large = store.token_overview(None)?;
        require_eq(
            &large.estimated_saved,
            &isize::MAX,
            "large aggregate saturates without sqlite SUM overflow",
        )?;
        Ok(())
    }

    #[test]
    fn token_trends_group_usage_by_period_and_bucket() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        for (session, created_at, bucket, baseline_kind, confidence, without, with) in [
            (
                "session",
                "2026-06-01 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                100_i64,
                25_i64,
            ),
            (
                "session",
                "2026-06-10 00:00:00",
                TOKEN_BUCKET_FULL_FILE_COMPRESSION,
                "full_file",
                "observed",
                50_i64,
                10_i64,
            ),
            (
                "session",
                "2026-07-01 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                80_i64,
                20_i64,
            ),
            (
                "other",
                "2026-06-03 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                999_i64,
                1_i64,
            ),
        ] {
            store.connection.execute(
                "
                INSERT INTO usage_events(
                    session_id,
                    command,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved,
                    token_savings_bucket,
                    baseline_kind,
                    confidence,
                    created_at
                )
                VALUES(?1, 'trend', ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ",
                params![
                    session,
                    without,
                    with,
                    without - with,
                    bucket,
                    baseline_kind,
                    confidence,
                    created_at
                ],
            )?;
        }

        let trends = store.token_trends(Some("session"), TokenTrendWindow::Month)?;
        require_eq(&trends.periods.len(), &2, "monthly periods")?;
        require_eq(
            &trends.periods[0].period,
            &"2026-06".to_string(),
            "first month",
        )?;
        require_eq(&trends.periods[0].calls, &2, "june call count")?;
        require_eq(
            &trends.periods[0].estimated_saved,
            &115,
            "june saved tokens",
        )?;
        require_eq(
            &trends.periods[0].buckets.len(),
            &2,
            "june preserves evidence buckets",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "full-file bucket remains visible",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].confidence,
            &"observed".to_string(),
            "bucket confidence remains visible",
        )?;
        require_eq(
            &trends.periods[1].period,
            &"2026-07".to_string(),
            "second month",
        )?;
        require_eq(&trends.periods[1].calls, &1, "july call count")?;
        Ok(())
    }

    #[test]
    fn token_trends_backfill_created_at_for_upgraded_databases() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("legacy.db");
        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO metadata(key, value) VALUES('schema_version', '7');
                CREATE TABLE usage_events (
                    id INTEGER PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    command TEXT NOT NULL,
                    path TEXT,
                    query TEXT,
                    estimated_tokens_without_projectatlas INTEGER,
                    estimated_tokens_with_projectatlas INTEGER,
                    estimated_tokens_saved INTEGER,
                    token_savings_bucket TEXT NOT NULL DEFAULT 'navigation_avoidance',
                    provider TEXT NOT NULL DEFAULT 'heuristic',
                    model TEXT NOT NULL DEFAULT 'unknown',
                    tokenizer_backend TEXT NOT NULL DEFAULT 'chars_div_4',
                    accuracy TEXT NOT NULL DEFAULT 'heuristic_estimate',
                    baseline_kind TEXT NOT NULL DEFAULT 'selected_candidates',
                    confidence TEXT NOT NULL DEFAULT 'inferred',
                    calculation_trace TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'
                );
                INSERT INTO usage_events(
                    session_id,
                    command,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                )
                VALUES('legacy-session', 'legacy', 100, 20, 80);
                ",
            )?;
        }

        let store = AtlasStore::open(&db_path)?;
        let null_created_at = store.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE created_at IS NULL OR created_at = ''",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&null_created_at, &0, "legacy created_at values backfilled")?;
        store.record_usage(&usage_from_estimates(
            "legacy-session",
            "new-call",
            None,
            None,
            50,
            10,
        ))?;
        let null_created_at = store.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE created_at IS NULL OR created_at = ''",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&null_created_at, &0, "new created_at values populated")?;
        let trends = store.token_trends(Some("legacy-session"), TokenTrendWindow::Month)?;
        require_eq(&trends.periods.is_empty(), &false, "trend periods exist")?;
        require_eq(
            &trends
                .periods
                .iter()
                .any(|period| period.period.starts_with("1970")),
            &false,
            "upgraded telemetry does not aggregate under 1970",
        )?;
        Ok(())
    }

    #[test]
    fn stores_project_root_in_metadata() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        store.set_project_root(Path::new("C:/workspace/example"))?;
        require_eq(
            &store.project_root()?,
            &Some("C:/workspace/example".to_string()),
            "project root metadata",
        )?;
        store.set_project_root(Path::new(r"\\?\C:\workspace\example"))?;
        require_eq(
            &store.project_root()?,
            &Some("C:/workspace/example".to_string()),
            "windows extended project root metadata",
        )?;
        store.set_project_root(Path::new(r"\\?\UNC\server\share\repo"))?;
        require_eq(
            &store.project_root()?,
            &Some("//server/share/repo".to_string()),
            "windows unc project root metadata",
        )?;
        Ok(())
    }

    #[test]
    fn partial_scan_updates_and_absents_paths() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.upsert_scan_nodes(&[test_file_node("src/a.rs", "hash-a2")])?;
        let updated = store
            .load_node_by_path("src/a.rs")?
            .ok_or_else(|| io::Error::other("updated node missing"))?;
        require_eq(
            &updated.node.content_hash,
            &Some("hash-a2".to_string()),
            "partial content hash",
        )?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_some(),
            &true,
            "unrelated node remains indexed",
        )?;
        store.mark_paths_absent(&["src/b.rs".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_none(),
            &true,
            "absent path is no longer returned",
        )?;
        Ok(())
    }

    #[test]
    fn approved_purpose_becomes_stale_when_file_hash_changes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        store.upsert_scan_nodes(&[test_file_node("src/main.rs", "hash-b")])?;

        let node = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("stale node missing"))?;
        require_eq(
            &node.purpose.status,
            &PurposeStatus::Stale,
            "changed approved file purpose status",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_nodes_are_loaded_bounded_from_sql() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut gradle_task_node = test_file_node("build.gradle.kts", "hash-gradle");
        gradle_task_node.extension = Some(".kts".to_string());
        gradle_task_node.language = Some("kotlin".to_string());
        store.replace_scan(&[
            test_folder_node("src/auth"),
            test_folder_node("src/ui"),
            test_file_node("src/auth/login.rs", "hash-login"),
            test_file_node("src/ui/button.rs", "hash-button"),
            gradle_task_node,
        ])?;
        store.set_purpose(
            "src/auth",
            "Authentication workflow folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose("src/ui", "User interface folder", PurposeSource::Agent)?;
        store.set_node_summary("src/auth/login.rs", "rust source defining login flow")?;

        let folders = store.load_ranked_nodes("authentication", NodeKind::Folder, None, 1, 0)?;
        require_eq(&folders.len(), &1, "bounded folder ranking")?;
        require_eq(
            &folders[0].node.path,
            &"src/auth".to_string(),
            "semantic folder ranking",
        )?;

        let files = store.load_ranked_nodes("login", NodeKind::File, Some("src/auth"), 10, 0)?;
        require_eq(&files.len(), &1, "folder-constrained file ranking")?;
        require_eq(
            &files[0].node.path,
            &"src/auth/login.rs".to_string(),
            "ranked file path",
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "build.gradle.kts".to_string(),
            language: Some("kotlin".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "build.gradle.kts".to_string(),
                language: Some("kotlin".to_string()),
                name: "bootRunE2E".to_string(),
                kind: SymbolKind::Function,
                signature: "tasks.register<BootRun>(\"bootRunE2E\")".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("gradle-kotlin-dsl-task".to_string()),
            }],
            relations: Vec::new(),
        })?;
        let gradle_files = store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(&gradle_files.len(), &1, "symbol-ranked file count")?;
        require_eq(
            &gradle_files[0].node.path,
            &"build.gradle.kts".to_string(),
            "symbol-ranked file path",
        )?;
        store.clear_symbol_graph_for_path("build.gradle.kts")?;
        let cleared_gradle_files =
            store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(
            &cleared_gradle_files.len(),
            &0,
            "cleared symbol-ranked file count",
        )?;
        Ok(())
    }

    #[test]
    fn folder_like_filters_treat_wildcards_as_literal_path_text() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("src/a%b"),
            test_folder_node("src/axb"),
            test_folder_node("src/a_b"),
            test_folder_node("src/acb"),
            test_file_node("src/a%b/target.rs", "hash-percent-target"),
            test_file_node("src/axb/false.rs", "hash-percent-false"),
            test_file_node("src/a_b/target.rs", "hash-underscore-target"),
            test_file_node("src/acb/false.rs", "hash-underscore-false"),
        ])?;
        for path in [
            "src/a%b/target.rs",
            "src/axb/false.rs",
            "src/a_b/target.rs",
            "src/acb/false.rs",
        ] {
            store.set_node_summary(path, "needle indexed summary")?;
        }

        let percent_files =
            store.load_ranked_nodes("needle", NodeKind::File, Some("src/a%b"), 10, 0)?;
        require_eq(&percent_files.len(), &1, "percent folder ranked count")?;
        require_eq(
            &percent_files[0].node.path,
            &"src/a%b/target.rs".to_string(),
            "percent folder ranked path",
        )?;
        require_eq(
            &store.source_file_byte_count(Some("src/a%b"))?,
            &12,
            "percent folder byte count",
        )?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src/a_b"), |path, _size| {
            visited.push(path);
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec!["src/a_b/target.rs".to_string()],
            "underscore folder token paths",
        )?;

        store.mark_paths_absent(&["src/a%b".to_string(), "src/a_b".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/axb/false.rs")?.is_some(),
            &true,
            "percent-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/acb/false.rs")?.is_some(),
            &true,
            "underscore-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a%b/target.rs")?.is_none(),
            &true,
            "percent folder target removed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a_b/target.rs")?.is_none(),
            &true,
            "underscore folder target removed",
        )?;
        Ok(())
    }

    #[test]
    fn sql_health_findings_match_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;

        let findings = store.unresolved_health_findings(&[])?;
        let duplicate = findings
            .iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate.id.clone(),
            category: duplicate.category.clone(),
            path: duplicate.path.clone(),
            related_path: duplicate.related_path.clone(),
            rationale: "Intentional mirror for test.".to_string(),
        })?;
        let remaining = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
        require_eq(&remaining.is_empty(), &true, "resolved SQL health finding")?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_filters_and_bounds_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("docs/a.rs", "hash-doc"),
        ])?;
        let query = HealthQuery {
            start_index: 1,
            limit: 1,
            category: Some("missing-purpose".to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some("src".to_string()),
            summary_only: false,
            scope: HealthScope::all(),
        };

        let page = store.unresolved_health_findings_page(&[], &query)?;
        require_eq(&page.unfiltered_total, &4, "unfiltered health total")?;
        require_eq(&page.total, &2, "filtered health total")?;
        require_eq(&page.returned, &1, "returned health rows")?;
        require_eq(
            &page.findings[0].path,
            &"src/a.rs".to_string(),
            "paged path",
        )?;

        let summary_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                summary_only: true,
                ..query
            },
        )?;
        require_eq(&summary_page.total, &2, "summary-only total")?;
        require_eq(
            &summary_page.findings.is_empty(),
            &true,
            "summary-only rows",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_skips_resolved_lifecycle_rows_before_paging()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("src/c.rs", "hash-c"),
            test_file_node("src/d.rs", "hash-d"),
        ])?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding_id("missing-purpose", "src/b.rs", None),
            category: "missing-purpose".to_string(),
            path: "src/b.rs".to_string(),
            related_path: None,
            rationale: "Resolved for pagination regression.".to_string(),
        })?;

        let page = store.unresolved_health_findings_page(
            &store.resolved_health_ids()?,
            &HealthQuery {
                start_index: 0,
                limit: 2,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some("src".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;

        require_eq(&page.total, &3, "filtered unresolved missing total")?;
        require_eq(&page.returned, &2, "returned unresolved missing rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["src/a.rs", "src/c.rs"],
            "resolved row skipped before limit",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_streams_duplicate_and_temp_rows()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("tmp"),
            test_folder_node("src"),
            test_folder_node("src/tmp"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose(".", "Repository root", PurposeSource::Agent)?;
        store.set_purpose("src", "Source folder", PurposeSource::Agent)?;
        store.set_purpose("tmp", "Temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/tmp", "Source temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/a.rs", "Shared implementation", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared implementation", PurposeSource::Agent)?;

        let duplicate_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some("src".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&duplicate_page.total, &1, "duplicate total")?;
        require_eq(&duplicate_page.returned, &1, "duplicate returned")?;
        require_eq(
            &duplicate_page.findings[0].category,
            &"duplicate-purpose".to_string(),
            "duplicate category",
        )?;

        let temp_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("repeated-temporary-folder".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&temp_page.total, &1, "temp total")?;
        require_eq(&temp_page.returned, &1, "temp returned")?;
        require_eq(
            &temp_page.findings[0].category,
            &"repeated-temporary-folder".to_string(),
            "temp category",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_source_only_filters_asset_noise()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.png".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".png".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_folder_node("assets"),
            asset_file,
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::source_only(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all unresolved rows")?;
        require_eq(&page.total, &3, "source-only missing total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "src/main.rs"],
            "source-only paths",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_filters_low_priority_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_file_node("src/helper.rs", "hash-helper"),
            test_file_node("build.gradle.kts", "hash-gradle"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &4, "default actionable rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "build.gradle.kts", "src/main.rs"],
            "folder-first high-impact queue paths",
        )?;

        let broad_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&broad_page.total, &5, "explicit broad queue rows")?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_keeps_asset_only_folders_without_asset_files()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &3, "all folders without asset files")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src"],
            "folder-first default queue keeps asset-only folders",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_pages_folders_before_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
            test_file_node("pyproject.toml", "hash-python"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 2,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &5, "default actionable total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src"],
            "small page keeps folders first",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_pages_stale_reviewed_files_before_high_impact_files()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
        ])?;
        store.set_purpose(
            "src/helper.rs",
            "Reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &3, "default actionable total")?;
        require_eq(&page.returned, &1, "small page returned")?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale reviewed file is globally prioritized",
        )?;
        require_eq(
            &page.findings[0].path,
            &"src/helper.rs".to_string(),
            "stale reviewed file path",
        )?;
        Ok(())
    }

    #[test]
    fn include_assets_queue_includes_asset_files_not_low_priority_source()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_with_assets(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(
            &page.total,
            &4,
            "assets included without broad source cleanup",
        )?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src", "assets/logo.svg"],
            "asset files included and low-priority source omitted",
        )?;
        Ok(())
    }

    #[test]
    fn legacy_human_stale_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-a")])?;
        store.set_purpose(
            "src/helper.rs",
            "Legacy reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/helper.rs')
            ",
            [],
        )?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-b")])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("stale-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &1, "legacy reviewed stale row total")?;
        require_eq(
            &page.findings[0].path,
            &"src/helper.rs".to_string(),
            "legacy reviewed stale file",
        )?;
        Ok(())
    }

    #[test]
    fn stale_imported_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;
        store.set_purpose(
            "src/imported.rs",
            "Imported helper implementation.",
            PurposeSource::Imported,
        )?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(
            &page.total,
            &2,
            "default queue includes stale imported file",
        )?;
        require_eq(
            &health_paths(&page),
            &vec!["src/imported.rs", "Cargo.toml"],
            "stale imported file is queued before high-impact files",
        )?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale imported finding category",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_purpose_health_is_contextual_for_folders() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;

        require_eq(&page.total, &0, "folder duplicates scoped by parent")?;
        Ok(())
    }

    #[test]
    fn contextual_folder_duplicate_identity_matches_unpaged_health_and_resolution()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
            test_folder_node("settings/worker"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/worker", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&page.total, &1, "contextual duplicate total")?;
        let paged_finding = page
            .findings
            .first()
            .ok_or_else(|| io::Error::other("paged duplicate missing"))?;
        require_eq(
            &paged_finding.path,
            &"settings/worker".to_string(),
            "paged duplicate path",
        )?;
        require_eq(
            &paged_finding.related_path,
            &Some("settings/service".to_string()),
            "paged related path",
        )?;

        let unpaged_duplicates = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .filter(|finding| finding.category == "duplicate-purpose")
            .collect::<Vec<_>>();
        require_eq(
            &unpaged_duplicates,
            &page.findings,
            "unpaged duplicate identity",
        )?;

        store.resolve_health_finding(&HealthResolution {
            finding_id: paged_finding.id.clone(),
            category: paged_finding.category.clone(),
            path: paged_finding.path.clone(),
            related_path: paged_finding.related_path.clone(),
            rationale: "Settings service and worker intentionally share a layer purpose."
                .to_string(),
        })?;
        let has_remaining_duplicate = store
            .unresolved_health_findings(&store.resolved_health_ids()?)?
            .into_iter()
            .any(|finding| finding.category == "duplicate-purpose");
        require_eq(
            &has_remaining_duplicate,
            &false,
            "resolved contextual duplicate",
        )?;
        Ok(())
    }

    #[test]
    fn agent_review_required_scope_expands_from_low_to_strict() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("src/detail.rs", "hash-detail"),
            asset_file,
        ])?;
        for (path, purpose) in [
            (".", "Imported repository root"),
            ("assets", "Imported asset folder"),
            ("src", "Imported Rust source folder"),
            ("Cargo.toml", "Imported Rust manifest"),
            ("src/detail.rs", "Imported implementation detail"),
            ("assets/logo.svg", "Imported SVG brand asset"),
        ] {
            store.set_purpose(path, purpose, PurposeSource::Imported)?;
        }

        let low = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(AGENT_REVIEW_REQUIRED_CATEGORY.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;
        require_eq(
            &health_paths(&low),
            &vec![".", "assets", "src", "Cargo.toml"],
            "low purpose review scope",
        )?;
        require_eq(
            &low.unfiltered_total,
            &6,
            "agent-review findings are counted once in unfiltered total",
        )?;

        let asset_scope = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_assets(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&asset_scope),
            &vec![".", "assets", "src", "Cargo.toml", "assets/logo.svg"],
            "asset purpose review scope",
        )?;

        let medium = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_source_files(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&medium),
            &vec![".", "assets", "src", "Cargo.toml", "src/detail.rs"],
            "medium purpose review scope",
        )?;

        let strict = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_strict(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&strict),
            &vec![
                ".",
                "assets",
                "src",
                "Cargo.toml",
                "assets/logo.svg",
                "src/detail.rs",
            ],
            "strict purpose review scope",
        )?;

        let all = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::all(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&all),
            &health_paths(&strict),
            "all health scope should include every purpose review candidate",
        )?;
        Ok(())
    }

    #[test]
    fn replace_scan_preserves_curated_purposes_and_reconciles_changed_paths()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
        ])?;
        store.set_purpose(".", "Agent-reviewed repository root", PurposeSource::Agent)?;
        store.set_purpose(
            "src",
            "Agent-reviewed Rust source folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed Rust entry point",
            PurposeSource::Agent,
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let nodes = store.load_nodes_by_paths(&[
            ".".to_string(),
            "src".to_string(),
            "src/main.rs".to_string(),
            "src/new.rs".to_string(),
        ])?;
        let by_path = nodes
            .iter()
            .map(|node| (node.node.path.as_str(), node))
            .collect::<HashMap<_, _>>();
        require_eq(
            &by_path["src/main.rs"].purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "unchanged file purpose preserved",
        )?;
        require_eq(
            &by_path["src/main.rs"].purpose.status,
            &PurposeStatus::Approved,
            "unchanged file purpose stays approved",
        )?;
        require_eq(
            &by_path["src/new.rs"].purpose.status,
            &PurposeStatus::Missing,
            "new file starts missing",
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-b"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let changed = store
            .load_nodes_by_paths(&["src/main.rs".to_string()])?
            .pop()
            .ok_or_else(|| io::Error::other("changed node missing"))?;
        require_eq(
            &changed.purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "changed file purpose text preserved",
        )?;
        require_eq(
            &changed.purpose.status,
            &PurposeStatus::Stale,
            "changed file purpose becomes stale",
        )?;

        store.replace_scan(&[test_folder_node("."), test_folder_node("src")])?;
        let removed = store.load_nodes_by_paths(&["src/main.rs".to_string()])?;
        require_eq(&removed.is_empty(), &true, "removed file is inactive")?;
        Ok(())
    }

    #[test]
    fn file_token_estimates_are_visited_without_loading_nodes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("tests/b.rs", "hash-b"),
        ])?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src"), |path, size_bytes| {
            visited.push((path, size_bytes));
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec![("src/a.rs".to_string(), Some(12))],
            "folder-scoped token estimate rows",
        )?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_replaces_and_clears_stale_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.replace_file_texts_for_paths(
            &["src/main.rs".to_string()],
            &[IndexedFileText {
                path: "src/main.rs".to_string(),
                content_hash: Some("hash-a".to_string()),
                byte_count: 12,
                line_count: 1,
                content: "needle old\n".to_string(),
            }],
        )?;
        let texts = store.load_file_texts_for_search(Some("needle"), true)?;
        require_eq(&texts.len(), &1, "indexed text row count")?;

        store.replace_file_texts_for_paths(&["src/main.rs".to_string()], &[])?;
        let missing = store.load_file_text("src/main.rs")?;
        require_eq(&missing.is_none(), &true, "cleared stale indexed text")?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_search_can_stop_without_collecting_all_rows() -> Result<(), Box<dyn Error>>
    {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.replace_file_texts_for_paths(
            &["src/a.rs".to_string(), "src/b.rs".to_string()],
            &[
                IndexedFileText {
                    path: "src/a.rs".to_string(),
                    content_hash: Some("hash-a".to_string()),
                    byte_count: 14,
                    line_count: 1,
                    content: "needle first\n".to_string(),
                },
                IndexedFileText {
                    path: "src/b.rs".to_string(),
                    content_hash: Some("hash-b".to_string()),
                    byte_count: 15,
                    line_count: 1,
                    content: "needle second\n".to_string(),
                },
            ],
        )?;

        let mut visited = Vec::new();
        store.visit_file_texts_for_search(Some("needle"), true, |text| {
            visited.push(text.path);
            Ok(false)
        })?;
        require_eq(&visited, &vec!["src/a.rs".to_string()], "early stop rows")?;
        Ok(())
    }

    #[test]
    fn suggested_purpose_is_not_approved() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Generated,
            "suggested source",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Suggested,
            "suggested status",
        )?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Approved,
            "agent-approved status",
        )?;
        Ok(())
    }

    #[test]
    fn agent_reviewed_marker_depends_on_agent_approved_source() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;

        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "generated suggestion is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Imported application entry point",
            PurposeSource::Imported,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "imported purpose is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "agent-approved purpose is agent reviewed",
        )?;

        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/main.rs')
            ",
            [],
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Agent,
            "legacy human source normalizes to agent",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "legacy approved human row remains reviewed",
        )?;

        store.replace_scan(&[test_file_node("src/main.rs", "hash-b")])?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Stale,
            "changed reviewed purpose becomes stale",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "stale purpose is not agent reviewed",
        )?;
        Ok(())
    }

    #[test]
    fn updates_content_summary_without_approving_purpose() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/lib.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/lib.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(24),
            mtime_ns: Some(10),
            content_hash: Some("def".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_node_summary(
            "src/lib.rs",
            "rust source defining library entry functions.",
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].summary,
            &Some("rust source defining library entry functions.".to_string()),
            "updated content summary",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Missing,
            "summary update does not approve purpose",
        )?;
        Ok(())
    }

    #[test]
    fn replaces_symbol_graph_idempotently() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let graph = SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "src/main.rs".to_string(),
                language: Some("rust".to_string()),
                name: "main".to_string(),
                kind: SymbolKind::Function,
                signature: "fn main()".to_string(),
                exported: true,
                documentation: Some("Run the application.".to_string()),
                line_start: 1,
                line_end: 3,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            }],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "println!".to_string(),
                kind: RelationKind::Calls,
                line: 2,
                context: "println!(\"hello\")".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };

        store.replace_symbol_graph(&graph)?;
        store.replace_symbol_graph(&graph)?;
        let symbols = store.load_symbols(Some("src/main.rs"), Some("main"), 10)?;
        let relations = store.load_symbol_relations(Some("src/main.rs"), Some("println"), 10)?;
        let metadata = store
            .load_source_parse_metadata("src/main.rs")?
            .ok_or_else(|| io::Error::other("missing source parse metadata"))?;
        require_eq(&symbols.len(), &1, "symbol count after replace")?;
        require_eq(&relations.len(), &1, "relation count after replace")?;
        require_eq(&metadata.parser, &ParserKind::TreeSitter, "metadata parser")?;
        require_eq(&metadata.symbol_count, &1, "metadata symbol count")?;
        require_eq(&metadata.relation_count, &1, "metadata relation count")?;
        require_eq(&symbols[0].exported, &true, "exported metadata")?;
        require_eq(
            &symbols[0].documentation,
            &Some("Run the application.".to_string()),
            "documentation metadata",
        )?;
        Ok(())
    }

    #[test]
    fn full_scan_removal_clears_source_parse_metadata() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/a.rs", "hash-a")])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        })?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?.is_some(),
            &true,
            "metadata exists before removal",
        )?;

        store.replace_scan(&[test_file_node("src/b.rs", "hash-b")])?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?,
            &None,
            "metadata cleared after full scan removal",
        )?;
        Ok(())
    }

    #[test]
    fn call_relations_are_limited_per_target() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut relations = Vec::new();
        for index in 0..5 {
            relations.push(SymbolRelation {
                path: format!("src/a{index}.rs"),
                source_name: format!("alpha_caller_{index}"),
                target_name: "alpha".to_string(),
                kind: RelationKind::Calls,
                line: index + 1,
                context: "alpha();".to_string(),
                parser: ParserKind::TreeSitter,
            });
        }
        relations.push(SymbolRelation {
            path: "src/z.rs".to_string(),
            source_name: "beta_caller".to_string(),
            target_name: "beta".to_string(),
            kind: RelationKind::Calls,
            line: 99,
            context: "beta();".to_string(),
            parser: ParserKind::TreeSitter,
        });
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations,
        })?;

        let loaded =
            store.load_call_relations_to_targets(&["alpha".to_string(), "beta".to_string()], 2)?;
        let alpha_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "alpha")
            .count();
        let beta_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "beta")
            .count();
        require_eq(&alpha_count, &2, "alpha per-target limit")?;
        require_eq(&beta_count, &1, "beta preserved despite alpha skew")?;
        Ok(())
    }

    #[test]
    fn stores_health_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;
        let duplicate = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        let duplicate_id = duplicate.id.clone();
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate_id.clone(),
            category: duplicate.category,
            path: duplicate.path,
            related_path: duplicate.related_path,
            rationale: "Paths intentionally mirror agent skill variants.".to_string(),
        })?;
        let ids = store.resolved_health_ids()?;
        require_eq(&ids, &vec![duplicate_id], "resolved ids")?;
        Ok(())
    }

    #[test]
    fn health_resolution_accepts_all_scope_agent_review_findings() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut asset_file = test_file_node("assets/logo.svg", "hash-logo");
        asset_file.extension = Some(".svg".to_string());
        asset_file.language = None;
        store.replace_scan(&[test_folder_node("assets"), asset_file])?;
        store.set_purpose(
            "assets/logo.svg",
            "Imported SVG brand asset purpose",
            PurposeSource::Imported,
        )?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(AGENT_REVIEW_REQUIRED_CATEGORY.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        let finding = page
            .findings
            .iter()
            .find(|finding| finding.path == "assets/logo.svg")
            .ok_or_else(|| io::Error::other("asset review finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding.id.clone(),
            category: finding.category.clone(),
            path: finding.path.clone(),
            related_path: finding.related_path.clone(),
            rationale: "Asset purpose imported from legacy metadata and intentionally accepted."
                .to_string(),
        })?;

        let remaining = store.unresolved_health_findings_page(
            &store.resolved_health_ids()?,
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(AGENT_REVIEW_REQUIRED_CATEGORY.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(
            &health_paths(&remaining).contains(&"assets/logo.svg"),
            &false,
            "resolved all-scope asset review finding",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_set_reports_unindexed_path_without_sqlite_leak() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.set_purpose("no/such/file.rs", "Missing file", PurposeSource::Agent)
        {
            Ok(()) => return Err(io::Error::other("missing path should fail").into()),
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("no/such/file.rs"),
            &true,
            "path named in error",
        )?;
        require_eq(
            &error.to_string().contains("sqlite error"),
            &false,
            "raw sqlite error hidden",
        )?;
        store.replace_scan(&[])?;
        let error = match store.set_purpose("src/main.rs", "Removed file", PurposeSource::Agent) {
            Ok(()) => return Err(io::Error::other("stale indexed path should fail").into()),
            Err(error) => error,
        };
        require_eq(
            &error.to_string().contains("src/main.rs"),
            &true,
            "stale path named in error",
        )?;
        Ok(())
    }

    #[test]
    fn health_resolution_requires_active_finding_tuple() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.resolve_health_finding(&HealthResolution {
            finding_id: "missing-id".to_string(),
            category: "duplicate-purpose".to_string(),
            path: "no/such/file.rs".to_string(),
            related_path: None,
            rationale: "typo".to_string(),
        }) {
            Ok(()) => {
                return Err(io::Error::other("nonexistent health finding should fail").into());
            }
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("not active"),
            &true,
            "inactive finding rejected",
        )?;
        Ok(())
    }

    /// Build a representative Rust file node for store tests.
    fn test_file_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent(path),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some(hash.to_string()),
        }
    }

    /// Build a representative folder node for store tests.
    fn test_folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: normalized_parent(path),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: Some(10),
            content_hash: None,
        }
    }

    /// Return the default low-cost purpose review query used by agent linting.
    fn low_query() -> HealthQuery {
        HealthQuery {
            start_index: 0,
            limit: 20,
            category: Some(AGENT_REVIEW_REQUIRED_CATEGORY.to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some(".".to_string()),
            summary_only: false,
            scope: HealthScope::purpose_default(),
        }
    }

    /// Collect health finding paths in returned order.
    fn health_paths(page: &HealthFindingsPage) -> Vec<&str> {
        page.findings
            .iter()
            .map(|finding| finding.path.as_str())
            .collect()
    }

    /// Require two test values to be equal without panicking.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }
}
