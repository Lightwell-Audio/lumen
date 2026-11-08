//! Compiles the C interface to the vendored time-stretch library (M9.5 S4c,
//! ADR-0132): one C++ file, and headers only from `vendor/`.
//!
//! The library is built without fast-math, without fused multiply-add
//! contraction and without a target-specific instruction set, so the
//! arithmetic is what is written and a build repeats itself: the output is
//! the same offline and live. (Bits equal between platforms are not claimed.)

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/stretch/shim.cpp");
    println!("cargo::rerun-if-changed=vendor");
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .file("src/stretch/shim.cpp")
        .include("vendor")
        // Vendored code: its warnings are upstream's.
        .warnings(false)
        // The FFT is the cost; a test build must not run it unoptimised.
        .opt_level(2)
        .debug(false)
        .flag_if_supported("-fno-fast-math")
        .flag_if_supported("-ffp-contract=off")
        .flag_if_supported("/fp:precise")
        .compile("lumen_stretch");
}
