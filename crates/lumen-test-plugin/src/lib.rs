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

mod component;
mod controller;
mod dsp;
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
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn BundleEntry(_bundle: *mut std::ffi::c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn BundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleEntry(_library: *mut std::ffi::c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    true
}

/// The module's entry point.
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut vst3::Steinberg::IPluginFactory {
    plugin_factory()
}
