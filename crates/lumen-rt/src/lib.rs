mod thread;

pub use thread::realtime_thread;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static RT_DEPTH: Cell<u32> = const { Cell::new(0) };
    static THREAD_VIOLATIONS: Cell<u64> = const { Cell::new(0) };
}
static TOTAL_VIOLATIONS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Clone, Copy)]
pub struct GuardedAllocator;

#[inline]
fn note_heap_operation() {
    let in_rt = RT_DEPTH.try_with(|depth| depth.get() > 0).unwrap_or(false);
    if in_rt {
        let _ = THREAD_VIOLATIONS.try_with(|count| count.set(count.get() + 1));
        TOTAL_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for GuardedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_heap_operation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_heap_operation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_heap_operation();
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_heap_operation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

static FAILURE_HOOK: std::sync::OnceLock<fn(usize)> = std::sync::OnceLock::new();

thread_local! {
    static IN_FAILURE_HOOK: Cell<bool> = const { Cell::new(false) };
}

pub fn on_allocation_failure(hook: fn(usize)) {
    let _ = FAILURE_HOOK.set(hook);
}

#[cold]
fn allocation_failed(size: usize) {
    let Some(hook) = FAILURE_HOOK.get() else {
        return;
    };
    let first = IN_FAILURE_HOOK
        .try_with(|busy| !busy.replace(true))
        .unwrap_or(false);
    if first {
        hook(size);
        let _ = IN_FAILURE_HOOK.try_with(|busy| busy.set(false));
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ReportingAllocator;

unsafe impl GlobalAlloc for ReportingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if ptr.is_null() {
            allocation_failed(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if ptr.is_null() {
            allocation_failed(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if new.is_null() {
            allocation_failed(new_size);
        }
        new
    }
}

#[macro_export]
macro_rules! install_guarded_allocator {
    () => {
        #[global_allocator]
        static LUMEN_GUARDED_ALLOCATOR: $crate::GuardedAllocator = $crate::GuardedAllocator;
    };
}

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

pub fn in_rt_section() -> bool {
    RT_DEPTH.with(|depth| depth.get() > 0)
}

pub fn thread_violations() -> u64 {
    THREAD_VIOLATIONS.with(Cell::get)
}

pub fn total_violations() -> u64 {
    TOTAL_VIOLATIONS.load(Ordering::Relaxed)
}

pub fn count_rt_violations<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = thread_violations();
    let result = rt_section(f);
    (result, thread_violations() - before)
}

pub fn guard_is_active() -> bool {
    let (_, seen) = count_rt_violations(|| {
        rtsan_standalone::disable();
        std::hint::black_box(Box::new(0_u8));
        rtsan_standalone::enable();
    });
    TOTAL_VIOLATIONS.fetch_sub(seen, Ordering::Relaxed);
    seen > 0
}

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
    const FLUSH: u64 = 0x8040;
    #[cfg(target_arch = "aarch64")]
    const FLUSH: u64 = 1 << 24;

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
    unsafe {
        std::arch::asm!("stmxcsr [{}]", in(reg) &raw mut csr, options(nostack, preserves_flags));
    }
    u64::from(csr)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn set_control_register(value: u64) {
    let csr = value as u32;
    unsafe {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &raw const csr, options(nostack, readonly, preserves_flags));
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn control_register() -> u64 {
    let fpcr: u64;
    unsafe {
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
    }
    fpcr
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn set_control_register(value: u64) {
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
