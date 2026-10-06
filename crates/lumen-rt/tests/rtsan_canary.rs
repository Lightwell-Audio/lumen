//! Proves RealtimeSanitizer is active in CI. Each test does something RTSan
//! intercepts inside a real-time section, so with `RTSAN_ENABLE=1` the
//! process must abort. CI runs each one on its own and expects failure.
//! Ignored in normal test runs.
//!
//! An uncontended `std::sync::Mutex` is not a usable canary: on Linux it is a
//! single atomic operation with no system call, which no sanitizer can see
//! (first CI run, 2026-10-06). RTSan catches a lock only when it blocks.

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn a_blocking_call_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        std::thread::sleep(std::time::Duration::from_millis(1));
    });
}

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn an_allocation_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        std::hint::black_box(vec![0.0_f32; 256]);
    });
}
