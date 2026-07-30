-- Historical physical schema produced by opening a ProjectAtlas 0.3.11
-- database with ProjectAtlas 0.3.26. The later telemetry fields were appended
-- after created_at by the released ALTER TABLE compatibility path.

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
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE usage_events
    ADD COLUMN accounting_layer TEXT NOT NULL DEFAULT 'modeled_avoidance';
ALTER TABLE usage_events
    ADD COLUMN estimate_method TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4';
ALTER TABLE usage_events
    ADD COLUMN denominator_kind TEXT NOT NULL DEFAULT 'selected_candidates';
ALTER TABLE usage_events
    ADD COLUMN baseline_identity TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_events
    ADD COLUMN baseline_fingerprint TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_events
    ADD COLUMN dedupe_scope TEXT NOT NULL DEFAULT 'session';

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
CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage_events(created_at);
CREATE INDEX IF NOT EXISTS idx_usage_session_created_at ON usage_events(session_id, created_at);
