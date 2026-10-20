//! POSIX shared memory and named semaphores.

use std::ffi::CString;
use std::io;
use std::ptr;
use std::time::Duration;

use super::Names;

fn c_name(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::other("a name with a NUL"))
}

/// A mapped block of shared memory, zero-filled when created.
pub struct Mapping {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: a mapping is plain memory addressed through a pointer that is
// valid for the object's life; sharing it is the point.
unsafe impl Send for Mapping {}
// SAFETY: as above; all access goes through atomics or raw copies.
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn create(name: &str, len: usize) -> io::Result<Self> {
        let name = c_name(name)?;
        // SAFETY: a valid NUL-terminated name; the descriptor is closed below.
        let fd = unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is the descriptor just opened.
        let sized = unsafe { libc::ftruncate(fd, len as libc::off_t) };
        if sized != 0 {
            let error = io::Error::last_os_error();
            // SAFETY: closing and removing what this function created.
            unsafe {
                libc::close(fd);
                libc::shm_unlink(name.as_ptr());
            }
            return Err(error);
        }
        // A mapping that fails leaves the named object behind unless it is
        // removed here: nothing else knows its name.
        Self::map(fd, len).inspect_err(|_| {
            // SAFETY: removing what this function created.
            unsafe { libc::shm_unlink(name.as_ptr()) };
        })
    }

    pub fn open(name: &str, len: usize) -> io::Result<Self> {
        let name = c_name(name)?;
        // SAFETY: a valid NUL-terminated name.
        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // A region smaller than the layout says would fault on first touch.
        // SAFETY: a plain struct filled by `fstat`.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is open and `stat` is a valid out pointer.
        let size_ok = unsafe { libc::fstat(fd, &mut stat) } == 0 && stat.st_size as usize >= len;
        if !size_ok {
            // SAFETY: closing the descriptor just opened.
            unsafe { libc::close(fd) };
            return Err(io::Error::other(
                "the shared region is smaller than its layout",
            ));
        }
        Self::map(fd, len)
    }

    fn map(fd: libc::c_int, len: usize) -> io::Result<Self> {
        // SAFETY: a fresh shared mapping of an open object of at least `len`
        // bytes; the descriptor is not needed once it is mapped.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: closing the descriptor, whether or not the map succeeded.
        unsafe { libc::close(fd) };
        if ptr == libc::MAP_FAILED {
            return Err(error);
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly what `map` mapped, once.
        unsafe { libc::munmap(self.ptr.cast(), self.len) };
    }
}

/// A wake-up: a counting semaphore that two processes can post and wait on.
/// A post that nobody waited for is remembered, so a waiter that checks its
/// condition after waking never misses one.
pub struct Signal {
    sem: *mut libc::sem_t,
}

// SAFETY: a semaphore handle may be used from any thread.
unsafe impl Send for Signal {}
// SAFETY: `sem_post` and the waits are thread-safe.
unsafe impl Sync for Signal {}

impl Signal {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = c_name(name)?;
        // SAFETY: a valid name; the variadic mode and value follow the C
        // declaration (`mode_t`, `unsigned`).
        let sem = unsafe {
            libc::sem_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL,
                0o600 as libc::c_uint,
                0 as libc::c_uint,
            )
        };
        Self::checked(sem)
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = c_name(name)?;
        // SAFETY: a valid NUL-terminated name.
        let sem = unsafe { libc::sem_open(name.as_ptr(), 0) };
        Self::checked(sem)
    }

    fn checked(sem: *mut libc::sem_t) -> io::Result<Self> {
        if sem == libc::SEM_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self { sem })
        }
    }

    pub fn set(&self) {
        // SAFETY: a live semaphore.
        unsafe { libc::sem_post(self.sem) };
    }

    /// Worker-only wait for the next block. The owner must post when stopping
    /// the worker (Stopper does); callback-side deadline waits use `wait`.
    #[cfg(target_os = "macos")]
    pub fn wait_for_post(&self) -> io::Result<()> {
        loop {
            // SAFETY: this object owns a live named semaphore for the whole wait.
            if unsafe { libc::sem_wait(self.sem) } == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    /// Waits for a post for up to `timeout`; true if one arrived.
    #[cfg(target_os = "linux")]
    pub fn wait(&self, timeout: Duration) -> bool {
        // SAFETY: a plain struct filled by `clock_gettime`.
        let mut now: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: a valid out pointer.
        unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
        let total = now.tv_nsec as u128 + u128::from(timeout.subsec_nanos());
        let deadline = libc::timespec {
            tv_sec: now.tv_sec.saturating_add(
                (timeout.as_secs() as libc::time_t).saturating_add((total / 1_000_000_000) as _),
            ),
            tv_nsec: (total % 1_000_000_000) as _,
        };
        loop {
            // SAFETY: a live semaphore and a valid absolute deadline.
            if unsafe { libc::sem_timedwait(self.sem, &deadline) } == 0 {
                return true;
            }
            if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return false;
            }
        }
    }

    /// Bounded client-side waits where POSIX has no timed semaphore wait.
    /// The macOS worker uses `wait_for_post` instead, with Stopper waking it.
    #[cfg(not(target_os = "linux"))]
    pub fn wait(&self, timeout: Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        loop {
            // SAFETY: a live semaphore.
            if unsafe { libc::sem_trywait(self.sem) } == 0 {
                return true;
            }
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        // SAFETY: closing the handle this object owns.
        unsafe { libc::sem_close(self.sem) };
    }
}

/// Removes the names, once both processes have opened what they name. The
/// objects live on until the last user lets go.
pub fn remove_names(names: &Names) {
    for (name, semaphore) in [
        (&names.region, false),
        (&names.go, true),
        (&names.done, true),
    ] {
        if let Ok(name) = c_name(name) {
            // SAFETY: valid NUL-terminated names; a missing name is not an error here.
            unsafe {
                if semaphore {
                    libc::sem_unlink(name.as_ptr());
                } else {
                    libc::shm_unlink(name.as_ptr());
                }
            }
        }
    }
}

/// Unix has no per-thread class like Windows' audio scheduling; a real-time
/// scheduling request needs privileges Lumen does not assume.
pub fn audio_thread_priority() -> bool {
    false
}
