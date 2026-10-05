//! Overhead of the guarded allocator compared with the plain system allocator.

use std::alloc::{GlobalAlloc, Layout, System};

use lumen_rt::GuardedAllocator;

fn main() {
    divan::main();
}

const LAYOUT: Layout = match Layout::from_size_align(64, 8) {
    Ok(layout) => layout,
    Err(_) => panic!("invalid layout"),
};

#[divan::bench]
fn system_alloc_free_64b() {
    // SAFETY: LAYOUT has non-zero size; the pointer is freed with the same layout.
    unsafe {
        let ptr = System.alloc(divan::black_box(LAYOUT));
        System.dealloc(divan::black_box(ptr), LAYOUT);
    }
}

#[divan::bench]
fn guarded_alloc_free_64b() {
    // SAFETY: LAYOUT has non-zero size; the pointer is freed with the same layout.
    unsafe {
        let ptr = GuardedAllocator.alloc(divan::black_box(LAYOUT));
        GuardedAllocator.dealloc(divan::black_box(ptr), LAYOUT);
    }
}
