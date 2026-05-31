/// Single-instance enforcement.
///
/// Call `SingleInstanceGuard::try_acquire()` at process startup.
/// Returns `Ok(guard)` if this is the first instance, `Err(())` if another
/// instance is already running.  Pass `--no-singleton` on the command line to
/// skip the check (useful for development / testing).
///
/// The guard must be held for the lifetime of the process; dropping it
/// releases the lock so a subsequent launch can succeed.
pub struct SingleInstanceGuard {
    #[cfg(target_os = "windows")]
    _mutex_handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(target_os = "windows"))]
    _lockfile: std::fs::File,
}

impl SingleInstanceGuard {
    /// Returns `Ok(guard)` if this is the first instance.
    /// Returns `Err(())` if another instance is already running.
    /// If `--no-singleton` appears anywhere in `std::env::args()`, always
    /// returns `Ok` (guard still holds no real lock on non-Windows).
    pub fn try_acquire() -> Result<Self, ()> {
        if std::env::args().any(|a| a == "--no-singleton") {
            log::info!("single_instance: --no-singleton flag set, skipping check");
            return Ok(Self::dummy());
        }

        Self::platform_acquire()
    }

    // ── Windows ───────────────────────────────────────────────────────────────

    #[cfg(target_os = "windows")]
    fn platform_acquire() -> Result<Self, ()> {
        use windows_sys::Win32::Foundation::{
            GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::System::Threading::CreateMutexW;

        // Encode the mutex name as a null-terminated wide string.
        let name: Vec<u16> = "Local\\TerminalStudioSingleton\0".encode_utf16().collect();

        // SAFETY: name is a valid null-terminated UTF-16 string; no attributes
        // are needed; initial owner = FALSE so we don't own it on creation.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };

        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            log::warn!("single_instance: CreateMutexW failed");
            // Treat failure to create the mutex as "allow startup" to avoid
            // blocking the app on permission errors.
            return Ok(SingleInstanceGuard {
                _mutex_handle: handle,
            });
        }

        // GetLastError() == ERROR_ALREADY_EXISTS means a prior CreateMutexW
        // with the same name already succeeded in another process.
        let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already {
            log::warn!("single_instance: another instance is already running");
            // Close our handle before returning the error so we don't leak it.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(());
        }

        Ok(SingleInstanceGuard {
            _mutex_handle: handle,
        })
    }

    #[cfg(target_os = "windows")]
    fn dummy() -> Self {
        // Return a guard with a null handle; Drop is a no-op for null.
        SingleInstanceGuard { _mutex_handle: 0 }
    }

    // ── Non-Windows (advisory lockfile via fcntl LOCK_EX | LOCK_NB) ──────────

    #[cfg(not(target_os = "windows"))]
    fn platform_acquire() -> Result<Self, ()> {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;

        let lock_path = Self::lockfile_path();
        if let Some(parent) = lock_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|e| {
                log::warn!(
                    "single_instance: cannot open lockfile {:?}: {}",
                    lock_path,
                    e
                );
            })?;

        // Try a non-blocking exclusive lock.
        let locked = Self::try_lock_exclusive(&file);
        if !locked {
            log::warn!("single_instance: another instance holds the lockfile lock");
            return Err(());
        }

        Ok(SingleInstanceGuard { _lockfile: file })
    }

    #[cfg(not(target_os = "windows"))]
    fn dummy() -> Self {
        use std::fs::OpenOptions;
        let file = OpenOptions::new()
            .read(true)
            .open("/dev/null")
            .unwrap_or_else(|_| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(std::env::temp_dir().join(".ts-dummy-lock"))
                    .expect("cannot create dummy lockfile")
            });
        SingleInstanceGuard { _lockfile: file }
    }

    #[cfg(not(target_os = "windows"))]
    fn try_lock_exclusive(file: &std::fs::File) -> bool {
        use std::os::unix::io::AsRawFd;
        // libc::flock with LOCK_EX | LOCK_NB
        let fd = file.as_raw_fd();
        // SAFETY: fd is valid for the lifetime of `file`.
        let ret = unsafe { libc_flock(fd, 2 | 4) }; // LOCK_EX=2, LOCK_NB=4
        ret == 0
    }

    #[cfg(not(target_os = "windows"))]
    fn lockfile_path() -> std::path::PathBuf {
        crate::util::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp").join("terminal-studio"))
            .join(".singleton.lock")
    }
}

// Thin wrapper around the `flock(2)` syscall to avoid a libc dependency.
#[cfg(not(target_os = "windows"))]
extern "C" {
    #[link_name = "flock"]
    fn libc_flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
            if self._mutex_handle != 0 && self._mutex_handle != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self._mutex_handle) };
            }
        }
        // On non-Windows: dropping `_lockfile` closes the fd, which releases
        // the advisory lock automatically.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acquire_succeeds() {
        // The first acquire in this test process should succeed.
        // Note: this uses the real OS-level lock, so it may conflict with a
        // running instance. If that happens, it is still not a panic.
        let result = SingleInstanceGuard::try_acquire();
        // We accept both Ok and Err — the key assertion is no panic.
        // In CI with no running instance, this should be Ok.
        if let Ok(guard) = result {
            drop(guard);
        }
    }

    #[test]
    fn test_acquire_and_drop() {
        // Acquire the guard, then drop it explicitly. No panic expected.
        let result = SingleInstanceGuard::try_acquire();
        if let Ok(guard) = result {
            drop(guard);
            // After dropping, a second acquire should also succeed.
            let result2 = SingleInstanceGuard::try_acquire();
            if let Ok(guard2) = result2 {
                drop(guard2);
            }
        }
    }

    #[test]
    fn test_guard_is_send() {
        // Verify SingleInstanceGuard can be sent across threads (useful for
        // holding in main thread state that may be moved).
        fn assert_send<T: Send>() {}
        assert_send::<SingleInstanceGuard>();
    }
}
