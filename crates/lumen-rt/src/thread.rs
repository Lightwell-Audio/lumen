//! Asking the system to run a thread as an audio thread.

use std::time::Duration;

/// Asks the system to schedule the calling thread as an audio thread whose
/// work comes every `period`; true if it agreed.
///
/// On macOS that is the time-constraint policy Core Audio's own threads run
/// with, which needs no privileges: half the period is asked for the work,
/// to be done within the period. Without it a thread of a busy machine, or
/// of a background process, can be left waiting past a block's deadline.
/// Elsewhere this does nothing and says so: Linux's real-time class needs
/// privileges Lumen does not assume, and Windows threads join the "Pro
/// Audio" class where they are made (`lumen-plugin-host`'s
/// `audio_thread_priority`).
pub fn realtime_thread(period: Duration) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::time_constraint(period)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = period;
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::time::Duration;

    /// The ratio of Mach absolute time to nanoseconds. Declared here because
    /// `libc` deprecates its copy in favour of a crate this one call does not
    /// warrant; the function is part of libSystem and stable.
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }

    pub(super) fn time_constraint(period: Duration) -> bool {
        let mut timebase = Timebase { numer: 0, denom: 0 };
        // SAFETY: a valid out pointer.
        if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.numer == 0 {
            return false;
        }
        let ticks = |ns: u128| {
            u32::try_from(ns * u128::from(timebase.denom) / u128::from(timebase.numer))
                .unwrap_or(u32::MAX)
        };
        let ns = period.as_nanos();
        let mut policy = libc::thread_time_constraint_policy {
            period: ticks(ns),
            computation: ticks(ns / 2),
            constraint: ticks(ns),
            preemptible: 1,
        };
        // SAFETY: this thread's own Mach port, and a policy of the flavor
        // and size named.
        let result = unsafe {
            let thread = libc::pthread_mach_thread_np(libc::pthread_self());
            libc::thread_policy_set(
                thread,
                libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
                (&raw mut policy).cast::<libc::integer_t>(),
                libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
            )
        };
        result == libc::KERN_SUCCESS
    }
}
