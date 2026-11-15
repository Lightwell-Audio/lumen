use std::ffi::c_void;

use vst3::ComRef;
use vst3::Steinberg::{IBStream, IBStreamTrait, kResultOk};

pub(crate) unsafe fn read_all(stream: *mut IBStream, limit: usize) -> Option<Vec<u8>> {
    let stream = unsafe { ComRef::from_raw(stream) }?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        let mut got = 0;
        let result = unsafe { stream.read(chunk.as_mut_ptr().cast::<c_void>(), 256, &mut got) };
        if result != kResultOk || got < 0 {
            return None;
        }
        if got == 0 {
            return Some(bytes);
        }
        bytes.extend_from_slice(&chunk[..got as usize]);
        if bytes.len() > limit {
            return None;
        }
    }
}

pub(crate) unsafe fn write_all(stream: *mut IBStream, bytes: &[u8]) -> bool {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return false;
    };
    let mut written = 0;
    let result = unsafe {
        stream.write(
            bytes.as_ptr().cast_mut().cast::<c_void>(),
            bytes.len() as i32,
            &mut written,
        )
    };
    result == kResultOk && written as usize == bytes.len()
}
