//! `SQLite` schema initialization and legacy repair behind the store facade.

use crate::{DbError, DbResult};
use rusqlite::{Connection, OptionalExtension};

/// Current `SQLite` schema version supported by this crate.
const SCHEMA_VERSION: i64 = 8;

/// Initialize the current schema and repair supported legacy columns.
pub(crate) fn initialize(connection: &Connection) -> DbResult<()> {
    reset_legacy_summary_schema(connection)?;
    connection.execute_batch(
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
    ensure_symbol_metadata_columns(connection)?;
    ensure_usage_event_metadata_columns(connection)?;
    connection.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage_events(created_at);
        CREATE INDEX IF NOT EXISTS idx_usage_session_created_at ON usage_events(session_id, created_at);
        ",
    )?;
    reconcile_schema_version(connection)
}

/// Reconcile the supported metadata schema version without changing its contract.
fn reconcile_schema_version(connection: &Connection) -> DbResult<()> {
    let stored = connection
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
                connection.execute(
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
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('schema_version', ?1)",
                [SCHEMA_VERSION.to_string()],
            )?;
        }
    }
    Ok(())
}

/// Add usage telemetry metadata columns to older databases.
fn ensure_usage_event_metadata_columns(connection: &Connection) -> DbResult<()> {
    let columns = table_columns(connection, "usage_events")?;
    for (name, definition) in [
        (
            "token_savings_bucket",
            "TEXT NOT NULL DEFAULT 'navigation_avoidance'",
        ),
        ("provider", "TEXT NOT NULL DEFAULT 'heuristic'"),
        ("model", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("tokenizer_backend", "TEXT NOT NULL DEFAULT 'chars_div_4'"),
        ("accuracy", "TEXT NOT NULL DEFAULT 'heuristic_estimate'"),
        (
            "baseline_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        ),
        ("confidence", "TEXT NOT NULL DEFAULT 'inferred'"),
        (
            "calculation_trace",
            "TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'",
        ),
        (
            "accounting_layer",
            "TEXT NOT NULL DEFAULT 'modeled_avoidance'",
        ),
        (
            "estimate_method",
            "TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4'",
        ),
        (
            "denominator_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        ),
        ("baseline_identity", "TEXT NOT NULL DEFAULT ''"),
        ("baseline_fingerprint", "TEXT NOT NULL DEFAULT ''"),
        ("dedupe_scope", "TEXT NOT NULL DEFAULT 'session'"),
        ("created_at", "TEXT"),
    ] {
        ensure_usage_event_column(connection, &columns, name, definition)?;
    }
    connection.execute(
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
    connection.execute(
        "UPDATE usage_events SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL OR created_at = ''",
        [],
    )?;
    Ok(())
}

/// Add one usage-event column when it is absent.
fn ensure_usage_event_column(
    connection: &Connection,
    columns: &[String],
    name: &str,
    definition: &str,
) -> DbResult<()> {
    if columns.iter().any(|column| column == name) {
        return Ok(());
    }
    connection.execute(
        &format!("ALTER TABLE usage_events ADD COLUMN {name} {definition}"),
        [],
    )?;
    Ok(())
}

/// Add optional symbol metadata columns to older databases.
fn ensure_symbol_metadata_columns(connection: &Connection) -> DbResult<()> {
    let columns = table_columns(connection, "symbols")?;
    if !columns.iter().any(|column| column == "exported") {
        connection.execute(
            "ALTER TABLE symbols ADD COLUMN exported INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "documentation") {
        connection.execute("ALTER TABLE symbols ADD COLUMN documentation TEXT", [])?;
    }
    Ok(())
}

/// Return the declared columns for one trusted static table name.
fn table_columns(connection: &Connection, table: &str) -> DbResult<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

/// Drop an in-progress generated summary table that lacks multi-level keys.
fn reset_legacy_summary_schema(connection: &Connection) -> DbResult<()> {
    let exists = connection
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
    let columns = table_columns(connection, "summaries")?;
    if !columns.iter().any(|column| column == "subject") {
        connection.execute("DROP TABLE summaries", [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;

    #[test]
    fn store_facade_preserves_schema_and_legacy_repairs() -> Result<(), Box<dyn Error>> {
        let store = crate::AtlasStore::in_memory()?;
        store.initialize_schema()?;
        let connection = &store.connection;

        let version = connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if version != SCHEMA_VERSION.to_string() {
            return Err(io::Error::other("schema version changed during extraction").into());
        }
        if object_names(connection, "table")?
            != [
                "file_texts",
                "health_resolutions",
                "metadata",
                "nodes",
                "purposes",
                "source_parse_metadata",
                "summaries",
                "symbol_relations",
                "symbols",
                "usage_events",
            ]
        {
            return Err(io::Error::other("table inventory changed during extraction").into());
        }
        if object_names(connection, "index")?
            != [
                "idx_file_texts_hash",
                "idx_health_resolutions_category",
                "idx_nodes_kind",
                "idx_nodes_parent",
                "idx_purposes_status",
                "idx_source_parse_metadata_parser",
                "idx_summaries_level",
                "idx_summaries_summary",
                "idx_symbol_relations_path",
                "idx_symbol_relations_target",
                "idx_symbols_kind",
                "idx_symbols_name",
                "idx_symbols_path",
                "idx_usage_created_at",
                "idx_usage_session",
                "idx_usage_session_created_at",
            ]
        {
            return Err(io::Error::other("index inventory changed during extraction").into());
        }
        require_column_contract(
            connection,
            "summaries",
            "subject",
            ("TEXT", true, Some("''"), false),
        )?;
        require_column_contract(
            connection,
            "symbols",
            "exported",
            ("INTEGER", true, Some("0"), false),
        )?;
        require_column_contract(
            connection,
            "usage_events",
            "dedupe_scope",
            ("TEXT", true, Some("'session'"), false),
        )?;
        if index_columns(connection, "idx_usage_session_created_at")?
            != ["session_id", "created_at"]
        {
            return Err(io::Error::other("compound usage index changed during extraction").into());
        }

        let legacy = tempfile::tempdir()?;
        let legacy_path = legacy.path().join("legacy.db");
        seed_legacy_schema(&legacy_path, 7)?;
        let legacy_store = crate::AtlasStore::open(&legacy_path)?;
        let legacy_connection = &legacy_store.connection;
        let version = legacy_connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if version != SCHEMA_VERSION.to_string() {
            return Err(io::Error::other("supported schema version was not reconciled").into());
        }
        require_column_contract(
            legacy_connection,
            "summaries",
            "subject",
            ("TEXT", true, Some("''"), false),
        )?;
        require_column_contract(
            legacy_connection,
            "symbols",
            "exported",
            ("INTEGER", true, Some("0"), false),
        )?;
        require_column_contract(
            legacy_connection,
            "symbols",
            "documentation",
            ("TEXT", false, None, false),
        )?;
        require_column_contract(
            legacy_connection,
            "usage_events",
            "created_at",
            ("TEXT", false, None, false),
        )?;
        let repaired_usage = legacy_connection.query_row(
            "SELECT accounting_layer, denominator_kind, dedupe_scope, created_at FROM usage_events WHERE session_id = 'legacy-session'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        if repaired_usage.0 != "observed_delta"
            || repaired_usage.1 != "full_file"
            || repaired_usage.2 != "event"
            || repaired_usage.3.is_empty()
        {
            return Err(io::Error::other("legacy usage metadata was not repaired").into());
        }

        let future = tempfile::tempdir()?;
        let future_path = future.path().join("future.db");
        seed_legacy_schema(&future_path, SCHEMA_VERSION + 1)?;
        match crate::AtlasStore::open(&future_path) {
            Err(DbError::SchemaVersion { found, expected })
                if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "future schema returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("future schema version was accepted").into()),
        }
        Ok(())
    }

    fn seed_legacy_schema(path: &std::path::Path, version: i64) -> DbResult<()> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE summaries(id INTEGER PRIMARY KEY, node_id INTEGER NOT NULL, summary TEXT);
            CREATE TABLE symbols(
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                language TEXT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                parent TEXT,
                parser TEXT NOT NULL,
                detail TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE usage_events(
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
                estimated_tokens_saved,
                token_savings_bucket
            ) VALUES('legacy-session', 'legacy', 100, 20, 80, 'full_file_compression');
            ",
        )?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES('schema_version', ?1)",
            [version.to_string()],
        )?;
        Ok(())
    }

    fn require_column_contract(
        connection: &Connection,
        table: &str,
        column: &str,
        expected: (&str, bool, Option<&str>, bool),
    ) -> Result<(), Box<dyn Error>> {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)? != 0,
            ))
        })?;
        let mut actual = None;
        for row in rows {
            let (name, declared_type, not_null, default_value, primary_key) = row?;
            if name == column {
                actual = Some((declared_type, not_null, default_value, primary_key));
                break;
            }
        }
        let actual = actual
            .ok_or_else(|| io::Error::other(format!("missing {table}.{column} schema contract")))?;
        let expected = (
            expected.0.to_string(),
            expected.1,
            expected.2.map(str::to_string),
            expected.3,
        );
        if actual != expected {
            return Err(io::Error::other(format!(
                "{table}.{column} schema contract changed: {actual:?}"
            ))
            .into());
        }
        Ok(())
    }

    fn index_columns(connection: &Connection, index: &str) -> DbResult<Vec<String>> {
        let mut statement = connection.prepare(&format!("PRAGMA index_info({index})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(2))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    fn object_names(connection: &Connection, kind: &str) -> DbResult<Vec<String>> {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = statement.query_map([kind], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }
}
