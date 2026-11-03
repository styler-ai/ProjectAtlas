//! Causal source-cursor coverage for native indirection, lifetime and fallback.

use super::*;
use projectatlas_core::IndexCancellation;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::time::Instant;

/// Fail with the violated capability or resource invariant.
fn require(value: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if !value {
        return Err(io::Error::other(message).into());
    }
    Ok(())
}

/// Read actual bytes through the worker-local opener with fresh admission metadata.
fn read(reader: &mut SourceFileReader<'_>, path: &Path) -> FsResult<Vec<u8>> {
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    let metadata = fs::symlink_metadata(path).map_err(|source| source_error(path, source))?;
    let mut bytes = Vec::new();
    reader
        .open_file(path, &metadata, &control)?
        .0
        .read_to_end(&mut bytes)
        .map_err(|source| source_error(path, source))?;
    Ok(bytes)
}

/// Give a test its own occupancy counter, avoiding concurrent-test interference.
fn reader_with_budget<'root>(
    root: &'root SourceRootReader,
    occupied: &'static AtomicUsize,
    limit: usize,
) -> SourceFileReader<'root> {
    SourceFileReader {
        occupied,
        limit,
        ..root.reader()
    }
}

#[test]
fn sibling_cursor_preserves_native_names_hardlinks_and_atomic_file_saves()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let directory = root.join("nested λ/sources");
    fs::create_dir_all(&directory)?;
    let path = directory.join("source λ.rs");
    let hardlink = directory.join("hardlink.rs");
    fs::write(&path, b"before save")?;
    fs::hard_link(&path, &hardlink)?;
    let source_root = SourceRootReader::open(root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    require(
        read(&mut reader, &path)? == b"before save",
        "cursor changed initial bytes",
    )?;
    require(
        reader.cursor.is_some() && reader.hits == 0 && OCCUPIED.load(Ordering::Relaxed) == 2,
        "first source did not retain the complete bounded parent chain",
    )?;
    require(
        read(&mut reader, &hardlink)? == b"before save" && reader.hits == 1,
        "sibling hardlink did not use the admitted parent cursor",
    )?;
    let pending = directory.join("saved.rs");
    fs::write(&pending, b"after save")?;
    fs::rename(&pending, &path)?;
    require(
        read(&mut reader, &path)? == b"after save" && reader.hits == 2,
        "retained directory prevented ordinary atomic file save",
    )?;
    require(
        read(&mut reader, &hardlink)? == b"before save",
        "atomic save rewrote a hardlink",
    )?;
    let other = root.join("other.rs");
    fs::write(&other, b"other parent")?;
    require(
        read(&mut reader, &other)? == b"other parent"
            && reader.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "parent change retained the prior directory chain",
    )?;
    fs::rename(&directory, root.join("moved-sources"))?;
    Ok(())
}

#[test]
fn cursor_releases_handles_on_leaf_parent_control_errors_and_unwinding()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let directory = root.join("src/group");
    fs::create_dir_all(&directory)?;
    let path = directory.join("source.rs");
    fs::write(&path, b"direct source")?;
    let metadata = fs::symlink_metadata(&path)?;
    let source_root = SourceRootReader::open(root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    for rejected in [
        directory.join("absent.rs"),
        root.join("src/absent/group/source.rs"),
        root.join("../outside.rs"),
    ] {
        read(&mut reader, &path)?;
        require(
            reader.open_file(&rejected, &metadata, &control).is_err()
                && reader.cursor.is_none()
                && OCCUPIED.load(Ordering::Relaxed) == 0,
            "leaf, partial-parent or invalid-path error leaked retained handles",
        )?;
        let moved = root.join("moved");
        fs::rename(&directory, &moved)?;
        fs::rename(&moved, &directory)?;
    }
    let cancellation = IndexCancellation::new();
    cancellation.cancel();
    for stopped in [
        IndexWorkControl::new(cancellation, None),
        IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now()),
    ] {
        read(&mut reader, &path)?;
        require(
            matches!(
                reader.open_file(&path, &metadata, &stopped),
                Err(FsError::IndexWork(_))
            ) && reader.cursor.is_none()
                && OCCUPIED.load(Ordering::Relaxed) == 0,
            "cancelled or expired cursor retained directory handles",
        )?;
    }
    read(&mut reader, &path)?;
    let unwound = std::panic::catch_unwind(move || {
        let _retained = reader;
        // Real unwinding exercises RAII without printing an expected panic.
        std::panic::resume_unwind(Box::new("source cursor cleanup control"));
    });
    require(
        unwound.is_err() && OCCUPIED.load(Ordering::Relaxed) == 0,
        "unwinding leaked a directory-chain reservation",
    )?;
    fs::rename(&directory, root.join("after-unwind"))?;
    Ok(())
}

#[test]
fn cursor_depth_and_shared_capacity_fall_back_to_the_same_secure_walk() -> Result<(), Box<dyn Error>>
{
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let shallow = root.join("src/group/source.rs");
    fs::create_dir_all(root.join("src/group"))?;
    fs::write(&shallow, b"shallow source")?;
    let mut deep = root.to_path_buf();
    for _ in 0..=DIRECTORY_CURSOR_MAX_DEPTH {
        deep.push("child");
    }
    fs::create_dir_all(&deep)?;
    let deep = deep.join("source.rs");
    fs::write(&deep, b"deep source")?;
    let source_root = SourceRootReader::open(root)?;
    let mut first = reader_with_budget(&source_root, &OCCUPIED, 2);
    let mut second = reader_with_budget(&source_root, &OCCUPIED, 2);
    require(
        read(&mut first, &shallow)? == b"shallow source" && first.cursor.is_some(),
        "available directory capacity was not used",
    )?;
    require(
        read(&mut second, &shallow)? == b"shallow source"
            && second.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 2,
        "shared-capacity fallback changed bytes or exceeded occupancy",
    )?;
    require(
        read(&mut first, &deep)? == b"deep source"
            && first.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "deep-path fallback retained a partial or over-depth chain",
    )?;
    require(
        read(&mut second, &shallow)? == b"shallow source" && second.cursor.is_some(),
        "released capacity did not become available to another visitor",
    )?;
    drop(second);
    require(
        OCCUPIED.load(Ordering::Relaxed) == 0,
        "cursor drop retained occupied capacity",
    )?;
    // Exhausted/deep paths preserve the original alias refusal, rather than
    // relaxing the traversal boundary to satisfy the resource limit.
    #[cfg(any(unix, windows))]
    {
        let alias = root.join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("src"), &alias)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(root.join("src"), &alias)?;
        let mut fallback = reader_with_budget(&source_root, &OCCUPIED, 0);
        require(
            read(&mut fallback, &alias.join("group/source.rs")).is_err(),
            "uncached capacity fallback followed an indirect parent",
        )?;
    }
    Ok(())
}

#[test]
fn concurrent_source_visitors_share_one_nonblocking_directory_handle_bound()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("src/group"))?;
    let path = temp.path().join("src/group/source.rs");
    fs::write(&path, b"concurrent source")?;
    let root = SourceRootReader::open(temp.path())?;
    let ready = std::sync::Barrier::new(17);
    let release = std::sync::Barrier::new(17);
    let outcomes = std::thread::scope(|scope| {
        let mut threads = Vec::new();
        for _ in 0..16 {
            threads.push(scope.spawn(|| {
                let mut reader = reader_with_budget(&root, &OCCUPIED, 4);
                let result = read(&mut reader, &path).map(|bytes| (bytes, reader.cursor.is_some()));
                ready.wait();
                release.wait();
                result
            }));
        }
        ready.wait();
        let occupied = OCCUPIED.load(Ordering::Relaxed);
        release.wait();
        (
            occupied,
            threads
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect::<Vec<_>>(),
        )
    });
    require(
        outcomes.0 == 4 && OCCUPIED.load(Ordering::Relaxed) == 0,
        "concurrent visitors exceeded or leaked the shared handle budget",
    )?;
    let mut cached = 0;
    for result in outcomes.1 {
        let (bytes, retained) =
            result.map_err(|_panic| io::Error::other("source visitor panicked"))??;
        require(
            bytes == b"concurrent source",
            "concurrent capacity fallback changed bytes",
        )?;
        cached += usize::from(retained);
    }
    require(
        cached == 2,
        "capacity contention did not use both reuse and fallback",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn cached_source_refuses_leaf_and_each_ancestor_alias_even_for_the_same_inode()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    for replaced_depth in 0..3 {
        for target_in_root in [false, true] {
            for same_inode in [false, true] {
                let temp = tempfile::tempdir()?;
                let root = temp.path().join("repo");
                let directory = root.join("src/group");
                fs::create_dir_all(&directory)?;
                let path = directory.join("source.rs");
                fs::write(&path, b"direct bytes")?;
                let admitted = fs::symlink_metadata(&path)?;
                let redirected = if target_in_root {
                    root.join("other")
                } else {
                    temp.path().join("outside")
                };
                fs::create_dir_all(redirected.join("group"))?;
                let target = match replaced_depth {
                    1 => redirected.join("group/source.rs"),
                    _ => redirected.join("source.rs"),
                };
                if same_inode {
                    fs::hard_link(&path, &target)?;
                } else {
                    fs::write(&target, b"redirected bytes")?;
                }
                let source_root = SourceRootReader::open(&root)?;
                let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
                read(&mut reader, &path)?;
                read(&mut reader, &path)?;
                require(
                    reader.hits == 1 && OCCUPIED.load(Ordering::Relaxed) == 2,
                    "alias control did not actually exercise an established cursor hit",
                )?;
                let replaced = match replaced_depth {
                    0 => path.clone(),
                    1 => root.join("src"),
                    _ => directory.clone(),
                };
                fs::rename(&replaced, temp.path().join("saved"))?;
                let link_target = if replaced_depth == 0 {
                    &target
                } else {
                    &redirected
                };
                std::os::unix::fs::symlink(link_target, &replaced)?;
                let control = IndexWorkControl::new(IndexCancellation::new(), None);
                require(
                    reader.open_file(&path, &admitted, &control).is_err()
                        && reader.cursor.is_none()
                        && OCCUPIED.load(Ordering::Relaxed) == 0,
                    "cached source admitted an inside/outside or same-inode alias",
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn cached_source_refuses_a_different_direct_parent_with_the_same_leaf_inode()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let directory = root.join("src/group");
    fs::create_dir_all(&directory)?;
    let path = directory.join("source.rs");
    fs::write(&path, b"retained bytes")?;
    let admitted = fs::symlink_metadata(&path)?;
    let source_root = SourceRootReader::open(root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    read(&mut reader, &path)?;
    fs::rename(&directory, root.join("saved"))?;
    fs::create_dir(&directory)?;
    fs::hard_link(root.join("saved/source.rs"), &path)?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    require(
        reader.open_file(&path, &admitted, &control).is_err()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "matching leaf inode hid replacement of a cached direct parent",
    )?;
    require(
        read(&mut reader, &path)? == b"retained bytes",
        "fresh cursor could not admit the ordinary replacement parent",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn cached_source_refuses_walker_metadata_from_a_replaced_root_and_accepts_restore()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    let path = root.join("src/group/source.rs");
    fs::create_dir_all(root.join("src/group"))?;
    fs::write(&path, b"selected root source")?;
    let source_root = SourceRootReader::open(&root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    read(&mut reader, &path)?;
    read(&mut reader, &path)?;
    require(
        reader.hits == 1 && OCCUPIED.load(Ordering::Relaxed) == 2,
        "root replacement control did not establish a hot sibling cursor",
    )?;
    let selected = temp.path().join("selected-root");
    fs::rename(&root, &selected)?;
    fs::create_dir_all(root.join("src/group"))?;
    fs::write(&path, b"replacement root source")?;
    let replacement_metadata = fs::symlink_metadata(&path)?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    require(
        reader
            .open_file(&path, &replacement_metadata, &control)
            .is_err()
            && reader.hits == 2
            && reader.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "hot cursor paired replacement-root metadata with selected-root bytes",
    )?;
    fs::rename(&root, temp.path().join("replacement-root"))?;
    fs::rename(&selected, &root)?;
    require(
        read(&mut reader, &path)? == b"selected root source",
        "fresh cursor did not recover after restoring the selected root",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn cached_source_refuses_fifo_after_an_established_sibling_hit() -> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    fs::create_dir_all(root.join("src/group"))?;
    let path = root.join("src/group/source.rs");
    fs::write(&path, b"regular source")?;
    let checked = path.clone();
    crate::regular_file::verify_fifo_refusal(&path, move || {
        let result = (|| -> FsResult<bool> {
            let source_root = SourceRootReader::open(&root)?;
            let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
            read(&mut reader, &checked)?;
            read(&mut reader, &checked)?;
            let metadata =
                fs::symlink_metadata(&checked).map_err(|source| source_error(&checked, source))?;
            fs::remove_file(&checked).map_err(|source| source_error(&checked, source))?;
            nix::unistd::mkfifo(
                &checked,
                nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
            )
            .map_err(|source| source_error(&checked, source.into()))?;
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            Ok(reader.hits == 1
                && matches!(
                    reader.open_file(&checked, &metadata, &control),
                    Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput
                )
                && reader.hits == 2
                && reader.cursor.is_none()
                && OCCUPIED.load(Ordering::Relaxed) == 0)
        })();
        matches!(result, Ok(true))
    })
}

#[cfg(windows)]
#[test]
fn cached_windows_leaf_refuses_inside_outside_and_same_object_symlinks()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    for inside in [false, true] {
        for same_object in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("repo");
            let directory = root.join("src/group");
            fs::create_dir_all(&directory)?;
            let path = directory.join("source.rs");
            fs::write(&path, b"direct bytes")?;
            let redirected = if inside {
                root.join("other.rs")
            } else {
                temp.path().join("outside.rs")
            };
            if same_object {
                fs::hard_link(&path, &redirected)?;
            } else {
                fs::write(&redirected, b"redirected bytes")?;
            }
            let metadata = fs::symlink_metadata(&path)?;
            let source_root = SourceRootReader::open(&root)?;
            let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
            read(&mut reader, &path)?;
            read(&mut reader, &path)?;
            require(
                reader.hits == 1,
                "Windows leaf control did not establish cursor reuse",
            )?;
            fs::remove_file(&path)?;
            std::os::windows::fs::symlink_file(&redirected, &path)?;
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            require(
                reader.open_file(&path, &metadata, &control).is_err()
                    && reader.hits == 2
                    && reader.cursor.is_none()
                    && OCCUPIED.load(Ordering::Relaxed) == 0,
                "cached Windows source followed a leaf symlink",
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn cached_windows_ancestors_pin_names_only_until_the_cursor_is_released()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let directory = root.join("src/group");
    fs::create_dir_all(&directory)?;
    let path = directory.join("source.rs");
    fs::write(&path, b"pinned ancestors")?;
    let source_root = SourceRootReader::open(root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    read(&mut reader, &path)?;
    for parent in [root.join("src"), directory] {
        require(
            fs::rename(&parent, root.join("moved")).is_err(),
            "cached Windows ancestor did not retain no-delete sharing",
        )?;
    }
    read(&mut reader, &path)?;
    require(
        reader.hits == 1,
        "Windows rename control did not reuse a live cursor",
    )?;
    drop(reader);
    require(
        OCCUPIED.load(Ordering::Relaxed) == 0,
        "idle reader retained directory handles",
    )?;
    fs::rename(root.join("src"), root.join("after-discovery"))?;
    Ok(())
}

#[cfg(windows)]
#[test]
fn cached_windows_source_refuses_an_empty_parent_retagged_as_a_junction()
-> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let directory = root.join("src/group");
    let redirected = root.join("redirected");
    fs::create_dir_all(&directory)?;
    fs::create_dir(&redirected)?;
    let path = directory.join("source.rs");
    fs::write(&path, b"retained source object")?;
    fs::hard_link(&path, redirected.join("source.rs"))?;
    let admitted = fs::symlink_metadata(&path)?;
    let source_root = SourceRootReader::open(root)?;
    let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
    read(&mut reader, &path)?;
    read(&mut reader, &path)?;
    require(
        reader.hits == 1 && reader.cursor.is_some(),
        "junction transition did not begin with a live sibling cursor",
    )?;
    fs::remove_file(&path)?;
    windows_junction::set_existing_directory(&directory, &redirected)?;
    require(
        fs::symlink_metadata(&directory)?.file_type().is_symlink()
            && fs::read(&path)? == b"retained source object",
        "native fixture did not retag the pinned empty directory in place",
    )?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    require(
        reader.open_file(&path, &admitted, &control).is_err()
            && reader.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "cached Windows source followed an in-place junction to the same leaf object",
    )?;
    fs::remove_dir(&directory)?;
    fs::create_dir(&directory)?;
    fs::write(&path, b"restored direct source")?;
    require(
        read(&mut reader, &path)? == b"restored direct source",
        "restored ordinary directory lost source admission",
    )?;
    Ok(())
}

/// The isolated native fixture alone calls the Windows control-code API.
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_junction {
    use std::fs;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;

    /// Set a junction on the existing empty object; no rename or recreation.
    pub(super) fn set_existing_directory(directory: &Path, target: &Path) -> io::Result<()> {
        let target = fs::canonicalize(target)?;
        let mut substitute: Vec<u16> = target.as_os_str().encode_wide().collect();
        let verbatim: Vec<_> = r"\\?\".encode_utf16().collect();
        if !substitute.starts_with(&verbatim) {
            return Err(io::Error::other(
                "junction target lacks native verbatim prefix",
            ));
        }
        // Canonical Win32 \\?\ becomes the native \??\ prefix, including UNC.
        substitute[1] = u16::from(b'?');
        let name_bytes = u16::try_from(substitute.len() * 2).map_err(io::Error::other)?;
        let data_length = name_bytes
            .checked_add(12)
            .ok_or_else(|| io::Error::other("junction target exceeds native buffer"))?;
        // SDK REPARSE_DATA_BUFFER: 8-byte header, four u16 mount-point fields,
        // substitute name + NUL, empty print name + NUL. All lengths are bytes.
        let mut buffer = Vec::with_capacity(usize::from(data_length) + 8);
        buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        for value in [data_length, 0, 0, name_bytes, name_bytes + 2, 0] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        for value in substitute.into_iter().chain([0, 0]) {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        let input_length = u32::try_from(buffer.len()).map_err(io::Error::other)?;
        let file = fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES | FILE_WRITE_DATA)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)?;
        let mut returned = 0;
        // SAFETY: the live native directory handle and initialized contiguous
        // input remain owned through this synchronous call. Its exact checked
        // length is supplied; there is no output or overlapped operation.
        let success = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                input_length,
                std::ptr::null_mut(),
                0,
                &raw mut returned,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
