//! Complete, bounded compatibility reads of the stored symbol-relation rows.

use super::{AtlasStore, DbError, DbResult, SqliteReadProgressGuard, like_query};
use projectatlas_core::graph::GraphContractError;
use projectatlas_core::symbols::{ParserKind, RelationKind, SymbolRelation};
use projectatlas_core::{IndexWorkControl, IndexWorkStage};
use rusqlite::limits::Limit;
use rusqlite::{Connection, ErrorCode, Row, params};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Validated resource envelope for one complete legacy relation array.
#[derive(Clone, Copy, Debug)]
pub struct LegacyRelationReadBudget {
    /// Maximum complete eligible rows.
    rows: usize,
    /// Maximum cooperative time spent inside this reader.
    deadline: Duration,
    /// Maximum decoded source and retained candidate bytes.
    intermediate_bytes: u64,
}

impl LegacyRelationReadBudget {
    /// Maximum UTF-8 query bytes before adding the two LIKE wildcards.
    pub const MAX_QUERY_BYTES: usize = 1_024;
    /// Absolute compatibility-array row ceiling.
    pub const MAX_ROWS: usize = 200;
    /// Absolute decoded/intermediate ceiling.
    pub const MAX_INTERMEDIATE_BYTES: u64 = 1_024 * 1_024;
    /// Shared instruction ceiling across candidate discovery and hydration.
    pub const MAX_INSTRUCTIONS: u64 = 100_000;
    /// Maximum cooperative query duration.
    pub const MAX_DEADLINE: Duration = Duration::from_millis(1_000);

    /// Validate a complete-array resource envelope.
    ///
    /// # Arguments
    ///
    /// `rows` bounds the complete result; `deadline` and `intermediate_bytes`
    /// may tighten the absolute query ceilings.
    ///
    /// # Returns
    ///
    /// A validated read budget.
    ///
    /// # Errors
    ///
    /// Rejects zero or oversized bounds before database work.
    pub fn new(rows: usize, deadline: Duration, intermediate_bytes: u64) -> DbResult<Self> {
        if rows == 0
            || rows > Self::MAX_ROWS
            || deadline.is_zero()
            || deadline > Self::MAX_DEADLINE
            || intermediate_bytes == 0
            || intermediate_bytes > Self::MAX_INTERMEDIATE_BYTES
        {
            return Err(GraphContractError::InvalidLimits {
                reason: "legacy relations require 1..200 rows, at most 1000 ms and 1 MiB intermediate bytes; use anchored detailed navigation for larger results",
            }.into());
        }
        Ok(Self {
            rows,
            deadline,
            intermediate_bytes,
        })
    }
}

/// Causal work performed by one complete compatibility read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LegacyRelationReadWork {
    /// Executed SQLite instructions, including all candidate and hydration statements.
    pub instructions: u64,
    /// Candidate identifiers observed, including the refusal sentinel.
    pub candidate_rows: u32,
    /// Admitted candidate storage, relation structures and raw text bytes.
    pub decoded_bytes: u64,
}

/// Complete legacy rows plus bounded owning-reader work.
#[derive(Debug)]
pub struct LegacyRelationRead {
    /// Complete eligible rows in deterministic legacy order.
    pub relations: Vec<SymbolRelation>,
    /// Actual candidate, instruction and byte work.
    pub work: LegacyRelationReadWork,
}

/// Resource that prevented a complete compatibility result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRelationReadLimit {
    /// More eligible rows exist than the requested complete-array bound.
    Rows,
    /// The shared SQLite instruction ceiling was reached.
    Instructions,
    /// The cooperative query deadline was reached.
    Deadline,
    /// Candidate or decoded result state cannot fit.
    IntermediateBytes,
    /// SQLite refused a wide value before owned decoding.
    ValueBytes,
    /// Query text exceeds the admitted UTF-8 pattern bytes.
    QueryBytes,
}

/// Scoped progress and native-value limits for one compatibility read.
struct LegacyReadGuard<'connection> {
    /// Reuses the storage owner's progress-hook cleanup.
    progress: SqliteReadProgressGuard<'connection>,
    /// Native length limit restored after this read.
    previous_length: i32,
    /// Native LIKE limit restored after this read.
    previous_pattern_length: i32,
    /// Shared instruction counter used by every prepared statement.
    instructions: Arc<AtomicU64>,
    /// Query-local deadline, independent of earlier source verification work.
    deadline: Instant,
}

impl<'connection> LegacyReadGuard<'connection> {
    /// Install one instruction-level callback and a bounded SQLite value length.
    fn new(
        connection: &'connection Connection,
        budget: LegacyRelationReadBudget,
        control: &IndexWorkControl,
    ) -> DbResult<Self> {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        let deadline = Instant::now().checked_add(budget.deadline).ok_or(
            GraphContractError::InvalidLimits {
                reason: "legacy deadline is unrepresentable",
            },
        )?;
        let previous_length = connection.limit(Limit::SQLITE_LIMIT_LENGTH)?;
        let previous_pattern_length = connection.limit(Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH)?;
        let length =
            i32::try_from(budget.intermediate_bytes).map_err(|source| DbError::InvalidCount {
                field: "legacy intermediate bytes",
                value: i64::MAX,
                source,
            })?;
        connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, previous_length.min(length))?;
        let mut guard = Self {
            progress: SqliteReadProgressGuard {
                connection,
                armed: false,
            },
            previous_length,
            previous_pattern_length,
            instructions: Arc::new(AtomicU64::new(0)),
            deadline,
        };
        connection.set_limit(
            Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
            previous_pattern_length.min((LegacyRelationReadBudget::MAX_QUERY_BYTES + 2) as i32),
        )?;
        let instructions = Arc::clone(&guard.instructions);
        let control = control.clone();
        connection.progress_handler(
            1,
            Some(move || {
                instructions.fetch_add(1, Ordering::Relaxed)
                    >= LegacyRelationReadBudget::MAX_INSTRUCTIONS - 1
                    || Instant::now() >= deadline
                    || control.check(IndexWorkStage::RepositoryTraversal).is_err()
            }),
        )?;
        guard.progress.armed = true;
        Ok(guard)
    }

    /// Restore the more restrictive remaining-byte value bound before row hydration.
    fn admit_value(&self, remaining: u64) -> DbResult<()> {
        let remaining = i32::try_from(remaining).map_err(|source| DbError::InvalidCount {
            field: "legacy remaining bytes",
            value: i64::MAX,
            source,
        })?;
        self.progress.connection.set_limit(
            Limit::SQLITE_LIMIT_LENGTH,
            self.previous_length.min(remaining),
        )?;
        Ok(())
    }

    /// Observe all query instructions and preserve caller cancellation/deadline errors.
    fn check(&self, control: &IndexWorkControl, work: &mut LegacyRelationReadWork) -> DbResult<()> {
        work.instructions = self.instructions.load(Ordering::Relaxed);
        control.check(IndexWorkStage::RepositoryTraversal)?;
        if work.instructions >= LegacyRelationReadBudget::MAX_INSTRUCTIONS {
            return Err(read_limit(LegacyRelationReadLimit::Instructions, *work));
        }
        if Instant::now() >= self.deadline {
            return Err(read_limit(LegacyRelationReadLimit::Deadline, *work));
        }
        Ok(())
    }
}

impl Drop for LegacyReadGuard<'_> {
    fn drop(&mut self) {
        drop(self.progress.connection.set_limit(
            Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
            self.previous_pattern_length,
        ));
        // This connection's prior nonnegative native limit remains valid.
        drop(
            self.progress
                .connection
                .set_limit(Limit::SQLITE_LIMIT_LENGTH, self.previous_length),
        );
    }
}

/// Construct a typed complete-array refusal without exposing stored text.
fn read_limit(limit: LegacyRelationReadLimit, work: LegacyRelationReadWork) -> DbError {
    DbError::LegacyRelationReadLimit { limit, work }
}

/// Admit logical candidate/result storage before retaining or copying it.
fn admit_bytes(
    work: &mut LegacyRelationReadWork,
    bytes: u64,
    budget: LegacyRelationReadBudget,
) -> DbResult<()> {
    let observed = work.decoded_bytes.saturating_add(bytes);
    if observed > budget.intermediate_bytes {
        return Err(read_limit(
            LegacyRelationReadLimit::IntermediateBytes,
            *work,
        ));
    }
    work.decoded_bytes = observed;
    Ok(())
}

impl AtlasStore {
    /// Read a complete small legacy relation array with bounded SQLite and decode work.
    ///
    /// # Arguments
    ///
    /// `file` and `query` preserve the low-level accessor's exact-file and SQL LIKE
    /// filtering. `budget` bounds the complete result; `control` retains caller
    /// cancellation and earlier deadlines.
    ///
    /// # Returns
    ///
    /// All eligible rows in path/line/source/target/identity order and causal work.
    /// No partial successful array is returned.
    ///
    /// # Errors
    ///
    /// Returns typed resource refusal, caller work failure, invalid stored values
    /// or SQLite errors. Scoped progress hooks and value limits are restored.
    pub fn load_symbol_relations_bounded(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        budget: LegacyRelationReadBudget,
        control: &IndexWorkControl,
    ) -> DbResult<LegacyRelationRead> {
        let mut work = LegacyRelationReadWork::default();
        if query.is_some_and(|query| query.len() > LegacyRelationReadBudget::MAX_QUERY_BYTES) {
            return Err(read_limit(LegacyRelationReadLimit::QueryBytes, work));
        }
        let snapshot = if self.connection.is_autocommit() {
            Some(self.connection.unchecked_transaction()?)
        } else {
            None
        };
        if file.is_some_and(|value| value.len() as u64 > budget.intermediate_bytes)
            || query.is_some_and(|value| {
                value.len() as u64 > budget.intermediate_bytes.saturating_sub(2)
            })
        {
            return Err(read_limit(LegacyRelationReadLimit::IntermediateBytes, work));
        }
        let pattern = query.map(like_query);
        admit_bytes(
            &mut work,
            pattern.as_ref().map_or(0, |value| value.len() as u64),
            budget,
        )?;
        let guard = LegacyReadGuard::new(&self.connection, budget, control)?;
        let operation = (|| {
            // Completeness needs only a sentinel, so candidate discovery never sorts wide keys.
            let candidates = match (file, query) {
                (Some(file), Some(_query)) => {
                    let mut statement = self.connection.prepare("SELECT id FROM symbol_relations WHERE path = ?1 AND (source_name LIKE ?2 OR target_name LIKE ?2 OR context LIKE ?2) LIMIT ?3")?;
                    collect_candidates(
                        &mut statement,
                        params![file, pattern.as_deref(), (budget.rows + 1) as i64],
                        budget,
                        &mut work,
                    )?
                }
                (Some(file), None) => {
                    let mut statement = self
                        .connection
                        .prepare("SELECT id FROM symbol_relations WHERE path = ?1 LIMIT ?2")?;
                    collect_candidates(
                        &mut statement,
                        params![file, (budget.rows + 1) as i64],
                        budget,
                        &mut work,
                    )?
                }
                (None, Some(_query)) => {
                    let mut statement = self.connection.prepare("SELECT id FROM symbol_relations WHERE source_name LIKE ?1 OR target_name LIKE ?1 OR context LIKE ?1 OR path LIKE ?1 LIMIT ?2")?;
                    collect_candidates(
                        &mut statement,
                        params![pattern.as_deref(), (budget.rows + 1) as i64],
                        budget,
                        &mut work,
                    )?
                }
                (None, None) => {
                    let mut statement = self
                        .connection
                        .prepare("SELECT id FROM symbol_relations LIMIT ?1")?;
                    collect_candidates(
                        &mut statement,
                        params![(budget.rows + 1) as i64],
                        budget,
                        &mut work,
                    )?
                }
            };
            let mut statement = self.connection.prepare("SELECT path, source_name, target_name, kind, line, context, parser FROM symbol_relations WHERE id = ?1")?;
            admit_bytes(
                &mut work,
                (size_of::<(i64, SymbolRelation)>() + size_of::<SymbolRelation>()) as u64
                    * candidates.len() as u64,
                budget,
            )?;
            let mut relations = Vec::with_capacity(candidates.len());
            for id in candidates {
                guard.check(control, &mut work)?;
                guard.admit_value(budget.intermediate_bytes.saturating_sub(work.decoded_bytes))?;
                let relation =
                    statement.query_row([id], |row| bounded_relation(row, budget, &mut work))??;
                relations.push((id, relation));
            }
            relations.sort_unstable_by(|(left_id, left), (right_id, right)| {
                (
                    &left.path,
                    left.line,
                    &left.source_name,
                    &left.target_name,
                    left_id,
                )
                    .cmp(&(
                        &right.path,
                        right.line,
                        &right.source_name,
                        &right.target_name,
                        right_id,
                    ))
            });
            Ok(relations
                .into_iter()
                .map(|(_, relation)| relation)
                .collect())
        })();
        guard.check(control, &mut work)?;
        drop(guard);
        match operation {
            Ok(relations) => {
                if let Some(snapshot) = snapshot {
                    snapshot.commit()?;
                }
                Ok(LegacyRelationRead { relations, work })
            }
            Err(DbError::Sqlite(error)) if error.sqlite_error_code() == Some(ErrorCode::TooBig) => {
                Err(read_limit(LegacyRelationReadLimit::ValueBytes, work))
            }
            Err(DbError::LegacyRelationReadLimit { limit, .. }) => Err(read_limit(limit, work)),
            Err(error) => Err(error),
        }
    }
}

/// Retain only row identifiers and refuse as soon as a completeness sentinel exists.
fn collect_candidates<P: rusqlite::Params>(
    statement: &mut rusqlite::Statement<'_>,
    params: P,
    budget: LegacyRelationReadBudget,
    work: &mut LegacyRelationReadWork,
) -> DbResult<Vec<i64>> {
    admit_bytes(work, (size_of::<i64>() * budget.rows) as u64, budget)?;
    let mut rows = statement.query(params)?;
    let mut candidates = Vec::with_capacity(budget.rows);
    while let Some(row) = rows.next()? {
        work.candidate_rows += 1;
        if candidates.len() == budget.rows {
            return Err(read_limit(LegacyRelationReadLimit::Rows, *work));
        }
        candidates.push(row.get(0)?);
    }
    Ok(candidates)
}

/// Check raw text widths before any owned relation decoding.
fn bounded_relation(
    row: &Row<'_>,
    budget: LegacyRelationReadBudget,
    work: &mut LegacyRelationReadWork,
) -> rusqlite::Result<DbResult<SymbolRelation>> {
    let mut bytes = 0_u64;
    for index in [0, 1, 2, 3, 5, 6] {
        bytes = bytes.saturating_add(row.get_ref(index)?.as_bytes()?.len() as u64);
    }
    if let Err(error) = admit_bytes(work, bytes, budget) {
        return Ok(Err(error));
    }
    let kind_value: String = row.get(3)?;
    let Some(kind) = RelationKind::from_db(&kind_value) else {
        return Ok(Err(DbError::InvalidEnum {
            field: "symbol relation kind",
            value: kind_value,
        }));
    };
    let raw_line = row.get::<_, i64>(4)?;
    let line = match usize::try_from(raw_line) {
        Ok(line) => line,
        Err(source) => {
            return Ok(Err(DbError::InvalidCount {
                field: "symbol relation line",
                value: raw_line,
                source,
            }));
        }
    };
    Ok(Ok(SymbolRelation {
        path: row.get(0)?,
        source_name: row.get(1)?,
        target_name: row.get(2)?,
        kind,
        line,
        context: row.get(5)?,
        parser: ParserKind::from_db(row.get_ref(6)?.as_str()?),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::symbols::SymbolGraph;
    use projectatlas_core::{IndexCancellation, IndexWorkFailure};
    use std::error::Error;
    use std::io;

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), None)
    }

    fn budget(rows: usize) -> DbResult<LegacyRelationReadBudget> {
        LegacyRelationReadBudget::new(
            rows,
            LegacyRelationReadBudget::MAX_DEADLINE,
            LegacyRelationReadBudget::MAX_INTERMEDIATE_BYTES,
        )
    }

    fn graph(count: usize) -> SymbolGraph {
        SymbolGraph {
            path: "lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: (0..count)
                .rev()
                .map(|index| SymbolRelation {
                    path: "lib.rs".to_string(),
                    source_name: format!("source_{index:05}"),
                    target_name: "target".to_string(),
                    kind: RelationKind::Calls,
                    line: index / 2 + 1,
                    context: format!("target({index});"),
                    parser: ParserKind::TreeSitter,
                })
                .collect(),
        }
    }

    fn limits(connection: &Connection) -> rusqlite::Result<(i32, i32)> {
        Ok((
            connection.limit(Limit::SQLITE_LIMIT_LENGTH)?,
            connection.limit(Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH)?,
        ))
    }

    #[test]
    fn complete_arrays_keep_filters_order_and_native_query_plans() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_symbol_graph(&graph(50))?;
        let original_limits = limits(&store.connection)?;
        for (file, query) in [
            (None, None),
            (Some("lib.rs"), None),
            (None, Some("source_0001_")),
            (Some("lib.rs"), Some("source_0001_")),
        ] {
            let old = store.load_symbol_relations(file, query, 200)?;
            let read = store.load_symbol_relations_bounded(file, query, budget(50)?, &control())?;
            require(
                read.relations == old,
                "complete legacy fields or order changed",
            )?;
            require(
                read.work.instructions > 0 && read.work.instructions < 100_000,
                "candidate and hydration instructions not counted",
            )?;
            require(
                read.work.decoded_bytes > 0 && read.work.decoded_bytes <= 1_024 * 1_024,
                "retained bytes not bounded",
            )?;
            require(
                limits(&store.connection)? == original_limits,
                "native limits leaked",
            )?;
        }
        for (sql, indexed) in [
            (
                "EXPLAIN QUERY PLAN SELECT id FROM symbol_relations WHERE path = 'lib.rs' LIMIT 51",
                true,
            ),
            (
                "EXPLAIN QUERY PLAN SELECT id FROM symbol_relations WHERE source_name LIKE '%absent%' LIMIT 51",
                false,
            ),
        ] {
            let plans = store
                .connection
                .prepare(sql)?
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            require(
                !plans.iter().any(|plan| plan.contains("TEMP B-TREE")),
                "candidate query sorts wide keys",
            )?;
            if indexed {
                require(
                    plans.iter().any(|plan| plan.contains("INDEX")),
                    "file lookup lost its index",
                )?;
            }
        }
        let mut tied = graph(2);
        for (index, relation) in tied.relations.iter_mut().enumerate() {
            relation.source_name = "same".to_string();
            relation.line = 1;
            relation.context = format!("ordered_{index}");
        }
        store.replace_symbol_graph(&tied)?;
        store.begin_index_read_snapshot()?;
        let read = store.load_symbol_relations_bounded(None, None, budget(2)?, &control())?;
        require(
            read.relations
                .iter()
                .map(|row| row.context.as_str())
                .collect::<Vec<_>>()
                == ["ordered_0", "ordered_1"],
            "equal sort keys lost the stable row-identity order",
        )?;
        require(
            !store.connection.is_autocommit(),
            "reader committed the caller's snapshot",
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn high_degree_refuses_without_hydration_and_sparse_queries_stop_by_vm_work()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_symbol_graph(&graph(10_001))?;
        let original_limits = limits(&store.connection)?;
        let broad = store.load_symbol_relations_bounded(None, None, budget(10)?, &control());
        require(
            matches!(
                broad,
                Err(DbError::LegacyRelationReadLimit {
                    limit: LegacyRelationReadLimit::Rows,
                    work: LegacyRelationReadWork {
                        candidate_rows: 11,
                        decoded_bytes: 80,
                        ..
                    },
                })
            ),
            "large result did not stop at ID-only sentinel",
        )?;
        let sparse =
            store.load_symbol_relations_bounded(None, Some("absent"), budget(10)?, &control());
        require(
            matches!(
                sparse,
                Err(DbError::LegacyRelationReadLimit {
                    limit: LegacyRelationReadLimit::Instructions,
                    work: LegacyRelationReadWork {
                        instructions: 100_000,
                        candidate_rows: 0,
                        decoded_bytes: 88
                    },
                })
            ),
            "sparse scan escaped shared VM bound",
        )?;
        require(
            limits(&store.connection)? == original_limits,
            "failed-read native limits leaked",
        )?;
        store.replace_symbol_graph(&graph(1))?;
        let retry = store.load_symbol_relations_bounded(
            Some("lib.rs"),
            Some("source_00000"),
            budget(1)?,
            &control(),
        )?;
        require(
            retry.relations.len() == 1,
            "read did not recover after VM interruption",
        )?;
        Ok(())
    }

    #[test]
    fn wide_values_pattern_caps_and_cancellation_restore_connection_state()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut source = graph(1);
        source.relations[0].context = "x".repeat(1_024 * 1_024 + 1);
        store.replace_symbol_graph(&source)?;
        let original_limits = limits(&store.connection)?;
        require(
            matches!(
                store.load_symbol_relations_bounded(None, None, budget(1)?, &control()),
                Err(DbError::LegacyRelationReadLimit {
                    limit: LegacyRelationReadLimit::ValueBytes,
                    ..
                })
            ),
            "wide stored value was not refused by native admission",
        )?;
        source.relations[0].context = "short".to_string();
        store.replace_symbol_graph(&source)?;
        for query in ["x".repeat(1_025), "é".repeat(513)] {
            require(
                matches!(
                    store.load_symbol_relations_bounded(None, Some(&query), budget(1)?, &control()),
                    Err(DbError::LegacyRelationReadLimit {
                        limit: LegacyRelationReadLimit::QueryBytes,
                        work: LegacyRelationReadWork {
                            instructions: 0,
                            ..
                        }
                    })
                ),
                "oversized pattern reached SQLite",
            )?;
        }
        for query in ["%".repeat(1_024), "é".repeat(512)] {
            let result =
                store.load_symbol_relations_bounded(None, Some(&query), budget(1)?, &control())?;
            require(
                result.work.instructions > 0,
                "maximum-byte pattern was not admitted",
            )?;
        }
        let cancellation = control();
        {
            let guard = LegacyReadGuard::new(&store.connection, budget(1)?, &cancellation)?;
            cancellation.cancel();
            require(
                store
                    .connection
                    .query_row("SELECT count(*) FROM symbol_relations", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .is_err(),
                "installed progress hook did not observe cancellation",
            )?;
            require(
                matches!(
                    guard.check(&cancellation, &mut LegacyRelationReadWork::default()),
                    Err(DbError::IndexWork(IndexWorkFailure::Cancelled { .. }))
                ),
                "cancellation lost its typed owner",
            )?;
        }
        require(
            limits(&store.connection)? == original_limits,
            "native limits leaked after failure",
        )?;
        require(
            store
                .load_symbol_relations_bounded(None, None, budget(1)?, &control())?
                .relations
                .len()
                == 1,
            "next read inherited cancellation or limits",
        )?;
        let tiny = LegacyRelationReadBudget::new(1, Duration::from_nanos(1), 1_024 * 1_024)?;
        require(
            matches!(
                store.load_symbol_relations_bounded(None, None, tiny, &control()),
                Err(DbError::LegacyRelationReadLimit {
                    limit: LegacyRelationReadLimit::Deadline,
                    ..
                })
            ),
            "local deadline was not observed",
        )?;
        require(
            limits(&store.connection)? == original_limits,
            "deadline leaked native limits",
        )?;
        Ok(())
    }
}
