//! Linux all-component source resolution and secure-backend compatibility proof.

use super::*;
use projectatlas_core::IndexCancellation;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::time::Instant;

/// Read admitted bytes through the actual automatically selected worker backend.
fn read(reader: &mut SourceFileReader<'_>, path: &Path) -> FsResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|source| source_error(path, source))?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    let mut bytes = Vec::new();
    reader
        .open_file(path, &metadata, &control)?
        .0
        .read_to_end(&mut bytes)
        .map_err(|source| source_error(path, source))?;
    Ok(bytes)
}

#[test]
fn kernel_source_resolution_does_not_retain_a_userspace_ancestor_chain()
-> Result<(), Box<dyn Error>> {
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};

    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("src/group"))?;
    let path = temp.path().join("src/group/source.rs");
    fs::write(&path, b"native source")?;
    let root = SourceRootReader::open(temp.path())?;
    // The product must also work on older kernels or syscall-restricted hosts.
    // Those hosts exercise the existing component backend, not this native proof.
    match openat2(
        &root.directory,
        Path::new("."),
        OpenHow::new()
            .flags(OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC)
            .resolve(ResolveFlag::RESOLVE_BENEATH | ResolveFlag::RESOLVE_NO_SYMLINKS),
    ) {
        Ok(file) => drop(file),
        Err(Errno::ENOSYS | Errno::EPERM | Errno::EACCES | Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    for limit in [0, DIRECTORY_HANDLE_CEILING] {
        let mut reader = SourceFileReader {
            limit,
            ..root.reader()
        };
        if read(&mut reader, &path)? != b"native source"
            || reader.cursor.is_some()
            || reader.resolver != SourceResolver::Kernel
        {
            return Err(
                "automatic kernel source resolution retained a userspace chain or used its quota"
                    .into(),
            );
        }
    }
    Ok(())
}

#[test]
fn kernel_availability_fallback_is_local_and_keeps_real_path_errors_strict()
-> Result<(), Box<dyn Error>> {
    use nix::errno::Errno;
    use nix::fcntl::OFlag;
    use nix::unistd::dup;

    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("src/group"))?;
    let relative = Path::new("src/group/source.rs");
    let path = temp.path().join(relative);
    fs::write(&path, b"fallback source")?;
    let metadata = fs::symlink_metadata(&path)?;
    let root = SourceRootReader::open(temp.path())?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    for unsupported in [Errno::ENOSYS, Errno::EPERM, Errno::EACCES, Errno::ENOENT] {
        let mut reader = root.reader();
        let mut probes = 0;
        let result =
            reader.try_kernel_open(relative, &path, &metadata, &control, |_, name, _| {
                probes += 1;
                if name != Path::new(".") {
                    return Err(Errno::EINVAL);
                }
                Err(unsupported)
            })?;
        if result.is_some() || probes != 1 || reader.resolver != SourceResolver::Components {
            return Err("unavailable syscall did not select the local component fallback".into());
        }
        reader.try_kernel_open(relative, &path, &metadata, &control, |_, _, _| {
            probes += 1;
            Err(Errno::EIO)
        })?;
        if probes != 1 || read(&mut reader, &path)? != b"fallback source" || reader.cursor.is_none()
        {
            return Err(
                "fallback reprobed the syscall or failed to open through the real cursor".into(),
            );
        }
        let alias = temp.path().join(format!("alias-{unsupported}"));
        std::os::unix::fs::symlink(temp.path().join("src"), &alias)?;
        if reader
            .open_file(&alias.join("group/source.rs"), &metadata, &control)
            .is_ok()
        {
            return Err("availability fallback followed a descendant alias".into());
        }
    }
    for refused in [
        Errno::EPERM,
        Errno::EACCES,
        Errno::EINVAL,
        Errno::EAGAIN,
        Errno::ELOOP,
        Errno::EXDEV,
        Errno::EIO,
        Errno::ENOENT,
    ] {
        let mut reader = root.reader();
        let mut calls = 0;
        let result = reader.try_kernel_open(
            relative,
            &path,
            &metadata,
            &control,
            |directory, name, flags| {
                calls += 1;
                if name == Path::new(".") {
                    return dup(directory);
                }
                if !flags.contains(
                    OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_NOCTTY | OFlag::O_CLOEXEC,
                ) || flags.intersects(OFlag::O_WRONLY | OFlag::O_RDWR)
                {
                    return Err(Errno::E2BIG);
                }
                Err(refused)
            },
        );
        if !matches!(result, Err(FsError::Io { source, .. }) if source.raw_os_error() == Some(refused as i32))
            || calls != 2
            || reader.resolver != SourceResolver::Kernel
        {
            return Err(
                "a real source error was mistaken for optional backend unavailability".into(),
            );
        }
    }
    let mut reader = root.reader();
    reader.try_kernel_open(
        relative,
        &path,
        &metadata,
        &control,
        |directory, name, _| {
            if name == Path::new(".") {
                dup(directory)
            } else {
                Err(Errno::ENOSYS)
            }
        },
    )?;
    if reader.resolver != SourceResolver::Components
        || read(&mut reader, &path)? != b"fallback source"
    {
        return Err("late syscall unavailability lost the secure fallback".into());
    }
    for refused in [Errno::EINVAL, Errno::EAGAIN, Errno::EIO] {
        let mut reader = root.reader();
        if !matches!(reader.try_kernel_open(relative, &path, &metadata, &control, |_, _, _| Err(refused)),
            Err(FsError::Io { source, .. }) if source.raw_os_error() == Some(refused as i32))
            || reader.resolver != SourceResolver::Unchecked
        {
            return Err("unexpected probe error was not retained as a refusal".into());
        }
    }
    Ok(())
}

#[test]
fn kernel_source_keeps_flags_native_names_root_aliases_hardlinks_and_saves()
-> Result<(), Box<dyn Error>> {
    use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let temp = tempfile::tempdir()?;
    let directory = temp.path().join("repo/src λ/group");
    fs::create_dir_all(&directory)?;
    let native = OsStr::from_bytes(b"source-\xff.rs");
    let path = directory.join(native);
    fs::write(&path, b"before save")?;
    fs::hard_link(&path, directory.join("hardlink.rs"))?;
    let alias = temp.path().join("selected λ");
    std::os::unix::fs::symlink(temp.path().join("repo"), &alias)?;
    let root = SourceRootReader::open(&alias)?;
    let path = alias.join("src λ/group").join(native);
    let mut reader = root.reader();
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    let (mut retained, _) = reader.open_file(&path, &fs::symlink_metadata(&path)?, &control)?;
    let flags = OFlag::from_bits_truncate(fcntl(&retained, FcntlArg::F_GETFL)?);
    let descriptor = FdFlag::from_bits_truncate(fcntl(&retained, FcntlArg::F_GETFD)?);
    if !flags.contains(OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW)
        || flags.intersects(OFlag::O_WRONLY | OFlag::O_RDWR)
        || !descriptor.contains(FdFlag::FD_CLOEXEC)
    {
        return Err("kernel source changed read-only, nonblocking or descriptor flags".into());
    }
    let pending = directory.join("pending.rs");
    fs::write(&pending, b"after save")?;
    fs::rename(&pending, &path)?;
    let mut bytes = Vec::new();
    retained.read_to_end(&mut bytes)?;
    if bytes != b"before save"
        || read(&mut reader, &path)? != b"after save"
        || read(&mut reader, &directory.join("hardlink.rs")).is_ok()
    {
        // The public selected spelling is intentional; a second root spelling
        // must not bypass strip_prefix merely because it names the same object.
        return Err("kernel source changed atomic saves or selected-root spelling".into());
    }
    if read(&mut reader, &alias.join("src λ/group/hardlink.rs"))? != b"before save" {
        return Err("kernel source changed ordinary hardlink bytes".into());
    }
    Ok(())
}

#[test]
fn kernel_source_refuses_leaf_and_every_ancestor_alias_with_matching_admission()
-> Result<(), Box<dyn Error>> {
    for depth in 0..3 {
        for outside in [false, true] {
            let temp = tempfile::tempdir()?;
            let root_path = temp.path().join("repo");
            let path = root_path.join("src/group/source.rs");
            fs::create_dir_all(root_path.join("src/group"))?;
            fs::write(&path, b"same source object")?;
            let root = SourceRootReader::open(&root_path)?;
            let mut reader = root.reader();
            read(&mut reader, &path)?;
            let replaced = [
                root_path.join("src"),
                root_path.join("src/group"),
                path.clone(),
            ][depth]
                .clone();
            let target = if outside {
                temp.path().join("moved")
            } else {
                root_path.join("moved")
            };
            fs::rename(&replaced, &target)?;
            let alias_target = if outside {
                target.clone()
            } else {
                PathBuf::from(["moved", "../moved", "../../moved"][depth])
            };
            std::os::unix::fs::symlink(alias_target, &replaced)?;
            // Follow only for admission: the inode is exactly the directly
            // opened source's inode, so identity checks cannot hide a lost flag.
            let metadata = fs::metadata(&path)?;
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            if reader.open_file(&path, &metadata, &control).is_ok() || reader.cursor.is_some() {
                return Err(
                    "kernel source followed an ancestor or leaf alias to the same object".into(),
                );
            }
            fs::remove_file(&replaced)?;
            fs::rename(&target, &replaced)?;
            if read(&mut reader, &path)? != b"same source object" {
                return Err("kernel source did not recover after restoring the direct path".into());
            }
        }
    }
    Ok(())
}

#[test]
fn kernel_source_binds_walker_metadata_and_preserves_control_and_nonregular_refusals()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let selected = temp.path().join("repo");
    let path = selected.join("src/group/source.rs");
    fs::create_dir_all(selected.join("src/group"))?;
    fs::write(&path, b"selected bytes")?;
    let root = SourceRootReader::open(&selected)?;
    let mut reader = root.reader();
    read(&mut reader, &path)?;
    let held = temp.path().join("held");
    fs::rename(&selected, &held)?;
    fs::create_dir_all(selected.join("src/group"))?;
    fs::write(&path, b"replacement bytes")?;
    let replacement = fs::metadata(&path)?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    if reader.open_file(&path, &replacement, &control).is_ok() {
        return Err(
            "kernel source paired replacement-root metadata with selected-root bytes".into(),
        );
    }
    fs::rename(&selected, temp.path().join("replacement"))?;
    fs::rename(&held, &selected)?;
    if read(&mut reader, &path)? != b"selected bytes" {
        return Err("kernel source did not recover the restored selected root".into());
    }
    let cancellation = IndexCancellation::new();
    cancellation.cancel();
    let metadata = fs::metadata(&path)?;
    for stopped in [
        IndexWorkControl::new(cancellation, None),
        IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now()),
    ] {
        if !matches!(
            reader.open_file(&path, &metadata, &stopped),
            Err(FsError::IndexWork(_))
        ) {
            return Err("kernel source ignored stopped work".into());
        }
    }
    if reader
        .open_file(&selected.join("src/group"), &metadata, &control)
        .is_ok()
    {
        return Err("kernel source admitted a directory".into());
    }
    let checked = path.clone();
    crate::regular_file::verify_fifo_refusal(&path, move || {
        let result = (|| -> FsResult<bool> {
            let root = SourceRootReader::open(&selected)?;
            let metadata =
                fs::symlink_metadata(&checked).map_err(|source| source_error(&checked, source))?;
            fs::remove_file(&checked).map_err(|source| source_error(&checked, source))?;
            nix::unistd::mkfifo(
                &checked,
                nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
            )
            .map_err(|source| source_error(&checked, source.into()))?;
            let control = IndexWorkControl::new(IndexCancellation::new(), None);
            Ok(
                matches!(root.reader().open_file(&checked, &metadata, &control),
                Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput),
            )
        })();
        matches!(result, Ok(true))
    })?;
    Ok(())
}

#[test]
fn kernel_selection_checks_paths_and_control_and_leaves_single_targets_uncached()
-> Result<(), Box<dyn Error>> {
    use nix::errno::Errno;
    use nix::unistd::dup;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("src/group"))?;
    let relative = Path::new("src/group/source.rs");
    let path = temp.path().join(relative);
    fs::write(&path, b"direct source")?;
    let metadata = fs::metadata(&path)?;
    let root = SourceRootReader::open(temp.path())?;
    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    for invalid in [
        Path::new(""),
        Path::new("."),
        Path::new("../source.rs"),
        Path::new("src/../source.rs"),
        &path,
    ] {
        let mut reader = root.reader();
        let mut calls = 0;
        let result = reader.try_kernel_open(invalid, &path, &metadata, &control, |_, _, _| {
            calls += 1;
            Err(Errno::EIO)
        });
        if !matches!(result, Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput)
            || calls != 0
            || reader.resolver != SourceResolver::Unchecked
        {
            return Err("invalid source selector reached the optional syscall".into());
        }
    }
    let mut reader = root.uncached();
    let mut calls = 0;
    if reader
        .try_kernel_open(relative, &path, &metadata, &control, |_, _, _| {
            calls += 1;
            Err(Errno::EIO)
        })?
        .is_some()
        || calls != 0
        || read(&mut reader, &path)? != b"direct source"
        || reader.cursor.is_some()
        || reader.resolver != SourceResolver::Components
    {
        return Err("single-target source unexpectedly selected a cached or kernel backend".into());
    }
    let cancellation = IndexCancellation::new();
    let control = IndexWorkControl::new(cancellation.clone(), None);
    let mut reader = root.reader();
    let mut calls = 0;
    let result = reader.try_kernel_open(
        relative,
        &path,
        &metadata,
        &control,
        |directory, name, _| {
            calls += 1;
            if name == Path::new(".") {
                cancellation.cancel();
                dup(directory)
            } else {
                Err(Errno::EIO)
            }
        },
    );
    if !matches!(result, Err(FsError::IndexWork(_))) || calls != 1 || reader.cursor.is_some() {
        return Err("cancelled availability probe continued into a source open".into());
    }
    let source = File::open(&path)?;
    let cancellation = IndexCancellation::new();
    let control = IndexWorkControl::new(cancellation.clone(), None);
    let mut reader = root.reader();
    let mut returned_descriptor = None;
    let result = reader.try_kernel_open(
        relative,
        &path,
        &metadata,
        &control,
        |directory, name, _| {
            if name == Path::new(".") {
                return dup(directory);
            }
            let opened = dup(&source)?;
            returned_descriptor = Some(opened.as_raw_fd());
            cancellation.cancel();
            Ok(opened)
        },
    );
    if !matches!(result, Err(FsError::IndexWork(_))) {
        return Err("source opened during cancellation escaped the post-open check".into());
    }
    let descriptor = returned_descriptor.ok_or("source opener was not reached")?;
    match fs::metadata(format!("/proc/self/fd/{descriptor}")) {
        // Parallel tests may already reuse the descriptor number for another
        // object; none can name this test's private, still-live source inode.
        Ok(current) if current.dev() != metadata.dev() || current.ino() != metadata.ino() => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(_) => return Err("cancelled source open leaked its returned descriptor".into()),
    }
    Ok(())
}

#[test]
fn kernel_resolution_accepts_a_fresh_direct_parent_instead_of_reusing_old_handles()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let path = src.join("group/source.rs");
    fs::create_dir_all(src.join("group"))?;
    fs::write(&path, b"original source")?;
    let root = SourceRootReader::open(temp.path())?;
    let mut reader = root.reader();
    read(&mut reader, &path)?;
    if reader.resolver != SourceResolver::Kernel {
        // The separately covered component cursor deliberately refuses stale
        // retained identities; this positive workflow is for fresh resolution.
        return Ok(());
    }
    let retired = temp.path().join("retired");
    fs::rename(&src, &retired)?;
    fs::create_dir_all(src.join("group"))?;
    fs::hard_link(retired.join("group/source.rs"), &path)?;
    if read(&mut reader, &path)? != b"original source" || reader.cursor.is_some() {
        return Err("fresh kernel resolution was confused by the previous parent object".into());
    }
    fs::remove_file(&path)?;
    fs::write(&path, b"current source")?;
    if read(&mut reader, &path)? != b"current source" {
        return Err("fresh kernel resolution lost ordinary replacement source bytes".into());
    }
    Ok(())
}
