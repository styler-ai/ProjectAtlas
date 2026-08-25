//! Native, lossless identity for one canonical project root.

use crate::{CoreError, CoreResult, normalize_native_path_display};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Version of the durable native project-root codec.
pub const CANONICAL_PROJECT_ROOT_CODEC_VERSION: u8 = 1;

/// One canonical native filesystem root.
///
/// Equality is native-path equality after filesystem canonicalization. The
/// value is the authority for routing and persistence; its display projection
/// is only for terminal diagnostics and compatibility metadata.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CanonicalProjectRoot(PathBuf);

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

    /// Return the bounded terminal/compatibility display projection.
    #[must_use]
    pub fn display_string(&self) -> String {
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
        return PathBuf::from(crate::normalize_native_path_display_str(value));
    }
    path
}

#[cfg(not(windows))]
/// Preserve the canonical path unchanged on non-Windows hosts.
fn normalize_native_identity_path(path: PathBuf) -> PathBuf {
    path
}

impl fmt::Display for CanonicalProjectRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display_string())
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
        return (bytes.len() == 1 || !bytes.ends_with(b"/"))
            && !bytes.windows(2).any(|pair| pair == b"//");
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        let separator = u16::from(b'/');
        let leading_unc = units.starts_with(&[separator, separator]);
        let body = if leading_unc { &units[2..] } else { &units[..] };
        (units.len() <= 3 || units.last() != Some(&separator))
            && !body.windows(2).any(|pair| pair == [separator, separator])
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
}
