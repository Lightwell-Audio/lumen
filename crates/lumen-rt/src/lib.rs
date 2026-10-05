//! Real-time safety tooling.
//!
//! Code that runs on the audio thread must never allocate, free, lock or
//! block. Lumen enforces this with tests rather than convention: a test binary
//! installs [`GuardedAllocator`] as its global allocator (see
//! [`install_guarded_allocator!`]) and runs real-time code inside
//! [`rt_section`]. Every heap operation performed inside a section is counted
//! as a violation, and tests assert that the count stays at zero.
//!
//! The guard only detects heap traffic. Locks and blocking system calls are
//! not detected here; ADR-0010 covers the plan for those.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    // Const-initialised and without a destructor, so accessing these from
    // inside the allocator never allocates.
    static RT_DEPTH: Cell<u32> = const { Cell::new(0) };
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
            RT_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
    }
    RT_DEPTH.with(|depth| depth.set(depth.get() + 1));
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
        std::hint::black_box(Box::new(0_u8));
    });
    // The probe itself is not a real violation; keep the global count honest.
    TOTAL_VIOLATIONS.fetch_sub(seen, Ordering::Relaxed);
    seen > 0
}
