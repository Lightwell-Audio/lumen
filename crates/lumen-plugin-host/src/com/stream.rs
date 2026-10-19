//! The in-memory stream a plugin writes its state to and reads it from.

use super::*;

/// An in-memory `IBStream` for plugin state. The size is capped, so a plugin
/// that writes without end cannot take the host's memory.
pub struct MemoryStream {
    inner: Mutex<(Vec<u8>, usize)>,
    cap: usize,
}

impl MemoryStream {
    pub fn open(data: Vec<u8>, cap: usize) -> (ComWrapper<Self>, ComPtr<IBStream>) {
        let wrapper = ComWrapper::new(Self {
            inner: Mutex::new((data, 0)),
            cap,
        });
        let ptr = wrapper.to_com_ptr::<IBStream>().expect("interface listed");
        (wrapper, ptr)
    }

    /// Everything written so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.inner.lock().map(|g| g.0.clone()).unwrap_or_default()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_read: *mut int32,
    ) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = (num_bytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        // SAFETY: the plugin's buffer holds `num_bytes` bytes and n <= num_bytes.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer.cast::<u8>(), n) };
        *pos += n;
        if !num_bytes_read.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *num_bytes_read = n as int32 };
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_written: *mut int32,
    ) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = num_bytes.max(0) as usize;
        if pos.saturating_add(n) > self.cap {
            return kInvalidArgument;
        }
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        // SAFETY: the plugin's buffer holds `num_bytes` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer.cast::<u8>(), data.as_mut_ptr().add(*pos), n)
        };
        *pos += n;
        if !num_bytes_written.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *num_bytes_written = n as int32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let base = match mode {
            m if m == kIBSeekSet as int32 => 0,
            m if m == kIBSeekCur as int32 => *pos as int64,
            m if m == kIBSeekEnd as int32 => data.len() as int64,
            _ => return kInvalidArgument,
        };
        let target = base.saturating_add(offset);
        if target < 0 || target as u64 > self.cap as u64 {
            return kInvalidArgument;
        }
        *pos = target as usize;
        if !result.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *result = target };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        if !pos.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *pos = guard.1 as int64 };
        }
        kResultOk
    }
}
