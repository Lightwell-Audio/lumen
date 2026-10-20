//! Deterministic VST3 test plugins: an instrument and an effect that sound
//! exactly the same every time, with faults that can be switched on to make
//! them misbehave (see [`faults`]). Hosts, the scanner and the golden
//! renders are tested against them, never against third-party software.
//!
//! Built as a module (`cdylib`) for scanning and hosting, and as a library
//! so the same classes can be tested in process.

// The bindings' enum constants are `i32` on Windows and `u32` elsewhere, so
// a cast one platform needs is a no-op on the other.
#![allow(clippy::unnecessary_cast)]

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
static BUNDLE_ENTERED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
extern "C" fn module_terminator() {
    if faults::Faults::from_env().has(faults::Kind::ExitCrash) {
        faults::crash();
    }
}

#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_term_func")]
static MODULE_TERMINATOR: extern "C" fn() = module_terminator;

mod component;
pub mod controller;
mod dsp;
pub mod editor;
mod factory;
pub mod faults;
pub mod params;
pub mod render;
mod state;
mod streams;
mod text;

pub use component::Processor;
pub use controller::Controller;
pub use factory::{
    EFFECT, EFFECT_CONTROLLER, INSTRUMENT, INSTRUMENT_CONTROLLER, component, controller, factory,
    plugin_factory,
};

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub extern "system" fn InitDll() -> bool {
    !faults::Faults::from_env().has(faults::Kind::EntryFalse)
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
#[allow(
    non_snake_case,
    reason = "the VST3 SDK requires the exported symbol bundleEntry"
)]
pub extern "system" fn bundleEntry(bundle: *mut std::ffi::c_void) -> bool {
    let accepted = !bundle.is_null() && !faults::Faults::from_env().has(faults::Kind::EntryFalse);
    if accepted {
        BUNDLE_ENTERED.store(true, Ordering::Release);
    }
    accepted
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
#[allow(
    non_snake_case,
    reason = "the VST3 SDK requires the exported symbol bundleExit"
)]
pub extern "system" fn bundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleEntry(_library: *mut std::ffi::c_void) -> bool {
    !faults::Faults::from_env().has(faults::Kind::EntryFalse)
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    true
}

/// The module's entry point.
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut vst3::Steinberg::IPluginFactory {
    #[cfg(target_os = "macos")]
    if faults::Faults::from_env().has(faults::Kind::EntryRequired)
        && !BUNDLE_ENTERED.load(Ordering::Acquire)
    {
        return std::ptr::null_mut();
    }
    plugin_factory()
}
