//! Purpose: Persist `ProjectAtlas` 3 indexes in `SQLite`.

use projectatlas_core::health::{HealthFinding, Severity, finding_id};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_core::telemetry::{TokenOverview, UsageEvent};
use projectatlas_core::{
    IndexedNode, Node, NodeKind, Overview, Purpose, PurposeSource, PurposeStatus,
    normalize_native_path_display, normalize_repo_path_prefix,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::num::TryFromIntError;
use std::path::Path;
use thiserror::Error;

/// Current `SQLite` schema version supported by this crate.
const SCHEMA_VERSION: i64 = 6;

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
}

/// Bounded health findings page returned by the database layer.
#[derive(Clone, Debug, Eq, PartialEq)]
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
        recommendation: "Set or approve a one-line purpose in the ProjectAtlas index.",
    },
    PurposeHealthSpec {
        status: "suggested",
        category: "suggested-purpose-review",
        message: "Path has a generated purpose suggestion but no agent-approved purpose.",
        recommendation: "Inspect the folder/file summary and approve or correct the purpose in SQLite.",
    },
    PurposeHealthSpec {
        status: "stale",
        category: "stale-purpose",
        message: "Path changed after its purpose was approved.",
        recommendation: "Inspect the current summary and approve or correct the one-line purpose.",
    },
];

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
            CREATE INDEX IF NOT EXISTS idx_symbol_relations_path ON symbol_relations(path);
            CREATE INDEX IF NOT EXISTS idx_symbol_relations_target ON symbol_relations(target_name);
            CREATE INDEX IF NOT EXISTS idx_health_resolutions_category ON health_resolutions(category);
            CREATE INDEX IF NOT EXISTS idx_file_texts_hash ON file_texts(content_hash);
            ",
        )?;
        self.ensure_symbol_metadata_columns()?;
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
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM symbols WHERE path = ?1", [&graph.path])?;
        transaction.execute(
            "DELETE FROM symbol_relations WHERE path = ?1",
            [&graph.path],
        )?;
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

    /// Clear symbols and relations for one live file path while preserving node summaries.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn clear_symbol_graph_for_path(&self, path: &str) -> DbResult<()> {
        self.connection
            .execute("DELETE FROM symbols WHERE path = ?1", [path])?;
        self.connection
            .execute("DELETE FROM symbol_relations WHERE path = ?1", [path])?;
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
        let node_id =
            self.connection
                .query_row("SELECT id FROM nodes WHERE path = ?1", [path], |row| {
                    row.get::<_, i64>(0)
                })?;
        Ok(node_id)
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
                WHERE n.exists_now = 1
                  AND n.kind = ?
            "
        );
        let mut values = Vec::new();
        for term in &terms {
            let pattern = sqlite_like_pattern(term);
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
            unfiltered_total += self.count_purpose_status_findings(spec, None, resolved_ids)?;
            if !purpose_health_spec_matches_query(spec, query) {
                continue;
            }

            let matching_count = self.count_purpose_status_findings(
                spec,
                query.path_prefix.as_deref(),
                resolved_ids,
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
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        }

        for category in ["duplicate-purpose", "repeated-temporary-folder"] {
            let unfiltered_count =
                self.count_structural_health_findings(category, None, resolved_ids)?;
            unfiltered_total += unfiltered_count;
            if !health_category_matches_query(category, Severity::Warning, query) {
                continue;
            }
            let matching_count = self.count_structural_health_findings(
                category,
                query.path_prefix.as_deref(),
                resolved_ids,
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
    fn count_purpose_status_findings(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
    ) -> DbResult<usize> {
        let (where_clause, values) = purpose_status_where_clause(spec, path_prefix, resolved_ids);
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
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_status_where_clause(spec, path_prefix, resolved_ids);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY n.path
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
    ) -> DbResult<usize> {
        match category {
            "duplicate-purpose" => self.count_duplicate_purpose_findings(path_prefix, resolved_ids),
            "repeated-temporary-folder" => {
                self.count_repeated_temp_folder_findings(path_prefix, resolved_ids)
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
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        match category {
            "duplicate-purpose" => self.load_duplicate_purpose_findings_page(
                path_prefix,
                resolved_ids,
                start_index,
                limit,
            ),
            "repeated-temporary-folder" => self.load_repeated_temp_folder_findings_page(
                path_prefix,
                resolved_ids,
                start_index,
                limit,
            ),
            _ => Ok(Vec::new()),
        }
    }

    /// Count duplicate-purpose findings directly in `SQLite`.
    fn count_duplicate_purpose_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
    ) -> DbResult<usize> {
        let (where_clause, values) =
            structural_finding_where_clause("duplicate-purpose", path_prefix, resolved_ids, 1);
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       p.purpose,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, purpose, related_path
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
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            structural_finding_where_clause("duplicate-purpose", path_prefix, resolved_ids, 1);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       p.purpose,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose)
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = 'approved'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, purpose, related_path
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
    ) -> DbResult<usize> {
        let mut total = 0_usize;
        for bucket in TEMP_FOLDER_BUCKETS {
            total +=
                self.count_repeated_temp_folder_bucket_findings(bucket, path_prefix, resolved_ids)?;
        }
        Ok(total)
    }

    /// Count one repeated temporary-folder bucket directly in `SQLite`.
    fn count_repeated_temp_folder_bucket_findings(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
    ) -> DbResult<usize> {
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            "repeated-temporary-folder",
            path_prefix,
            resolved_ids,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT path,
                       FIRST_VALUE(path) OVER (ORDER BY path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes
                WHERE exists_now = 1
                  AND kind = 'folder'
                  AND (lower(path) = ?1 OR lower(path) LIKE ?2)
            ),
            findings AS (
                SELECT path, related_path
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
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut total = 0_usize;
        let mut findings = Vec::new();
        for bucket in TEMP_FOLDER_BUCKETS {
            let matching_count =
                self.count_repeated_temp_folder_bucket_findings(bucket, path_prefix, resolved_ids)?;
            if findings.len() < limit && total + matching_count > start_index {
                let local_start = start_index.saturating_sub(total);
                let local_limit = limit - findings.len();
                findings.extend(self.load_repeated_temp_folder_bucket_findings_page(
                    bucket,
                    path_prefix,
                    resolved_ids,
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
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT path,
                       FIRST_VALUE(path) OVER (ORDER BY path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes
                WHERE exists_now = 1
                  AND kind = 'folder'
                  AND (lower(path) = ?1 OR lower(path) LIKE ?2)
            ),
            findings AS (
                SELECT path, related_path
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
        let mut statement = self.connection.prepare(
            "
            SELECT n.path, n.kind, p.purpose
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = 'approved'
              AND p.purpose IS NOT NULL
              AND (n.kind, lower(p.purpose)) IN (
                  SELECT n2.kind, lower(p2.purpose)
                  FROM nodes n2
                  JOIN purposes p2 ON p2.node_id = n2.id
                  WHERE n2.exists_now = 1
                    AND p2.status = 'approved'
                    AND p2.purpose IS NOT NULL
                  GROUP BY n2.kind, lower(p2.purpose)
                  HAVING COUNT(*) > 1
              )
            ORDER BY n.kind, lower(p.purpose), n.path
            ",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut current_key: Option<(String, String)> = None;
        let mut first_path = String::new();
        for row in rows {
            let (path, kind_value, purpose) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value.clone(),
            })?;
            let key = (kind_value, purpose.to_lowercase());
            if current_key.as_ref() != Some(&key) {
                current_key = Some(key);
                first_path = path;
                continue;
            }
            let finding = HealthFinding {
                id: finding_id("duplicate-purpose", &path, Some(&first_path)),
                severity: Severity::Warning,
                category: "duplicate-purpose".to_string(),
                path,
                related_path: Some(first_path.clone()),
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
                estimated_tokens_saved
            )
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                event.session_id,
                event.command,
                event.path,
                event.query,
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
                event.estimated_tokens_saved
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
                   estimated_tokens_with_projectatlas, estimated_tokens_saved
            FROM usage_events
            WHERE session_id = ?1
            ORDER BY id
            "
        } else {
            "
            SELECT session_id, command, path, query, estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas, estimated_tokens_saved
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
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            "
        } else {
            "
            SELECT
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            "
        };
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, i64>(0)?,
                token_total_from_sql("estimated_tokens_without_projectatlas", row.get(1)?),
                token_total_from_sql("estimated_tokens_with_projectatlas", row.get(2)?),
            ))
        };
        let (calls, without, with) = if let Some(session) = session_id {
            self.connection.query_row(sql, [session], mapper)?
        } else {
            self.connection.query_row(sql, [], mapper)?
        };
        let calls = row_token_count("token_overview_calls", calls)?;
        Ok(token_overview_from_totals(calls, without, with))
    }

    /// Mark a deterministic health finding as agent-resolved.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn resolve_health_finding(&self, resolution: &HealthResolution) -> DbResult<()> {
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
             + CASE WHEN lower(COALESCE(s.summary, '')) LIKE ? ESCAPE '\\' THEN 10 ELSE 0 END)"
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

/// Parse a stored purpose source value into the domain enum.
fn parse_source(value: &str) -> DbResult<PurposeSource> {
    let source = match value {
        "missing" => PurposeSource::Missing,
        "imported" => PurposeSource::Imported,
        "generated" => PurposeSource::Generated,
        "agent" => PurposeSource::Agent,
        "human" => PurposeSource::Human,
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

/// Convert one telemetry row count into a non-negative wide integer.
fn row_token_count(field: &'static str, value: i64) -> DbResult<u128> {
    u128::try_from(value).map_err(|source| DbError::InvalidCount {
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

/// Build a token overview from Rust-side aggregate totals.
fn token_overview_from_totals(calls: u128, without: u128, with: u128) -> TokenOverview {
    TokenOverview::from_estimated_totals(calls, without, with)
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

/// Build the shared SQL filter for purpose lifecycle health findings.
fn purpose_status_where_clause(
    spec: PurposeHealthSpec,
    path_prefix: Option<&str>,
    resolved_ids: &[String],
) -> (String, Vec<Value>) {
    let mut clauses = vec!["n.exists_now = 1".to_string(), "p.status = ?1".to_string()];
    let mut values = vec![Value::from(spec.status.to_string())];

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
    first_placeholder: usize,
) -> (String, Vec<Value>) {
    let mut placeholder = first_placeholder;
    let mut clauses = Vec::new();
    let mut values = Vec::new();

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
        store.record_usage(&UsageEvent {
            session_id: "session".to_string(),
            command: "outline".to_string(),
            path: Some("src/main.rs".to_string()),
            query: None,
            estimated_tokens_without_projectatlas: Some(100),
            estimated_tokens_with_projectatlas: Some(20),
            estimated_tokens_saved: Some(1),
        })?;
        store.record_usage(&UsageEvent {
            session_id: "session".to_string(),
            command: "unknown".to_string(),
            path: None,
            query: None,
            estimated_tokens_without_projectatlas: None,
            estimated_tokens_with_projectatlas: None,
            estimated_tokens_saved: None,
        })?;
        store.record_usage(&UsageEvent {
            session_id: "other-session".to_string(),
            command: "outline".to_string(),
            path: Some("src/lib.rs".to_string()),
            query: None,
            estimated_tokens_without_projectatlas: Some(200),
            estimated_tokens_with_projectatlas: Some(50),
            estimated_tokens_saved: Some(150),
        })?;
        let overview = store.token_overview(Some("session"))?;
        require_eq(&overview.calls, &1, "usage call count")?;
        require_eq(&overview.estimated_saved, &80, "saved token count")?;
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

        store.record_usage(&UsageEvent {
            session_id: "negative".to_string(),
            command: "outline".to_string(),
            path: None,
            query: None,
            estimated_tokens_without_projectatlas: Some(20),
            estimated_tokens_with_projectatlas: Some(50),
            estimated_tokens_saved: Some(999),
        })?;
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

        store.record_usage(&UsageEvent {
            session_id: "zero-baseline".to_string(),
            command: "outline".to_string(),
            path: None,
            query: None,
            estimated_tokens_without_projectatlas: Some(0),
            estimated_tokens_with_projectatlas: Some(12),
            estimated_tokens_saved: Some(999),
        })?;
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
        store.replace_scan(&[
            test_folder_node("src/auth"),
            test_folder_node("src/ui"),
            test_file_node("src/auth/login.rs", "hash-login"),
            test_file_node("src/ui/button.rs", "hash-button"),
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
        require_eq(&symbols.len(), &1, "symbol count after replace")?;
        require_eq(&relations.len(), &1, "relation count after replace")?;
        require_eq(&symbols[0].exported, &true, "exported metadata")?;
        require_eq(
            &symbols[0].documentation,
            &Some("Run the application.".to_string()),
            "documentation metadata",
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
        let store = AtlasStore::in_memory()?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: "duplicate-purpose:a:b".to_string(),
            category: "duplicate-purpose".to_string(),
            path: "a".to_string(),
            related_path: Some("b".to_string()),
            rationale: "Paths intentionally mirror agent skill variants.".to_string(),
        })?;
        let ids = store.resolved_health_ids()?;
        require_eq(
            &ids,
            &vec!["duplicate-purpose:a:b".to_string()],
            "resolved ids",
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
