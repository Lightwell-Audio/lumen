//! Process CPU time and age, for idle and start-up measurements.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    fn times() -> (FILETIME, FILETIME, FILETIME) {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the pseudo-handle is always valid; out pointers are valid.
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
        (created, kernel, user)
    }

    /// User plus kernel CPU time of this process, in seconds.
    pub fn cpu_seconds() -> f64 {
        let (_, kernel, user) = times();
        (ticks(kernel) + ticks(user)) as f64 * 1e-7
    }

    /// Milliseconds since the OS created this process.
    pub fn age_ms() -> f64 {
        let (created, _, _) = times();
        let mut now = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        // SAFETY: valid out pointer.
        unsafe { GetSystemTimePreciseAsFileTime(&mut now) };
        ticks(now).saturating_sub(ticks(created)) as f64 * 1e-4
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn cpu_seconds() -> f64 {
        f64::NAN
    }
    pub fn age_ms() -> f64 {
        f64::NAN
    }
}

pub use imp::{age_ms, cpu_seconds};
