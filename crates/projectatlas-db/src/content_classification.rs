//! Persist and batch file content classifications inside index publication.

use super::{AtlasStore, DbError, DbResult, IndexPublicationGuard, numbered_placeholders};
use projectatlas_core::Node;
use projectatlas_core::language::ContentClassification;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum exact paths admitted to one classification batch.
pub const MAX_FILE_CONTENT_CLASSIFICATION_PATHS: usize = 256;
/// Maximum rows returned by one classification/path page.
pub const MAX_FILE_CONTENT_CLASSIFICATION_PAGE_ROWS: u32 = 1_000;
/// Paths bound per statement, below supported `SQLite` variable ceilings.
const FILE_CONTENT_CLASSIFICATION_BIND_PATHS: usize = 48;

/// One persisted classification for an exact admitted file path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContentClassification {
    /// Repository-relative file path using forward slashes.
    pub path: String,
    /// Registry-owned closed content role.
    pub classification: ContentClassification,
}

/// One bounded classification/path page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContentClassificationPage {
    /// Rows in stable repository-path order.
    pub rows: Vec<FileContentClassification>,
    /// Whether at least one additional row exists.
    pub truncated: bool,
}

impl IndexPublicationGuard<'_> {
    /// Upsert one bounded classification batch inside the parent publication.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation for duplicate, blank, oversized, absent,
    /// inactive, or non-file paths, or when `SQLite` rejects the batch.
    pub fn upsert_file_content_classification_batch(
        &mut self,
        rows: &[FileContentClassification],
    ) -> DbResult<()> {
        validate_classification_batch(&self.store.connection, rows)?;
        if rows.is_empty() {
            return Ok(());
        }
        let values = (0..rows.len())
            .map(|index| format!("(?{}, ?{})", index * 2 + 1, index * 2 + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO file_content_classifications(path, classification)
             VALUES {values}
             ON CONFLICT(path) DO UPDATE SET classification = excluded.classification"
        );
        let parameters = rows
            .iter()
            .flat_map(|row| {
                [
                    Value::Text(row.path.clone()),
                    Value::Text(row.classification.as_str().to_string()),
                ]
            })
            .collect::<Vec<_>>();
        let savepoint = self.store.validated_savepoint()?;
        savepoint
            .prepare_cached(&sql)?
            .execute(params_from_iter(parameters))?;
        savepoint.commit()?;
        Ok(())
    }
}

impl AtlasStore {
    /// Load classifications for exact file paths in one bounded set of statements.
    ///
    /// # Errors
    ///
    /// Returns an error for an oversized request, missing classification, invalid
    /// persisted value, cancellation, or `SQLite` failure. No partial result is
    /// returned.
    pub fn file_content_classifications_for_paths(
        &self,
        paths: &[String],
    ) -> DbResult<Vec<FileContentClassification>> {
        let paths = unique_paths(paths)?;
        let mut by_path = BTreeMap::new();
        for chunk in paths.chunks(FILE_CONTENT_CLASSIFICATION_BIND_PATHS) {
            let placeholders = numbered_placeholders(1, chunk.len());
            let sql = format!(
                "SELECT path, classification
                   FROM file_content_classifications
                  WHERE path IN ({placeholders})
                  ORDER BY path"
            );
            let mut statement = self.connection.prepare_cached(&sql)?;
            let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (path, value) = row?;
                let classification = parse_classification(value)?;
                by_path.insert(path, classification);
            }
        }
        paths
            .into_iter()
            .map(|path| {
                let classification = by_path.remove(&path).ok_or_else(|| {
                    DbError::FileContentClassificationMissing { path: path.clone() }
                })?;
                Ok(FileContentClassification {
                    path,
                    classification,
                })
            })
            .collect()
    }

    /// Load one stable path page through the classification/path index.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit, corrupt persisted value, or
    /// `SQLite` failure.
    pub fn file_content_classification_page(
        &self,
        classification: ContentClassification,
        after_path: Option<&str>,
        limit: u32,
    ) -> DbResult<FileContentClassificationPage> {
        if limit == 0 || limit > MAX_FILE_CONTENT_CLASSIFICATION_PAGE_ROWS {
            return Err(DbError::FileContentClassificationLimit {
                requested: limit,
                maximum: MAX_FILE_CONTENT_CLASSIFICATION_PAGE_ROWS,
            });
        }
        let fetch = i64::from(limit) + 1;
        let mut statement = self.connection.prepare_cached(
            "SELECT path, classification
               FROM file_content_classifications
              WHERE classification = ?1 AND path > ?2
              ORDER BY classification, path
              LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![classification.as_str(), after_path.unwrap_or(""), fetch],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        let mut rows = rows
            .map(|row| {
                let (path, value) = row?;
                Ok(FileContentClassification {
                    path,
                    classification: parse_classification(value)?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        let truncated = rows.len() > limit as usize;
        if truncated {
            rows.pop();
        }
        Ok(FileContentClassificationPage { rows, truncated })
    }
}

/// Remove rows whose owning file node is no longer current.
pub(crate) fn delete_absent_file_content_classifications(connection: &Connection) -> DbResult<()> {
    connection.execute(
        "DELETE FROM file_content_classifications
          WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    Ok(())
}

/// Remove classifications before a node changes away from file ownership.
pub(crate) fn delete_non_file_content_classifications(
    connection: &Connection,
    nodes: &[Node],
) -> DbResult<()> {
    let mut statement =
        connection.prepare_cached("DELETE FROM file_content_classifications WHERE path = ?1")?;
    for node in nodes
        .iter()
        .filter(|node| node.kind != projectatlas_core::NodeKind::File)
    {
        statement.execute([&node.path])?;
    }
    Ok(())
}

/// Require exactly one classification for every current admitted file.
pub(crate) fn validate_complete_file_content_classifications(
    connection: &Connection,
) -> DbResult<()> {
    let missing = connection
        .query_row(
            "SELECT nodes.path
               FROM nodes
               LEFT JOIN file_content_classifications AS classification
                 ON classification.path = nodes.path
              WHERE nodes.kind = 'file'
                AND nodes.exists_now = 1
                AND classification.path IS NULL
              ORDER BY nodes.path
              LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(path) = missing {
        return Err(DbError::FileContentClassificationMissing { path });
    }
    let stale = connection
        .query_row(
            "SELECT classification.path
               FROM file_content_classifications AS classification
               JOIN nodes ON nodes.path = classification.path
              WHERE nodes.kind <> 'file' OR nodes.exists_now <> 1
              ORDER BY classification.path
              LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(path) = stale {
        return Err(DbError::FileContentClassificationNotCurrent { path });
    }
    Ok(())
}

/// Reject malformed input before any batch mutation.
fn validate_classification_batch(
    connection: &Connection,
    rows: &[FileContentClassification],
) -> DbResult<()> {
    if rows.len() > MAX_FILE_CONTENT_CLASSIFICATION_PATHS {
        return Err(DbError::FileContentClassificationBatchTooLarge {
            requested: rows.len(),
            maximum: MAX_FILE_CONTENT_CLASSIFICATION_PATHS,
        });
    }
    let mut paths = BTreeSet::new();
    for row in rows {
        if row.path.is_empty() {
            return Err(DbError::PathNotIndexed {
                path: row.path.clone(),
            });
        }
        if !paths.insert(row.path.as_str()) {
            return Err(DbError::FileContentClassificationDuplicatePath {
                path: row.path.clone(),
            });
        }
    }
    for chunk in rows.chunks(FILE_CONTENT_CLASSIFICATION_BIND_PATHS) {
        let values = (0..chunk.len())
            .map(|index| format!("(?{})", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "WITH requested(path) AS (VALUES {values})
             SELECT requested.path
               FROM requested
               LEFT JOIN nodes
                 ON nodes.path = requested.path
                AND nodes.kind = 'file'
                AND nodes.exists_now = 1
              WHERE nodes.path IS NULL
              ORDER BY requested.path
              LIMIT 1"
        );
        let invalid = connection
            .query_row(
                &sql,
                params_from_iter(chunk.iter().map(|row| &row.path)),
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(path) = invalid {
            return Err(DbError::PathNotIndexed { path });
        }
    }
    Ok(())
}

/// Normalize a bounded exact-path request without duplicate round trips.
fn unique_paths(paths: &[String]) -> DbResult<Vec<String>> {
    let paths = paths.iter().cloned().collect::<BTreeSet<_>>();
    if paths.len() > MAX_FILE_CONTENT_CLASSIFICATION_PATHS {
        return Err(DbError::FileContentClassificationBatchTooLarge {
            requested: paths.len(),
            maximum: MAX_FILE_CONTENT_CLASSIFICATION_PATHS,
        });
    }
    Ok(paths.into_iter().collect())
}

/// Parse one closed persisted classification without fallback.
pub(crate) fn parse_classification(value: String) -> DbResult<ContentClassification> {
    ContentClassification::from_db(&value).ok_or(DbError::InvalidEnum {
        field: "file_content_classifications.classification",
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::{Node, NodeKind};
    use std::error::Error;
    use std::fs;
    use std::io;

    #[test]
    fn classifications_batch_page_reopen_and_follow_file_ownership() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[
            file_node("docs/a.md"),
            file_node("docs/b.md"),
            file_node("src/lib.rs"),
            folder_node("target"),
        ])?;

        let rows = vec![
            classified("docs/a.md", ContentClassification::Documentation),
            classified("docs/b.md", ContentClassification::Documentation),
            classified("src/lib.rs", ContentClassification::Source),
        ];
        let mut publication = store.begin_index_publication("classification-round-trip")?;
        publication.upsert_file_content_classification_batch(&rows)?;
        publication.complete()?;

        let exact = store.file_content_classifications_for_paths(&[
            "src/lib.rs".to_string(),
            "docs/b.md".to_string(),
            "docs/a.md".to_string(),
        ])?;
        require_eq(&exact, &rows, "stable exact-path classifications")?;
        let first = store.file_content_classification_page(
            ContentClassification::Documentation,
            None,
            1,
        )?;
        require(
            first.truncated && first.rows == vec![rows[0].clone()],
            "classification LIMIT + 1 page",
        )?;
        let second = store.file_content_classification_page(
            ContentClassification::Documentation,
            Some("docs/a.md"),
            10,
        )?;
        require(
            !second.truncated && second.rows == vec![rows[1].clone()],
            "classification keyset continuation",
        )?;
        let plan = store
            .connection
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT path, classification
                   FROM file_content_classifications
                  WHERE classification = 'documentation' AND path > ''
                  ORDER BY classification, path
                  LIMIT 11",
            )?
            .query_map([], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            plan.iter().any(|detail| {
                detail.contains("idx_file_content_classifications_classification_path")
            }) && plan
                .iter()
                .all(|detail| !detail.contains("USE TEMP B-TREE")),
            &format!("classification page did not use its covering index: {plan:?}"),
        )?;
        drop(store);

        let mut reopened = AtlasStore::open_for_project(&database, &root)?;
        require_eq(
            &reopened.file_content_classifications_for_paths(&["src/lib.rs".to_string()])?,
            &vec![rows[2].clone()],
            "reopened classification",
        )?;
        reopened.replace_scan(&[
            file_node("docs/a.md"),
            file_node("docs/b.md"),
            folder_node("src/lib.rs"),
        ])?;
        let missing = require_error(
            reopened.file_content_classifications_for_paths(&["src/lib.rs".to_string()]),
            "file-to-folder transition retained its classification",
        )?;
        require(
            matches!(missing, DbError::FileContentClassificationMissing { .. }),
            "file-to-folder cleanup returned the wrong error",
        )?;
        Ok(())
    }

    #[test]
    fn classifications_reject_invalid_ownership_values_and_batches_atomically()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            file_node("src/a.rs"),
            file_node("src/b.rs"),
            folder_node("src"),
        ])?;

        let folder_error = store.connection.execute(
            "INSERT INTO file_content_classifications(path, classification)
             VALUES('src', 'source')",
            [],
        );
        require(
            folder_error.is_err(),
            "folder accepted a file classification",
        )?;
        let value_error = store.connection.execute(
            "UPDATE file_content_classifications
                SET classification = 'executable'
              WHERE path = 'src/a.rs'",
            [],
        );
        require(
            value_error.is_err(),
            "open classification value was accepted",
        )?;

        let duplicate = classified("src/a.rs", ContentClassification::Source);
        let mut publication = store.begin_index_publication("classification-atomicity")?;
        let duplicate_error = require_error(
            publication.upsert_file_content_classification_batch(&[duplicate.clone(), duplicate]),
            "duplicate classification path was accepted",
        )?;
        require(
            matches!(
                duplicate_error,
                DbError::FileContentClassificationDuplicatePath { .. }
            ),
            "duplicate classification returned the wrong error",
        )?;
        let missing_error = require_error(
            publication.upsert_file_content_classification_batch(&[classified(
                "missing.rs",
                ContentClassification::Source,
            )]),
            "missing path accepted a classification",
        )?;
        require(
            matches!(missing_error, DbError::PathNotIndexed { .. }),
            "missing classification path returned the wrong error",
        )?;
        publication.connection.execute_batch(
            "CREATE TEMP TRIGGER abort_second_classification
             BEFORE UPDATE OF classification ON file_content_classifications
             WHEN NEW.path = 'src/b.rs'
             BEGIN SELECT RAISE(ABORT, 'injected classification failure'); END;",
        )?;
        let injected = require_error(
            publication.upsert_file_content_classification_batch(&[
                classified("src/a.rs", ContentClassification::Source),
                classified("src/b.rs", ContentClassification::Documentation),
            ]),
            "injected classification failure committed",
        )?;
        require(
            matches!(injected, DbError::Sqlite(_)),
            "injected classification failure returned the wrong error",
        )?;
        let unchanged = publication.file_content_classifications_for_paths(&[
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
        ])?;
        require(
            unchanged
                .iter()
                .all(|row| row.classification == ContentClassification::Opaque),
            "failed classification statement exposed partial mutation",
        )?;
        publication.complete()?;
        Ok(())
    }

    #[test]
    fn classification_reads_fail_closed_on_corrupt_closed_value() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[file_node("src/lib.rs")])?;
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON")?;
        store.connection.execute(
            "UPDATE file_content_classifications
                SET classification = 'corrupt'
              WHERE path = 'src/lib.rs'",
            [],
        )?;
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF")?;
        let error = require_error(
            store.file_content_classifications_for_paths(&["src/lib.rs".to_string()]),
            "corrupt classification was coerced",
        )?;
        require(
            matches!(error, DbError::InvalidEnum { .. }),
            "corrupt classification returned the wrong error",
        )?;
        Ok(())
    }

    fn classified(path: &str, classification: ContentClassification) -> FileContentClassification {
        FileContentClassification {
            path: path.to_string(),
            classification,
        }
    }

    fn file_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: path.rsplit_once('/').map(|(parent, _)| parent.to_string()),
            extension: None,
            language: None,
            size_bytes: Some(1),
            mtime_ns: Some(1),
            content_hash: Some(format!("hash-{path}")),
        }
    }

    fn folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: path.rsplit_once('/').map(|(parent, _)| parent.to_string()),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: None,
            content_hash: None,
        }
    }

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn require_error<T>(result: DbResult<T>, message: &str) -> Result<DbError, Box<dyn Error>> {
        match result {
            Ok(_) => Err(io::Error::other(message).into()),
            Err(error) => Ok(error),
        }
    }

    fn require_eq<T: std::fmt::Debug + PartialEq>(
        actual: &T,
        expected: &T,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
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
