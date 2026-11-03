//! The real hive: `HKEY_CURRENT_USER`, through the registry's own calls.
//! Only a normal start uses it (`Launch::from_args` with no isolation
//! flags).

use std::io;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW,
    RegSetKeyValueW,
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

use super::Hive;

pub(super) struct Registry;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn checked(code: u32) -> io::Result<()> {
    match code {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        code => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

impl Hive for Registry {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buf: Vec<u16> = vec![0; 260];
        loop {
            let mut bytes = (buf.len() * 2) as u32;
            // SAFETY: the strings are NUL-terminated; `buf` holds `bytes`.
            let code = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    buf.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            match code {
                ERROR_SUCCESS => {
                    let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
                    return Some(String::from_utf16_lossy(&buf[..len]));
                }
                ERROR_MORE_DATA if buf.len() < 32 * 1024 => buf.resize(bytes as usize / 2 + 1, 0),
                _ => return None,
            }
        }
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        let (key, name, value) = (wide(key), wide(name), wide(value));
        // SAFETY: the strings are NUL-terminated; the data is `value`.
        let code = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        match code {
            ERROR_SUCCESS => Ok(()),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }

    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: the strings are NUL-terminated.
        checked(unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) })
    }

    fn remove_key(&mut self, key: &str) -> io::Result<()> {
        let key = wide(key);
        // SAFETY: the string is NUL-terminated.
        checked(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) })
    }

    fn announce(&mut self) {
        // SAFETY: no items are passed with this event.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as _,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }
}
