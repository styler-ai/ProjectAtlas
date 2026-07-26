//! Hostile process peer for the parser supervisor's bounded protocol and cleanup contract.

use std::env;
use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
use projectatlas_core::optional_parser_protocol::PARSER_WINDOWS_BROKER_ADMISSION_RECORD;
use projectatlas_core::optional_parser_protocol::{
    PARSER_FRAME_HEADER_BYTES, PARSER_MAX_STDERR_BYTES, PARSER_PROTOCOL_VERSION,
    ParserArtifactIdentity, ParserCompletion, ParserCompletionEvidence, ParserContainmentKind,
    ParserControl, ParserFailure, ParserFailureCode, ParserFrame, ParserFrameHeader,
    ParserFrameKind, ParserLanguageIdentity, ParserProgress, ParserProgressStage, ParserReady,
    ParserRequest, ParserRequestIdentity, ParserResponseIdentity, ParserSessionIdentity,
    ParserSyntaxKind, decode_parser_request_for_session, decode_parser_session_open,
    encode_parser_control,
};

#[allow(dead_code, unused_imports)]
#[path = "../src/parser_supervisor.rs"]
mod parser_supervisor;

/// Exact synthetic artifact identity shared with the test-only launch authority.
const TEST_ARTIFACT_BYTES: &[u8] = b"parser-supervisor-hostile-peer";
/// Cargo-visible target name used for libtest-compatible substring filtering.
const HARNESS_NAME: &str = "parser_supervisor_adversarial";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args();
    let _program = arguments.next();
    let first_argument = arguments.next();
    if first_argument.as_deref() == Some("--peer") {
        let scenario = arguments
            .next()
            .ok_or_else(|| io::Error::other("hostile scenario is missing"))?;
        if let Some(argument) = arguments.next() {
            return Err(io::Error::other(format!(
                "unexpected adversarial harness argument {argument:?}"
            ))
            .into());
        }
        return hostile_peer(&scenario);
    }

    let mut filter = None;
    let mut exact = false;
    for argument in first_argument.into_iter().chain(arguments) {
        match argument.as_str() {
            "--nocapture" => {}
            "--exact" => exact = true,
            _ if argument.starts_with('-') || filter.is_some() => {
                return Err(io::Error::other(format!(
                    "unexpected adversarial harness argument {argument:?}"
                ))
                .into());
            }
            _ => filter = Some(argument),
        }
    }
    let should_run = filter.is_none_or(|filter| {
        if exact {
            HARNESS_NAME == filter
        } else {
            HARNESS_NAME.contains(&filter)
        }
    });
    if should_run {
        #[cfg(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64")
        ))]
        parser_supervisor::run_adversarial_process_suite(&env::current_exe()?)?;
    }
    Ok(())
}

/// Run one closed hostile protocol behavior over this process's standard streams.
fn hostile_peer(scenario: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut diagnostic = io::stderr().lock();

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    match scenario {
        "admission-forged" => {
            diagnostic.write_all(&vec![b'x'; PARSER_WINDOWS_BROKER_ADMISSION_RECORD.len()])?;
            return Ok(());
        }
        "admission-truncated" => {
            diagnostic.write_all(
                &PARSER_WINDOWS_BROKER_ADMISSION_RECORD
                    [..PARSER_WINDOWS_BROKER_ADMISSION_RECORD.len() / 2],
            )?;
            return Ok(());
        }
        "admission-stall" => {
            thread::sleep(Duration::from_secs(5));
            return Ok(());
        }
        "admission-flood" => {
            diagnostic.write_all(&PARSER_WINDOWS_BROKER_ADMISSION_RECORD)?;
            diagnostic.write_all(&vec![b'x'; PARSER_MAX_STDERR_BYTES + 1])?;
            diagnostic.flush()?;
        }
        _ => {
            diagnostic.write_all(&PARSER_WINDOWS_BROKER_ADMISSION_RECORD)?;
            diagnostic.flush()?;
        }
    }

    let opening = read_frame(&mut input)?;
    let opening = decode_parser_session_open(ParserFrame::decode_exact(&opening)?)?;
    if scenario == "pre-ready-stall" {
        thread::sleep(Duration::from_secs(5));
        return Ok(());
    }

    match scenario {
        "ready-malformed" => {
            write_frame(&mut output, ParserFrameKind::Ready, b"{}")?;
            return Ok(());
        }
        "ready-truncated" => {
            write_truncated(&mut output, ParserFrameKind::Ready)?;
            return Ok(());
        }
        "ready-oversized" => {
            write_oversized_header(&mut output, ParserFrameKind::Ready)?;
            return Ok(());
        }
        _ => {}
    }

    let artifact = if scenario == "ready-artifact" {
        ParserArtifactIdentity::for_bytes(b"other-artifact")
    } else {
        ParserArtifactIdentity::for_bytes(TEST_ARTIFACT_BYTES)
    };
    let session = if scenario == "ready-session" {
        ParserSessionIdentity::for_entropy(b"stale-session")
    } else {
        opening.session().clone()
    };
    let containment = if scenario == "ready-containment" {
        other_containment(host_containment())
    } else {
        host_containment()
    };
    write_control(
        &mut output,
        &ParserControl::Ready(ParserReady::new(session, artifact, containment)),
    )?;
    if matches!(
        scenario,
        "ready-session" | "ready-artifact" | "ready-containment"
    ) {
        return Ok(());
    }

    if scenario == "blocked-write" || scenario == "admission-flood" {
        thread::sleep(Duration::from_secs(5));
        return Ok(());
    }

    let request_bytes = read_frame(&mut input)?;
    let request = decode_parser_request_for_session(
        ParserFrame::decode_exact(&request_bytes)?,
        opening.session(),
        &ParserArtifactIdentity::for_bytes(TEST_ARTIFACT_BYTES),
    )?;
    let source = read_frame(&mut input)?;
    request.validate_source_frame(ParserFrame::decode_exact(&source)?)?;

    match scenario {
        "progress-session" => {
            let forged = request_with(&request, Some("session"), None)?;
            write_progress(&mut output, &forged, 1, 1)?;
        }
        "progress-request" => {
            let forged = request_with(&request, None, Some(1))?;
            write_progress(&mut output, &forged, 1, 1)?;
        }
        "progress-duplicate" => {
            write_progress(&mut output, &request, 1, 1)?;
            write_progress(&mut output, &request, 1, 2)?;
        }
        "progress-gap" => {
            write_progress(&mut output, &request, 1, 1)?;
            write_progress(&mut output, &request, 3, 2)?;
        }
        "progress-regression" => {
            write_progress(&mut output, &request, 1, 2)?;
            write_progress(&mut output, &request, 2, 1)?;
        }
        "progress-endless" => {
            for sequence in 1..=1024 {
                write_progress(&mut output, &request, sequence, sequence)?;
            }
            thread::sleep(Duration::from_secs(5));
        }
        "progress-no-work" => {
            for sequence in 1..=1024 {
                write_progress(&mut output, &request, sequence, 0)?;
            }
            thread::sleep(Duration::from_secs(5));
        }
        "completion-malformed" => {
            write_frame(&mut output, ParserFrameKind::Completion, b"{}")?;
        }
        "completion-truncated" => {
            write_truncated(&mut output, ParserFrameKind::Completion)?;
        }
        "completion-oversized" => {
            write_oversized_header(&mut output, ParserFrameKind::Completion)?;
            thread::sleep(Duration::from_secs(5));
        }
        "failure-exit" => {
            write_control(
                &mut output,
                &ParserControl::Failure(ParserFailure::new(
                    ParserResponseIdentity::for_request(&request),
                    ParserFailureCode::ParseRejected,
                )),
            )?;
        }
        "stderr-flood" => {
            diagnostic.write_all(&vec![b'x'; PARSER_MAX_STDERR_BYTES + 1])?;
            diagnostic.flush()?;
            thread::sleep(Duration::from_secs(5));
        }
        "stderr-completion" => {
            diagnostic.write_all(b"bounded diagnostic")?;
            diagnostic.flush()?;
            write_completion(
                &mut output,
                &request,
                request.source().byte_len(),
                1,
                1,
                "source_file",
            )?;
        }
        "limit-source" => write_completion(
            &mut output,
            &request,
            request.source().byte_len().saturating_add(1),
            1,
            1,
            "source_file",
        )?,
        "limit-nodes" => write_completion(
            &mut output,
            &request,
            request.source().byte_len(),
            request.limits().node_count().saturating_add(1),
            1,
            "source_file",
        )?,
        "limit-depth" => write_completion(
            &mut output,
            &request,
            request.source().byte_len(),
            1,
            request.limits().tree_depth().saturating_add(1),
            "source_file",
        )?,
        "limit-output" => write_completion(
            &mut output,
            &request,
            request.source().byte_len(),
            1,
            1,
            "source_file_with_a_deliberately_long_but_valid_name",
        )?,
        "healthy" => write_completion(
            &mut output,
            &request,
            request.source().byte_len(),
            1,
            1,
            "source_file",
        )?,
        _ => {}
    }
    if scenario == "healthy" {
        io::copy(&mut input, &mut io::sink())?;
    }
    Ok(())
}

/// Return the containment identity expected by the current production supervisor branch.
fn host_containment() -> ParserContainmentKind {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        ParserContainmentKind::WindowsAppContainerJob
    } else {
        ParserContainmentKind::LinuxLandlockSeccomp
    }
}

/// Return the other closed containment identity for forged READY coverage.
fn other_containment(containment: ParserContainmentKind) -> ParserContainmentKind {
    match containment {
        ParserContainmentKind::LinuxLandlockSeccomp => {
            ParserContainmentKind::WindowsAppContainerJob
        }
        ParserContainmentKind::WindowsAppContainerJob => {
            ParserContainmentKind::LinuxLandlockSeccomp
        }
    }
}

/// Clone a request while forging only its session or request identity.
fn request_with(
    request: &ParserRequest,
    forged: Option<&str>,
    request_offset: Option<u64>,
) -> Result<ParserRequest, Box<dyn std::error::Error>> {
    let session = if forged == Some("session") {
        ParserSessionIdentity::for_entropy(b"replayed-session")
    } else {
        request.session().clone()
    };
    let request_id = ParserRequestIdentity::new(
        request
            .request_id()
            .get()
            .saturating_add(request_offset.unwrap_or_default()),
    )?;
    Ok(ParserRequest::new(
        session,
        request_id,
        request.artifact().clone(),
        ParserLanguageIdentity::new(request.language().as_str())?,
        request.source().clone(),
        request.limits(),
    ))
}

/// Emit one strict progress response for the selected request.
fn write_progress(
    output: &mut impl Write,
    request: &ParserRequest,
    sequence: u32,
    completed_work: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    write_control(
        output,
        &ParserControl::Progress(ParserProgress::new(
            ParserResponseIdentity::for_request(request),
            sequence,
            ParserProgressStage::Parsing,
            completed_work,
            Some(1024),
        )?),
    )
}

/// Emit one strict completion with caller-selected bounded evidence.
fn write_completion(
    output: &mut impl Write,
    request: &ParserRequest,
    end_byte: u32,
    named_nodes: u32,
    depth: u32,
    root_kind: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let evidence = ParserCompletionEvidence::new(
        ParserSyntaxKind::new(root_kind)?,
        0,
        end_byte,
        false,
        named_nodes,
        0,
        0,
        depth,
    )?;
    write_control(
        output,
        &ParserControl::Completion(ParserCompletion::new(
            ParserResponseIdentity::for_request(request),
            evidence,
        )),
    )
}

/// Read one complete bounded supervisor frame in the trusted test peer.
fn read_frame(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0_u8; PARSER_FRAME_HEADER_BYTES];
    input.read_exact(&mut header)?;
    let decoded =
        ParserFrameHeader::decode(&header).map_err(|error| io::Error::other(error.to_string()))?;
    let payload_len = usize::try_from(decoded.payload_len())
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut bytes = Vec::with_capacity(PARSER_FRAME_HEADER_BYTES.saturating_add(payload_len));
    bytes.extend_from_slice(&header);
    bytes.resize(PARSER_FRAME_HEADER_BYTES.saturating_add(payload_len), 0);
    input.read_exact(&mut bytes[PARSER_FRAME_HEADER_BYTES..])?;
    Ok(bytes)
}

/// Encode and flush one strict typed control response.
fn write_control(
    output: &mut impl Write,
    control: &ParserControl,
) -> Result<(), Box<dyn std::error::Error>> {
    output.write_all(&encode_parser_control(control)?)?;
    output.flush()?;
    Ok(())
}

/// Write one caller-owned payload beneath a valid fixed header.
fn write_frame(output: &mut impl Write, kind: ParserFrameKind, payload: &[u8]) -> io::Result<()> {
    let payload_len = u32::try_from(payload.len()).map_err(io::Error::other)?;
    let header = ParserFrameHeader::new(kind, payload_len)
        .map_err(|error| io::Error::other(error.to_string()))?;
    output.write_all(&header.encode())?;
    output.write_all(payload)?;
    output.flush()
}

/// Write fewer payload bytes than the fixed header declares.
fn write_truncated(output: &mut impl Write, kind: ParserFrameKind) -> io::Result<()> {
    let header =
        ParserFrameHeader::new(kind, 16).map_err(|error| io::Error::other(error.to_string()))?;
    output.write_all(&header.encode())?;
    output.write_all(b"{}")?;
    output.flush()
}

/// Write a declaration one byte above the selected frame-kind ceiling.
fn write_oversized_header(output: &mut impl Write, kind: ParserFrameKind) -> io::Result<()> {
    let declared = kind.maximum_payload_bytes().saturating_add(1).to_be_bytes();
    output.write_all(&[
        b'P',
        b'A',
        PARSER_PROTOCOL_VERSION,
        kind.as_u8(),
        declared[0],
        declared[1],
        declared[2],
        declared[3],
    ])?;
    output.flush()
}
