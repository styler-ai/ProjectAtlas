//! Real-process proof for parent-owned full-scan staging and atomic publication.

use assert_cmd::Command;
use rusqlite::Connection;
use serde_json::Value;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Output;

const OLD_MARKER: &str = "publication_old_marker";
const NEW_MARKER: &str = "publication_new_marker";
const FAILED_MARKER: &str = "publication_failed_marker";
const STAGING_FILE_PREFIX: &str = ".projectatlas-full-scan-";
const SOURCE_PATH: &str = "src/資料_😀.rs";

#[test]
fn task_arri_e2e_arri_7_6_parent_owned_full_scan_publication() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repository_資料_😀");
    let source_dir = repo.join("src");
    fs::create_dir_all(&source_dir)?;
    let source_path = repo.join(SOURCE_PATH);
    fs::write(&source_path, source_with_marker(OLD_MARKER))?;

    let init = projectatlas(&repo)?
        .args(["--format", "json", "init"])
        .output()?;
    require_success("init", &init)?;
    let db = repo.join(".projectatlas").join("projectatlas.db");
    let first = publication_state(&db)?;
    require_eq(
        &search_count(&repo, &db, OLD_MARKER)?,
        &1,
        "initial lexical result count",
    )?;

    fs::write(&source_path, source_with_marker(NEW_MARKER))?;
    let scan = projectatlas_with_db(&repo, &db)?
        .args(["--format", "json", "scan"])
        .output()?;
    require_success("second scan", &scan)?;
    let second = publication_state(&db)?;
    require_eq(
        second.0.as_str(),
        other_slot(&first.0)?,
        "second active slot",
    )?;
    require_eq(&second.1, &(first.1 + 1), "second publication epoch")?;
    require_eq(
        &search_count(&repo, &db, NEW_MARKER)?,
        &1,
        "new lexical result count",
    )?;
    require_eq(
        &search_count(&repo, &db, OLD_MARKER)?,
        &0,
        "old lexical visibility",
    )?;
    require(
        slot_content(&db, &first.0, SOURCE_PATH)?.contains(OLD_MARKER),
        "previous active slot did not retain the old lexical generation",
    )?;
    require(
        slot_content(&db, &second.0, SOURCE_PATH)?.contains(NEW_MARKER),
        "new active slot did not contain the new lexical generation",
    )?;
    require_no_staging_files(&db)?;

    install_inactive_import_failure(&db)?;
    fs::write(&source_path, source_with_marker(FAILED_MARKER))?;
    let failed_scan = projectatlas_with_db(&repo, &db)?
        .args(["--format", "json", "scan"])
        .output()?;
    if failed_scan.status.success() {
        return Err(io::Error::other(format!(
            "injected publication failure unexpectedly succeeded: {}",
            String::from_utf8_lossy(&failed_scan.stdout)
        ))
        .into());
    }
    let stderr = String::from_utf8_lossy(&failed_scan.stderr);
    if !stderr.contains("injected inactive-slot import failure") {
        return Err(
            io::Error::other(format!("failed publication lost its root cause: {stderr}")).into(),
        );
    }

    require_eq(
        &search_count(&repo, &db, NEW_MARKER)?,
        &1,
        "last valid lexical result count",
    )?;
    require_eq(
        &search_count(&repo, &db, FAILED_MARKER)?,
        &0,
        "failed lexical generation visibility",
    )?;
    require(
        slot_content(&db, &second.0, SOURCE_PATH)?.contains(NEW_MARKER),
        "failed import damaged the last valid active generation",
    )?;
    require(
        slot_content(&db, &first.0, SOURCE_PATH)?.contains(OLD_MARKER),
        "failed import damaged the retained rollback generation",
    )?;
    require_eq(
        &publication_state(&db)?,
        &second,
        "publication state after failed import",
    )?;
    require_no_staging_files(&db)?;
    Ok(())
}

fn projectatlas(repo: &Path) -> Result<Command, Box<dyn Error>> {
    let mut command = Command::cargo_bin("projectatlas")?;
    command
        .current_dir(repo)
        .env("PROJECTATLAS_NO_TELEMETRY", "1");
    Ok(command)
}

fn projectatlas_with_db(repo: &Path, db: &Path) -> Result<Command, Box<dyn Error>> {
    let mut command = projectatlas(repo)?;
    command.arg("--db").arg(db);
    Ok(command)
}

fn require_success(operation: &str, output: &Output) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

fn search_count(repo: &Path, db: &Path, marker: &str) -> Result<usize, Box<dyn Error>> {
    let output = projectatlas_with_db(repo, db)?
        .args([
            "--format",
            "json",
            "search",
            marker,
            "--file-pattern",
            "src/*.rs",
        ])
        .output()?;
    require_success("search", &output)?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    report["returned"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| io::Error::other("search output omitted a valid returned count").into())
}

fn publication_state(db: &Path) -> Result<(String, i64), Box<dyn Error>> {
    let connection = Connection::open(db)?;
    Ok(connection.query_row(
        "SELECT active_slot, active_epoch
         FROM graph_publication_state
         WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

fn slot_content(db: &Path, slot: &str, path: &str) -> Result<String, Box<dyn Error>> {
    let connection = Connection::open(db)?;
    Ok(connection.query_row(
        "SELECT content FROM file_texts
         WHERE structural_slot = ?1 AND path = ?2",
        [slot, path],
        |row| row.get(0),
    )?)
}

fn install_inactive_import_failure(db: &Path) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(db)?;
    let has_fts = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_schema
             WHERE type = 'table' AND name = 'file_text_fts'
         )",
        [],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if !has_fts {
        return Err(io::Error::other(
            "bundled SQLite omitted the FTS import boundary required by this E2E",
        )
        .into());
    }
    connection.execute_batch(
        "DROP TRIGGER file_text_fts_insert;
         CREATE TRIGGER file_text_fts_insert
         AFTER INSERT ON file_texts
         BEGIN
             SELECT CASE
                 WHEN new.structural_slot != (
                     SELECT active_slot FROM graph_publication_state WHERE singleton = 1
                 )
                 THEN RAISE(ABORT, 'injected inactive-slot import failure')
             END;
             INSERT INTO file_text_fts(
                 structural_slot, last_changed_epoch, path, content
             ) VALUES(
                 new.structural_slot, new.last_changed_epoch, new.path, new.content
             );
         END;",
    )?;
    Ok(())
}

fn require_no_staging_files(db: &Path) -> Result<(), Box<dyn Error>> {
    let parent = db
        .parent()
        .ok_or_else(|| io::Error::other("database path has no parent"))?;
    let retained = fs::read_dir(parent)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| os_str_starts_with(name, STAGING_FILE_PREFIX))
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "full-scan staging cleanup retained files: {retained:?}"
    ))
    .into())
}

fn os_str_starts_with(value: &OsStr, prefix: &str) -> bool {
    value.to_string_lossy().starts_with(prefix)
}

fn source_with_marker(marker: &str) -> String {
    format!("pub fn indexed() -> &'static str {{ \"{marker}\" }}\n")
}

fn other_slot(slot: &str) -> Result<&'static str, Box<dyn Error>> {
    match slot {
        "a" => Ok("b"),
        "b" => Ok("a"),
        other => Err(io::Error::other(format!("unexpected structural slot {other}")).into()),
    }
}

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

fn require_eq<T>(actual: &T, expected: &T, field: &str) -> Result<(), Box<dyn Error>>
where
    T: std::fmt::Debug + PartialEq + ?Sized,
{
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{field} mismatch: expected {expected:?}, found {actual:?}"
        ))
        .into())
    }
}
