//! Bounded archive adapter for portable derived graph snapshots.

use super::{CliError, runtime::lossless_native_path_display};
use projectatlas_db::{
    AtlasStore, DerivedGraphSnapshot, DerivedGraphSnapshotImport, DerivedSnapshotContent,
    MAX_DERIVED_SNAPSHOT_JSON_BYTES,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::{Archive, Builder, EntryType, Header};

#[cfg(feature = "derived-snapshot-signatures")]
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Stable archive container format version.
const ARCHIVE_FORMAT_VERSION: u32 = 1;
/// Required top-level archive directory.
const ARCHIVE_ROOT: &str = "projectatlas-derived-snapshot";
/// Required manifest entry path.
const MANIFEST_PATH: &str = "projectatlas-derived-snapshot/manifest.json";
/// Required portable graph entry path.
const PAYLOAD_PATH: &str = "projectatlas-derived-snapshot/graph.json";
/// Optional signature entry path.
const SIGNATURE_PATH: &str = "projectatlas-derived-snapshot/signature.json";
/// Maximum accepted compressed archive size.
const MAX_COMPRESSED_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum expanded manifest size.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Maximum expanded signature size.
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
/// Maximum number of allowed archive entries.
const MAX_ARCHIVE_ENTRIES: usize = 3;
/// Maximum accepted Zstandard decoder window log.
const MAX_ZSTD_WINDOW_LOG: u32 = 27;
#[cfg(feature = "derived-snapshot-signatures")]
/// Domain separator for archive signatures.
const SIGNATURE_DOMAIN: &[u8] = b"projectatlas.derived-snapshot.archive-signature.v1";

/// Successful portable archive export.
#[derive(Debug, Serialize)]
pub(super) struct SnapshotExportReport {
    /// Lossless UTF-8 written archive path, when one is available.
    pub(super) archive: Option<String>,
    /// Portable snapshot digest.
    pub(super) snapshot_digest: String,
    /// Uncompressed payload bytes.
    pub(super) payload_bytes: u64,
    /// Final compressed archive bytes.
    pub(super) compressed_bytes: u64,
    /// Signature handling result.
    pub(super) signature: SnapshotSignatureState,
    /// Exported content inventory.
    pub(super) content: Vec<DerivedSnapshotContent>,
}

/// Successful portable archive import.
#[derive(Debug, Serialize)]
pub(super) struct SnapshotImportReport {
    /// Lossless UTF-8 read archive path, when one is available.
    pub(super) archive: Option<String>,
    /// Signature handling result.
    pub(super) signature: SnapshotSignatureState,
    #[serde(flatten)]
    /// Atomic publication result.
    pub(super) publication: DerivedGraphSnapshotImport,
}

/// Honest signature handling state for the selected build and request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SnapshotSignatureState {
    /// No signature was requested or present for local use.
    UnsignedLocal,
    #[cfg(not(feature = "derived-snapshot-signatures"))]
    /// A signature exists but this build cannot verify it.
    PresentUnverified,
    #[cfg(feature = "derived-snapshot-signatures")]
    /// The embedded public key verifies the signature.
    VerifiedEmbedded,
    #[cfg(feature = "derived-snapshot-signatures")]
    /// The signature verifies against an explicitly trusted public key.
    VerifiedTrusted,
}

/// Export one deterministic tar.zst archive without overwriting an existing path.
pub(super) fn export_snapshot_archive(
    store: &AtlasStore,
    output: &Path,
    #[cfg(feature = "derived-snapshot-signatures")] signing_key: Option<&Path>,
) -> Result<SnapshotExportReport, CliError> {
    if output.exists() {
        return Err(CliError::InvalidInput(format!(
            "snapshot output '{}' already exists",
            output.display()
        )));
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "snapshot output directory '{}' does not exist",
            parent.display()
        )));
    }

    let snapshot = store.export_derived_graph_snapshot()?;
    let payload = snapshot.to_json()?;
    let manifest = SnapshotArchiveManifest {
        format_version: ARCHIVE_FORMAT_VERSION,
        root: ARCHIVE_ROOT.to_string(),
        payload: PAYLOAD_PATH.to_string(),
        payload_bytes: usize_to_u64(payload.len())?,
        payload_blake3: blake3::hash(&payload).to_hex().to_string(),
        snapshot_digest: snapshot.digest().to_string(),
    };
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let signature = {
        #[cfg(feature = "derived-snapshot-signatures")]
        {
            signing_key
                .map(|path| sign_archive(path, &manifest_bytes, &payload))
                .transpose()?
        }
        #[cfg(not(feature = "derived-snapshot-signatures"))]
        {
            None::<Vec<u8>>
        }
    };
    let signature_state = if signature.is_some() {
        #[cfg(feature = "derived-snapshot-signatures")]
        {
            SnapshotSignatureState::VerifiedEmbedded
        }
        #[cfg(not(feature = "derived-snapshot-signatures"))]
        {
            SnapshotSignatureState::PresentUnverified
        }
    } else {
        SnapshotSignatureState::UnsignedLocal
    };

    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| CliError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    {
        let encoder =
            zstd::stream::write::Encoder::new(temporary.as_file_mut(), 9).map_err(|source| {
                CliError::Io {
                    path: output.to_path_buf(),
                    source,
                }
            })?;
        let mut archive = Builder::new(encoder);
        append_entry(&mut archive, MANIFEST_PATH, &manifest_bytes)?;
        append_entry(&mut archive, PAYLOAD_PATH, &payload)?;
        if let Some(signature) = signature.as_deref() {
            append_entry(&mut archive, SIGNATURE_PATH, signature)?;
        }
        let encoder = archive.into_inner().map_err(|source| CliError::Io {
            path: output.to_path_buf(),
            source,
        })?;
        encoder.finish().map_err(|source| CliError::Io {
            path: output.to_path_buf(),
            source,
        })?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| CliError::Io {
            path: output.to_path_buf(),
            source,
        })?;
    let compressed_bytes = temporary
        .as_file()
        .metadata()
        .map_err(|source| CliError::Io {
            path: output.to_path_buf(),
            source,
        })?
        .len();
    if compressed_bytes > MAX_COMPRESSED_ARCHIVE_BYTES {
        return Err(CliError::InvalidInput(format!(
            "compressed snapshot is {compressed_bytes} bytes; maximum is {MAX_COMPRESSED_ARCHIVE_BYTES}"
        )));
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| CliError::Io {
            path: output.to_path_buf(),
            source: error.error,
        })?;

    Ok(SnapshotExportReport {
        archive: lossless_native_path_display(output),
        snapshot_digest: snapshot.digest().to_string(),
        payload_bytes: usize_to_u64(payload.len())?,
        compressed_bytes,
        signature: signature_state,
        content: snapshot.content().to_vec(),
    })
}

/// Validate one archive completely, then publish through the database boundary.
pub(super) fn import_snapshot_archive(
    store: &mut AtlasStore,
    archive_path: &Path,
    required_digest: Option<&str>,
    #[cfg(feature = "derived-snapshot-signatures")] trusted_public_key: Option<&Path>,
) -> Result<SnapshotImportReport, CliError> {
    let archive = read_archive(archive_path)?;
    let manifest = serde_json::from_slice::<SnapshotArchiveManifest>(&archive.manifest)?;
    manifest.validate()?;
    if manifest.payload_bytes != usize_to_u64(archive.payload.len())?
        || manifest.payload_blake3 != blake3::hash(&archive.payload).to_hex().to_string()
    {
        return invalid("snapshot archive payload digest or byte count does not match");
    }
    let snapshot = DerivedGraphSnapshot::from_json(&archive.payload)?;
    if snapshot.digest() != manifest.snapshot_digest {
        return invalid("snapshot archive manifest names a different snapshot digest");
    }
    if let Some(required) = required_digest {
        require_digest(required)?;
        if required != snapshot.digest() {
            return invalid("snapshot digest does not match the required trust pin");
        }
    }
    let signature = verify_archive_signature(
        archive.signature.as_deref(),
        &archive.manifest,
        &archive.payload,
        #[cfg(feature = "derived-snapshot-signatures")]
        trusted_public_key,
    )?;
    let publication = store.import_derived_graph_snapshot(&snapshot)?;
    Ok(SnapshotImportReport {
        archive: lossless_native_path_display(archive_path),
        signature,
        publication,
    })
}

/// Strict archive manifest bound to one payload.
#[derive(Debug, Deserialize, Serialize)]
struct SnapshotArchiveManifest {
    /// Stable archive format version.
    format_version: u32,
    /// Required archive root.
    root: String,
    /// Required portable graph entry path.
    payload: String,
    /// Exact uncompressed payload bytes.
    payload_bytes: u64,
    /// BLAKE3 digest of the encoded payload.
    payload_blake3: String,
    /// Integrity digest declared by the portable snapshot.
    snapshot_digest: String,
}

impl SnapshotArchiveManifest {
    /// Validate the closed archive contract and bounded payload metadata.
    fn validate(&self) -> Result<(), CliError> {
        if self.format_version != ARCHIVE_FORMAT_VERSION {
            return invalid("unsupported snapshot archive version");
        }
        if self.root != ARCHIVE_ROOT || self.payload != PAYLOAD_PATH {
            return invalid("snapshot archive root or payload path is invalid");
        }
        if self.payload_bytes > MAX_DERIVED_SNAPSHOT_JSON_BYTES {
            return invalid("snapshot archive payload exceeds the expanded limit");
        }
        require_digest(&self.payload_blake3)?;
        require_digest(&self.snapshot_digest)
    }
}

/// Bounded entries read from one validated archive shape.
struct SnapshotArchiveParts {
    /// Raw manifest bytes.
    manifest: Vec<u8>,
    /// Raw portable graph bytes.
    payload: Vec<u8>,
    /// Optional raw signature bytes.
    signature: Option<Vec<u8>>,
}

/// Read an archive without extracting entries to disk.
fn read_archive(path: &Path) -> Result<SnapshotArchiveParts, CliError> {
    let compressed_bytes = fs::metadata(path)
        .map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if compressed_bytes > MAX_COMPRESSED_ARCHIVE_BYTES {
        return invalid("compressed snapshot archive exceeds the input limit");
    }
    let file = File::open(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut decoder = zstd::stream::read::Decoder::new(file).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decoder
        .window_log_max(MAX_ZSTD_WINDOW_LOG)
        .map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut archive = Archive::new(decoder);
    let mut manifest = None;
    let mut payload = None;
    let mut signature = None;
    let mut entries = 0_usize;
    for entry in archive.entries().map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })? {
        entries = entries.saturating_add(1);
        if entries > MAX_ARCHIVE_ENTRIES {
            return invalid("snapshot archive contains too many entries");
        }
        let entry = entry.map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if entry.header().entry_type() != EntryType::Regular {
            return invalid("snapshot archive contains a non-regular entry");
        }
        let entry_path_bytes = entry.path_bytes();
        let entry_path = std::str::from_utf8(entry_path_bytes.as_ref()).map_err(|_source| {
            CliError::InvalidInput("snapshot archive path is not UTF-8".into())
        })?;
        let (slot, maximum) = match entry_path {
            MANIFEST_PATH => (&mut manifest, MAX_MANIFEST_BYTES),
            PAYLOAD_PATH => (&mut payload, MAX_DERIVED_SNAPSHOT_JSON_BYTES),
            SIGNATURE_PATH => (&mut signature, MAX_SIGNATURE_BYTES),
            _ => return invalid("snapshot archive contains an unexpected path"),
        };
        if slot.is_some() {
            return invalid("snapshot archive contains a duplicate path");
        }
        let entry_size = entry.size();
        if entry_size > maximum {
            return invalid("snapshot archive entry exceeds its expanded limit");
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry_size).map_err(|_source| {
            CliError::InvalidInput("snapshot archive entry size cannot be represented".into())
        })?);
        entry
            .take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| CliError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if usize_to_u64(bytes.len())? != entry_size {
            return invalid("snapshot archive entry is truncated");
        }
        *slot = Some(bytes);
    }
    Ok(SnapshotArchiveParts {
        manifest: manifest
            .ok_or_else(|| CliError::InvalidInput("snapshot archive manifest is missing".into()))?,
        payload: payload
            .ok_or_else(|| CliError::InvalidInput("snapshot archive payload is missing".into()))?,
        signature,
    })
}

/// Append one deterministic regular-file archive entry.
fn append_entry<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), CliError> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(usize_to_u64(bytes.len())?);
    header.set_cksum();
    archive
        .append_data(&mut header, path, bytes)
        .map_err(|source| CliError::Io {
            path: PathBuf::from(path),
            source,
        })
}

/// Validate a lowercase BLAKE3 hexadecimal digest.
fn require_digest(value: &str) -> Result<(), CliError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        invalid("snapshot digest is not 64 lowercase hexadecimal characters")
    }
}

/// Convert an in-memory length to the archive count type.
fn usize_to_u64(value: usize) -> Result<u64, CliError> {
    u64::try_from(value)
        .map_err(|_source| CliError::InvalidInput("snapshot size cannot be represented".into()))
}

/// Return a typed invalid-input error.
fn invalid<T>(message: &str) -> Result<T, CliError> {
    Err(CliError::InvalidInput(message.to_string()))
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Self-contained Ed25519 signature record.
#[derive(Deserialize, Serialize)]
struct SnapshotArchiveSignature {
    /// Signature algorithm and framing contract.
    algorithm: String,
    /// Embedded public key as lowercase hexadecimal.
    public_key: String,
    /// BLAKE3 identity of the public key.
    key_id: String,
    /// Ed25519 signature as lowercase hexadecimal.
    signature: String,
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Sign the framed manifest and payload digest.
fn sign_archive(key_path: &Path, manifest: &[u8], payload: &[u8]) -> Result<Vec<u8>, CliError> {
    require_private_signing_key_permissions(key_path)?;
    let mut secret = read_hex_file::<32>(key_path, "Ed25519 signing key")?;
    let signing_key = SigningKey::from_bytes(&secret);
    secret.fill(0);
    let public_key = signing_key.verifying_key().to_bytes();
    let signature = signing_key.sign(&signature_digest(manifest, payload)?);
    Ok(serde_json::to_vec(&SnapshotArchiveSignature {
        algorithm: "ed25519-blake3-v1".to_string(),
        public_key: encode_hex(&public_key),
        key_id: blake3::hash(&public_key).to_hex().to_string(),
        signature: encode_hex(&signature.to_bytes()),
    })?)
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Verify an optional signature and explicit trust policy.
fn verify_archive_signature(
    signature: Option<&[u8]>,
    manifest: &[u8],
    payload: &[u8],
    trusted_public_key: Option<&Path>,
) -> Result<SnapshotSignatureState, CliError> {
    let Some(signature) = signature else {
        if trusted_public_key.is_some() {
            return invalid("trusted signature policy requires a signed snapshot archive");
        }
        return Ok(SnapshotSignatureState::UnsignedLocal);
    };
    let signature = serde_json::from_slice::<SnapshotArchiveSignature>(signature)?;
    if signature.algorithm != "ed25519-blake3-v1" {
        return invalid("snapshot archive signature algorithm is unsupported");
    }
    let public_bytes = decode_hex_array::<32>(&signature.public_key, "snapshot public key")?;
    if signature.key_id != blake3::hash(&public_bytes).to_hex().to_string() {
        return invalid("snapshot signature key identity does not match its public key");
    }
    let verifying_key = VerifyingKey::from_bytes(&public_bytes)
        .map_err(|_source| CliError::InvalidInput("snapshot public key is invalid".into()))?;
    let signature_bytes = decode_hex_array::<64>(&signature.signature, "snapshot signature")?;
    let signature = Signature::try_from(signature_bytes.as_slice())
        .map_err(|_source| CliError::InvalidInput("snapshot signature is invalid".into()))?;
    verifying_key
        .verify(&signature_digest(manifest, payload)?, &signature)
        .map_err(|_source| {
            CliError::InvalidInput("snapshot archive signature verification failed".into())
        })?;
    if let Some(trusted_path) = trusted_public_key {
        let trusted = read_hex_file::<32>(trusted_path, "trusted Ed25519 public key")?;
        if trusted != public_bytes {
            return invalid("snapshot signer is not the explicitly trusted public key");
        }
        Ok(SnapshotSignatureState::VerifiedTrusted)
    } else {
        Ok(SnapshotSignatureState::VerifiedEmbedded)
    }
}

#[cfg(not(feature = "derived-snapshot-signatures"))]
/// Report signature presence honestly when verification support is absent.
#[allow(clippy::unnecessary_wraps)]
fn verify_archive_signature(
    signature: Option<&[u8]>,
    _manifest: &[u8],
    _payload: &[u8],
) -> Result<SnapshotSignatureState, CliError> {
    Ok(if signature.is_some() {
        SnapshotSignatureState::PresentUnverified
    } else {
        SnapshotSignatureState::UnsignedLocal
    })
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Compute the domain-separated digest signed by Ed25519.
fn signature_digest(manifest: &[u8], payload: &[u8]) -> Result<[u8; 32], CliError> {
    let mut digest = blake3::Hasher::new();
    digest.update(SIGNATURE_DOMAIN);
    digest.update(&usize_to_u64(manifest.len())?.to_le_bytes());
    digest.update(manifest);
    digest.update(&usize_to_u64(payload.len())?.to_le_bytes());
    digest.update(payload);
    Ok(*digest.finalize().as_bytes())
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Read one bounded fixed-size hexadecimal key file.
fn read_hex_file<const N: usize>(path: &Path, label: &str) -> Result<[u8; N], CliError> {
    let metadata = fs::metadata(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > 4_096 {
        return Err(CliError::InvalidInput(format!(
            "{label} file must be a regular file no larger than 4096 bytes"
        )));
    }
    let value = fs::read_to_string(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decode_hex_array::<N>(value.trim(), label)
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Decode one fixed-size hexadecimal key or signature.
fn decode_hex_array<const N: usize>(value: &str, label: &str) -> Result<[u8; N], CliError> {
    if value.len() != N * 2 {
        return Err(CliError::InvalidInput(format!(
            "{label} must contain exactly {} hexadecimal characters",
            N * 2
        )));
    }
    let mut decoded = [0_u8; N];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        decoded[index] = decode_nibble(pair[0])
            .and_then(|high| decode_nibble(pair[1]).map(|low| (high << 4) | low))
            .ok_or_else(|| CliError::InvalidInput(format!("{label} is not hexadecimal")))?;
    }
    Ok(decoded)
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Decode one hexadecimal nibble.
const fn decode_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(feature = "derived-snapshot-signatures")]
/// Encode bytes as lowercase hexadecimal.
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(all(feature = "derived-snapshot-signatures", unix))]
/// Require a signing key inaccessible to group and other Unix users.
fn require_private_signing_key_permissions(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return invalid("Ed25519 signing key must not be accessible by group or other users");
    }
    Ok(())
}

#[cfg(all(feature = "derived-snapshot-signatures", not(unix)))]
/// Require a regular signing key file on platforms without Unix mode bits.
fn require_private_signing_key_permissions(path: &Path) -> Result<(), CliError> {
    if path.is_file() {
        Ok(())
    } else {
        invalid("Ed25519 signing key path is not a regular file")
    }
}

#[cfg(test)]
mod tests {
    use super::{MANIFEST_PATH, PAYLOAD_PATH, append_entry, read_archive, require_digest};

    #[test]
    fn archive_paths_and_digest_shape_are_closed() {
        assert!(MANIFEST_PATH.starts_with("projectatlas-derived-snapshot/"));
        assert!(PAYLOAD_PATH.starts_with("projectatlas-derived-snapshot/"));
        assert!(require_digest(&"a".repeat(64)).is_ok());
        assert!(require_digest(&"A".repeat(64)).is_err());
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn archive_reader_rejects_duplicate_and_unexpected_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        fn archive_with(
            root: &std::path::Path,
            name: &str,
            paths: &[&str],
        ) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
            let path = root.join(name);
            let file = std::fs::File::create(&path)?;
            let encoder = zstd::stream::write::Encoder::new(file, 1)?;
            let mut archive = tar::Builder::new(encoder);
            for entry_path in paths {
                append_entry(&mut archive, entry_path, b"{}")?;
            }
            archive.into_inner()?.finish()?;
            Ok(path)
        }

        let temp = tempfile::tempdir()?;
        let duplicate = archive_with(
            temp.path(),
            "duplicate.tar.zst",
            &[MANIFEST_PATH, MANIFEST_PATH, PAYLOAD_PATH],
        )?;
        assert!(read_archive(&duplicate).is_err());
        let unexpected = archive_with(
            temp.path(),
            "unexpected.tar.zst",
            &[MANIFEST_PATH, "other-root/graph.json"],
        )?;
        assert!(read_archive(&unexpected).is_err());
        Ok(())
    }

    #[cfg(feature = "derived-snapshot-signatures")]
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn explicit_ed25519_policy_rejects_tampering_and_untrusted_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::{SnapshotSignatureState, encode_hex, sign_archive, verify_archive_signature};
        use ed25519_dalek::SigningKey;
        use std::fs;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let secret_path = temp.path().join("signing-key.hex");
        let trusted_path = temp.path().join("trusted-key.hex");
        let secret = [7_u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        fs::write(&secret_path, encode_hex(&secret))?;
        fs::write(
            &trusted_path,
            encode_hex(&signing_key.verifying_key().to_bytes()),
        )?;
        #[cfg(unix)]
        fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o600))?;
        let manifest = br#"{"format_version":1}"#;
        let payload = br#"{"graph":[]}"#;
        let signature = sign_archive(&secret_path, manifest, payload)?;
        assert_eq!(
            verify_archive_signature(Some(&signature), manifest, payload, Some(&trusted_path))?,
            SnapshotSignatureState::VerifiedTrusted
        );
        assert!(verify_archive_signature(None, manifest, payload, Some(&trusted_path)).is_err());
        assert!(
            verify_archive_signature(
                Some(&signature),
                manifest,
                br#"{"graph":["tampered"]}"#,
                Some(&trusted_path)
            )
            .is_err()
        );
        fs::write(&trusted_path, encode_hex(&[9_u8; 32]))?;
        assert!(
            verify_archive_signature(Some(&signature), manifest, payload, Some(&trusted_path))
                .is_err()
        );
        Ok(())
    }
}
