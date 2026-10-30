//! Scheduling for threads that keep time: the audio threads, and threads
//! that send at the audio clock's pace (MIDI out).
//!
//! On Windows they join the "Pro Audio" class of the multimedia scheduler,
//! so a busy machine does not leave them waiting behind ordinary threads
//! for a time slice (15.6 ms or more). Elsewhere a real-time request needs
//! privileges Lumen does not assume, and the guard is never made.

/// The calling thread's place in the multimedia scheduler, for the life of
/// the guard.
pub struct ProAudio(#[cfg(windows)] windows_sys::Win32::Foundation::HANDLE);

impl ProAudio {
    /// Joins the calling thread to the "Pro Audio" class, or `None` where
    /// that is not possible (the scheduler is off, or not Windows).
    pub fn join() -> Option<Self> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
            let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
            let mut index = 0;
            // SAFETY: `task` is a NUL-terminated UTF-16 string that outlives
            // the call; `index` is a valid out-pointer.
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
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this
        // thread (the guard is not Send) and is reverted once.
        unsafe { windows_sys::Win32::System::Threading::AvRevertMmThreadCharacteristics(self.0) };
    }
}
