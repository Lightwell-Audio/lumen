//! Installed ASIO drivers, as registered under `HKLM\SOFTWARE\ASIO`.

use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};

use super::wide;

#[derive(Debug, Clone)]
pub struct RegisteredDriver {
    /// Registry key name; what users see in driver lists.
    pub key_name: String,
    pub clsid: String,
}

/// Lists registered drivers in registry order. Read-only; never loads a driver.
pub fn installed_drivers() -> Vec<RegisteredDriver> {
    let mut drivers = Vec::new();
    let mut root: HKEY = ptr::null_mut();
    let path = wide("SOFTWARE\\ASIO");
    // SAFETY: valid NUL-terminated path; `root` receives the opened key.
    let status =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut root) };
    if status != ERROR_SUCCESS {
        return drivers;
    }
    for index in 0.. {
        let mut name = [0_u16; 256];
        let mut len = name.len() as u32;
        // SAFETY: `name` holds `len` UTF-16 units; optional outputs are null.
        let status = unsafe {
            RegEnumKeyExW(
                root,
                index,
                name.as_mut_ptr(),
                &mut len,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        let key_name = String::from_utf16_lossy(&name[..len as usize]);
        let Some(clsid) = read_string(root, &key_name, "CLSID") else {
            continue;
        };
        drivers.push(RegisteredDriver { key_name, clsid });
    }
    // SAFETY: `root` was opened above and is closed exactly once.
    unsafe { RegCloseKey(root) };
    drivers
}

fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut buf = [0_u16; 512];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `bytes` bytes; strings are NUL-terminated.
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let units = (bytes as usize / 2).saturating_sub(1); // drop the terminator
    Some(String::from_utf16_lossy(&buf[..units]))
}
