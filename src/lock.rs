//! Single-instance lock mechanism based on kernel-managed `flock(2)` on Unix.
//!
//! Automatically releases locks upon process exit or termination (including crashes or SIGKILL).

use std::fs::File;
use std::path::PathBuf;

/// A process-level lock that ensures only one instance of a component runs concurrently.
#[derive(Debug)]
pub struct SingleInstanceLock {
    #[allow(dead_code)]
    file: File,
    path: PathBuf,
}

impl SingleInstanceLock {
    /// Returns the filesystem path of the acquired lock file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Attempts to acquire an exclusive non-blocking lock on `$XDG_RUNTIME_DIR/{name}.lock`.
    /// Returns `Ok(SingleInstanceLock)` if successfully acquired.
    /// Returns `Err(String)` if another instance holds the lock or an error occurred.
    pub fn try_acquire(name: &str) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::io::AsRawFd;

            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
                let uid = unsafe { libc::getuid() };
                format!("/run/user/{uid}")
            });

            let lock_path = PathBuf::from(runtime_dir).join(format!("{name}.lock"));
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&lock_path)
                .map_err(|e| format!("Failed to open lock file {}: {e}", lock_path.display()))?;

            let res = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if res != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EWOULDBLOCK)
                    || err.raw_os_error() == Some(libc::EAGAIN)
                {
                    return Err(format!(
                        "Another instance is already running (locked {})",
                        lock_path.display()
                    ));
                }
                return Err(format!(
                    "Failed to acquire lock on {}: {err}",
                    lock_path.display()
                ));
            }

            Ok(Self {
                file,
                path: lock_path,
            })
        }

        #[cfg(not(target_os = "linux"))]
        {
            let temp_dir = std::env::temp_dir();
            let lock_path = temp_dir.join(format!("{name}.lock"));
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&lock_path)
                .map_err(|e| format!("Failed to open lock file {}: {e}", lock_path.display()))?;
            Ok(Self {
                file,
                path: lock_path,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_single_instance_lock_acquisition_and_release() {
        let lock_name = format!("openwhisper-test-lock-{}", std::process::id());

        let lock1 = SingleInstanceLock::try_acquire(&lock_name);
        assert!(lock1.is_ok(), "First lock acquisition should succeed");

        let lock2 = SingleInstanceLock::try_acquire(&lock_name);
        assert!(
            lock2.is_err(),
            "Second lock acquisition on same name must fail"
        );

        drop(lock1);

        let lock3 = SingleInstanceLock::try_acquire(&lock_name);
        assert!(lock3.is_ok(), "Re-acquiring lock after drop should succeed");
    }
}
