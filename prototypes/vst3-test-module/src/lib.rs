//! A VST3 module that misbehaves on purpose, chosen by its own file name:
//! `*crash*` dereferences null in `GetPluginFactory`, `*hang*` never returns,
//! anything else returns no factory. Seed of the "test plugin" the quality
//! bar asks for; M6 grows it into a full plugin with configurable faults.

#![cfg(windows)]

use std::ffi::c_void;
use std::time::Duration;

use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};

fn own_file_name() -> String {
    let mut module = std::ptr::null_mut();
    let mut buf = [0_u16; 1024];
    // SAFETY: looks up the module containing this function; no refcount change.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            own_file_name as *const () as *const u16,
            &mut module,
        );
        let len = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32);
        String::from_utf16_lossy(&buf[..len as usize]).to_lowercase()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn InitDll() -> bool {
    true
}

#[unsafe(no_mangle)]
pub extern "system" fn ExitDll() -> bool {
    true
}

#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut c_void {
    let name = own_file_name();
    if name.contains("crash") {
        // SAFETY: deliberately not safe; simulates a plugin crashing in its factory.
        unsafe { std::ptr::null_mut::<u32>().write_volatile(0xDEAD) };
    }
    if name.contains("hang") {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    std::ptr::null_mut()
}
