use std::time::Duration;

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
