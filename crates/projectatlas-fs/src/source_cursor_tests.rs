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
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        resolver: SourceResolver::Components,
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
fn cursor_falls_back_under_process_descriptor_pressure() -> Result<(), Box<dyn Error>> {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // Changing a descriptor limit is safe only in this exact isolated child.
    const CHILD: &str = "PROJECTATLAS_SOURCE_CURSOR_CAPACITY_CHILD";
    const SELECTOR: &str =
        "source_reader::cursor_tests::cursor_falls_back_under_process_descriptor_pressure";
    if std::env::var_os(CHILD).is_none() {
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--exact", SELECTOR, "--test-threads=1", "--nocapture"])
            .env(CHILD, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let started = Instant::now();
        let stop_reason = loop {
            match child.try_wait() {
                Ok(Some(_)) => break None,
                Ok(None) => {}
                Err(error) => {
                    break Some(format!(
                        "waiting for descriptor-pressure child failed: {error}"
                    ));
                }
            }
            if started.elapsed() > Duration::from_secs(15) {
                break Some("isolated descriptor-pressure test exceeded its bound".to_owned());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if let Some(reason) = &stop_reason {
            let kill_error = child.kill().err();
            let stopping = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if stopping.elapsed() < Duration::from_secs(5) => {}
                    result => return Err(io::Error::other(format!(
                        "{reason}; child exit unconfirmed: {result:?}; kill error: {kill_error:?}",
                    ))
                    .into()),
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(io::Error::other(format!(
                "{reason}; child reaped; kill error: {kill_error:?}"
            ))
            .into());
        }
        // Collect pipes only after the exact child has observably exited.
        let output = child.wait_with_output()?;
        require(
            output.status.success(),
            &format!(
                "isolated descriptor-pressure control failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
        )?;
        return Ok(());
    }
    let original = getrlimit(Resource::RLIMIT_NOFILE)?;
    require(
        original.1 >= 64,
        "native descriptor-pressure fixture requires a hard limit of 64",
    )?;
    setrlimit(Resource::RLIMIT_NOFILE, 64, original.1)?;
    let result = verify_descriptor_pressure_fallback();
    setrlimit(Resource::RLIMIT_NOFILE, original.0, original.1)?;
    result
}

/// Fill actual process slots until the kernel reports the expected capacity error.
#[cfg(unix)]
fn occupy_descriptors(held: &mut Vec<File>) -> io::Result<()> {
    loop {
        match File::open("/dev/null") {
            Ok(file) => held.push(file),
            Err(error) if error.raw_os_error() == Some(nix::libc::EMFILE) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

/// Prove low-handle construction and hot-leaf fallback using real native failures.
#[cfg(unix)]
fn verify_descriptor_pressure_fallback() -> Result<(), Box<dyn Error>> {
    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let mut directory = root.to_path_buf();
    for _ in 0..DIRECTORY_CURSOR_MAX_DEPTH {
        directory.push("child");
    }
    fs::create_dir_all(&directory)?;
    let path = directory.join("source.rs");
    fs::write(&path, b"source under descriptor pressure")?;
    let linked = root.join("same-object.rs");
    fs::hard_link(&path, &linked)?;
    let admitted = fs::symlink_metadata(&path)?;
    let source_root = SourceRootReader::open(root)?;
    require(
        source_root.directory_handle_limit == 8,
        "cursor did not derive its real capacity from the low soft limit",
    )?;
    let mut reader =
        reader_with_budget(&source_root, &OCCUPIED, source_root.directory_handle_limit);
    let mut held = Vec::new();
    occupy_descriptors(&mut held)?;
    require(
        held.len() >= 16,
        "descriptor fixture cannot release enough construction slots",
    )?;
    held.truncate(held.len() - 4);
    require(
        read(&mut source_root.uncached(), &path)? == b"source under descriptor pressure",
        "low-handle secure walk could not read the capacity-control source",
    )?;
    require(
        read(&mut reader, &path).is_ok_and(|bytes| bytes == b"source under descriptor pressure")
            && reader.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "cursor construction exhaustion did not fall back to the successful secure walk",
    )?;

    // Enough room to establish a hot cursor, then another owner fills the rest.
    held.truncate(held.len() - 12);
    read(&mut reader, &path)?;
    require(
        reader.cursor.is_some() && OCCUPIED.load(Ordering::Relaxed) == 8,
        "leaf-exhaustion control did not retain its complete parent chain",
    )?;
    occupy_descriptors(&mut held)?;
    require(
        read(&mut reader, &path).is_ok_and(|bytes| bytes == b"source under descriptor pressure")
            && reader.hits == 1
            && reader.cursor.is_none()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "hot cursor handle exhaustion did not release ancestors before secure fallback",
    )?;

    // Hot fallback freed eight ancestor handles. Refill, then leave only four
    // slots so fixture writes work while the eight-directory cursor cannot fit.
    occupy_descriptors(&mut held)?;
    held.truncate(held.len() - 4);
    // Fallback must still reject a changed file object paired with old metadata.
    fs::remove_file(&path)?;
    fs::write(&path, b"replacement under pressure")?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    require(
        reader.open_file(&path, &admitted, &control).is_err()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "descriptor-pressure fallback paired replacement bytes with old file identity",
    )?;
    require(
        read(&mut reader, &path)? == b"replacement under pressure",
        "fresh metadata fallback refused a legitimate replacement under pressure",
    )?;
    // Use the ORIGINAL regular metadata: following the alias would open exactly
    // that same inode, so the leaf-identity check cannot hide a no-follow failure.
    fs::remove_file(&path)?;
    std::os::unix::fs::symlink(&linked, &path)?;
    require(
        reader.open_file(&path, &admitted, &control).is_err()
            && OCCUPIED.load(Ordering::Relaxed) == 0,
        "descriptor-pressure fallback followed a same-object source alias",
    )?;
    drop(held);
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
fn cached_source_refuses_an_alias_to_its_retained_ancestor() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::MetadataExt;

    static OCCUPIED: AtomicUsize = AtomicUsize::new(0);
    for replace_outer in [false, true] {
        for target_in_root in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("repo");
            let directory = root.join("src/group");
            fs::create_dir_all(&directory)?;
            let path = directory.join("source.rs");
            fs::write(&path, b"retained source")?;
            let admitted = fs::symlink_metadata(&path)?;
            let source_root = SourceRootReader::open(&root)?;
            let mut reader = reader_with_budget(&source_root, &OCCUPIED, 4);
            read(&mut reader, &path)?;
            read(&mut reader, &path)?;
            require(
                reader.hits == 1,
                "alias control did not establish cursor reuse",
            )?;
            let replaced = if replace_outer {
                root.join("src")
            } else {
                directory
            };
            let retained = if target_in_root {
                replaced
                    .parent()
                    .ok_or("ancestor has no parent")?
                    .join("retained")
            } else {
                temp.path().join("retained")
            };
            fs::rename(&replaced, &retained)?;
            // The relative sibling stays beneath the held parent capability,
            // so general sandbox confinement cannot hide a no-follow failure.
            let link_target = if target_in_root {
                Path::new("retained")
            } else {
                &retained
            };
            std::os::unix::fs::symlink(link_target, &replaced)?;
            let direct = fs::metadata(&retained)?;
            let followed = fs::metadata(&replaced)?;
            require(
                (direct.dev(), direct.ino()) == (followed.dev(), followed.ino())
                    && fs::read(&path)? == b"retained source",
                "alias control did not retain the original directory and source objects",
            )?;
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            require(
                reader.open_file(&path, &admitted, &control).is_err()
                    && reader.cursor.is_none()
                    && OCCUPIED.load(Ordering::Relaxed) == 0,
                "matching directory identity hid a cached ancestor alias",
            )?;
            fs::remove_file(&replaced)?;
            fs::rename(&retained, &replaced)?;
            require(
                read(&mut reader, &path)? == b"retained source",
                "restored ancestor did not recover",
            )?;
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
fn windows_cursor_capacity_retry_keeps_non_capacity_failures_strict() -> Result<(), Box<dyn Error>>
{
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_NO_SYSTEM_RESOURCES,
        ERROR_NOT_ENOUGH_MEMORY, ERROR_OPERATION_ABORTED, ERROR_TOO_MANY_OPEN_FILES,
    };
    for (code, capacity) in [
        (ERROR_TOO_MANY_OPEN_FILES, true),
        (ERROR_NO_SYSTEM_RESOURCES, true),
        (ERROR_ACCESS_DENIED, false),
        (ERROR_FILE_NOT_FOUND, false),
        (ERROR_NOT_ENOUGH_MEMORY, false),
        (ERROR_OPERATION_ABORTED, false),
    ] {
        let error = source_error(
            Path::new("source.rs"),
            io::Error::from_raw_os_error(code as i32),
        );
        require(
            directory_handle_capacity_error(&error) == capacity,
            "Windows source retry crossed its native handle-capacity boundary",
        )?;
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
