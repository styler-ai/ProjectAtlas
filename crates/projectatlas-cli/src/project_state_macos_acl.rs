//! Opaque, owned Darwin ACL snapshots accessed only through borrowed file descriptors.
//!
//! This is the sole reviewed unsafe exception for generated-state ACL preservation.
//! The six declarations follow Apple's Libc sys/acl.h. Native ACL allocations are
//! never decoded, shared, or transferred; their external encoding is only compared.

#![allow(unsafe_code)]

use std::ffi::{c_int, c_void};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::ptr::NonNull;

/// Generous ceiling above Darwin's documented 128-entry, page-sized ACL family.
const MAX_ENCODED_ACL_BYTES: usize = 1024 * 1024;

unsafe extern "C" {
    /// Return an independently allocated extended ACL, or null with native errno.
    fn acl_get_fd(fd: c_int) -> *mut c_void;
    /// Apply the extended ACL to the object held by fd without a pathname lookup.
    fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
    /// Allocate an ACL with capacity for count entries, initially empty.
    fn acl_init(count: c_int) -> *mut c_void;
    /// Release an allocation returned by the native ACL family.
    fn acl_free(allocation: *mut c_void) -> c_int;
    /// Return the required external-encoding byte length, or minus one on error.
    fn acl_size(acl: *mut c_void) -> isize;
    /// Write the external encoding into an aligned buffer of at least size bytes.
    fn acl_copy_ext(buffer: *mut c_void, acl: *mut c_void, size: isize) -> isize;
}

/// Preserve absence separately from an allocated native ACL, including empty ACLs.
pub(super) struct AccessAcl {
    /// A native snapshot owns its opaque allocation and its complete comparison image.
    value: Option<NativeAllocation>,
}

impl AccessAcl {
    /// Capture the descriptor's extended ACL without following any project name.
    pub(super) fn read(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native ACL snapshot requires a regular file or directory",
            ));
        }
        // SAFETY: the borrowed File keeps this fd live for the complete native call.
        let pointer = unsafe { acl_get_fd(file.as_raw_fd()) };
        if pointer.is_null() {
            let source = io::Error::last_os_error();
            // Apple's filesec getter reports ENOENT when no ACL is attached. A
            // live descriptor still identifies an object even after its unlink.
            if source.kind() == io::ErrorKind::NotFound {
                file.metadata()?;
                return Ok(Self { value: None });
            }
            return Err(source);
        }
        NativeAllocation::capture(pointer).map(|value| Self { value: Some(value) })
    }

    /// Allocate a canonical empty extended ACL for descriptor-bound clearing.
    pub(super) fn empty() -> io::Result<Self> {
        // SAFETY: zero is a valid initial capacity; the result is independently owned.
        let pointer = unsafe { acl_init(0) };
        NativeAllocation::capture(pointer).map(|value| Self { value: Some(value) })
    }

    /// Apply this opaque snapshot, clearing inherited policy when the source is absent.
    pub(super) fn apply(&self, file: &File) -> io::Result<()> {
        let Some(value) = &self.value else {
            return Self::empty()?.apply(file);
        };
        // SAFETY: File and the independently owned native ACL remain live; neither
        // object is consumed by acl_set_fd, and the library interprets its own ACL.
        if unsafe { acl_set_fd(file.as_raw_fd(), value.pointer.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Require no entries or native ACL flags, while retaining the captured absence state.
    pub(super) fn is_empty(&self) -> io::Result<bool> {
        Ok(self.value.is_none() || self == &Self::empty()?)
    }

    /// Expose the native no-ACL result to the owning absence/empty compatibility test.
    #[cfg(test)]
    pub(super) fn is_absent(&self) -> bool {
        self.value.is_none()
    }
}

impl PartialEq for AccessAcl {
    fn eq(&self, other: &Self) -> bool {
        self.value.as_ref().map(|value| &value.encoding)
            == other.value.as_ref().map(|value| &value.encoding)
    }
}

impl Eq for AccessAcl {}

/// Own exactly one native allocation and its OS-generated opaque comparison bytes.
struct NativeAllocation {
    /// Non-null pointer returned by `acl_get_fd` or `acl_init`; never decoded.
    pointer: NonNull<c_void>,
    /// Portable-endian external encoding includes the ACL flags and ordered ACEs.
    encoding: Vec<u8>,
}

impl NativeAllocation {
    /// Establish RAII ownership before any fallible serialization or buffer allocation.
    fn capture(pointer: *mut c_void) -> io::Result<Self> {
        let pointer = NonNull::new(pointer).ok_or_else(io::Error::last_os_error)?;
        let mut allocation = Self {
            pointer,
            encoding: Vec::new(),
        };
        allocation.encoding = allocation.external_encoding()?;
        Ok(allocation)
    }

    /// Compare native encodings without introducing an ACL parser or ABI layout model.
    fn external_encoding(&self) -> io::Result<Vec<u8>> {
        // SAFETY: this object uniquely owns a valid native ACL allocation.
        let native_size = unsafe { acl_size(self.pointer.as_ptr()) };
        if native_size < 0 {
            return Err(io::Error::last_os_error());
        }
        let size = usize::try_from(native_size)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        if !(1..=MAX_ENCODED_ACL_BYTES).contains(&size) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native ACL encoding size is unsupported",
            ));
        }
        // Darwin's external representation contains UUID bytes and 32-bit fields.
        // Machine words provide sufficient alignment on both supported 64-bit ABIs;
        // all bits remain valid words after the native call. Padding starts initialized.
        let word_count = size.div_ceil(std::mem::size_of::<usize>());
        let mut words = Vec::<usize>::new();
        words
            .try_reserve_exact(word_count)
            .map_err(|source| io::Error::new(io::ErrorKind::OutOfMemory, source))?;
        words.resize(word_count, 0);
        // SAFETY: the aligned, initialized buffer holds at least native_size bytes;
        // acl_copy_ext writes at most that size and does not retain either pointer.
        let copied = unsafe {
            acl_copy_ext(
                words.as_mut_ptr().cast(),
                self.pointer.as_ptr(),
                native_size,
            )
        };
        if copied < 0 {
            return Err(io::Error::last_os_error());
        }
        if copied != native_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native ACL encoding length changed",
            ));
        }
        let mut encoding = Vec::new();
        encoding
            .try_reserve_exact(size)
            .map_err(|source| io::Error::new(io::ErrorKind::OutOfMemory, source))?;
        encoding.extend(words.iter().flat_map(|word| word.to_ne_bytes()).take(size));
        Ok(encoding)
    }
}

impl Drop for NativeAllocation {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of the allocation returned by native ACL
        // routines; no pointer escapes and all borrowers end before this drop.
        let _ = unsafe { acl_free(self.pointer.as_ptr()) };
    }
}
