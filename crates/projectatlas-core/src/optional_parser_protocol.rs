//! Closed, bounded wire contract between the parser supervisor and resident workers.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::num::NonZeroU64;
use thiserror::Error;

/// Current parser-worker protocol version.
pub const PARSER_PROTOCOL_VERSION: u8 = 1;
/// Fixed byte length of every frame header.
pub const PARSER_FRAME_HEADER_BYTES: usize = 8;
/// Largest raw source accepted by one parse request.
pub const PARSER_MAX_SOURCE_BYTES: u32 = 16 * 1024 * 1024;
/// Largest payload accepted by any single parser frame.
pub const PARSER_MAX_FRAME_PAYLOAD_BYTES: u32 = PARSER_MAX_SOURCE_BYTES;
/// Largest serialized control payload accepted by one frame.
pub const PARSER_MAX_CONTROL_BYTES: u32 = 256 * 1024;
/// Largest serialized worker result accepted for one request.
pub const PARSER_MAX_OUTPUT_BYTES: u32 = 128 * 1024;
/// Largest structural-node count accepted for one request.
pub const PARSER_MAX_NODE_COUNT: u32 = 10_000_000;
/// Largest structural depth accepted for one request.
pub const PARSER_MAX_TREE_DEPTH: u32 = 16_384;
/// Largest progress-work counter accepted for one request.
pub const PARSER_MAX_WORK_UNITS: u32 = 10_000_000;
/// Largest number of progress messages accepted for one request.
pub const PARSER_MAX_PROGRESS_MESSAGES: u32 = 4_096;
/// Largest UTF-8 language identity.
pub const PARSER_MAX_LANGUAGE_ID_BYTES: usize = 128;
/// Largest UTF-8 syntax-kind identity.
pub const PARSER_MAX_SYNTAX_KIND_BYTES: usize = 256;
/// Fresh unpredictable bytes hashed into one supervised worker-session identity.
pub const PARSER_SESSION_ENTROPY_BYTES: usize = 32;
/// Maximum diagnostic bytes drained from one supervised worker session.
pub const PARSER_MAX_STDERR_BYTES: usize = 64 * 1024;
/// Hard per-worker committed-memory/address-space ceiling on accepted targets.
pub const PARSER_WORKER_PROCESS_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
/// Hard aggregate worker Job Object memory ceiling on Windows.
pub const PARSER_WORKER_JOB_MEMORY_BYTES: u64 = PARSER_WORKER_PROCESS_MEMORY_BYTES;
/// Reserved Windows containment-broker exit code for an observed Job memory-limit message.
pub const PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE: i32 = 124;
/// Exact Windows broker admission record required before `SessionOpen`.
pub const PARSER_WINDOWS_BROKER_ADMISSION_RECORD: [u8; 16] = [
    0x50, 0x41, 0x54, 0x4c, 0x41, 0x53, 0x2d, 0x41, 0x44, 0x4d, 0x49, 0x54, 0x00, 0x00, 0x00, 0x01,
];

/// Fixed header marker for the parser-worker protocol.
const FRAME_MAGIC: [u8; 2] = *b"PA";
/// Canonical hexadecimal length of a BLAKE3 digest.
const BLAKE3_HEX_BYTES: usize = 64;

/// Failure while framing, decoding, or validating parser-worker traffic.
#[derive(Debug, Error)]
pub enum ParserProtocolError {
    /// Fewer than the fixed header bytes were supplied.
    #[error("parser frame header has {actual} bytes; expected {expected}")]
    HeaderTooShort {
        /// Observed bytes.
        actual: usize,
        /// Required fixed bytes.
        expected: usize,
    },
    /// The fixed protocol marker did not match.
    #[error("parser frame marker is invalid")]
    InvalidFrameMarker,
    /// The frame or control payload uses another protocol version.
    #[error("parser protocol version {actual} is unsupported; expected {expected}")]
    UnsupportedVersion {
        /// Rejected version.
        actual: u8,
        /// Only accepted version.
        expected: u8,
    },
    /// A frame kind is outside the closed protocol.
    #[error("parser frame kind {actual} is unknown")]
    UnknownFrameKind {
        /// Rejected numeric kind.
        actual: u8,
    },
    /// A declared payload exceeds its kind-specific ceiling.
    #[error("parser {kind:?} payload declares {actual} bytes; maximum is {maximum}")]
    FramePayloadTooLarge {
        /// Closed frame kind.
        kind: ParserFrameKind,
        /// Declared payload bytes.
        actual: u32,
        /// Kind-specific ceiling.
        maximum: u32,
    },
    /// The bytes stop before the declared payload ends.
    #[error("parser frame declares {declared} payload bytes but only {available} are available")]
    TruncatedFrame {
        /// Declared payload bytes.
        declared: u32,
        /// Available payload bytes.
        available: usize,
    },
    /// Bytes remain after the one declared frame.
    #[error("parser frame has {actual} payload bytes; declared exactly {declared}")]
    TrailingFrameBytes {
        /// Declared payload bytes.
        declared: u32,
        /// Available payload bytes.
        actual: usize,
    },
    /// A raw-source frame was used where a control frame was required.
    #[error("parser frame kind {kind:?} is not valid for this operation")]
    UnexpectedFrameKind {
        /// Unexpected closed kind.
        kind: ParserFrameKind,
    },
    /// Strict JSON decoding failed.
    #[error("invalid parser {kind:?} control payload")]
    InvalidControlJson {
        /// Expected control kind.
        kind: ParserFrameKind,
        /// JSON or typed-deserialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// Control serialization failed.
    #[error("could not serialize parser {kind:?} control payload")]
    ControlSerialization {
        /// Serialized control kind.
        kind: ParserFrameKind,
        /// JSON serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// A typed field violated its local representation contract.
    #[error("invalid parser protocol field {field}: {reason}")]
    InvalidField {
        /// Stable field identity.
        field: &'static str,
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// Raw source length differs from the authenticated request identity.
    #[error("parser source has {actual} bytes; request declares {expected}")]
    SourceLengthMismatch {
        /// Authenticated length.
        expected: u32,
        /// Observed length.
        actual: usize,
    },
    /// Raw source content differs from the authenticated request identity.
    #[error("parser source BLAKE3 digest does not match the request")]
    SourceDigestMismatch,
    /// A containment-ready frame belongs to another launch or artifact.
    #[error("parser ready {field} does not match the supervised launch")]
    ReadyIdentityMismatch {
        /// Mismatched launch identity component.
        field: &'static str,
    },
    /// A request belongs to another supervised worker session or artifact.
    #[error("parser request {field} does not match the ready worker session")]
    RequestIdentityMismatch {
        /// Mismatched worker-session identity component.
        field: &'static str,
    },
    /// A response belongs to another request, artifact, language, or source.
    #[error("parser response {field} does not match the request")]
    ResponseIdentityMismatch {
        /// Mismatched identity component.
        field: &'static str,
    },
    /// A response exceeds a request-specific count or output limit.
    #[error("parser {field} value {actual} exceeds request limit {maximum}")]
    RequestLimitExceeded {
        /// Limited field.
        field: &'static str,
        /// Observed value.
        actual: u32,
        /// Request ceiling.
        maximum: u32,
    },
    /// Progress sequencing or monotonic state regressed.
    #[error("parser progress field {field} is not monotonic")]
    ProgressRegression {
        /// Regressed progress field.
        field: &'static str,
    },
}

/// Closed frame kinds used on the resident worker's standard streams.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ParserFrameKind {
    /// Strict JSON parse request from supervisor to worker.
    Request = 1,
    /// Exact unencoded source bytes from supervisor to worker.
    RawSource = 2,
    /// Strict JSON progress observation from worker to supervisor.
    Progress = 3,
    /// Strict JSON successful completion from worker to supervisor.
    Completion = 4,
    /// Strict JSON closed failure from worker to supervisor.
    Failure = 5,
    /// Strict JSON containment-ready state from worker to supervisor.
    Ready = 6,
    /// Strict JSON session opening from supervisor to a contained worker.
    SessionOpen = 7,
}

impl ParserFrameKind {
    /// Return the stable numeric wire value.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Return the maximum payload bytes for this frame kind.
    #[must_use]
    pub const fn maximum_payload_bytes(self) -> u32 {
        match self {
            Self::RawSource => PARSER_MAX_SOURCE_BYTES,
            Self::Request
            | Self::Progress
            | Self::Completion
            | Self::Failure
            | Self::Ready
            | Self::SessionOpen => PARSER_MAX_CONTROL_BYTES,
        }
    }

    /// Return whether this kind carries strict JSON control data.
    #[must_use]
    pub const fn is_control(self) -> bool {
        !matches!(self, Self::RawSource)
    }
}

impl TryFrom<u8> for ParserFrameKind {
    type Error = ParserProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Request),
            2 => Ok(Self::RawSource),
            3 => Ok(Self::Progress),
            4 => Ok(Self::Completion),
            5 => Ok(Self::Failure),
            6 => Ok(Self::Ready),
            7 => Ok(Self::SessionOpen),
            actual => Err(ParserProtocolError::UnknownFrameKind { actual }),
        }
    }
}

/// Validated fixed parser frame header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserFrameHeader {
    /// Closed payload kind.
    kind: ParserFrameKind,
    /// Declared payload byte length.
    payload_len: u32,
}

impl ParserFrameHeader {
    /// Construct and bound a fixed frame header before payload allocation.
    ///
    /// # Errors
    ///
    /// Returns an error when `payload_len` exceeds the selected kind's ceiling.
    pub fn new(kind: ParserFrameKind, payload_len: u32) -> Result<Self, ParserProtocolError> {
        let maximum = kind.maximum_payload_bytes();
        if payload_len > maximum {
            return Err(ParserProtocolError::FramePayloadTooLarge {
                kind,
                actual: payload_len,
                maximum,
            });
        }
        Ok(Self { kind, payload_len })
    }

    /// Decode and validate the fixed header before inspecting or allocating its payload.
    ///
    /// # Errors
    ///
    /// Returns an error for short input, an invalid marker, unsupported version,
    /// unknown kind, or a kind-specific declared-length overflow.
    pub fn decode(bytes: &[u8]) -> Result<Self, ParserProtocolError> {
        if bytes.len() < PARSER_FRAME_HEADER_BYTES {
            return Err(ParserProtocolError::HeaderTooShort {
                actual: bytes.len(),
                expected: PARSER_FRAME_HEADER_BYTES,
            });
        }
        if bytes[..2] != FRAME_MAGIC {
            return Err(ParserProtocolError::InvalidFrameMarker);
        }
        let version = bytes[2];
        if version != PARSER_PROTOCOL_VERSION {
            return Err(ParserProtocolError::UnsupportedVersion {
                actual: version,
                expected: PARSER_PROTOCOL_VERSION,
            });
        }
        let kind = ParserFrameKind::try_from(bytes[3])?;
        let payload_len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        Self::new(kind, payload_len)
    }

    /// Encode the fixed header in canonical network byte order.
    #[must_use]
    pub fn encode(self) -> [u8; PARSER_FRAME_HEADER_BYTES] {
        let payload_len = self.payload_len.to_be_bytes();
        [
            FRAME_MAGIC[0],
            FRAME_MAGIC[1],
            PARSER_PROTOCOL_VERSION,
            self.kind.as_u8(),
            payload_len[0],
            payload_len[1],
            payload_len[2],
            payload_len[3],
        ]
    }

    /// Return the closed payload kind.
    #[must_use]
    pub const fn kind(self) -> ParserFrameKind {
        self.kind
    }

    /// Return the validated declared payload length.
    #[must_use]
    pub const fn payload_len(self) -> u32 {
        self.payload_len
    }
}

/// One exactly framed borrowed parser payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserFrame<'a> {
    /// Validated fixed header.
    header: ParserFrameHeader,
    /// Exact borrowed payload.
    payload: &'a [u8],
}

impl<'a> ParserFrame<'a> {
    /// Decode exactly one complete frame without allocating its payload.
    ///
    /// Header validation, including the declared bound, occurs before the
    /// declared length is used to address the payload.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid header, truncation, or trailing bytes.
    pub fn decode_exact(bytes: &'a [u8]) -> Result<Self, ParserProtocolError> {
        let header = ParserFrameHeader::decode(bytes)?;
        let declared = header.payload_len();
        let available = bytes.len().saturating_sub(PARSER_FRAME_HEADER_BYTES);
        let declared_usize = declared as usize;
        if available < declared_usize {
            return Err(ParserProtocolError::TruncatedFrame {
                declared,
                available,
            });
        }
        if available > declared_usize {
            return Err(ParserProtocolError::TrailingFrameBytes {
                declared,
                actual: available,
            });
        }
        Ok(Self {
            header,
            payload: &bytes[PARSER_FRAME_HEADER_BYTES..],
        })
    }

    /// Return the validated closed frame kind.
    #[must_use]
    pub const fn kind(self) -> ParserFrameKind {
        self.header.kind()
    }

    /// Borrow the exact payload bytes.
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }
}

/// Encode one bounded parser frame.
///
/// Raw source bytes are copied exactly; no text or base64 transformation occurs.
///
/// # Errors
///
/// Returns an error when the payload exceeds `u32` or its kind-specific ceiling.
pub fn encode_parser_frame(
    kind: ParserFrameKind,
    payload: &[u8],
) -> Result<Vec<u8>, ParserProtocolError> {
    let payload_len = u32::try_from(payload.len()).map_err(|_source| {
        ParserProtocolError::FramePayloadTooLarge {
            kind,
            actual: u32::MAX,
            maximum: kind.maximum_payload_bytes(),
        }
    })?;
    let header = ParserFrameHeader::new(kind, payload_len)?;
    let mut encoded = Vec::with_capacity(PARSER_FRAME_HEADER_BYTES + payload.len());
    encoded.extend_from_slice(&header.encode());
    encoded.extend_from_slice(payload);
    Ok(encoded)
}

/// Validated current protocol version carried by every control payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserProtocolVersion(u8);

impl ParserProtocolVersion {
    /// Return the only supported protocol version.
    #[must_use]
    pub const fn current() -> Self {
        Self(PARSER_PROTOCOL_VERSION)
    }

    /// Return the numeric wire version.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ParserProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let actual = u8::deserialize(deserializer)?;
        if actual == PARSER_PROTOCOL_VERSION {
            Ok(Self(actual))
        } else {
            Err(serde::de::Error::custom(
                ParserProtocolError::UnsupportedVersion {
                    actual,
                    expected: PARSER_PROTOCOL_VERSION,
                },
            ))
        }
    }
}

/// Non-zero identity of one supervisor request within a worker session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserRequestIdentity(NonZeroU64);

impl ParserRequestIdentity {
    /// Construct a non-zero request identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is zero.
    pub fn new(value: u64) -> Result<Self, ParserProtocolError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(ParserProtocolError::InvalidField {
                field: "request_id",
                reason: "expected a non-zero integer",
            })
    }

    /// Return the numeric session-local identity.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Canonical lowercase BLAKE3 digest used by protocol identities.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserContentDigest(String);

impl ParserContentDigest {
    /// Validate a canonical lowercase BLAKE3 digest.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-canonical digest.
    pub fn new(value: impl Into<String>) -> Result<Self, ParserProtocolError> {
        let value = value.into();
        if value.len() != BLAKE3_HEX_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ParserProtocolError::InvalidField {
                field: "blake3",
                reason: "expected 64 lowercase hexadecimal characters",
            });
        }
        Ok(Self(value))
    }

    /// Hash exact bytes into the canonical protocol representation.
    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    /// Borrow the canonical lowercase hexadecimal value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for ParserContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Authenticated identity of one complete verified platform-pack artifact.
///
/// This is the BLAKE3 of the exact immutable artifact-manifest bytes, which in
/// turn bind the worker, logical capability manifest, and grammar payloads. It
/// is not the identity of a grammar currently loaded into the worker.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserArtifactIdentity(ParserContentDigest);

impl ParserArtifactIdentity {
    /// Construct a pack-artifact identity from its authenticated BLAKE3 digest.
    #[must_use]
    pub const fn new(digest: ParserContentDigest) -> Self {
        Self(digest)
    }

    /// Hash exact immutable artifact-manifest bytes.
    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self(ParserContentDigest::for_bytes(bytes))
    }

    /// Borrow the canonical platform-pack artifact digest.
    #[must_use]
    pub const fn digest(&self) -> &ParserContentDigest {
        &self.0
    }
}

/// Unpredictable identity of one supervised worker process session.
///
/// The supervisor derives this value from operating-system entropy before it
/// launches the worker. Passing it to the trusted worker without grammar or
/// source input and requiring it in READY plus every later request prevents
/// stale process traffic from being replayed across sessions.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserSessionIdentity(ParserContentDigest);

impl ParserSessionIdentity {
    /// Construct a session identity from a caller-authenticated digest.
    #[must_use]
    pub const fn new(digest: ParserContentDigest) -> Self {
        Self(digest)
    }

    /// Hash caller-provided session entropy into the canonical wire identity.
    #[must_use]
    pub fn for_entropy(entropy: &[u8]) -> Self {
        Self(ParserContentDigest::for_bytes(entropy))
    }

    /// Borrow the canonical session digest.
    #[must_use]
    pub const fn digest(&self) -> &ParserContentDigest {
        &self.0
    }
}

/// Bounded supervisor opening that a worker reads only after containment.
///
/// This frame carries process-session freshness but deliberately contains no
/// grammar identity, repository path, or source bytes. A Linux worker installs
/// its own boundary before reading this frame; Windows may deliver it only
/// after launcher admission and resume.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserSessionOpen {
    /// Closed protocol version.
    protocol_version: ParserProtocolVersion,
    /// Supervisor-generated unpredictable process-session identity.
    session: ParserSessionIdentity,
}

impl ParserSessionOpen {
    /// Construct the current protocol's session opening.
    #[must_use]
    pub const fn new(session: ParserSessionIdentity) -> Self {
        Self {
            protocol_version: ParserProtocolVersion::current(),
            session,
        }
    }

    /// Borrow the supervised process-session identity.
    #[must_use]
    pub const fn session(&self) -> &ParserSessionIdentity {
        &self.session
    }
}

/// Closed containment boundary admitted before the worker emits READY.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParserContainmentKind {
    /// Linux worker self-restriction through hard limits, Landlock, and seccomp.
    LinuxLandlockSeccomp,
    /// Windows restricted `AppContainer` child attached to a kill-on-close Job Object.
    WindowsAppContainerJob,
}

/// Authenticated containment-ready state emitted before grammar or source input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserReady {
    /// Closed protocol version.
    protocol_version: ParserProtocolVersion,
    /// Supervisor-provided unpredictable process-session identity.
    session: ParserSessionIdentity,
    /// Exact immutable platform-pack artifact observed by the worker.
    artifact: ParserArtifactIdentity,
    /// Platform containment boundary installed before this state was emitted.
    containment: ParserContainmentKind,
}

impl ParserReady {
    /// Construct the current protocol's containment-ready state.
    #[must_use]
    pub const fn new(
        session: ParserSessionIdentity,
        artifact: ParserArtifactIdentity,
        containment: ParserContainmentKind,
    ) -> Self {
        Self {
            protocol_version: ParserProtocolVersion::current(),
            session,
            artifact,
            containment,
        }
    }

    /// Validate READY against the exact supervised launch contract.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first mismatched launch identity component.
    pub fn validate_for(
        &self,
        session: &ParserSessionIdentity,
        artifact: &ParserArtifactIdentity,
        containment: ParserContainmentKind,
    ) -> Result<(), ParserProtocolError> {
        if self.protocol_version != ParserProtocolVersion::current() {
            return Err(ready_identity_mismatch("protocol_version"));
        }
        if &self.session != session {
            return Err(ready_identity_mismatch("session"));
        }
        if &self.artifact != artifact {
            return Err(ready_identity_mismatch("artifact"));
        }
        if self.containment != containment {
            return Err(ready_identity_mismatch("containment"));
        }
        Ok(())
    }

    /// Borrow the supervised process-session identity.
    #[must_use]
    pub const fn session(&self) -> &ParserSessionIdentity {
        &self.session
    }

    /// Borrow the exact immutable platform-pack artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> &ParserArtifactIdentity {
        &self.artifact
    }

    /// Return the admitted containment boundary.
    #[must_use]
    pub const fn containment(&self) -> ParserContainmentKind {
        self.containment
    }
}

/// Bounded stable language identity for one grammar-affined worker.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserLanguageIdentity(String);

impl ParserLanguageIdentity {
    /// Validate a stable lowercase ASCII language identity.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, oversized, or unsafe identity.
    pub fn new(value: impl Into<String>) -> Result<Self, ParserProtocolError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let starts_alphanumeric = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        if value.len() > PARSER_MAX_LANGUAGE_ID_BYTES
            || !starts_alphanumeric
            || !bytes.all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.' | b'+' | b'#')
            })
        {
            return Err(ParserProtocolError::InvalidField {
                field: "language_id",
                reason: "expected a bounded lowercase ASCII language identity",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the stable language identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserLanguageIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for ParserLanguageIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Authenticated identity of one exact raw-source frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParserSourceIdentity {
    /// Exact unencoded source bytes.
    byte_len: u32,
    /// BLAKE3 of the exact unencoded source bytes.
    blake3: ParserContentDigest,
}

impl ParserSourceIdentity {
    /// Authenticate one bounded raw-source payload.
    ///
    /// # Errors
    ///
    /// Returns an error when the source exceeds the hard byte ceiling.
    pub fn for_bytes(source: &[u8]) -> Result<Self, ParserProtocolError> {
        let byte_len =
            u32::try_from(source.len()).map_err(|_source| ParserProtocolError::InvalidField {
                field: "source.byte_len",
                reason: "source length exceeds u32",
            })?;
        Self::new(byte_len, ParserContentDigest::for_bytes(source))
    }

    /// Construct a bounded source identity from authenticated metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when `byte_len` exceeds the hard source ceiling.
    pub fn new(byte_len: u32, blake3: ParserContentDigest) -> Result<Self, ParserProtocolError> {
        if byte_len > PARSER_MAX_SOURCE_BYTES {
            return Err(ParserProtocolError::InvalidField {
                field: "source.byte_len",
                reason: "source exceeds the hard byte ceiling",
            });
        }
        Ok(Self { byte_len, blake3 })
    }

    /// Validate exact source bytes against both authenticated length and digest.
    ///
    /// # Errors
    ///
    /// Returns an error for a length or BLAKE3 mismatch.
    pub fn validate_bytes(&self, source: &[u8]) -> Result<(), ParserProtocolError> {
        if source.len() != self.byte_len as usize {
            return Err(ParserProtocolError::SourceLengthMismatch {
                expected: self.byte_len,
                actual: source.len(),
            });
        }
        if ParserContentDigest::for_bytes(source) != self.blake3 {
            return Err(ParserProtocolError::SourceDigestMismatch);
        }
        Ok(())
    }

    /// Return the authenticated source byte length.
    #[must_use]
    pub const fn byte_len(&self) -> u32 {
        self.byte_len
    }

    /// Borrow the authenticated source digest.
    #[must_use]
    pub const fn blake3(&self) -> &ParserContentDigest {
        &self.blake3
    }
}

impl<'de> Deserialize<'de> for ParserSourceIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ParserSourceIdentityWire::deserialize(deserializer)?;
        Self::new(wire.byte_len, wire.blake3).map_err(serde::de::Error::custom)
    }
}

/// Strict wire projection for a source identity.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserSourceIdentityWire {
    /// Declared exact raw-source bytes.
    byte_len: u32,
    /// Authenticated raw-source digest.
    blake3: ParserContentDigest,
}

/// Per-request output and structural bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ParserRequestLimits {
    /// Serialized completion-byte ceiling.
    output_bytes: u32,
    /// Structural-node ceiling.
    node_count: u32,
    /// Structural-depth ceiling.
    tree_depth: u32,
}

impl ParserRequestLimits {
    /// Construct request limits within the hard protocol ceilings.
    ///
    /// # Errors
    ///
    /// Returns an error when any limit is zero or exceeds its hard ceiling.
    pub fn new(
        output_bytes: u32,
        node_count: u32,
        tree_depth: u32,
    ) -> Result<Self, ParserProtocolError> {
        validate_nonzero_limit("limits.output_bytes", output_bytes, PARSER_MAX_OUTPUT_BYTES)?;
        validate_nonzero_limit("limits.node_count", node_count, PARSER_MAX_NODE_COUNT)?;
        validate_nonzero_limit("limits.tree_depth", tree_depth, PARSER_MAX_TREE_DEPTH)?;
        Ok(Self {
            output_bytes,
            node_count,
            tree_depth,
        })
    }

    /// Return the serialized completion-byte ceiling.
    #[must_use]
    pub const fn output_bytes(self) -> u32 {
        self.output_bytes
    }

    /// Return the structural-node ceiling.
    #[must_use]
    pub const fn node_count(self) -> u32 {
        self.node_count
    }

    /// Return the structural-depth ceiling.
    #[must_use]
    pub const fn tree_depth(self) -> u32 {
        self.tree_depth
    }
}

impl<'de> Deserialize<'de> for ParserRequestLimits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ParserRequestLimitsWire::deserialize(deserializer)?;
        Self::new(wire.output_bytes, wire.node_count, wire.tree_depth)
            .map_err(serde::de::Error::custom)
    }
}

/// Strict wire projection for per-request limits.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserRequestLimitsWire {
    /// Serialized completion-byte ceiling.
    output_bytes: u32,
    /// Structural-node ceiling.
    node_count: u32,
    /// Structural-depth ceiling.
    tree_depth: u32,
}

/// Strict parse request sent before its matching raw-source frame.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserRequest {
    /// Closed protocol version.
    protocol_version: ParserProtocolVersion,
    /// Unpredictable supervised worker-session identity.
    session: ParserSessionIdentity,
    /// Session-local request identity.
    request_id: ParserRequestIdentity,
    /// Exact verified platform-pack artifact identity expected by the supervisor.
    artifact: ParserArtifactIdentity,
    /// Grammar-affined language identity expected by the supervisor.
    language: ParserLanguageIdentity,
    /// Authenticated identity of the following raw-source frame.
    source: ParserSourceIdentity,
    /// Request-specific bounds under the hard protocol ceilings.
    limits: ParserRequestLimits,
}

impl ParserRequest {
    /// Construct a request for the current closed protocol.
    #[must_use]
    pub const fn new(
        session: ParserSessionIdentity,
        request_id: ParserRequestIdentity,
        artifact: ParserArtifactIdentity,
        language: ParserLanguageIdentity,
        source: ParserSourceIdentity,
        limits: ParserRequestLimits,
    ) -> Self {
        Self {
            protocol_version: ParserProtocolVersion::current(),
            session,
            request_id,
            artifact,
            language,
            source,
            limits,
        }
    }

    /// Return the protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> ParserProtocolVersion {
        self.protocol_version
    }

    /// Borrow the supervised worker-session identity.
    #[must_use]
    pub const fn session(&self) -> &ParserSessionIdentity {
        &self.session
    }

    /// Return the request identity.
    #[must_use]
    pub const fn request_id(&self) -> ParserRequestIdentity {
        self.request_id
    }

    /// Borrow the expected loaded-artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> &ParserArtifactIdentity {
        &self.artifact
    }

    /// Borrow the expected language identity.
    #[must_use]
    pub const fn language(&self) -> &ParserLanguageIdentity {
        &self.language
    }

    /// Borrow the authenticated source identity.
    #[must_use]
    pub const fn source(&self) -> &ParserSourceIdentity {
        &self.source
    }

    /// Return the request-specific limits.
    #[must_use]
    pub const fn limits(&self) -> ParserRequestLimits {
        self.limits
    }

    /// Validate this request against the ready worker's process session and artifact.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first mismatched worker-session identity component.
    pub fn validate_for_session(
        &self,
        session: &ParserSessionIdentity,
        artifact: &ParserArtifactIdentity,
    ) -> Result<(), ParserProtocolError> {
        if &self.session != session {
            return Err(request_identity_mismatch("session"));
        }
        if &self.artifact != artifact {
            return Err(request_identity_mismatch("artifact"));
        }
        Ok(())
    }

    /// Validate the exact following raw-source frame.
    ///
    /// # Errors
    ///
    /// Returns an error for another frame kind or a source length/digest mismatch.
    pub fn validate_source_frame(&self, frame: ParserFrame<'_>) -> Result<(), ParserProtocolError> {
        if frame.kind() != ParserFrameKind::RawSource {
            return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
        }
        self.source.validate_bytes(frame.payload())
    }
}

/// Complete identity copied into every worker response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserResponseIdentity {
    /// Closed protocol version.
    protocol_version: ParserProtocolVersion,
    /// Unpredictable supervised worker-session identity.
    session: ParserSessionIdentity,
    /// Session-local request identity.
    request_id: ParserRequestIdentity,
    /// Exact verified platform-pack artifact identity.
    artifact: ParserArtifactIdentity,
    /// Grammar-affined language identity.
    language: ParserLanguageIdentity,
    /// Authenticated source identity.
    source: ParserSourceIdentity,
}

impl ParserResponseIdentity {
    /// Copy the complete expected response identity from a request.
    #[must_use]
    pub fn for_request(request: &ParserRequest) -> Self {
        Self {
            protocol_version: request.protocol_version,
            session: request.session.clone(),
            request_id: request.request_id,
            artifact: request.artifact.clone(),
            language: request.language.clone(),
            source: request.source.clone(),
        }
    }

    /// Validate all response identity components against a request.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first mismatched identity component.
    pub fn validate_for(&self, request: &ParserRequest) -> Result<(), ParserProtocolError> {
        if self.protocol_version != request.protocol_version {
            return Err(response_identity_mismatch("protocol_version"));
        }
        if self.session != request.session {
            return Err(response_identity_mismatch("session"));
        }
        if self.request_id != request.request_id {
            return Err(response_identity_mismatch("request_id"));
        }
        if self.artifact != request.artifact {
            return Err(response_identity_mismatch("artifact"));
        }
        if self.language != request.language {
            return Err(response_identity_mismatch("language"));
        }
        if self.source != request.source {
            return Err(response_identity_mismatch("source"));
        }
        Ok(())
    }

    /// Return the request identity.
    #[must_use]
    pub const fn request_id(&self) -> ParserRequestIdentity {
        self.request_id
    }

    /// Borrow the supervised worker-session identity.
    #[must_use]
    pub const fn session(&self) -> &ParserSessionIdentity {
        &self.session
    }
}

/// Closed worker progress stages in monotonic order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParserProgressStage {
    /// The request and source identities were accepted.
    Accepted,
    /// The grammar is parsing the authenticated source.
    Parsing,
    /// Bounded structural evidence is being collected.
    CollectingEvidence,
}

impl ParserProgressStage {
    /// Return the monotonic stage rank.
    const fn rank(self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Parsing => 1,
            Self::CollectingEvidence => 2,
        }
    }
}

/// Whether a valid progress message advanced observable work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParserProgressDisposition {
    /// The stage or completed-work count advanced.
    Advanced,
    /// Only the sequence advanced, so a no-progress watchdog must keep aging.
    NoProgress,
}

/// One strictly ordered progress observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParserProgress {
    /// Complete response identity.
    identity: ParserResponseIdentity,
    /// One-based contiguous message sequence.
    sequence: u32,
    /// Monotonic closed stage.
    stage: ParserProgressStage,
    /// Monotonic completed-work count.
    completed_work: u32,
    /// Stable total-work count when measurable.
    total_work: Option<u32>,
}

impl ParserProgress {
    /// Construct one bounded progress observation.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid sequence or work count.
    pub fn new(
        identity: ParserResponseIdentity,
        sequence: u32,
        stage: ParserProgressStage,
        completed_work: u32,
        total_work: Option<u32>,
    ) -> Result<Self, ParserProtocolError> {
        validate_nonzero_limit("progress.sequence", sequence, PARSER_MAX_PROGRESS_MESSAGES)?;
        if completed_work > PARSER_MAX_WORK_UNITS {
            return Err(ParserProtocolError::InvalidField {
                field: "progress.completed_work",
                reason: "completed work exceeds the hard count ceiling",
            });
        }
        if let Some(total) = total_work {
            validate_nonzero_limit("progress.total_work", total, PARSER_MAX_WORK_UNITS)?;
            if completed_work > total {
                return Err(ParserProtocolError::InvalidField {
                    field: "progress.completed_work",
                    reason: "completed work exceeds total work",
                });
            }
        }
        Ok(Self {
            identity,
            sequence,
            stage,
            completed_work,
            total_work,
        })
    }

    /// Validate request identity and monotonic progress semantics.
    ///
    /// A valid message that advances only its sequence returns
    /// [`ParserProgressDisposition::NoProgress`], allowing the supervisor to
    /// distinguish liveness traffic from meaningful forward progress.
    ///
    /// # Errors
    ///
    /// Returns an error for identity mismatch, non-contiguous sequence,
    /// stage/count regression, or a changing total-work claim.
    pub fn validate_for(
        &self,
        request: &ParserRequest,
        previous: Option<&Self>,
    ) -> Result<ParserProgressDisposition, ParserProtocolError> {
        self.identity.validate_for(request)?;
        let Some(previous) = previous else {
            if self.sequence != 1 {
                return Err(progress_regression("sequence"));
            }
            return Ok(ParserProgressDisposition::Advanced);
        };
        if self.identity != previous.identity {
            return Err(progress_regression("identity"));
        }
        if self.sequence != previous.sequence.saturating_add(1) {
            return Err(progress_regression("sequence"));
        }
        if self.stage.rank() < previous.stage.rank() {
            return Err(progress_regression("stage"));
        }
        if self.completed_work < previous.completed_work {
            return Err(progress_regression("completed_work"));
        }
        if self.total_work != previous.total_work {
            return Err(progress_regression("total_work"));
        }
        if self.stage == previous.stage && self.completed_work == previous.completed_work {
            Ok(ParserProgressDisposition::NoProgress)
        } else {
            Ok(ParserProgressDisposition::Advanced)
        }
    }

    /// Return the contiguous progress sequence.
    #[must_use]
    pub const fn sequence(&self) -> u32 {
        self.sequence
    }
}

impl<'de> Deserialize<'de> for ParserProgress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ParserProgressWire::deserialize(deserializer)?;
        Self::new(
            wire.identity,
            wire.sequence,
            wire.stage,
            wire.completed_work,
            wire.total_work,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Strict wire projection for one progress observation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserProgressWire {
    /// Complete response identity.
    identity: ParserResponseIdentity,
    /// One-based contiguous message sequence.
    sequence: u32,
    /// Monotonic closed stage.
    stage: ParserProgressStage,
    /// Monotonic completed-work count.
    completed_work: u32,
    /// Stable total-work count when measurable.
    total_work: Option<u32>,
}

/// Bounded syntax-kind identity returned as structural evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ParserSyntaxKind(String);

impl ParserSyntaxKind {
    /// Validate a non-empty bounded syntax-kind identity.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or control-character content.
    pub fn new(value: impl Into<String>) -> Result<Self, ParserProtocolError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > PARSER_MAX_SYNTAX_KIND_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(ParserProtocolError::InvalidField {
                field: "evidence.root_kind",
                reason: "expected bounded non-control UTF-8",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the exact syntax-kind identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserSyntaxKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Small bounded structural evidence returned instead of a syntax tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParserCompletionEvidence {
    /// Exact root syntax kind.
    root_kind: ParserSyntaxKind,
    /// Root start byte in the authenticated source.
    root_start_byte: u32,
    /// Root end byte in the authenticated source.
    root_end_byte: u32,
    /// Exact Tree-sitter root error state used by retained fixture contracts.
    root_has_error: bool,
    /// Named structural-node count.
    named_node_count: u32,
    /// Error-node count.
    error_node_count: u32,
    /// Missing-node count.
    missing_node_count: u32,
    /// Maximum observed structural depth.
    maximum_depth: u32,
}

impl ParserCompletionEvidence {
    /// Construct bounded structural evidence.
    ///
    /// # Errors
    ///
    /// Returns an error for an inverted range or a hard count/depth overflow.
    pub fn new(
        root_kind: ParserSyntaxKind,
        root_start_byte: u32,
        root_end_byte: u32,
        root_has_error: bool,
        named_node_count: u32,
        error_node_count: u32,
        missing_node_count: u32,
        maximum_depth: u32,
    ) -> Result<Self, ParserProtocolError> {
        if root_start_byte > root_end_byte {
            return Err(ParserProtocolError::InvalidField {
                field: "evidence.root_range",
                reason: "root start exceeds root end",
            });
        }
        for (field, count) in [
            ("evidence.named_node_count", named_node_count),
            ("evidence.error_node_count", error_node_count),
            ("evidence.missing_node_count", missing_node_count),
        ] {
            if count > PARSER_MAX_NODE_COUNT {
                return Err(ParserProtocolError::InvalidField {
                    field,
                    reason: "count exceeds the hard node ceiling",
                });
            }
        }
        if maximum_depth > PARSER_MAX_TREE_DEPTH {
            return Err(ParserProtocolError::InvalidField {
                field: "evidence.maximum_depth",
                reason: "depth exceeds the hard tree ceiling",
            });
        }
        Ok(Self {
            root_kind,
            root_start_byte,
            root_end_byte,
            root_has_error,
            named_node_count,
            error_node_count,
            missing_node_count,
            maximum_depth,
        })
    }

    /// Validate evidence against request-specific source and structural limits.
    ///
    /// # Errors
    ///
    /// Returns an error when the range, node count, or depth exceeds the request.
    pub fn validate_for(&self, request: &ParserRequest) -> Result<(), ParserProtocolError> {
        let source_len = request.source.byte_len();
        if self.root_end_byte > source_len {
            return Err(request_limit_exceeded(
                "evidence.root_end_byte",
                self.root_end_byte,
                source_len,
            ));
        }
        let limits = request.limits;
        for (field, count) in [
            ("evidence.named_node_count", self.named_node_count),
            ("evidence.error_node_count", self.error_node_count),
            ("evidence.missing_node_count", self.missing_node_count),
        ] {
            if count > limits.node_count {
                return Err(request_limit_exceeded(field, count, limits.node_count));
            }
        }
        if self.maximum_depth > limits.tree_depth {
            return Err(request_limit_exceeded(
                "evidence.maximum_depth",
                self.maximum_depth,
                limits.tree_depth,
            ));
        }
        Ok(())
    }

    /// Borrow the exact root syntax kind.
    #[must_use]
    pub const fn root_kind(&self) -> &ParserSyntaxKind {
        &self.root_kind
    }

    /// Return the root start byte.
    #[must_use]
    pub const fn root_start_byte(&self) -> u32 {
        self.root_start_byte
    }

    /// Return the root end byte.
    #[must_use]
    pub const fn root_end_byte(&self) -> u32 {
        self.root_end_byte
    }

    /// Return the exact Tree-sitter root error state.
    #[must_use]
    pub const fn root_has_error(&self) -> bool {
        self.root_has_error
    }

    /// Return the named structural-node count.
    #[must_use]
    pub const fn named_node_count(&self) -> u32 {
        self.named_node_count
    }

    /// Return the error-node count.
    #[must_use]
    pub const fn error_node_count(&self) -> u32 {
        self.error_node_count
    }

    /// Return the missing-node count.
    #[must_use]
    pub const fn missing_node_count(&self) -> u32 {
        self.missing_node_count
    }

    /// Return the maximum observed structural depth.
    #[must_use]
    pub const fn maximum_depth(&self) -> u32 {
        self.maximum_depth
    }
}

impl<'de> Deserialize<'de> for ParserCompletionEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ParserCompletionEvidenceWire::deserialize(deserializer)?;
        Self::new(
            wire.root_kind,
            wire.root_start_byte,
            wire.root_end_byte,
            wire.root_has_error,
            wire.named_node_count,
            wire.error_node_count,
            wire.missing_node_count,
            wire.maximum_depth,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Strict wire projection for structural completion evidence.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserCompletionEvidenceWire {
    /// Exact root syntax kind.
    root_kind: ParserSyntaxKind,
    /// Root start byte.
    root_start_byte: u32,
    /// Root end byte.
    root_end_byte: u32,
    /// Exact Tree-sitter root error state.
    root_has_error: bool,
    /// Named structural-node count.
    named_node_count: u32,
    /// Error-node count.
    error_node_count: u32,
    /// Missing-node count.
    missing_node_count: u32,
    /// Maximum observed structural depth.
    maximum_depth: u32,
}

/// Successful worker completion for one authenticated request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserCompletion {
    /// Complete response identity.
    identity: ParserResponseIdentity,
    /// Small bounded structural evidence.
    evidence: ParserCompletionEvidence,
}

impl ParserCompletion {
    /// Construct a successful worker completion.
    #[must_use]
    pub const fn new(identity: ParserResponseIdentity, evidence: ParserCompletionEvidence) -> Self {
        Self { identity, evidence }
    }

    /// Validate response identity and structural limits.
    ///
    /// # Errors
    ///
    /// Returns an error for an identity mismatch or structural request-limit overflow.
    fn validate_for(&self, request: &ParserRequest) -> Result<(), ParserProtocolError> {
        self.identity.validate_for(request)?;
        self.evidence.validate_for(request)
    }

    /// Borrow the bounded structural evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ParserCompletionEvidence {
        &self.evidence
    }
}

/// Stable closed worker failure codes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParserFailureCode {
    /// Request control failed validation.
    InvalidRequest,
    /// Raw source failed authentication or validation.
    InvalidSource,
    /// The resident worker does not own the requested language.
    LanguageMismatch,
    /// The loaded parser artifact does not match the expected identity.
    ArtifactMismatch,
    /// The parser rejected the source without producing valid evidence.
    ParseRejected,
    /// A bounded resource limit was reached.
    LimitExceeded,
    /// The supervisor cancelled the request.
    Cancelled,
    /// The worker encountered a protocol invariant violation.
    ProtocolViolation,
    /// The worker failed without a more specific safe classification.
    InternalFailure,
}

/// Closed worker failure for one authenticated request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserFailure {
    /// Complete response identity.
    identity: ParserResponseIdentity,
    /// Stable closed failure classification.
    code: ParserFailureCode,
}

impl ParserFailure {
    /// Construct a closed worker failure.
    #[must_use]
    pub const fn new(identity: ParserResponseIdentity, code: ParserFailureCode) -> Self {
        Self { identity, code }
    }

    /// Validate the complete response identity against a request.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first mismatched identity component.
    pub fn validate_for(&self, request: &ParserRequest) -> Result<(), ParserProtocolError> {
        self.identity.validate_for(request)
    }

    /// Return the stable failure code.
    #[must_use]
    pub const fn code(&self) -> ParserFailureCode {
        self.code
    }
}

/// Closed strict-JSON control messages in the parser-worker protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParserControl {
    /// Supervisor session opening before containment-ready acknowledgement.
    SessionOpen(ParserSessionOpen),
    /// Worker containment-ready state.
    Ready(ParserReady),
    /// Supervisor parse request.
    Request(ParserRequest),
    /// Worker progress observation.
    Progress(ParserProgress),
    /// Worker successful completion.
    Completion(ParserCompletion),
    /// Worker closed failure.
    Failure(ParserFailure),
}

impl ParserControl {
    /// Return the closed frame kind for this control message.
    #[must_use]
    pub const fn frame_kind(&self) -> ParserFrameKind {
        match self {
            Self::SessionOpen(_) => ParserFrameKind::SessionOpen,
            Self::Ready(_) => ParserFrameKind::Ready,
            Self::Request(_) => ParserFrameKind::Request,
            Self::Progress(_) => ParserFrameKind::Progress,
            Self::Completion(_) => ParserFrameKind::Completion,
            Self::Failure(_) => ParserFrameKind::Failure,
        }
    }
}

/// Deterministically serialize and frame one strict control message.
///
/// Struct field order and closed enums define the canonical JSON ordering; the
/// protocol deliberately contains no map-valued control fields.
///
/// # Errors
///
/// Returns an error for serialization failure or a control-frame byte overflow.
pub fn encode_parser_control(control: &ParserControl) -> Result<Vec<u8>, ParserProtocolError> {
    let kind = control.frame_kind();
    let payload = match control {
        ParserControl::SessionOpen(value) => serde_json::to_vec(value),
        ParserControl::Ready(value) => serde_json::to_vec(value),
        ParserControl::Request(value) => serde_json::to_vec(value),
        ParserControl::Progress(value) => serde_json::to_vec(value),
        ParserControl::Completion(value) => serde_json::to_vec(value),
        ParserControl::Failure(value) => serde_json::to_vec(value),
    }
    .map_err(|source| ParserProtocolError::ControlSerialization { kind, source })?;
    encode_parser_frame(kind, &payload)
}

/// Decode one strict pre-READY supervisor session opening.
///
/// # Errors
///
/// Returns an error for another frame kind, unknown fields, malformed JSON, or
/// any typed session validation failure.
pub fn decode_parser_session_open(
    frame: ParserFrame<'_>,
) -> Result<ParserSessionOpen, ParserProtocolError> {
    if frame.kind() != ParserFrameKind::SessionOpen {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    decode_control_json(frame.payload(), ParserFrameKind::SessionOpen)
}

/// Decode and validate READY against the exact supervised launch contract.
///
/// # Errors
///
/// Returns an error for another frame kind, strict JSON failure, or launch-
/// identity mismatch.
pub fn decode_parser_ready_for_launch(
    frame: ParserFrame<'_>,
    session: &ParserSessionIdentity,
    artifact: &ParserArtifactIdentity,
    containment: ParserContainmentKind,
) -> Result<ParserReady, ParserProtocolError> {
    if frame.kind() != ParserFrameKind::Ready {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    let ready: ParserReady = decode_control_json(frame.payload(), ParserFrameKind::Ready)?;
    ready.validate_for(session, artifact, containment)?;
    Ok(ready)
}

/// Decode one strict request for the exact ready worker session.
///
/// # Errors
///
/// Returns an error for another frame kind, unknown fields, malformed JSON,
/// typed request validation failure, or worker-session identity mismatch.
pub fn decode_parser_request_for_session(
    frame: ParserFrame<'_>,
    session: &ParserSessionIdentity,
    artifact: &ParserArtifactIdentity,
) -> Result<ParserRequest, ParserProtocolError> {
    if frame.kind() != ParserFrameKind::Request {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    let request: ParserRequest = decode_control_json(frame.payload(), ParserFrameKind::Request)?;
    request.validate_for_session(session, artifact)?;
    Ok(request)
}

/// Decode and validate one progress frame against its request and prior progress.
///
/// # Errors
///
/// Returns an error for another frame kind, strict JSON failure, response-identity
/// mismatch, non-contiguous sequence, regressed work, or changed total work.
pub fn decode_parser_progress_for_request(
    frame: ParserFrame<'_>,
    request: &ParserRequest,
    previous: Option<&ParserProgress>,
) -> Result<(ParserProgress, ParserProgressDisposition), ParserProtocolError> {
    if frame.kind() != ParserFrameKind::Progress {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    let progress: ParserProgress = decode_control_json(frame.payload(), ParserFrameKind::Progress)?;
    let disposition = progress.validate_for(request, previous)?;
    Ok((progress, disposition))
}

/// Decode and validate one completion against the exact request and received bytes.
///
/// The request-specific output budget is checked against the original frame
/// payload before JSON decoding, so whitespace or alternate JSON escaping
/// cannot evade the byte ceiling through canonical re-serialization.
///
/// # Errors
///
/// Returns an error for another frame kind, an exact payload-byte overflow,
/// strict JSON failure, response-identity mismatch, or structural limit breach.
pub fn decode_parser_completion_for_request(
    frame: ParserFrame<'_>,
    request: &ParserRequest,
) -> Result<ParserCompletion, ParserProtocolError> {
    if frame.kind() != ParserFrameKind::Completion {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    let output_bytes = u32::try_from(frame.payload().len()).map_err(|_source| {
        request_limit_exceeded(
            "completion.output_bytes",
            u32::MAX,
            request.limits.output_bytes,
        )
    })?;
    if output_bytes > request.limits.output_bytes {
        return Err(request_limit_exceeded(
            "completion.output_bytes",
            output_bytes,
            request.limits.output_bytes,
        ));
    }
    let completion: ParserCompletion =
        decode_control_json(frame.payload(), ParserFrameKind::Completion)?;
    completion.validate_for(request)?;
    Ok(completion)
}

/// Decode and validate one failure frame against its exact request identity.
///
/// # Errors
///
/// Returns an error for another frame kind, strict JSON failure, or response-
/// identity mismatch.
pub fn decode_parser_failure_for_request(
    frame: ParserFrame<'_>,
    request: &ParserRequest,
) -> Result<ParserFailure, ParserProtocolError> {
    if frame.kind() != ParserFrameKind::Failure {
        return Err(ParserProtocolError::UnexpectedFrameKind { kind: frame.kind() });
    }
    let failure: ParserFailure = decode_control_json(frame.payload(), ParserFrameKind::Failure)?;
    failure.validate_for(request)?;
    Ok(failure)
}

/// Decode one concrete strict control payload.
fn decode_control_json<T>(payload: &[u8], kind: ParserFrameKind) -> Result<T, ParserProtocolError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(payload)
        .map_err(|source| ParserProtocolError::InvalidControlJson { kind, source })
}

/// Validate one non-zero request limit against its hard protocol ceiling.
fn validate_nonzero_limit(
    field: &'static str,
    value: u32,
    maximum: u32,
) -> Result<(), ParserProtocolError> {
    if value == 0 || value > maximum {
        return Err(ParserProtocolError::InvalidField {
            field,
            reason: "expected a non-zero value within the hard ceiling",
        });
    }
    Ok(())
}

/// Construct a stable response-identity mismatch.
const fn response_identity_mismatch(field: &'static str) -> ParserProtocolError {
    ParserProtocolError::ResponseIdentityMismatch { field }
}

/// Construct a stable containment-ready identity mismatch.
const fn ready_identity_mismatch(field: &'static str) -> ParserProtocolError {
    ParserProtocolError::ReadyIdentityMismatch { field }
}

/// Construct a stable request-to-session identity mismatch.
const fn request_identity_mismatch(field: &'static str) -> ParserProtocolError {
    ParserProtocolError::RequestIdentityMismatch { field }
}

/// Construct a stable request-limit overflow.
const fn request_limit_exceeded(
    field: &'static str,
    actual: u32,
    maximum: u32,
) -> ParserProtocolError {
    ParserProtocolError::RequestLimitExceeded {
        field,
        actual,
        maximum,
    }
}

/// Construct a stable progress-regression error.
const fn progress_regression(field: &'static str) -> ParserProtocolError {
    ParserProtocolError::ProgressRegression { field }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;
    use std::io;

    #[test]
    fn control_and_raw_source_frames_round_trip_deterministically()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, source) = request_fixture(64 * 1024)?;
        let controls = [
            ParserControl::SessionOpen(ParserSessionOpen::new(request.session.clone())),
            ParserControl::Ready(ParserReady::new(
                request.session.clone(),
                request.artifact.clone(),
                ParserContainmentKind::LinuxLandlockSeccomp,
            )),
            ParserControl::Request(request.clone()),
            ParserControl::Progress(ParserProgress::new(
                ParserResponseIdentity::for_request(&request),
                1,
                ParserProgressStage::Accepted,
                0,
                Some(10),
            )?),
            ParserControl::Completion(ParserCompletion::new(
                ParserResponseIdentity::for_request(&request),
                evidence_fixture()?,
            )),
            ParserControl::Failure(ParserFailure::new(
                ParserResponseIdentity::for_request(&request),
                ParserFailureCode::ParseRejected,
            )),
        ];
        for control in controls {
            let first = encode_parser_control(&control)?;
            let second = encode_parser_control(&control)?;
            require_eq(&first, &second, "control serialization")?;
            let frame = ParserFrame::decode_exact(&first)?;
            match &control {
                ParserControl::SessionOpen(expected) => {
                    require_eq(
                        &decode_parser_session_open(frame)?,
                        expected,
                        "session-open round trip",
                    )?;
                }
                ParserControl::Ready(expected) => {
                    require_eq(
                        &decode_parser_ready_for_launch(
                            frame,
                            request.session(),
                            request.artifact(),
                            ParserContainmentKind::LinuxLandlockSeccomp,
                        )?,
                        expected,
                        "ready round trip",
                    )?;
                }
                ParserControl::Request(expected) => {
                    require_eq(
                        &decode_parser_request_for_session(
                            frame,
                            request.session(),
                            request.artifact(),
                        )?,
                        expected,
                        "request round trip",
                    )?;
                }
                ParserControl::Progress(expected) => {
                    let (actual, disposition) =
                        decode_parser_progress_for_request(frame, &request, None)?;
                    require_eq(&actual, expected, "progress round trip")?;
                    require_eq(
                        &disposition,
                        &ParserProgressDisposition::Advanced,
                        "progress disposition",
                    )?;
                }
                ParserControl::Completion(expected) => {
                    require_eq(
                        &decode_parser_completion_for_request(frame, &request)?,
                        expected,
                        "completion round trip",
                    )?;
                }
                ParserControl::Failure(expected) => {
                    require_eq(
                        &decode_parser_failure_for_request(frame, &request)?,
                        expected,
                        "failure round trip",
                    )?;
                }
            }
        }

        let encoded_source = encode_parser_frame(ParserFrameKind::RawSource, source)?;
        let source_frame = ParserFrame::decode_exact(&encoded_source)?;
        require_eq(&source_frame.payload(), &source, "raw-source round trip")?;
        request.validate_source_frame(source_frame)?;
        Ok(())
    }

    #[test]
    fn strict_controls_reject_unknown_fields_and_frame_kinds()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, _source) = request_fixture(64 * 1024)?;
        let mut value = serde_json::to_value(&request)?;
        let Some(object) = value.as_object_mut() else {
            return Err("request did not serialize as an object".into());
        };
        object.insert(
            "repository_path".to_string(),
            serde_json::json!("src/lib.rs"),
        );
        let frame_bytes =
            encode_parser_frame(ParserFrameKind::Request, &serde_json::to_vec(&value)?)?;
        let error = decode_parser_request_for_session(
            ParserFrame::decode_exact(&frame_bytes)?,
            request.session(),
            request.artifact(),
        );
        require(
            matches!(error, Err(ParserProtocolError::InvalidControlJson { .. })),
            "unknown request field was accepted",
        )?;

        let mut unknown = ParserFrameHeader::new(ParserFrameKind::Request, 0)?.encode();
        unknown[3] = 99;
        require(
            matches!(
                ParserFrameHeader::decode(&unknown),
                Err(ParserProtocolError::UnknownFrameKind { actual: 99 })
            ),
            "unknown frame kind was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn protocol_version_and_response_identity_mismatches_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, _source) = request_fixture(64 * 1024)?;
        let mut value = serde_json::to_value(&request)?;
        value["protocol_version"] = serde_json::json!(PARSER_PROTOCOL_VERSION + 1);
        let frame_bytes =
            encode_parser_frame(ParserFrameKind::Request, &serde_json::to_vec(&value)?)?;
        require(
            matches!(
                decode_parser_request_for_session(
                    ParserFrame::decode_exact(&frame_bytes)?,
                    request.session(),
                    request.artifact(),
                ),
                Err(ParserProtocolError::InvalidControlJson { .. })
            ),
            "unsupported control version was accepted",
        )?;

        let (other_request, _other_source) = request_fixture(64 * 1024)?;
        let mismatched = ParserCompletion::new(
            ParserResponseIdentity {
                request_id: ParserRequestIdentity::new(2)?,
                ..ParserResponseIdentity::for_request(&other_request)
            },
            evidence_fixture()?,
        );
        let mismatched_frame = encode_parser_control(&ParserControl::Completion(mismatched))?;
        require(
            matches!(
                decode_parser_completion_for_request(
                    ParserFrame::decode_exact(&mismatched_frame)?,
                    &request,
                ),
                Err(ParserProtocolError::ResponseIdentityMismatch {
                    field: "request_id"
                })
            ),
            "mismatched response request identity was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn ready_is_bound_to_the_exact_session_artifact_and_containment()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, _source) = request_fixture(64 * 1024)?;
        let ready = ParserReady::new(
            request.session.clone(),
            request.artifact.clone(),
            ParserContainmentKind::LinuxLandlockSeccomp,
        );
        let encoded = encode_parser_control(&ParserControl::Ready(ready))?;

        for (session, artifact, containment, field) in [
            (
                ParserSessionIdentity::for_entropy(b"another-session"),
                request.artifact.clone(),
                ParserContainmentKind::LinuxLandlockSeccomp,
                "session",
            ),
            (
                request.session.clone(),
                ParserArtifactIdentity::for_bytes(b"another-artifact"),
                ParserContainmentKind::LinuxLandlockSeccomp,
                "artifact",
            ),
            (
                request.session,
                request.artifact,
                ParserContainmentKind::WindowsAppContainerJob,
                "containment",
            ),
        ] {
            require(
                matches!(
                    decode_parser_ready_for_launch(
                        ParserFrame::decode_exact(&encoded)?,
                        &session,
                        &artifact,
                        containment,
                    ),
                    Err(ParserProtocolError::ReadyIdentityMismatch { field: actual })
                        if actual == field
                ),
                "mismatched containment-ready identity was accepted",
            )?;
        }
        Ok(())
    }

    #[test]
    fn session_open_carries_only_protocol_and_process_freshness()
    -> Result<(), Box<dyn std::error::Error>> {
        let open = ParserSessionOpen::new(ParserSessionIdentity::for_entropy(b"session"));
        let mut value = serde_json::to_value(&open)?;
        let Some(object) = value.as_object_mut() else {
            return Err("session opening did not serialize as an object".into());
        };
        require(
            object.len() == 2
                && object.contains_key("protocol_version")
                && object.contains_key("session"),
            "session opening exposed more than protocol and process freshness",
        )?;
        object.insert("grammar_id".to_owned(), serde_json::json!("rust"));
        let encoded =
            encode_parser_frame(ParserFrameKind::SessionOpen, &serde_json::to_vec(&value)?)?;
        require(
            matches!(
                decode_parser_session_open(ParserFrame::decode_exact(&encoded)?),
                Err(ParserProtocolError::InvalidControlJson { .. })
            ),
            "session opening accepted grammar input before READY",
        )?;
        Ok(())
    }

    #[test]
    fn requests_and_responses_cannot_be_replayed_across_sessions()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, _source) = request_fixture(64 * 1024)?;
        let other_session = ParserSessionIdentity::for_entropy(b"another-session");
        let encoded_request = encode_parser_control(&ParserControl::Request(request.clone()))?;
        require(
            matches!(
                decode_parser_request_for_session(
                    ParserFrame::decode_exact(&encoded_request)?,
                    &other_session,
                    request.artifact(),
                ),
                Err(ParserProtocolError::RequestIdentityMismatch { field: "session" })
            ),
            "request from another worker session was accepted",
        )?;
        require(
            matches!(
                decode_parser_request_for_session(
                    ParserFrame::decode_exact(&encoded_request)?,
                    request.session(),
                    &ParserArtifactIdentity::for_bytes(b"another-artifact"),
                ),
                Err(ParserProtocolError::RequestIdentityMismatch { field: "artifact" })
            ),
            "request for another pack artifact was accepted",
        )?;

        let other_request = ParserRequest::new(
            other_session,
            request.request_id,
            request.artifact.clone(),
            request.language.clone(),
            request.source.clone(),
            request.limits,
        );
        let progress = ParserProgress::new(
            ParserResponseIdentity::for_request(&request),
            1,
            ParserProgressStage::Accepted,
            0,
            Some(1),
        )?;
        let encoded_progress = encode_parser_control(&ParserControl::Progress(progress))?;
        require(
            matches!(
                decode_parser_progress_for_request(
                    ParserFrame::decode_exact(&encoded_progress)?,
                    &other_request,
                    None,
                ),
                Err(ParserProtocolError::ResponseIdentityMismatch { field: "session" })
            ),
            "progress from another worker session was accepted",
        )?;

        let completion = ParserCompletion::new(
            ParserResponseIdentity::for_request(&request),
            evidence_fixture()?,
        );
        let encoded_completion = encode_parser_control(&ParserControl::Completion(completion))?;
        require(
            matches!(
                decode_parser_completion_for_request(
                    ParserFrame::decode_exact(&encoded_completion)?,
                    &other_request,
                ),
                Err(ParserProtocolError::ResponseIdentityMismatch { field: "session" })
            ),
            "response from another worker session was accepted",
        )?;

        let failure = ParserFailure::new(
            ParserResponseIdentity::for_request(&request),
            ParserFailureCode::ParseRejected,
        );
        let encoded_failure = encode_parser_control(&ParserControl::Failure(failure))?;
        require(
            matches!(
                decode_parser_failure_for_request(
                    ParserFrame::decode_exact(&encoded_failure)?,
                    &other_request,
                ),
                Err(ParserProtocolError::ResponseIdentityMismatch { field: "session" })
            ),
            "failure from another worker session was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn declared_source_overflow_is_rejected_from_header_alone()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut header = ParserFrameHeader::new(ParserFrameKind::RawSource, 0)?.encode();
        header[4..8].copy_from_slice(&(PARSER_MAX_SOURCE_BYTES + 1).to_be_bytes());
        require(
            matches!(
                ParserFrameHeader::decode(&header),
                Err(ParserProtocolError::FramePayloadTooLarge {
                    kind: ParserFrameKind::RawSource,
                    actual,
                    maximum: PARSER_MAX_SOURCE_BYTES,
                }) if actual == PARSER_MAX_SOURCE_BYTES + 1
            ),
            "oversized declared source was not rejected from the header",
        )?;
        let mut ready_header = ParserFrameHeader::new(ParserFrameKind::Ready, 0)?.encode();
        ready_header[4..8].copy_from_slice(&(PARSER_MAX_CONTROL_BYTES + 1).to_be_bytes());
        require(
            matches!(
                ParserFrameHeader::decode(&ready_header),
                Err(ParserProtocolError::FramePayloadTooLarge {
                    kind: ParserFrameKind::Ready,
                    actual,
                    maximum: PARSER_MAX_CONTROL_BYTES,
                }) if actual == PARSER_MAX_CONTROL_BYTES + 1
            ),
            "oversized declared READY was not rejected from the header",
        )?;
        Ok(())
    }

    #[test]
    fn source_length_and_digest_are_both_authenticated() -> Result<(), Box<dyn std::error::Error>> {
        let identity = ParserSourceIdentity::for_bytes(b"abc")?;
        require(
            matches!(
                identity.validate_bytes(b"ab"),
                Err(ParserProtocolError::SourceLengthMismatch { .. })
            ),
            "source length mismatch was accepted",
        )?;
        require(
            matches!(
                identity.validate_bytes(b"abd"),
                Err(ParserProtocolError::SourceDigestMismatch)
            ),
            "source digest mismatch was accepted",
        )?;
        identity.validate_bytes(b"abc")?;
        Ok(())
    }

    #[test]
    fn progress_is_contiguous_monotonic_and_exposes_no_progress()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, _source) = request_fixture(64 * 1024)?;
        let identity = ParserResponseIdentity::for_request(&request);
        let first = ParserProgress::new(
            identity.clone(),
            1,
            ParserProgressStage::Accepted,
            0,
            Some(10),
        )?;
        require_eq(
            &first.validate_for(&request, None)?,
            &ParserProgressDisposition::Advanced,
            "initial progress",
        )?;
        let heartbeat = ParserProgress::new(
            identity.clone(),
            2,
            ParserProgressStage::Accepted,
            0,
            Some(10),
        )?;
        require_eq(
            &heartbeat.validate_for(&request, Some(&first))?,
            &ParserProgressDisposition::NoProgress,
            "no-progress heartbeat",
        )?;
        let advanced = ParserProgress::new(
            identity.clone(),
            3,
            ParserProgressStage::Parsing,
            4,
            Some(10),
        )?;
        require_eq(
            &advanced.validate_for(&request, Some(&heartbeat))?,
            &ParserProgressDisposition::Advanced,
            "advanced progress",
        )?;
        let regressed =
            ParserProgress::new(identity, 4, ParserProgressStage::Accepted, 4, Some(10))?;
        require(
            matches!(
                regressed.validate_for(&request, Some(&advanced)),
                Err(ParserProtocolError::ProgressRegression { field: "stage" })
            ),
            "progress stage regression was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn exact_frame_decode_rejects_trailing_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoded = encode_parser_frame(ParserFrameKind::RawSource, b"abc")?;
        encoded.push(0);
        require(
            matches!(
                ParserFrame::decode_exact(&encoded),
                Err(ParserProtocolError::TrailingFrameBytes {
                    declared: 3,
                    actual: 4,
                })
            ),
            "trailing frame bytes were accepted",
        )?;
        Ok(())
    }

    #[test]
    fn request_and_completion_limits_are_enforced() -> Result<(), Box<dyn std::error::Error>> {
        require(
            ParserRequestLimits::new(PARSER_MAX_OUTPUT_BYTES + 1, 1, 1).is_err(),
            "oversized output limit was accepted",
        )?;
        require(
            ParserRequestLimits::new(1, 0, 1).is_err(),
            "zero node limit was accepted",
        )?;
        require(
            ParserRequestLimits::new(1, 1, PARSER_MAX_TREE_DEPTH + 1).is_err(),
            "oversized depth limit was accepted",
        )?;

        let (unbounded_request, _source) = request_fixture(PARSER_MAX_OUTPUT_BYTES)?;
        let completion = ParserCompletion::new(
            ParserResponseIdentity::for_request(&unbounded_request),
            evidence_fixture()?,
        );
        require(
            !completion.evidence().root_has_error(),
            "completion root error state did not round trip",
        )?;
        let mut missing_root_error = serde_json::to_value(&completion)?;
        missing_root_error
            .get_mut("evidence")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|evidence| evidence.remove("root_has_error"))
            .ok_or_else(|| io::Error::other("completion fixture lacked root_has_error"))?;
        let missing_root_error_frame = encode_parser_frame(
            ParserFrameKind::Completion,
            &serde_json::to_vec(&missing_root_error)?,
        )?;
        require(
            matches!(
                decode_parser_completion_for_request(
                    ParserFrame::decode_exact(&missing_root_error_frame)?,
                    &unbounded_request,
                ),
                Err(ParserProtocolError::InvalidControlJson {
                    kind: ParserFrameKind::Completion,
                    ..
                })
            ),
            "completion accepted missing root_has_error evidence",
        )?;
        let canonical_payload = serde_json::to_vec(&completion)?;
        let canonical_bytes = u32::try_from(canonical_payload.len())?;
        let request = ParserRequest::new(
            unbounded_request.session,
            unbounded_request.request_id,
            unbounded_request.artifact,
            unbounded_request.language,
            unbounded_request.source,
            ParserRequestLimits::new(canonical_bytes, 100, 16)?,
        );
        let canonical_frame = encode_parser_frame(ParserFrameKind::Completion, &canonical_payload)?;
        decode_parser_completion_for_request(
            ParserFrame::decode_exact(&canonical_frame)?,
            &request,
        )?;

        let mut padded_payload = canonical_payload;
        padded_payload.push(b' ');
        let padded_frame = encode_parser_frame(ParserFrameKind::Completion, &padded_payload)?;
        require(
            matches!(
                decode_parser_completion_for_request(
                    ParserFrame::decode_exact(&padded_frame)?,
                    &request,
                ),
                Err(ParserProtocolError::RequestLimitExceeded {
                    field: "completion.output_bytes",
                    actual,
                    maximum,
                })
                    if actual == canonical_bytes + 1 && maximum == canonical_bytes
            ),
            "padded completion exceeded exact request bytes without rejection",
        )?;
        Ok(())
    }

    /// Build one complete valid request and its raw source.
    fn request_fixture(
        output_bytes: u32,
    ) -> Result<(ParserRequest, &'static [u8]), Box<dyn std::error::Error>> {
        let source = b"fn main() {}";
        Ok((
            ParserRequest::new(
                ParserSessionIdentity::for_entropy(b"supervised-session"),
                ParserRequestIdentity::new(1)?,
                ParserArtifactIdentity::for_bytes(b"artifact"),
                ParserLanguageIdentity::new("rust")?,
                ParserSourceIdentity::for_bytes(source)?,
                ParserRequestLimits::new(output_bytes, 100, 16)?,
            ),
            source,
        ))
    }

    /// Build small valid completion evidence.
    fn evidence_fixture() -> Result<ParserCompletionEvidence, ParserProtocolError> {
        ParserCompletionEvidence::new(
            ParserSyntaxKind::new("source_file")?,
            0,
            12,
            false,
            5,
            0,
            0,
            3,
        )
    }

    /// Return a test error when `condition` is false.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Return a test error when two values differ.
    fn require_eq<T>(
        actual: &T,
        expected: &T,
        context: &'static str,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{context}: expected {expected:?}, found {actual:?}"
            ))
            .into())
        }
    }
}
