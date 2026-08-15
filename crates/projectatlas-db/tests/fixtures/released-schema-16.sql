-- Captured from the released ProjectAtlas v0.4.4 runtime for migration compatibility tests.
/* WARNING: Script requires that SQLITE_DBCONFIG_DEFENSIVE be disabled */
PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
INSERT INTO metadata VALUES('file_text_fts_source_revision','0');
INSERT INTO metadata VALUES('file_text_fts_projection_revision','0');
INSERT INTO metadata VALUES('project_root','__PROJECT_ROOT__');
INSERT INTO metadata VALUES('schema_version','16');
CREATE TABLE nodes (
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
CREATE TABLE purposes (
        node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
        purpose TEXT,
        source TEXT NOT NULL,
        status TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_by TEXT
    );
CREATE TABLE summaries (
        id INTEGER PRIMARY KEY,
        node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        summary_level TEXT NOT NULL DEFAULT 'node',
        subject TEXT NOT NULL DEFAULT '',
        summary TEXT,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(node_id, summary_level, subject)
    );
CREATE TABLE symbols (
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
CREATE TABLE symbol_relations (
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
CREATE TABLE health_resolutions (
        finding_id TEXT PRIMARY KEY,
        category TEXT NOT NULL,
        path TEXT NOT NULL,
        related_path TEXT,
        rationale TEXT NOT NULL,
        resolved_by TEXT NOT NULL DEFAULT 'agent',
        resolved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
CREATE TABLE file_texts (
        path TEXT PRIMARY KEY,
        content_hash TEXT,
        byte_count INTEGER NOT NULL,
        line_count INTEGER NOT NULL,
        content TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
CREATE TABLE source_parse_metadata (
        path TEXT PRIMARY KEY,
        language TEXT,
        source_parser TEXT NOT NULL,
        fact_parser TEXT NOT NULL,
        symbol_count INTEGER NOT NULL,
        relation_count INTEGER NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
CREATE TABLE project_identity (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        project_instance_id BLOB NOT NULL UNIQUE
            CHECK(
                typeof(project_instance_id) = 'blob'
                AND length(project_instance_id) = 16
                AND project_instance_id <> X'00000000000000000000000000000000'
            ),
        active_generation INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(active_generation) = 'integer' AND active_generation >= 0)
    );
INSERT INTO project_identity VALUES(1,X'e400bfb41e28bcd98f0e55d7448984e3',0);
CREATE TABLE graph_entities (
        entity_key BLOB PRIMARY KEY NOT NULL
            CHECK(typeof(entity_key) = 'blob' AND length(entity_key) = 32),
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        canonical_identity TEXT NOT NULL
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        entity_kind TEXT NOT NULL
            CHECK(entity_kind IN ('project', 'folder', 'file', 'package', 'symbol', 'external')),
        repository_path TEXT,
        package_manager TEXT,
        package_name TEXT,
        manifest_path TEXT,
        symbol_name TEXT,
        symbol_kind TEXT,
        symbol_parent TEXT,
        symbol_signature TEXT,
        external_system TEXT,
        external_identity TEXT,
        UNIQUE(project_instance_id, entity_key),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        FOREIGN KEY(repository_path) REFERENCES nodes(path) ON DELETE CASCADE,
        FOREIGN KEY(manifest_path) REFERENCES nodes(path) ON DELETE CASCADE,
        CHECK(repository_path IS NULL OR (typeof(repository_path) = 'text' AND length(repository_path) > 0)),
        CHECK(package_manager IS NULL OR (typeof(package_manager) = 'text' AND length(package_manager) > 0)),
        CHECK(package_name IS NULL OR (typeof(package_name) = 'text' AND length(package_name) > 0)),
        CHECK(manifest_path IS NULL OR (typeof(manifest_path) = 'text' AND length(manifest_path) > 0)),
        CHECK(symbol_name IS NULL OR (typeof(symbol_name) = 'text' AND length(symbol_name) > 0)),
        CHECK(symbol_kind IS NULL OR symbol_kind IN (
            'function', 'method', 'class', 'struct', 'enum', 'trait', 'interface',
            'module', 'type', 'value', 'import', 'package', 'workspace', 'dependency', 'unknown'
        )),
        CHECK(symbol_parent IS NULL OR (typeof(symbol_parent) = 'text' AND length(symbol_parent) > 0)),
        CHECK(symbol_signature IS NULL OR (typeof(symbol_signature) = 'text' AND length(symbol_signature) > 0)),
        CHECK(external_system IS NULL OR (typeof(external_system) = 'text' AND length(external_system) > 0)),
        CHECK(external_identity IS NULL OR (typeof(external_identity) = 'text' AND length(external_identity) > 0)),
        CHECK(
            (entity_kind = 'project'
                AND repository_path IS NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind IN ('folder', 'file')
                AND repository_path IS NOT NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'package'
                AND repository_path IS NULL
                AND package_manager IS NOT NULL AND package_name IS NOT NULL AND manifest_path IS NOT NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'symbol'
                AND repository_path IS NOT NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NOT NULL AND symbol_kind IS NOT NULL AND symbol_signature IS NOT NULL
                AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'external'
                AND repository_path IS NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NOT NULL AND external_identity IS NOT NULL)
        )
    );
CREATE TABLE graph_relations (
        relation_key BLOB PRIMARY KEY NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        canonical_identity TEXT NOT NULL
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        source_entity_key BLOB NOT NULL
            CHECK(typeof(source_entity_key) = 'blob' AND length(source_entity_key) = 32),
        relation_scope TEXT NOT NULL CHECK(relation_scope IN ('legacy', 'extended')),
        relation_kind TEXT NOT NULL,
        resolution_status TEXT NOT NULL
            CHECK(resolution_status IN ('resolved', 'ambiguous', 'unresolved', 'external')),
        target_entity_key BLOB
            CHECK(target_entity_key IS NULL OR (typeof(target_entity_key) = 'blob' AND length(target_entity_key) = 32)),
        reference_text TEXT
            CHECK(reference_text IS NULL OR (typeof(reference_text) = 'text' AND length(reference_text) > 0)),
        candidate_count INTEGER
            CHECK(candidate_count IS NULL OR (typeof(candidate_count) = 'integer' AND candidate_count > 0)),
        confidence TEXT NOT NULL CHECK(confidence IN ('exact', 'high', 'medium', 'low')),
        completeness TEXT NOT NULL CHECK(completeness IN ('complete', 'partial')),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        FOREIGN KEY(project_instance_id, source_entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, target_entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        CHECK(
            (relation_scope = 'legacy'
                AND relation_kind IN ('contains', 'imports', 'calls', 'depends-on'))
            OR (relation_scope = 'extended'
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'deploys', 'reads', 'writes'))
        ),
        CHECK(
            (resolution_status IN ('resolved', 'external')
                AND target_entity_key IS NOT NULL
                AND reference_text IS NULL AND candidate_count IS NULL)
            OR (resolution_status = 'ambiguous'
                AND target_entity_key IS NULL
                AND reference_text IS NOT NULL AND candidate_count IS NOT NULL)
            OR (resolution_status = 'unresolved'
                AND target_entity_key IS NULL
                AND reference_text IS NOT NULL AND candidate_count IS NULL)
        )
    );
CREATE TABLE graph_relation_occurrences (
        id INTEGER PRIMARY KEY,
        relation_key BLOB NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        file_path TEXT NOT NULL
            CHECK(typeof(file_path) = 'text' AND length(file_path) > 0),
        start_line INTEGER NOT NULL
            CHECK(typeof(start_line) = 'integer' AND start_line > 0),
        start_column INTEGER NOT NULL
            CHECK(typeof(start_column) = 'integer' AND start_column >= 0),
        end_line INTEGER NOT NULL
            CHECK(typeof(end_line) = 'integer' AND end_line > 0),
        end_column INTEGER NOT NULL
            CHECK(typeof(end_column) = 'integer' AND end_column >= 0),
        FOREIGN KEY(relation_key) REFERENCES graph_relations(relation_key) ON DELETE CASCADE,
        FOREIGN KEY(file_path) REFERENCES nodes(path) ON DELETE CASCADE,
        UNIQUE(relation_key, file_path, start_line, start_column, end_line, end_column),
        CHECK(end_line > start_line OR (end_line = start_line AND end_column >= start_column))
    );
CREATE TABLE graph_coverage (
        id INTEGER PRIMARY KEY,
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project', 'path')),
        scope_path TEXT
            CHECK(scope_path IS NULL OR (typeof(scope_path) = 'text' AND length(scope_path) > 0)),
        relation_scope TEXT CHECK(relation_scope IS NULL OR relation_scope IN ('legacy', 'extended')),
        relation_kind TEXT,
        state TEXT NOT NULL
            CHECK(state IN ('complete', 'partial', 'failed', 'ignored', 'oversized', 'quarantined', 'stale')),
        total INTEGER NOT NULL CHECK(typeof(total) = 'integer' AND total >= 0),
        covered INTEGER NOT NULL CHECK(typeof(covered) = 'integer' AND covered >= 0),
        omitted INTEGER NOT NULL CHECK(typeof(omitted) = 'integer' AND omitted >= 0),
        reason TEXT CHECK(reason IS NULL OR (typeof(reason) = 'text' AND length(reason) > 0)),
        reached_limit TEXT
            CHECK(reached_limit IS NULL OR reached_limit IN ('rows', 'occurrences', 'depth', 'output_bytes')),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        CHECK(
            (scope_kind = 'project' AND scope_path IS NULL)
            OR (scope_kind = 'path' AND scope_path IS NOT NULL)
        ),
        CHECK(
            (relation_scope IS NULL AND relation_kind IS NULL)
            OR (relation_scope = 'legacy'
                AND relation_kind IN ('contains', 'imports', 'calls', 'depends-on'))
            OR (relation_scope = 'extended'
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'deploys', 'reads', 'writes'))
        ),
        CHECK(total = covered + omitted),
        CHECK(
            (state = 'complete' AND omitted = 0 AND reason IS NULL AND reached_limit IS NULL)
            OR (state = 'partial' AND covered > 0 AND omitted > 0 AND reason IS NOT NULL)
            OR (state IN ('failed', 'ignored', 'oversized', 'quarantined', 'stale')
                AND covered = 0 AND omitted > 0 AND reason IS NOT NULL)
        )
    );
CREATE TABLE graph_resolution_keys (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        canonical_identity TEXT NOT NULL
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        PRIMARY KEY(project_instance_id, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT
    ) STRICT;
CREATE TABLE graph_entity_exports (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        entity_key BLOB NOT NULL
            CHECK(typeof(entity_key) = 'blob' AND length(entity_key) = 32),
        owner_path TEXT NOT NULL
            CHECK(typeof(owner_path) = 'text' AND length(owner_path) > 0),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        PRIMARY KEY(project_instance_id, entity_key, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id, entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, resolution_domain, key_digest)
            REFERENCES graph_resolution_keys(project_instance_id, resolution_domain, key_digest)
                ON DELETE CASCADE,
        FOREIGN KEY(owner_path) REFERENCES nodes(path) ON DELETE CASCADE
    ) STRICT;
CREATE TABLE graph_relation_dependencies (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        relation_key BLOB NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        owner_path TEXT NOT NULL
            CHECK(typeof(owner_path) = 'text' AND length(owner_path) > 0),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        PRIMARY KEY(project_instance_id, relation_key, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id, relation_key)
            REFERENCES graph_relations(project_instance_id, relation_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, resolution_domain, key_digest)
            REFERENCES graph_resolution_keys(project_instance_id, resolution_domain, key_digest)
                ON DELETE CASCADE,
        FOREIGN KEY(owner_path) REFERENCES nodes(path) ON DELETE CASCADE
    ) STRICT;
CREATE TABLE usage_instances (
        instance_row_id INTEGER PRIMARY KEY,
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        runtime_instance_id BLOB NOT NULL
            CHECK(
                length(runtime_instance_id) = 16
                AND runtime_instance_id <> X'00000000000000000000000000000000'
            ),
        owner TEXT NOT NULL
            CHECK(owner IN ('cli_invocation', 'mcp_process', 'library_handle', 'migrated_legacy')),
        caller_label TEXT
            CHECK(caller_label IS NULL OR (typeof(caller_label) = 'text' AND length(caller_label) > 0)),
        state TEXT NOT NULL DEFAULT 'active'
            CHECK(state IN ('active', 'sealed', 'expired')),
        started_at_epoch INTEGER NOT NULL
            CHECK(typeof(started_at_epoch) = 'integer' AND started_at_epoch >= 0),
        last_seen_at_epoch INTEGER NOT NULL
            CHECK(typeof(last_seen_at_epoch) = 'integer' AND last_seen_at_epoch >= started_at_epoch),
        sealed_at_epoch INTEGER
            CHECK(sealed_at_epoch IS NULL OR (typeof(sealed_at_epoch) = 'integer' AND sealed_at_epoch >= started_at_epoch)),
        raw_detail_complete INTEGER NOT NULL DEFAULT 1
            CHECK(raw_detail_complete IN (0, 1)),
        clock_anomaly INTEGER NOT NULL DEFAULT 0
            CHECK(clock_anomaly IN (0, 1)),
        CHECK(
            (state = 'active' AND sealed_at_epoch IS NULL)
            OR (state IN ('sealed', 'expired') AND sealed_at_epoch IS NOT NULL)
        ),
        UNIQUE(project_instance_id, runtime_instance_id)
    ) STRICT;
CREATE TABLE usage_bucket_dimensions (
        dimension_id INTEGER PRIMARY KEY,
        token_savings_bucket TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        tokenizer_backend TEXT NOT NULL,
        accuracy TEXT NOT NULL,
        baseline_kind TEXT NOT NULL,
        confidence TEXT NOT NULL,
        accounting_layer TEXT NOT NULL,
        estimate_method TEXT NOT NULL,
        denominator_kind TEXT NOT NULL,
        dedupe_scope TEXT NOT NULL,
        overflow INTEGER NOT NULL DEFAULT 0 CHECK(overflow IN (0, 1)),
        UNIQUE(
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
            overflow
        )
    ) STRICT;
INSERT INTO usage_bucket_dimensions VALUES(1,'<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>','<overflow>',1);
CREATE TABLE usage_instance_baselines (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        baseline_key BLOB NOT NULL
            CHECK(length(baseline_key) = 32),
        baseline_identity TEXT NOT NULL,
        baseline_fingerprint TEXT NOT NULL,
        denominator_kind TEXT NOT NULL,
        maximum_without INTEGER NOT NULL
            CHECK(typeof(maximum_without) = 'integer' AND maximum_without >= 0),
        emitted_with INTEGER NOT NULL
            CHECK(typeof(emitted_with) = 'integer' AND emitted_with >= 0),
        calls INTEGER NOT NULL
            CHECK(typeof(calls) = 'integer' AND calls > 0),
        witness_logical_bytes INTEGER NOT NULL
            CHECK(typeof(witness_logical_bytes) = 'integer' AND witness_logical_bytes >= 0),
        PRIMARY KEY(instance_row_id, baseline_key)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_labels (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        caller_label TEXT NOT NULL CHECK(length(caller_label) > 0),
        last_seen_at_epoch INTEGER NOT NULL CHECK(last_seen_at_epoch >= 0),
        detail_complete INTEGER NOT NULL DEFAULT 1 CHECK(detail_complete IN (0, 1)),
        PRIMARY KEY(project_instance_id, caller_label)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_global_aggregates (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(calls) = 'integer' AND calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(estimated_without) = 'integer' AND estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(estimated_with) = 'integer' AND estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_without) = 'integer' AND observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_with) = 'integer' AND observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_without) = 'integer' AND modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_with) = 'integer' AND modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(deduped_modeled_without) = 'integer' AND deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(deduped_modeled_with) = 'integer' AND deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(repeated_baselines) = 'integer' AND repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_file_read_replacements) = 'integer' AND observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_file_reads_avoided) = 'integer' AND modeled_file_reads_avoided >= 0),
        PRIMARY KEY(project_instance_id, dimension_id)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_instance_aggregates (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(instance_row_id, dimension_id)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_daily_aggregates (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        day_epoch INTEGER NOT NULL CHECK(day_epoch >= 0),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(project_instance_id, day_epoch, dimension_id)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_instance_daily_aggregates (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        day_epoch INTEGER NOT NULL CHECK(day_epoch >= 0),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(instance_row_id, day_epoch, dimension_id)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_retention_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        policy_version INTEGER NOT NULL CHECK(policy_version > 0),
        logical_byte_version INTEGER NOT NULL CHECK(logical_byte_version > 0),
        raw_rows INTEGER NOT NULL DEFAULT 0 CHECK(raw_rows >= 0),
        raw_logical_bytes INTEGER NOT NULL DEFAULT 0 CHECK(raw_logical_bytes >= 0),
        baseline_rows INTEGER NOT NULL DEFAULT 0 CHECK(baseline_rows >= 0),
        baseline_logical_bytes INTEGER NOT NULL DEFAULT 0 CHECK(baseline_logical_bytes >= 0),
        dimension_rows INTEGER NOT NULL DEFAULT 0 CHECK(dimension_rows >= 0),
        instance_rows INTEGER NOT NULL DEFAULT 0 CHECK(instance_rows >= 0),
        label_rows INTEGER NOT NULL DEFAULT 0 CHECK(label_rows >= 0),
        daily_rows INTEGER NOT NULL DEFAULT 0 CHECK(daily_rows >= 0),
        label_tombstone_rows INTEGER NOT NULL DEFAULT 0 CHECK(label_tombstone_rows >= 0),
        instance_tombstone_rows INTEGER NOT NULL DEFAULT 0 CHECK(instance_tombstone_rows >= 0),
        pruned_raw_rows INTEGER NOT NULL DEFAULT 0 CHECK(pruned_raw_rows >= 0),
        pruned_instance_rows INTEGER NOT NULL DEFAULT 0 CHECK(pruned_instance_rows >= 0),
        evicted_tombstones INTEGER NOT NULL DEFAULT 0 CHECK(evicted_tombstones >= 0),
        writes_since_checkpoint INTEGER NOT NULL DEFAULT 0 CHECK(writes_since_checkpoint >= 0),
        last_maintenance_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_maintenance_epoch >= 0),
        last_checkpoint_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_checkpoint_epoch >= 0),
        oldest_retained_epoch INTEGER,
        raw_detail_complete INTEGER NOT NULL DEFAULT 1 CHECK(raw_detail_complete IN (0, 1)),
        dimension_detail_complete INTEGER NOT NULL DEFAULT 1
            CHECK(dimension_detail_complete IN (0, 1)),
        label_history_complete INTEGER NOT NULL DEFAULT 1
            CHECK(label_history_complete IN (0, 1)),
        maintenance_pending INTEGER NOT NULL DEFAULT 0 CHECK(maintenance_pending IN (0, 1)),
        clock_anomaly INTEGER NOT NULL DEFAULT 0 CHECK(clock_anomaly IN (0, 1)),
        spill_state TEXT NOT NULL DEFAULT 'not_applicable'
            CHECK(spill_state = 'not_applicable'),
        checkpoint_state TEXT NOT NULL DEFAULT 'not_due'
            CHECK(checkpoint_state IN ('not_due', 'completed', 'busy', 'error'))
    ) STRICT;
INSERT INTO usage_retention_state VALUES(1,1,1,0,0,0,0,1,0,0,0,0,0,0,0,0,0,1786791223,0,NULL,1,1,1,0,0,'not_applicable','not_due');
CREATE TABLE usage_label_tombstones (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        caller_label TEXT NOT NULL,
        expired_at_epoch INTEGER NOT NULL CHECK(expired_at_epoch >= 0),
        last_instance_id BLOB
            CHECK(last_instance_id IS NULL OR length(last_instance_id) = 16),
        PRIMARY KEY(project_instance_id, caller_label)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_instance_tombstones (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        runtime_instance_id BLOB NOT NULL CHECK(length(runtime_instance_id) = 16),
        retired_at_epoch INTEGER NOT NULL CHECK(retired_at_epoch >= 0),
        PRIMARY KEY(project_instance_id, runtime_instance_id)
    ) STRICT, WITHOUT ROWID;
CREATE TABLE usage_events (
        id INTEGER PRIMARY KEY,
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        command TEXT NOT NULL,
        path TEXT,
        query TEXT,
        estimated_tokens_without_projectatlas INTEGER,
        estimated_tokens_with_projectatlas INTEGER,
        estimated_tokens_saved INTEGER,
        calculation_trace TEXT NOT NULL,
        baseline_identity TEXT NOT NULL,
        baseline_fingerprint TEXT NOT NULL,
        created_at_epoch INTEGER NOT NULL CHECK(created_at_epoch >= 0),
        logical_bytes INTEGER NOT NULL CHECK(logical_bytes >= 0)
    ) STRICT;
PRAGMA writable_schema=ON;
INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','file_text_fts','file_text_fts',0,'CREATE VIRTUAL TABLE file_text_fts USING fts5(
        content,
        content=''file_texts'',
        content_rowid=''rowid'',
        tokenize=''trigram case_sensitive 0''
    )');
CREATE TABLE IF NOT EXISTS 'file_text_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO file_text_fts_data VALUES(1,X'');
INSERT INTO file_text_fts_data VALUES(10,X'00000000000000');
CREATE TABLE IF NOT EXISTS 'file_text_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS 'file_text_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
CREATE TABLE IF NOT EXISTS 'file_text_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;
INSERT INTO file_text_fts_config VALUES('version',4);
CREATE INDEX idx_nodes_kind ON nodes(kind);
CREATE INDEX idx_nodes_parent ON nodes(parent_path);
CREATE INDEX idx_purposes_status ON purposes(status);
CREATE INDEX idx_summaries_level ON summaries(summary_level);
CREATE INDEX idx_summaries_summary ON summaries(summary);
CREATE INDEX idx_symbols_path ON symbols(path);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_kind ON symbols(kind);
CREATE INDEX idx_symbol_relations_path ON symbol_relations(path);
CREATE INDEX idx_symbol_relations_target ON symbol_relations(target_name);
CREATE INDEX idx_health_resolutions_category ON health_resolutions(category);
CREATE INDEX idx_file_texts_hash ON file_texts(content_hash);
CREATE INDEX idx_graph_entities_path
        ON graph_entities(repository_path, entity_kind, entity_key);
CREATE INDEX idx_graph_entities_package
        ON graph_entities(package_manager, package_name, manifest_path, entity_key);
CREATE INDEX idx_graph_entities_manifest_path
        ON graph_entities(manifest_path, entity_key);
CREATE INDEX idx_graph_entities_symbol
        ON graph_entities(repository_path, symbol_name, symbol_kind, symbol_parent, symbol_signature, entity_key);
CREATE INDEX idx_graph_entities_external
        ON graph_entities(external_system, external_identity, entity_key);
CREATE INDEX idx_graph_relations_source_kind
        ON graph_relations(source_entity_key, relation_scope, relation_kind, relation_key);
CREATE INDEX idx_graph_relations_target_kind
        ON graph_relations(target_entity_key, relation_scope, relation_kind, relation_key);
CREATE INDEX idx_graph_relations_kind_order
        ON graph_relations(relation_scope, relation_kind, relation_key);
CREATE INDEX idx_graph_relations_kind_resolution
        ON graph_relations(relation_scope, relation_kind, resolution_status, relation_key);
CREATE INDEX idx_graph_occurrences_file_span
        ON graph_relation_occurrences(file_path, start_line, start_column, relation_key);
CREATE UNIQUE INDEX idx_graph_coverage_identity
        ON graph_coverage(
            project_instance_id,
            scope_kind,
            ifnull(scope_path, ''),
            ifnull(relation_scope, ''),
            ifnull(relation_kind, '')
        );
CREATE INDEX idx_graph_coverage_scope_state
        ON graph_coverage(scope_kind, scope_path, state, id);
CREATE INDEX idx_graph_coverage_scope_order
        ON graph_coverage(scope_kind, scope_path, relation_scope, relation_kind, state, id);
CREATE INDEX idx_graph_coverage_path
        ON graph_coverage(scope_path, id);
CREATE INDEX idx_graph_coverage_relation_state
        ON graph_coverage(relation_scope, relation_kind, state, id);
CREATE INDEX idx_source_parse_metadata_source_parser_path
        ON source_parse_metadata(source_parser, path);
CREATE INDEX idx_source_parse_metadata_fact_parser_path
        ON source_parse_metadata(fact_parser, path);
CREATE INDEX idx_graph_coverage_discovery_state
        ON graph_coverage(state, scope_path, id);
CREATE INDEX idx_graph_coverage_discovery_reason
        ON graph_coverage(reason, scope_path, id);
CREATE UNIQUE INDEX idx_graph_relations_project_key
        ON graph_relations(project_instance_id, relation_key);
CREATE INDEX idx_graph_entity_exports_key
        ON graph_entity_exports(
            project_instance_id, resolution_domain, key_digest, entity_key
        );
CREATE INDEX idx_graph_entity_exports_owner
        ON graph_entity_exports(
            project_instance_id, owner_path, resolution_domain, key_digest, entity_key
        );
CREATE INDEX idx_graph_relation_dependencies_key
        ON graph_relation_dependencies(
            project_instance_id, resolution_domain, key_digest, owner_path, relation_key
        );
CREATE INDEX idx_graph_relation_dependencies_owner
        ON graph_relation_dependencies(
            project_instance_id, owner_path, resolution_domain, key_digest, relation_key
        );
CREATE INDEX idx_usage_instances_label_state
        ON usage_instances(project_instance_id, caller_label, state, started_at_epoch, instance_row_id);
CREATE INDEX idx_usage_instances_state_seen
        ON usage_instances(project_instance_id, state, last_seen_at_epoch, instance_row_id);
CREATE INDEX idx_usage_instances_retention
        ON usage_instances(state, last_seen_at_epoch, instance_row_id);
CREATE INDEX idx_usage_labels_seen
        ON usage_labels(project_instance_id, last_seen_at_epoch, caller_label);
CREATE INDEX idx_usage_labels_retention
        ON usage_labels(last_seen_at_epoch, project_instance_id, caller_label);
CREATE INDEX idx_usage_daily_retention
        ON usage_daily_aggregates(day_epoch, project_instance_id, dimension_id);
CREATE INDEX idx_usage_instance_daily_retention
        ON usage_instance_daily_aggregates(day_epoch, instance_row_id, dimension_id);
CREATE INDEX idx_usage_label_tombstones_expiry
        ON usage_label_tombstones(project_instance_id, expired_at_epoch, caller_label);
CREATE INDEX idx_usage_instance_tombstones_expiry
        ON usage_instance_tombstones(project_instance_id, retired_at_epoch, runtime_instance_id);
CREATE INDEX idx_usage_label_tombstones_retention
        ON usage_label_tombstones(expired_at_epoch, project_instance_id, caller_label);
CREATE INDEX idx_usage_instance_tombstones_retention
        ON usage_instance_tombstones(retired_at_epoch, project_instance_id, runtime_instance_id);
CREATE INDEX idx_usage_created_at
        ON usage_events(created_at_epoch, id);
CREATE INDEX idx_usage_instance_created
        ON usage_events(instance_row_id, created_at_epoch, id);
CREATE INDEX idx_symbol_import_alias_lookup
        ON symbol_relations(kind, path, line, source_name, target_name);
PRAGMA writable_schema=OFF;
COMMIT;
