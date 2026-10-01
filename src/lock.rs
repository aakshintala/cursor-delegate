//! One exclusive flock per canonical working directory, held by a read-write job's supervisor.
//! The kernel drops it when that process exits, so a crash needs no cleanup.

use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub struct WriteLock {
    file: std::fs::File,
}

impl WriteLock {
    pub fn fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
}

pub enum AcquireError {
    Busy { holder: String },
    Io(std::io::Error),
}

/// Canonical path, falling back to the given string when the directory cannot be resolved.
fn cwd_key(cwd: &str) -> String {
    std::fs::canonicalize(cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cwd.to_string())
}

fn lock_path(cwd: &str) -> PathBuf {
    let mut hasher = std::hash::DefaultHasher::new();
    cwd_key(cwd).hash(&mut hasher);
    std::env::temp_dir()
        .join("delegate-locks")
        .join(format!("{:016x}.lock", hasher.finish()))
}

fn would_block(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
        || matches!(
            err.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        )
}

/// `flock(LOCK_EX|LOCK_NB)`. On success the file contains `job_id` and `FD_CLOEXEC` is clear
/// so the supervisor can inherit this fd. On `EWOULDBLOCK` the error carries the holder id.
pub fn try_acquire(cwd: &str, job_id: &str) -> Result<WriteLock, AcquireError> {
    let path = lock_path(cwd);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(AcquireError::Io)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(AcquireError::Io)?;
    let fd = file.as_raw_fd();
    // Safety: `fd` is the open lock file. LOCK_NB returns immediately when another process holds it.
    let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if would_block(&err) {
            let mut holder = String::new();
            let _ = file.take(256).read_to_string(&mut holder);
            return Err(AcquireError::Busy {
                holder: holder.trim().to_string(),
            });
        }
        return Err(AcquireError::Io(err));
    }
    file.set_len(0).map_err(AcquireError::Io)?;
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(AcquireError::Io)?;
    file.write_all(job_id.as_bytes())
        .map_err(AcquireError::Io)?;
    clear_cloexec(fd).map_err(AcquireError::Io)?;
    Ok(WriteLock { file })
}

fn clear_cloexec(fd: i32) -> std::io::Result<()> {
    // Safety: `fd` is open. FD_CLOEXEC is per descriptor, so clearing it only affects this one.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Take ownership of a lock fd inherited across `exec`, and set `FD_CLOEXEC` so the agent
/// and the gate do not keep the lock after this process dies.
///
/// # Safety
///
/// `fd` must be an open file descriptor inherited from the parent `run` process. It is
/// not closed or owned by anything else in this process.
pub unsafe fn adopt(fd: i32) -> OwnedFd {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags >= 0 {
        unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    }
    unsafe { OwnedFd::from_raw_fd(fd) }
}
