use std::time::{Duration, Instant};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CREATE_WAITABLE_TIMER_MANUAL_RESET,
        CreateWaitableTimerExW, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        SetWaitableTimer, TIMER_ALL_ACCESS, WaitForSingleObject,
    };

    pub struct Pacer(HANDLE);

    impl Pacer {
        pub fn new() -> Self {
            let handle = unsafe {
                CreateWaitableTimerExW(
                    ptr::null(),
                    ptr::null(),
                    CREATE_WAITABLE_TIMER_MANUAL_RESET | CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
            Self(handle)
        }

        pub fn sleep_until(&self, when: Instant) {
            let now = Instant::now();
            if when <= now {
                return;
            }
            let left = when - now;
            if self.0.is_null() {
                std::thread::sleep(left);
                return;
            }
            let due = -((left.as_nanos() / 100).max(1) as i64);
            unsafe {
                SetWaitableTimer(self.0, &due, 0, None, ptr::null(), 0);
                WaitForSingleObject(self.0, u32::MAX);
            }
        }
    }

    impl Drop for Pacer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    pub fn cpu_time(pid: u32) -> Option<Duration> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok =
            unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return None;
        }
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
    }

    pub fn describe() -> Vec<String> {
        let power = std::process::Command::new("powercfg")
            .arg("/getactivescheme")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".into());
        vec![format!("power scheme: {power}")]
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub struct Pacer;

    impl Pacer {
        pub fn new() -> Self {
            Self
        }

        pub fn sleep_until(&self, when: Instant) {
            let now = Instant::now();
            if when > now {
                std::thread::sleep(when - now);
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn cpu_time(pid: u32) -> Option<Duration> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = stat.rsplit_once(')')?.1;
        let mut fields = rest.split_whitespace();
        let user: u64 = fields.nth(11)?.parse().ok()?;
        let system: u64 = fields.next()?.parse().ok()?;
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
        Some(Duration::from_secs_f64(
            (user + system) as f64 / ticks as f64,
        ))
    }

    #[cfg(target_os = "macos")]
    pub fn cpu_time(pid: u32) -> Option<Duration> {
        let pid = libc::pid_t::try_from(pid).ok()?;
        let mut usage: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let result =
            unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, (&raw mut usage).cast()) };
        (result == 0)
            .then(|| Duration::from_nanos(usage.ri_user_time.saturating_add(usage.ri_system_time)))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn cpu_time(_pid: u32) -> Option<Duration> {
        None
    }

    pub fn describe() -> Vec<String> {
        Vec::new()
    }
}

pub use imp::{Pacer, cpu_time, describe};

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn kernel_cpu_times_are_available_for_a_live_process() {
        let pid = std::process::id();
        let before = super::cpu_time(pid).expect("kernel usage of this process");
        let after = super::cpu_time(pid).expect("kernel usage again");
        assert!(after >= before);
        assert_eq!(super::cpu_time(u32::MAX), None);
    }
}

pub struct Contention {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Contention {
    pub fn start(count: usize) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let threads = (0..count)
            .map(|_| {
                let stop = std::sync::Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                })
            })
            .collect();
        Self { stop, threads }
    }
}

impl Drop for Contention {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
