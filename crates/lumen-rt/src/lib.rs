//! Real-time safety tooling.
//!
//! Code that runs on the audio thread must never allocate, free, lock or
//! block. Lumen enforces this with tests rather than convention: a test binary
//! installs [`GuardedAllocator`] as its global allocator (see
//! [`install_guarded_allocator!`]) and runs real-time code inside
//! [`rt_section`]. Every heap operation performed inside a section is counted
//! as a violation, and tests assert that the count stays at zero.
//!
//! The guard only detects heap traffic. For locks and blocking system calls,
//! sections also mark a RealtimeSanitizer real-time context: when a test is
//! built with `RTSAN_ENABLE=1` on Linux or macOS, RTSan aborts with a stack
//! trace on any unsafe call inside a section. Without that variable the
//! calls compile to nothing (ADR-0010).
//!
//! [`DenormalGuard`] makes the audio thread flush denormals to zero, so
//! decaying filters and reverbs never hit the slow path.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

// Const-initialised and without a destructor, so accessing these from inside
// the allocator never allocates.
thread_local! {
    // False positive in clippy 1.99: the initializers are already `const`.
    #[allow(clippy::missing_const_for_thread_local)]
    static RT_DEPTH: Cell<u32> = const { Cell::new(0) };
    #[allow(clippy::missing_const_for_thread_local)]
    static THREAD_VIOLATIONS: Cell<u64> = const { Cell::new(0) };
}

static TOTAL_VIOLATIONS: AtomicU64 = AtomicU64::new(0);

/// Global allocator that counts heap operations made inside [`rt_section`].
///
/// It forwards everything to the system allocator, so it is safe to install
/// in any binary. The overhead is one thread-local read per heap operation.
#[derive(Debug, Default, Clone, Copy)]
pub struct GuardedAllocator;

#[inline]
fn note_heap_operation() {
    // `try_with` because thread-locals are unavailable while a thread exits.
    let in_rt = RT_DEPTH.try_with(|depth| depth.get() > 0).unwrap_or(false);
    if in_rt {
        let _ = THREAD_VIOLATIONS.try_with(|count| count.set(count.get() + 1));
        TOTAL_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the only addition is a counter update that never allocates.
unsafe impl GlobalAlloc for GuardedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_heap_operation();
        // SAFETY: forwarded unchanged; the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_heap_operation();
        // SAFETY: forwarded unchanged; the caller upholds the contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_heap_operation();
        // SAFETY: forwarded unchanged; `ptr` was allocated by `System` via this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_heap_operation();
        // SAFETY: forwarded unchanged; `ptr` was allocated by `System` via this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Installs [`GuardedAllocator`] as the global allocator of the current binary.
///
/// Use once per test or benchmark binary that checks real-time safety.
#[macro_export]
macro_rules! install_guarded_allocator {
    () => {
        #[global_allocator]
        static LUMEN_GUARDED_ALLOCATOR: $crate::GuardedAllocator = $crate::GuardedAllocator;
    };
}

/// Runs `f` as real-time code on the current thread. Sections may nest.
pub fn rt_section<R>(f: impl FnOnce() -> R) -> R {
    struct Exit;
    impl Drop for Exit {
        fn drop(&mut self) {
            rtsan_standalone::realtime_exit();
            RT_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
    }
    RT_DEPTH.with(|depth| depth.set(depth.get() + 1));
    rtsan_standalone::ensure_initialized();
    rtsan_standalone::realtime_enter();
    let _exit = Exit;
    f()
}

/// Returns `true` if the current thread is inside an [`rt_section`].
pub fn in_rt_section() -> bool {
    RT_DEPTH.with(|depth| depth.get() > 0)
}

/// Heap operations performed inside real-time sections on this thread.
pub fn thread_violations() -> u64 {
    THREAD_VIOLATIONS.with(Cell::get)
}

/// Heap operations performed inside real-time sections on any thread.
pub fn total_violations() -> u64 {
    TOTAL_VIOLATIONS.load(Ordering::Relaxed)
}

/// Runs `f` inside an [`rt_section`] and returns its result together with
/// the number of heap operations it performed.
pub fn count_rt_violations<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = thread_violations();
    let result = rt_section(f);
    (result, thread_violations() - before)
}

/// Returns `true` if [`GuardedAllocator`] is the global allocator.
///
/// Tests call this first so a missing [`install_guarded_allocator!`] fails
/// loudly instead of silently passing every check.
pub fn guard_is_active() -> bool {
    let (_, seen) = count_rt_violations(|| {
        // A deliberate allocation; keep RealtimeSanitizer from aborting on it.
        rtsan_standalone::disable();
        std::hint::black_box(Box::new(0_u8));
        rtsan_standalone::enable();
    });
    // The probe itself is not a real violation; keep the global count honest.
    TOTAL_VIOLATIONS.fetch_sub(seen, Ordering::Relaxed);
    seen > 0
}

/// Flushes denormals to zero on the current thread until dropped, then
/// restores the previous setting. Create one at the top of every audio
/// callback; it costs two register accesses.
///
/// x86-64: sets FTZ and DAZ in MXCSR. AArch64: sets FZ in FPCR. Elsewhere
/// it does nothing.
#[derive(Debug)]
pub struct DenormalGuard {
    #[cfg_attr(
        not(any(target_arch = "x86_64", target_arch = "aarch64")),
        allow(dead_code)
    )]
    previous: u64,
}

impl DenormalGuard {
    #[cfg(target_arch = "x86_64")]
    const FLUSH: u64 = 0x8040; // FTZ (bit 15) | DAZ (bit 6)
    #[cfg(target_arch = "aarch64")]
    const FLUSH: u64 = 1 << 24; // FZ

    #[inline]
    pub fn new() -> Self {
        let previous = control_register();
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        set_control_register(previous | Self::FLUSH);
        Self { previous }
    }
}

impl Default for DenormalGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DenormalGuard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        set_control_register(self.previous);
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn control_register() -> u64 {
    let mut csr: u32 = 0;
    // SAFETY: STMXCSR stores the 32-bit MXCSR register to valid, writable memory.
    unsafe {
        std::arch::asm!("stmxcsr [{}]", in(reg) &raw mut csr, options(nostack, preserves_flags));
    }
    u64::from(csr)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn set_control_register(value: u64) {
    let csr = value as u32;
    // SAFETY: LDMXCSR loads MXCSR from valid memory. Only the FTZ and DAZ
    // bits differ from the value read back, which affects float results only.
    unsafe {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &raw const csr, options(nostack, readonly, preserves_flags));
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn control_register() -> u64 {
    let fpcr: u64;
    // SAFETY: reading FPCR has no side effects.
    unsafe {
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
    }
    fpcr
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn set_control_register(value: u64) {
    // SAFETY: writing FPCR changes floating-point behaviour on this thread only.
    unsafe {
        std::arch::asm!("msr fpcr, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline]
fn control_register() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn denormals_flush_inside_the_guard_and_not_after() {
        let tiny = black_box(f32::MIN_POSITIVE);
        assert!(black_box(tiny * 0.5) > 0.0, "denormals already flushed");
        {
            let _guard = DenormalGuard::new();
            assert_eq!(black_box(tiny * 0.5), 0.0);
        }
        assert!(black_box(tiny * 0.5) > 0.0, "setting not restored");
    }
}
