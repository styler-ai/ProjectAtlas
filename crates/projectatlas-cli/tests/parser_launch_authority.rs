//! Prove Linux launches consume sealed bytes rather than mutable parser-pack paths.

#![cfg(all(
    debug_assertions,
    feature = "optional-parser-supervisor",
    target_os = "linux",
    target_arch = "x86_64"
))]

use projectatlas_cli::optional_parser_lifecycle::OptionalParserPackLifecycle;
use projectatlas_cli::parser_supervisor::install_linux_launch_test_hook;
use projectatlas_core::IndexCancellation;
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_ID, OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION,
    OptionalParserPackArtifactManifest, OptionalParserPackManifest, ParserPackPayloadRole,
};
use projectatlas_core::optional_parser_protocol::ParserRequestLimits;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const OPTIONAL_PARSER_ARCHIVE_ENV: &str = "PROJECTATLAS_OPTIONAL_PARSER_ARCHIVE";
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Atomically replace one immutable-slot path with same-size invalid bytes.
fn replace_with_invalid_bytes(path: &Path) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let original = fs::read(path)?;
    if original.is_empty() {
        return Err(io::Error::other("launch payload is empty"));
    }
    let replacement = vec![0xa5; original.len()];
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("launch payload has no parent"))?;
    let mut parent_permissions = fs::metadata(parent)?.permissions();
    parent_permissions.set_mode(0o755);
    fs::set_permissions(parent, parent_permissions)?;
    let pending = parent.join(format!(
        ".{}.authority-race",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("launch payload name is not UTF-8"))?
    ));
    fs::write(&pending, &replacement)?;
    let mut pending_permissions = fs::metadata(&pending)?.permissions();
    pending_permissions.set_mode(fs::metadata(path)?.permissions().mode());
    fs::set_permissions(&pending, pending_permissions)?;
    fs::rename(&pending, path)?;
    Ok((original, replacement))
}

/// Restore one path after the test has proved the hostile replacement remained live.
fn restore_bytes(path: &Path, original: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("launch payload has no parent"))?;
    let pending = parent.join(format!(
        ".{}.authority-restore",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("launch payload name is not UTF-8"))?
    ));
    fs::write(&pending, original)?;
    let mut pending_permissions = fs::metadata(&pending)?.permissions();
    pending_permissions.set_mode(fs::metadata(path)?.permissions().mode());
    fs::set_permissions(&pending, pending_permissions)?;
    fs::rename(pending, path)
}

#[test]
#[ignore = "requires one exact workflow-built Linux optional parser-pack archive"]
fn sealed_worker_and_grammar_survive_concurrent_path_replacement() -> Result<(), Box<dyn Error>> {
    let archive = std::env::var_os(OPTIONAL_PARSER_ARCHIVE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("real optional parser archive environment is absent"))?;
    let temp = tempfile::tempdir()?;
    let repository = temp.path().join("repository");
    let storage = temp.path().join("storage");
    fs::create_dir(&repository)?;
    let lifecycle = OptionalParserPackLifecycle::new(&repository, Some(storage.clone()))?;
    let installed = lifecycle.install(&archive)?;
    let artifact = installed
        .artifact
        .ok_or_else(|| io::Error::other("installed parser artifact report is absent"))?
        .artifact;
    lifecycle.enable(&artifact)?;

    let slot = storage
        .join(OPTIONAL_PARSER_PACK_ID)
        .join("versions")
        .join(OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION)
        .join(&artifact);
    let accepted =
        OptionalParserPackManifest::from_json(&fs::read(slot.join(ACCEPTED_MANIFEST_FILE_NAME))?)?;
    let grammar = accepted
        .grammars()
        .iter()
        .find(|grammar| grammar.language_id == "rust")
        .or_else(|| accepted.grammars().first())
        .ok_or_else(|| io::Error::other("accepted parser manifest has no grammar"))?
        .clone();
    let artifact_manifest: OptionalParserPackArtifactManifest =
        serde_json::from_slice(&fs::read(slot.join(ARTIFACT_MANIFEST_FILE_NAME))?)?;
    let worker_path = artifact_manifest
        .files
        .iter()
        .find(|payload| matches!(payload.role, ParserPackPayloadRole::Worker))
        .map(|payload| slot.join(Path::new(payload.path.as_str())))
        .ok_or_else(|| io::Error::other("artifact has no worker payload"))?;
    let grammar_path = artifact_manifest
        .files
        .iter()
        .find(|payload| {
            matches!(
                &payload.role,
                ParserPackPayloadRole::GrammarLibrary { language_id }
                    if language_id == &grammar.language_id
            )
        })
        .map(|payload| slot.join(Path::new(payload.path.as_str())))
        .ok_or_else(|| io::Error::other("artifact has no selected grammar payload"))?;

    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    install_linux_launch_test_hook(move || {
        let _ready_send_result = ready_sender.send(());
        let _release_receive_result = release_receiver.recv_timeout(TEST_TIMEOUT);
    })?;

    let mut selection = lifecycle
        .resolve_selected_pack()?
        .ok_or_else(|| io::Error::other("enabled parser selection was not resolved"))?;
    let language_id = grammar.language_id.clone();
    let source = grammar.fixtures.positive.source.into_bytes();
    let parse = thread::spawn(move || {
        let limits = ParserRequestLimits::new(1024 * 1024, 100_000, 512)?;
        let deadline = Instant::now()
            .checked_add(TEST_TIMEOUT)
            .ok_or_else(|| io::Error::other("parse deadline overflow"))?;
        let operation = selection.supervisor_mut().parse(
            &language_id,
            &source,
            limits,
            deadline,
            Duration::from_secs(5),
            &IndexCancellation::new(),
        );
        let cleanup = selection.supervisor_mut().shutdown();
        operation?;
        cleanup?;
        Ok::<(), Box<dyn Error + Send + Sync>>(())
    });

    ready_receiver.recv_timeout(TEST_TIMEOUT)?;
    let worker_replacement = replace_with_invalid_bytes(&worker_path);
    let grammar_replacement = replace_with_invalid_bytes(&grammar_path);
    let _release_send_result = release_sender.send(());
    let (worker_original, worker_attacker) = worker_replacement?;
    let (grammar_original, grammar_attacker) = grammar_replacement?;
    let parse_result = parse
        .join()
        .map_err(|_panic| io::Error::other("sealed-authority parse thread panicked"))?;
    parse_result.map_err(|error| io::Error::other(error.to_string()))?;

    if fs::read(&worker_path)? != worker_attacker || fs::read(&grammar_path)? != grammar_attacker {
        return Err(io::Error::other(
            "launch source replacements did not remain live through successful parsing",
        )
        .into());
    }
    restore_bytes(&worker_path, &worker_original)?;
    restore_bytes(&grammar_path, &grammar_original)?;
    Ok(())
}
