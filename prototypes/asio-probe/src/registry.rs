use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};

use crate::wide;

#[derive(Debug, Clone)]
pub struct RegisteredDriver {
    pub key_name: String,
    pub clsid: String,
    pub description: Option<String>,
}

pub fn installed_drivers() -> Vec<RegisteredDriver> {
    let mut drivers = Vec::new();
    let mut root: HKEY = ptr::null_mut();
    let path = wide("SOFTWARE\\ASIO");
    let status =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut root) };
    if status != ERROR_SUCCESS {
        return drivers;
    }
    for index in 0.. {
        let mut name = [0_u16; 256];
        let mut len = name.len() as u32;
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
        let description = read_string(root, &key_name, "Description");
        drivers.push(RegisteredDriver {
            key_name,
            clsid,
            description,
        });
    }
    unsafe { RegCloseKey(root) };
    drivers
}

fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut buf = [0_u16; 512];
    let mut bytes = (buf.len() * 2) as u32;
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
    let units = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..units]))
}
