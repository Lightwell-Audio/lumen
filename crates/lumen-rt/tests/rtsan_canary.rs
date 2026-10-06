//! Proves RealtimeSanitizer is active in CI. Each test does something RTSan
//! intercepts inside a real-time section, so with `RTSAN_ENABLE=1` the
//! process must abort. CI runs each one on its own and expects failure.
//! Ignored in normal test runs.
//!
//! Chosen from a probe run under the LLVM 20.1 runtime (2026-10-06, WSL):
//! RTSan catches file opens (`open64`), console writes (`write`), contended
//! locks (`syscall`, the futex wait) and heap allocation (`malloc`). It does
//! not catch `std::thread::sleep` (Rust calls `clock_nanosleep`), file
//! metadata (`statx`), or an uncontended `Mutex`, which is one atomic
//! operation with no system call.

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn blocking_file_io_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        let _ = std::fs::File::open("/dev/null");
    });
}

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn waiting_on_a_held_lock_inside_a_real_time_section_is_caught() {
    let shared = Arc::new(Mutex::new(0));
    let held = shared.lock().unwrap();
    let waiter = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            lumen_rt::rt_section(|| {
                *shared.lock().unwrap() += 1;
            });
        })
    };
    // Hold the lock long enough that the real-time thread has to wait.
    std::thread::sleep(Duration::from_millis(200));
    drop(held);
    waiter.join().unwrap();
}

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn an_allocation_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        std::hint::black_box(vec![0.0_f32; 256]);
    });
}
