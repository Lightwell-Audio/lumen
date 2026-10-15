//! Fixed-size text fields of the VST3 structs.

use std::ffi::c_char;

use vst3::Steinberg::Vst::TChar;

/// Copies `src` into a NUL-terminated `char8` field, truncating to fit.
pub(crate) fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let room = dst.len().saturating_sub(1);
    let mut n = 0;
    for (d, b) in dst.iter_mut().zip(src.bytes().take(room)) {
        *d = b as c_char;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// Copies `src` into a NUL-terminated UTF-16 field, truncating to fit.
pub(crate) fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let room = dst.len().saturating_sub(1);
    let mut n = 0;
    for (d, u) in dst.iter_mut().zip(src.encode_utf16().take(room)) {
        *d = u as TChar;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// Reads a NUL-terminated `char8` field.
#[cfg(test)]
pub(crate) fn read_cstring(src: &[c_char]) -> String {
    let bytes: Vec<u8> = src
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads a NUL-terminated UTF-16 field.
#[cfg(test)]
pub(crate) fn read_wstring(src: &[TChar]) -> String {
    let units: Vec<u16> = src.iter().take_while(|&&c| c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_and_truncates() {
        let mut narrow = [1 as c_char; 8];
        copy_cstring("Hello", &mut narrow);
        assert_eq!(read_cstring(&narrow), "Hello");
        copy_cstring("A very long name", &mut narrow);
        assert_eq!(read_cstring(&narrow), "A very ");
        let mut wide = [1 as TChar; 6];
        copy_wstring("Gain", &mut wide);
        assert_eq!(read_wstring(&wide), "Gain");
        copy_wstring("Gain and more", &mut wide);
        assert_eq!(read_wstring(&wide), "Gain ");
    }
}
