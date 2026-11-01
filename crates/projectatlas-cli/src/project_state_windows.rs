//! Publish the retained Windows staging object rather than resolving its old name.
#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_RENAME_INFO, FILE_RENAME_INFO_0, FileRenameInfo, SetFileInformationByHandle,
};

/// Rename the held file, preserving the caller's replacement/no-clobber contract.
///
/// # Errors
/// Returns invalid path/size errors or the native rename refusal. The caller
/// retains its original handle and remains responsible for owned failure cleanup.
pub(super) fn publish(file: &File, destination: &Path, replace: bool) -> io::Result<()> {
    let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
    if name.is_empty() || name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "publication destination is empty or contains NUL",
        ));
    }
    let length = name
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination is too long"))?;
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = offset
        .checked_add(length as usize)
        .and_then(|bytes| bytes.checked_add(size_of::<u16>()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rename buffer is too large"))?;
    let capacity = bytes.max(size_of::<FILE_RENAME_INFO>());
    let size = u32::try_from(capacity)
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidInput, source))?;
    // Native pointer-sized words provide FILE_RENAME_INFO's required alignment;
    // all remaining bytes, including its optional UTF-16 terminator, start zero.
    let mut buffer = vec![0_usize; capacity.div_ceil(size_of::<usize>())];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: the allocation is aligned for the canonical SDK structure, has
    // room for its full header and the checked variable-length UTF-16 name,
    // and stays alive during this synchronous native call. No pointer escapes.
    // The SDK operates on the caller's retained file handle, never its old path.
    let success = unsafe {
        information.write(FILE_RENAME_INFO {
            Anonymous: FILE_RENAME_INFO_0 {
                ReplaceIfExists: replace,
            },
            RootDirectory: std::ptr::null_mut(),
            FileNameLength: length,
            FileName: [0],
        });
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            (&raw mut (*information).FileName).cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            information.cast(),
            size,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;

    /// Publishing a readonly stage never clears its retained access attributes.
    #[test]
    fn publication_preserves_the_readonly_stage_attribute() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let stage = root.path().join("stage");
        let output = root.path().join("output");
        let file = fs::OpenOptions::new()
            .write(true)
            .access_mode(0xc001_0000)
            .share_mode(4)
            .create_new(true)
            .open(&stage)?;
        (&file).write_all(b"readonly payload")?;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions)?;
        publish(&file, &output, false)?;
        drop(file);
        let valid = fs::read(&output)? == b"readonly payload"
            && fs::metadata(&output)?.permissions().readonly()
            && !stage.exists();
        let mut permissions = fs::metadata(&output)?.permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "Restore only this disposable readonly fixture after verification"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&output, permissions)?;
        if !valid {
            return Err(io::Error::other(
                "publication cleared readonly stage policy",
            ));
        }
        Ok(())
    }

    /// An exchanged old name cannot select a foreign object for publication.
    #[test]
    fn publication_renames_the_retained_object_and_preserves_no_clobber() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let stage = root.path().join("stage");
        let moved = root.path().join("moved");
        let destination = root.path().join("output");
        let file = fs::OpenOptions::new()
            .write(true)
            .access_mode(0xc001_0000)
            .share_mode(4)
            .create_new(true)
            .open(&stage)?;
        (&file).write_all(b"retained payload")?;
        fs::rename(&stage, &moved)?;
        fs::write(&stage, b"foreign entry")?;
        fs::write(&destination, b"existing destination")?;
        if publish(&file, &destination, false).is_ok()
            || fs::read(&destination)? != b"existing destination"
            || fs::read(&stage)? != b"foreign entry"
            || !moved.exists()
        {
            return Err(io::Error::other(
                "no-clobber publication changed an existing entry",
            ));
        }
        publish(&file, &destination, true)?;
        drop(file);
        if fs::read(&destination)? != b"retained payload"
            || fs::read(&stage)? != b"foreign entry"
            || moved.exists()
        {
            return Err(io::Error::other(
                "publication resolved the exchanged stage name",
            ));
        }
        Ok(())
    }
}
