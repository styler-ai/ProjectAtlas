//! Purpose: Persist `ProjectAtlas` 3 indexes in `SQLite`.

use projectatlas_core::health::{HealthFinding, Severity, finding_id};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_core::telemetry::{TokenOverview, UsageEvent};
use projectatlas_core::{
    IndexedNode, Node, NodeKind, Overview, Purpose, PurposeSource, PurposeStatus,
    normalize_native_path_display,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// This removes symbols, relations, and the node-level observed summary so
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
        findings.extend(self.purpose_status_findings(
            "missing",
            "missing-purpose",
            "Path is indexed but has no approved purpose.",
            "Set or approve a one-line purpose in the ProjectAtlas index.",
        )?);
        findings.extend(self.purpose_status_findings(
            "suggested",
            "suggested-purpose-review",
            "Path has a generated purpose suggestion but no agent-approved purpose.",
            "Inspect the folder/file summary and approve or correct the purpose in SQLite.",
        )?);
        findings.extend(self.purpose_status_findings(
            "stale",
            "stale-purpose",
            "Path changed after its purpose was approved.",
            "Inspect the current summary and approve or correct the one-line purpose.",
        )?);
        findings.extend(self.duplicate_purpose_findings()?);
        findings.extend(self.repeated_temp_folder_findings()?);
        Ok(findings
            .into_iter()
            .filter(|finding| !resolved_ids.iter().any(|id| id == &finding.id))
            .collect())
    }

    /// Build findings for one purpose lifecycle status.
    fn purpose_status_findings(
        &self,
        status: &str,
        category: &str,
        message: &str,
        recommendation: &str,
    ) -> DbResult<Vec<HealthFinding>> {
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
        let rows = statement.query_map([status], |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            let path = row?;
            findings.push(HealthFinding {
                id: finding_id(category, &path, None),
                severity: Severity::Warning,
                category: category.to_string(),
                path,
                related_path: None,
                message: message.to_string(),
                recommendation: recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Build duplicate-purpose health findings through grouped SQL candidates.
    fn duplicate_purpose_findings(&self) -> DbResult<Vec<HealthFinding>> {
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
        let mut findings = Vec::new();
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
            findings.push(HealthFinding {
                id: finding_id("duplicate-purpose", &path, Some(&first_path)),
                severity: Severity::Warning,
                category: "duplicate-purpose".to_string(),
                path,
                related_path: Some(first_path.clone()),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation:
                    "Review whether these paths duplicate responsibility or need clearer purposes."
                        .to_string(),
            });
        }
        Ok(findings)
    }

    /// Build repeated temporary/generated folder findings.
    fn repeated_temp_folder_findings(&self) -> DbResult<Vec<HealthFinding>> {
        let suspicious = ["tmp", "temp", "cache", "generated", "out", "output"];
        let mut findings = Vec::new();
        for bucket in suspicious {
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
            let mut paths = Vec::new();
            for row in rows {
                paths.push(row?);
            }
            let Some(first_path) = paths.first().cloned() else {
                continue;
            };
            if paths.len() < 2 {
                continue;
            }
            for path in paths.into_iter().skip(1) {
                findings.push(HealthFinding {
                    id: finding_id("repeated-temporary-folder", &path, Some(&first_path)),
                    severity: Severity::Warning,
                    category: "repeated-temporary-folder".to_string(),
                    path,
                    related_path: Some(first_path.clone()),
                    message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                    recommendation:
                        "Consolidate temporary/generated output roots or add an allowlist rationale."
                            .to_string(),
                });
            }
        }
        Ok(findings)
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
                COALESCE(SUM(estimated_tokens_without_projectatlas), 0),
                COALESCE(SUM(estimated_tokens_with_projectatlas), 0),
                COALESCE(SUM(estimated_tokens_saved), 0)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
              AND estimated_tokens_saved IS NOT NULL
            "
        } else {
            "
            SELECT
                COUNT(*),
                COALESCE(SUM(estimated_tokens_without_projectatlas), 0),
                COALESCE(SUM(estimated_tokens_with_projectatlas), 0),
                COALESCE(SUM(estimated_tokens_saved), 0)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
              AND estimated_tokens_saved IS NOT NULL
            "
        };
        let mut statement = self.connection.prepare(sql)?;
        let counts = if let Some(session) = session_id {
            statement.query_row([session], token_overview_counts_from_row)?
        } else {
            statement.query_row([], token_overview_counts_from_row)?
        };
        token_overview_from_counts(counts)
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

/// Convert an aggregate signed database total into a platform `isize`.
fn count_to_isize(field: &'static str, value: i64) -> DbResult<isize> {
    isize::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Read token overview aggregate counts from a SQL row.
fn token_overview_counts_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(i64, i64, i64, i64)> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, i64>(3)?,
    ))
}

/// Build a token overview from aggregate SQL counts.
fn token_overview_from_counts(counts: (i64, i64, i64, i64)) -> DbResult<TokenOverview> {
    let calls = count_to_usize("usage_calls", counts.0)?;
    let without = count_to_usize("estimated_tokens_without_projectatlas", counts.1)?;
    let with = count_to_usize("estimated_tokens_with_projectatlas", counts.2)?;
    let saved = count_to_isize("estimated_tokens_saved", counts.3)?;
    let savings_rate = if without == 0 {
        None
    } else {
        Some(saved as f64 / without as f64)
    };
    Ok(TokenOverview {
        calls,
        estimated_without_projectatlas: without,
        estimated_with_projectatlas: with,
        estimated_saved: saved,
        savings_rate,
    })
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

/// Generate the durable node-level observed summary.
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
            estimated_tokens_saved: Some(80),
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
    fn updates_observed_node_summary_without_approving_purpose() -> Result<(), Box<dyn Error>> {
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
            "updated observed summary",
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
