//! Regular-file admission for readers that intentionally follow selected aliases.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Open a regular file for reading, following an explicitly selected symlink.
///
/// Unix opens use nonblocking mode so a FIFO substituted after an earlier path
/// inspection cannot wait for a writer. Descriptor metadata, rather than a path
/// precheck, rejects streams, directories and devices before reading. Callers
/// retain responsibility for root selection, alias continuity and read bounds.
/// This does not promise a hard deadline for ordinary filesystem I/O.
///
/// # Errors
/// Returns filesystem errors or [`io::ErrorKind::InvalidInput`] when the opened
/// object is not a regular file.
pub fn open_regular_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "selected input is not a regular file",
        ));
    }
    Ok(file)
}

/// Exercise a FIFO refusal with bounded observation and release a blocking mutant.
///
/// # Errors
/// Returns an error when the reader blocks, panics or admits the FIFO.
#[cfg(all(test, unix))]
pub(crate) fn verify_fifo_refusal(
    path: &Path,
    read: impl FnOnce() -> bool + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    let (send, receive) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _sent = send.send(read());
    });
    let result = receive.recv_timeout(Duration::from_secs(2));
    if result.is_err() {
        // Closing the temporary writer also ends any accidentally admitted read.
        let writer = OpenOptions::new()
            .write(true)
            .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
            .open(path);
        drop(writer);
    }
    worker.join().map_err(|_panic| "FIFO reader panicked")?;
    if !result? {
        return Err("substituted FIFO was admitted as a regular input".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open_regular_file;
    use std::error::Error;
    use std::fs;
    use std::io::{self, Read};

    #[cfg(unix)]
    #[test]
    fn regular_reader_refuses_fifo_without_a_writer() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let selected = temp.path().join("selected");
        nix::unistd::mkfifo(
            &selected,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )?;
        let checked = selected.clone();
        super::verify_fifo_refusal(
            &selected,
            move || matches!(open_regular_file(&checked), Err(error) if error.kind() == io::ErrorKind::InvalidInput),
        )
    }

    #[test]
    fn regular_reader_preserves_bytes_aliases_and_absence() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let selected = temp.path().join("selected");
        fs::write(&selected, b"saved bytes\n")?;
        let mut bytes = Vec::new();
        open_regular_file(&selected)?.read_to_end(&mut bytes)?;
        if bytes != b"saved bytes\n" {
            return Err("regular file reader changed saved bytes".into());
        }
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(&selected, &alias)?;
            let mut aliased = Vec::new();
            open_regular_file(&alias)?.read_to_end(&mut aliased)?;
            if aliased != bytes {
                return Err("selected regular file alias was not followed".into());
            }
        }
        if !matches!(open_regular_file(&temp.path().join("absent")), Err(error) if error.kind() == io::ErrorKind::NotFound)
            || open_regular_file(temp.path()).is_ok()
        {
            return Err("regular file reader changed absence or directory admission".into());
        }
        Ok(())
    }
}
