//! Proves RealtimeSanitizer is active in CI. This test takes a lock inside a
//! real-time section, so with `RTSAN_ENABLE=1` the process must abort. CI
//! runs it and expects failure. Ignored in normal test runs.

use std::sync::Mutex;

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn a_lock_inside_a_real_time_section_is_caught() {
    let mutex = Mutex::new(0);
    lumen_rt::rt_section(|| {
        *mutex.lock().unwrap() += 1;
    });
}
