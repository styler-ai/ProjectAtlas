//! Native, lossless identity for one canonical project root.

use crate::{CoreError, CoreResult, normalize_native_path_display};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Version of the durable native project-root codec.
pub const CANONICAL_PROJECT_ROOT_CODEC_VERSION: u8 = 1;

/// One canonical native filesystem root.
///
/// Equality is native-path equality after filesystem canonicalization. The
/// value is the authority for routing and persistence; its display projection
/// is only for terminal diagnostics and compatibility metadata.
#[derive(Clone, Debug)]
pub struct CanonicalProjectRoot(PathBuf);

impl PartialEq for CanonicalProjectRoot {
    fn eq(&self, other: &Self) -> bool {
        native_identity_paths_equal(&self.0, &other.0)
    }
}

impl Eq for CanonicalProjectRoot {}

impl Hash for CanonicalProjectRoot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        #[cfg(windows)]
        windows_identity::ordinal_key(&self.0).hash(state);
        #[cfg(not(windows))]
        self.0.hash(state);
    }
}

impl CanonicalProjectRoot {
    /// Canonicalize an existing absolute directory into a native identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is relative, missing, or cannot be
    /// canonicalized by the host filesystem.
    pub fn from_path(path: &Path) -> CoreResult<Self> {
        if !path.is_absolute() {
            return Err(CoreError::InvalidCanonicalProjectRoot {
                path: path.to_path_buf(),
                reason: "project root must be absolute",
            });
        }
        let canonical =
            fs::canonicalize(path).map_err(|source| CoreError::CanonicalProjectRootIo {
                path: path.to_path_buf(),
                source,
            })?;
        if !fs::metadata(&canonical)
            .map_err(|source| CoreError::CanonicalProjectRootIo {
                path: canonical.clone(),
                source,
            })?
            .is_dir()
        {
            return Err(CoreError::InvalidCanonicalProjectRoot {
                path: canonical,
                reason: "project root must be a directory",
            });
        }
        Self::from_decoded_path(canonical)
    }

    /// Construct an identity from a durable native codec value.
    ///
    /// This is deliberately private: active roots must enter through
    /// [`Self::from_path`], which proves that the current filesystem object is
    /// an existing directory. Historical moved-root identities may be decoded
    /// while their old path is absent, but they still have to satisfy the
    /// absolute, canonical lexical native-path contract.
    fn from_decoded_path(path: PathBuf) -> CoreResult<Self> {
        let path = normalize_native_identity_path(path);
        if !path.is_absolute() {
            return Err(CoreError::InvalidCanonicalProjectRoot {
                path,
                reason: "project root must be absolute",
            });
        }
        if native_path_has_interior_nul(&path) {
            return Err(CoreError::CanonicalProjectRootCodec {
                reason: "native path contains an interior NUL",
            });
        }
        if !is_canonical_lexical_path(&path) {
            return Err(CoreError::CanonicalProjectRootCodec {
                reason: "native path is not canonically lexical",
            });
        }
        Ok(Self(path))
    }

    /// Return the canonical native path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Consume the identity and return its canonical native path.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.0
    }

    /// Return the UTF-8 terminal/compatibility display projection.
    ///
    /// A native root containing bytes that are not UTF-8 has no lossless text
    /// display. Callers carrying identity or compatibility state must retain
    /// this typed refusal instead of turning the path into replacement text.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NonUtf8Path`] when the native path has no lossless
    /// UTF-8 display projection.
    pub fn display_string(&self) -> CoreResult<String> {
        let value = self.0.to_str().ok_or_else(|| CoreError::NonUtf8Path {
            path: self.0.clone(),
        })?;
        Ok(crate::normalize_native_path_display_str(value))
    }

    /// Return an explicitly lossy rendering for terminal-only diagnostics.
    ///
    /// This method must not be used for identity comparison, persistence,
    /// compatibility keys, or structured adapter results.
    #[must_use]
    pub fn display_string_lossy(&self) -> String {
        normalize_native_path_display(&self.0)
    }

    /// Encode the native path without lossy text conversion.
    ///
    /// # Errors
    ///
    /// Returns an error when the host cannot encode its native path format.
    pub fn encode(&self) -> CoreResult<Vec<u8>> {
        let mut encoded = vec![CANONICAL_PROJECT_ROOT_CODEC_VERSION, platform_tag()];
        encoded.extend(native_path_bytes(self.0.as_os_str())?);
        Ok(encoded)
    }

    /// Decode one versioned native path codec value.
    ///
    /// # Errors
    ///
    /// Returns an error when the version, platform, bytes, or decoded path is
    /// invalid.
    pub fn decode(encoded: &[u8]) -> CoreResult<Self> {
        if encoded.len() < 3 {
            return Err(CoreError::CanonicalProjectRootCodec {
                reason: "codec value is truncated",
            });
        }
        if encoded[0] != CANONICAL_PROJECT_ROOT_CODEC_VERSION {
            return Err(CoreError::CanonicalProjectRootCodec {
                reason: "unsupported codec version",
            });
        }
        if encoded[1] != platform_tag() {
            return Err(CoreError::CanonicalProjectRootCodec {
                reason: "codec platform does not match this host",
            });
        }
        let path = native_path_from_bytes(&encoded[2..])?;
        Self::from_decoded_path(path)
    }
}

#[cfg(windows)]
/// Normalize Windows extended-path prefixes for native identity equality.
fn normalize_native_identity_path(path: PathBuf) -> PathBuf {
    if let Some(value) = path.to_str() {
        let normalized = PathBuf::from(crate::normalize_native_path_display_str(value));
        if normalized.is_absolute() {
            return normalized;
        }
    }
    path
}

#[cfg(not(windows))]
/// Preserve the canonical path unchanged on non-Windows hosts.
fn normalize_native_identity_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
/// Compare Windows native paths with Windows' invariant ordinal case rules.
fn native_identity_paths_equal(left: &Path, right: &Path) -> bool {
    windows_identity::ordinal_key(left) == windows_identity::ordinal_key(right)
}

#[cfg(not(windows))]
/// Compare native paths without applying a platform-specific case rule.
fn native_identity_paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the stable standard library does not expose Windows invariant ordinal case mapping; this bounded native query keeps identity lossless"
)]
/// Windows-native ordinal casing shared by root equality and hashing.
mod windows_identity {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    /// Windows flag for invariant uppercase mapping.
    const LCMAP_UPPERCASE: u32 = 0x0000_0200;
    /// Null-terminated invariant locale name passed to the Windows mapper.
    const INVARIANT_LOCALE: [u16; 1] = [0];

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        /// Map UTF-16 units using the Windows invariant locale.
        fn LCMapStringEx(
            locale_name: *const u16,
            map_flags: u32,
            source: *const u16,
            source_length: i32,
            destination: *mut u16,
            destination_length: i32,
            version_information: *const c_void,
            reserved: *mut c_void,
            sort_handle: isize,
        ) -> i32;
    }

    /// Return the invariant ordinal-uppercase UTF-16 units for one native path.
    pub(super) fn ordinal_key(path: &Path) -> Vec<u16> {
        let source = path.as_os_str().encode_wide().collect::<Vec<_>>();
        let Ok(source_length) = i32::try_from(source.len()) else {
            return source;
        };
        if source_length == 0 {
            return source;
        }

        // SAFETY: `source` is a live UTF-16 slice with an explicit length; the
        // null destination asks Windows only for the required output units.
        let required = unsafe {
            LCMapStringEx(
                INVARIANT_LOCALE.as_ptr(),
                LCMAP_UPPERCASE,
                source.as_ptr(),
                source_length,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        let Ok(required_length) = usize::try_from(required) else {
            return source;
        };
        if required_length == 0 {
            return source;
        }

        let Ok(destination_length) = i32::try_from(required_length) else {
            return source;
        };
        let mut destination = vec![0; required_length];
        // SAFETY: `destination` has exactly the size returned by the sizing
        // call, and both UTF-16 buffers use explicit lengths without NUL reads.
        let written = unsafe {
            LCMapStringEx(
                INVARIANT_LOCALE.as_ptr(),
                LCMAP_UPPERCASE,
                source.as_ptr(),
                source_length,
                destination.as_mut_ptr(),
                destination_length,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        let Ok(written_length) = usize::try_from(written) else {
            return source;
        };
        if written_length == 0 || written_length > destination.len() {
            return source;
        }
        destination.truncate(written_length);
        destination
    }
}

impl fmt::Display for CanonicalProjectRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display_string_lossy())
    }
}

#[cfg(unix)]
/// Return the host-native path codec tag.
fn platform_tag() -> u8 {
    1
}

#[cfg(windows)]
/// Return the host-native path codec tag.
fn platform_tag() -> u8 {
    2
}

#[cfg(not(any(unix, windows)))]
/// Return the host-native path codec tag.
fn platform_tag() -> u8 {
    3
}

#[cfg(unix)]
/// Encode an operating-system path without UTF-8 conversion.
#[allow(clippy::unnecessary_wraps)]
fn native_path_bytes(path: &OsStr) -> CoreResult<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_bytes().to_vec())
}

#[cfg(windows)]
/// Encode an operating-system path without UTF-8 conversion.
#[allow(clippy::unnecessary_wraps)]
fn native_path_bytes(path: &OsStr) -> CoreResult<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    let mut bytes = Vec::new();
    for unit in path.encode_wide() {
        bytes.extend(unit.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(not(any(unix, windows)))]
/// Encode an operating-system path using the host fallback representation.
fn native_path_bytes(path: &OsStr) -> CoreResult<Vec<u8>> {
    path.to_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| CoreError::CanonicalProjectRootCodec {
            reason: "native path encoding is unavailable on this host",
        })
}

#[cfg(unix)]
/// Decode an operating-system path without UTF-8 conversion.
///
/// The Unix conversion is infallible, but this result remains `CoreResult` to
/// match the fallible Windows codec helper at the shared decode call site.
#[allow(clippy::unnecessary_wraps)]
fn native_path_from_bytes(bytes: &[u8]) -> CoreResult<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
/// Decode a Windows UTF-16 operating-system path.
#[allow(clippy::chunks_exact_to_as_chunks)]
fn native_path_from_bytes(bytes: &[u8]) -> CoreResult<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    if units.len() * 2 != bytes.len() {
        return Err(CoreError::CanonicalProjectRootCodec {
            reason: "windows codec value has an odd byte length",
        });
    }
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(any(unix, windows)))]
/// Decode a host fallback path representation.
fn native_path_from_bytes(bytes: &[u8]) -> CoreResult<PathBuf> {
    let value =
        String::from_utf8(bytes.to_vec()).map_err(|_| CoreError::CanonicalProjectRootCodec {
            reason: "native path encoding is unavailable on this host",
        })?;
    Ok(PathBuf::from(value))
}

/// Return whether a decoded path has the lexical form emitted by canonicalization.
fn is_canonical_lexical_path(path: &Path) -> bool {
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        (bytes.len() == 1 || !bytes.ends_with(b"/")) && !bytes.windows(2).any(|pair| pair == b"//")
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        let is_separator = |unit: &u16| *unit == u16::from(b'/') || *unit == u16::from(b'\\');
        let leading_unc =
            units.first().is_some_and(is_separator) && units.get(1).is_some_and(is_separator);
        let body = if leading_unc { &units[2..] } else { &units[..] };
        let has_normal_component = path
            .components()
            .any(|component| matches!(component, std::path::Component::Normal(_)));
        !body.first().is_some_and(is_separator)
            && !body
                .windows(2)
                .any(|pair| is_separator(&pair[0]) && is_separator(&pair[1]))
            && (!units.last().is_some_and(is_separator) || !has_normal_component)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let value = path.to_string_lossy();
        return !value.ends_with('/') && !value.contains("//");
    }
}

#[cfg(unix)]
/// Return whether a Unix path contains an interior NUL byte.
fn native_path_has_interior_nul(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().contains(&0)
}

#[cfg(windows)]
/// Return whether a Windows path contains an interior NUL code unit.
fn native_path_has_interior_nul(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().any(|unit| unit == 0)
}

#[cfg(not(any(unix, windows)))]
/// Return whether a fallback path contains an interior NUL character.
fn native_path_has_interior_nul(path: &Path) -> bool {
    path.to_string_lossy().contains('\0')
}

#[cfg(test)]
mod tests {
    use super::CanonicalProjectRoot;
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn canonical_root_codec_round_trips_native_path() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let root = CanonicalProjectRoot::from_path(directory.path())?;
        let decoded = CanonicalProjectRoot::decode(&root.encode()?)?;
        if root != decoded || root.as_path() != decoded.as_path() {
            return Err("canonical root codec changed the native path".into());
        }
        Ok(())
    }

    #[test]
    fn canonical_root_codec_rejects_malformed_payloads() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let root = CanonicalProjectRoot::from_path(directory.path())?;
        let encoded = root.encode()?;

        for malformed in [
            encoded[..2].to_vec(),
            {
                let mut value = encoded.clone();
                value[0] = 0xff;
                value
            },
            {
                let mut value = encoded.clone();
                value[1] = 0xff;
                value
            },
        ] {
            if CanonicalProjectRoot::decode(&malformed).is_ok() {
                return Err("malformed canonical-root payload was accepted".into());
            }
        }

        let mut nul = encoded[..2].to_vec();
        nul.extend(super::native_path_bytes(std::ffi::OsStr::new(
            "/tmp\0root",
        ))?);
        if CanonicalProjectRoot::decode(&nul).is_ok() {
            return Err("interior-NUL canonical-root payload was accepted".into());
        }

        let relative = PathBuf::from("relative/project");
        let mut relative_payload = encoded[..2].to_vec();
        relative_payload.extend(super::native_path_bytes(relative.as_os_str())?);
        if CanonicalProjectRoot::decode(&relative_payload).is_ok() {
            return Err("relative canonical-root payload was accepted".into());
        }

        let noncanonical = if cfg!(windows) {
            PathBuf::from(r"C:\temp\..\project")
        } else {
            PathBuf::from("/tmp/../project")
        };
        let mut noncanonical_payload = encoded[..2].to_vec();
        noncanonical_payload.extend(super::native_path_bytes(noncanonical.as_os_str())?);
        if CanonicalProjectRoot::decode(&noncanonical_payload).is_ok() {
            return Err("non-canonical lexical root payload was accepted".into());
        }
        #[cfg(windows)]
        {
            let mut odd = encoded[..2].to_vec();
            odd.push(0);
            if CanonicalProjectRoot::decode(&odd).is_ok() {
                return Err("odd-byte Windows root payload was accepted".into());
            }
        }
        Ok(())
    }

    #[test]
    fn canonical_root_requires_an_existing_directory() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let regular_file = directory.path().join("regular-file");
        let missing = directory.path().join("missing-directory");
        std::fs::write(&regular_file, b"not a root")?;
        if CanonicalProjectRoot::from_path(&regular_file).is_ok()
            || CanonicalProjectRoot::from_path(&missing).is_ok()
        {
            return Err("non-directory canonical root was accepted".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn canonical_root_codec_round_trips_non_utf8_path() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::ffi::OsStringExt;
        let directory = tempdir()?;
        let name = std::ffi::OsString::from_vec(vec![b'r', b'o', b'o', b't', 0x80]);
        let path = directory.path().join(&name);
        fs::create_dir(&path)?;
        let root = CanonicalProjectRoot::from_path(&path)?;
        if root != CanonicalProjectRoot::decode(&root.encode()?)? {
            return Err("non-UTF-8 root codec changed the native path".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn canonical_root_display_refuses_raw_bytes_without_colliding_with_replacement_text()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::ffi::OsStringExt;

        let directory = tempdir()?;
        let raw_name = OsString::from_vec(vec![b'r', b'o', b'o', b't', 0x80]);
        let raw_path = directory.path().join(&raw_name);
        let replacement_path = directory.path().join("root�");
        fs::create_dir(&raw_path)?;
        fs::create_dir(&replacement_path)?;

        let raw = CanonicalProjectRoot::from_path(&raw_path)?;
        let replacement = CanonicalProjectRoot::from_path(&replacement_path)?;
        if raw == replacement || raw.encode()? == replacement.encode()? {
            return Err("raw and replacement-character roots collided".into());
        }
        if !matches!(
            raw.display_string(),
            Err(crate::CoreError::NonUtf8Path { .. })
        ) {
            return Err("raw root did not return typed display unavailability".into());
        }
        if raw.display_string_lossy() != replacement.display_string()? {
            return Err("test roots did not demonstrate their lossy display collision".into());
        }
        if CanonicalProjectRoot::decode(&raw.encode()?)? != raw
            || CanonicalProjectRoot::decode(&replacement.encode()?)? != replacement
        {
            return Err("native root codec round-trip changed one root".into());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn canonical_root_codec_preserves_volume_guid_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let path = PathBuf::from(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\repo");
        let mut encoded = vec![
            super::CANONICAL_PROJECT_ROOT_CODEC_VERSION,
            super::platform_tag(),
        ];
        encoded.extend(super::native_path_bytes(path.as_os_str())?);
        let decoded = CanonicalProjectRoot::decode(&encoded)?;
        if !decoded.as_path().is_absolute() || decoded.as_path() != path {
            return Err("volume-GUID identity lost its native absolute path".into());
        }
        if CanonicalProjectRoot::decode(&decoded.encode()?)? != decoded {
            return Err("volume-GUID identity codec round-trip changed its native path".into());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn canonical_root_codec_rejects_noncanonical_volume_separators_and_accepts_roots()
    -> Result<(), Box<dyn std::error::Error>> {
        let valid_roots = [
            (PathBuf::from("C:\\"), PathBuf::from("C:/")),
            (
                PathBuf::from(r"\\server\share\"),
                PathBuf::from("//server/share/"),
            ),
            (
                PathBuf::from(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\"),
                PathBuf::from(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\"),
            ),
        ];
        for (path, expected) in valid_roots {
            let mut encoded = vec![
                super::CANONICAL_PROJECT_ROOT_CODEC_VERSION,
                super::platform_tag(),
            ];
            encoded.extend(super::native_path_bytes(path.as_os_str())?);
            let decoded = CanonicalProjectRoot::decode(&encoded)?;
            if !decoded.as_path().is_absolute() || decoded.as_path() != expected {
                return Err(
                    "Windows root identity was not retained as an absolute native path".into(),
                );
            }
            if CanonicalProjectRoot::decode(&decoded.encode()?)? != decoded {
                return Err("Windows root identity codec round-trip changed its path".into());
            }
        }

        for path in [
            PathBuf::from(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\\repo"),
            PathBuf::from(r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\repo\"),
        ] {
            let mut encoded = vec![
                super::CANONICAL_PROJECT_ROOT_CODEC_VERSION,
                super::platform_tag(),
            ];
            encoded.extend(super::native_path_bytes(path.as_os_str())?);
            if CanonicalProjectRoot::decode(&encoded).is_ok() {
                return Err("noncanonical volume-GUID separator form was accepted".into());
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn canonical_root_codec_keeps_extended_drive_and_unc_compatibility()
    -> Result<(), Box<dyn std::error::Error>> {
        for (encoded_path, expected_path) in [
            (r"\\?\C:\repo", PathBuf::from("C:/repo")),
            (
                r"\\?\UNC\server\share\repo",
                PathBuf::from("//server/share/repo"),
            ),
        ] {
            let path = PathBuf::from(encoded_path);
            let mut encoded = vec![
                super::CANONICAL_PROJECT_ROOT_CODEC_VERSION,
                super::platform_tag(),
            ];
            encoded.extend(super::native_path_bytes(path.as_os_str())?);
            let decoded = CanonicalProjectRoot::decode(&encoded)?;
            if decoded.as_path() != expected_path
                || CanonicalProjectRoot::decode(&decoded.encode()?)? != decoded
            {
                return Err("extended Windows identity compatibility changed".into());
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn canonical_root_case_only_rename_preserves_identity_collections_and_codec()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::{HashMap, HashSet};

        let directory = tempdir()?;
        let original_path = directory.path().join("CaseOnlyRoot");
        let staging_path = directory.path().join("CaseOnlyRootStaging");
        let renamed_path = directory.path().join("caseonlyroot");
        std::fs::create_dir(&original_path)?;
        let original = CanonicalProjectRoot::from_path(&original_path)?;

        std::fs::rename(&original_path, &staging_path)?;
        std::fs::rename(&staging_path, &renamed_path)?;
        let renamed = CanonicalProjectRoot::from_path(&renamed_path)?;
        if original.encode()? == renamed.encode()? {
            return Err("case-only rename did not retain distinct native spellings".into());
        }
        if original != renamed {
            return Err("case-only rename changed the native project identity".into());
        }

        let mut identities = HashSet::new();
        identities.insert(original.clone());
        identities.insert(renamed.clone());
        if identities.len() != 1 {
            return Err("case-equivalent roots were not deduplicated".into());
        }
        let mut values = HashMap::new();
        values.insert(original, "case-only root");
        if values.get(&renamed).copied() != Some("case-only root") {
            return Err("case-equivalent root was not found by HashMap lookup".into());
        }

        let encoded = renamed.encode()?;
        let decoded = CanonicalProjectRoot::decode(&encoded)?;
        if decoded.as_path() != renamed.as_path() || decoded.encode()? != encoded {
            return Err("case-only root codec round-trip changed native UTF-16".into());
        }
        Ok(())
    }
}
