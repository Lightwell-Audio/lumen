pub struct ProAudio(#[cfg(windows)] windows_sys::Win32::Foundation::HANDLE);

impl ProAudio {
    pub fn join() -> Option<Self> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
            let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
            let mut index = 0;
            let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
            (!handle.is_null()).then_some(Self(handle))
        }
        #[cfg(not(windows))]
        None
    }
}

#[cfg(windows)]
impl Drop for ProAudio {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Threading::AvRevertMmThreadCharacteristics(self.0) };
    }
}
