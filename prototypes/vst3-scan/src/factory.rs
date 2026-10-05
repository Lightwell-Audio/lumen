//! Child-process side: load one module and enumerate its factory.
//!
//! Runs only inside a sandboxed scan process. Loading a module executes
//! third-party code (`DllMain`, `InitDll`), which may crash or hang.

use std::ffi::c_char;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::time::Instant;

use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, PFactoryInfo, kResultOk,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};

use crate::report::{ClassInfo, FactoryReport};

type InitDllFn = unsafe extern "system" fn() -> bool;
type GetPluginFactoryFn = unsafe extern "system" fn() -> *mut IPluginFactory;

fn text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Formats a class id the way `moduleinfo.json` does on Windows, where class
/// ids use COM GUID byte order.
fn cid_string(tuid: &[c_char; 16]) -> String {
    let b: Vec<u8> = tuid.iter().map(|&c| c as u8).collect();
    let data1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let data2 = u16::from_le_bytes([b[4], b[5]]);
    let data3 = u16::from_le_bytes([b[6], b[7]]);
    let tail: String = b[8..].iter().map(|x| format!("{x:02X}")).collect();
    format!("{data1:08X}{data2:04X}{data3:04X}{tail}")
}

pub fn enumerate(binary: &Path) -> Result<FactoryReport, String> {
    // A crash must end this process at once rather than wait behind a
    // Windows Error Reporting dialog (see ADR-0005).
    // SAFETY: process-wide error-mode flags.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };

    let path: Vec<u16> = binary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let started = Instant::now();
    // SAFETY: NUL-terminated path. Altered search path lets the module find
    // DLLs that sit next to it.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            ptr::null_mut(),
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    };
    if module.is_null() {
        return Err(format!(
            "LoadLibrary failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let load_ms = started.elapsed().as_secs_f64() * 1e3;
    // SAFETY: NUL-terminated export names on a loaded module.
    let symbol = |name: &[u8]| unsafe { GetProcAddress(module, name.as_ptr()) };

    let enumerate_started = Instant::now();
    let init_dll = symbol(b"InitDll\0").map(|f| {
        // SAFETY: the VST3 Windows module entry has this signature.
        let init: InitDllFn = unsafe { std::mem::transmute(f) };
        // SAFETY: called once, before GetPluginFactory, as the spec requires.
        unsafe { init() }
    });
    let get_factory = symbol(b"GetPluginFactory\0").ok_or("no GetPluginFactory export")?;
    // SAFETY: the VST3 factory export has this signature.
    let get_factory: GetPluginFactoryFn = unsafe { std::mem::transmute(get_factory) };
    // SAFETY: the exported entry point returns an owned factory reference or
    // null, which is what `from_raw` takes ownership of.
    let factory =
        unsafe { ComPtr::from_raw(get_factory()) }.ok_or("GetPluginFactory returned null")?;

    // SAFETY: plain C structs; zero is a valid initial state.
    let mut info: PFactoryInfo = unsafe { std::mem::zeroed() };
    // SAFETY: valid out pointer for the duration of the call.
    unsafe { factory.getFactoryInfo(&mut info) };
    let factory2 = factory.cast::<IPluginFactory2>();
    // SAFETY: plain query on a live factory.
    let count = unsafe { factory.countClasses() }.max(0);
    let mut classes = Vec::with_capacity(count as usize);
    for index in 0..count {
        if let Some(f2) = &factory2 {
            // SAFETY: plain C struct; zero is a valid initial state.
            let mut c: PClassInfo2 = unsafe { std::mem::zeroed() };
            // SAFETY: `index < count` and `c` is a valid out pointer.
            let result = unsafe { f2.getClassInfo2(index, &mut c) };
            if result == kResultOk {
                classes.push(ClassInfo {
                    name: text(&c.name),
                    category: text(&c.category),
                    sub_categories: text(&c.subCategories),
                    vendor: text(&c.vendor),
                    version: text(&c.version),
                    sdk_version: text(&c.sdkVersion),
                    cid: cid_string(&c.cid),
                });
                continue;
            }
        }
        // SAFETY: plain C struct; zero is a valid initial state.
        let mut c: PClassInfo = unsafe { std::mem::zeroed() };
        // SAFETY: `index < count` and `c` is a valid out pointer.
        let result = unsafe { factory.getClassInfo(index, &mut c) };
        if result == kResultOk {
            classes.push(ClassInfo {
                name: text(&c.name),
                category: text(&c.category),
                sub_categories: String::new(),
                vendor: String::new(),
                version: String::new(),
                sdk_version: String::new(),
                cid: cid_string(&c.cid),
            });
        }
    }
    let report = FactoryReport {
        vendor: text(&info.vendor),
        url: text(&info.url),
        classes,
        load_ms,
        init_dll,
        enumerate_ms: enumerate_started.elapsed().as_secs_f64() * 1e3,
    };
    drop(factory2);
    drop(factory);
    if let Some(f) = symbol(b"ExitDll\0") {
        // SAFETY: same signature as InitDll; called once after the factory is released.
        let exit: InitDllFn = unsafe { std::mem::transmute(f) };
        // SAFETY: as above.
        unsafe { exit() };
    }
    // Deliberately no FreeLibrary: the process exits next, and some plugins
    // crash while unloading.
    Ok(report)
}
