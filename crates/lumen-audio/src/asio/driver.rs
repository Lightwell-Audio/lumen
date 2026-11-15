use std::ffi::{c_char, c_void};
use std::ptr;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED, CoCreateInstance,
    CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WS_OVERLAPPEDWINDOW,
};
use windows_sys::core::GUID;

use super::ffi::{self, AsioChannelInfo, AsioError, IAsio};
use super::wide;

pub struct ComApartment;

impl ComApartment {
    pub fn enter() -> Result<Self, String> {
        let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err(format!("CoInitializeEx failed: 0x{hr:08x}"));
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub struct HiddenWindow(pub HWND);

impl HiddenWindow {
    pub fn create() -> Result<Self, String> {
        let class = wide("STATIC");
        let title = wide("ASIO driver host");
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("CreateWindowExW failed".into());
        }
        Ok(Self(hwnd))
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

pub fn pump_messages() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BufferSizes {
    pub min: i32,
    pub max: i32,
    pub preferred: i32,
    pub granularity: i32,
}

pub struct Driver {
    raw: *mut IAsio,
}

macro_rules! call {
    ($driver:expr, $method:ident $(, $arg:expr)*) => {{
        let raw = $driver.raw;
        unsafe { ((*(*raw).vtbl).$method)(raw $(, $arg)*) }
    }};
}

impl Driver {
    pub fn create(clsid: &str) -> Result<Self, String> {
        let text = wide(clsid);
        let mut guid: GUID = GUID::from_u128(0);
        let hr = unsafe { CLSIDFromString(text.as_ptr(), &mut guid) };
        if hr < 0 {
            return Err(format!("invalid CLSID {clsid}: 0x{hr:08x}"));
        }
        let mut object: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &guid,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &guid,
                &mut object,
            )
        };
        if hr < 0 || object.is_null() {
            return Err(format!("CoCreateInstance failed: 0x{hr:08x}"));
        }
        Ok(Self { raw: object.cast() })
    }

    pub fn raw(&self) -> *mut IAsio {
        self.raw
    }

    pub fn init(&self, window: HWND) -> Result<(), String> {
        if call!(self, init, window) == ffi::ASIO_TRUE {
            Ok(())
        } else {
            Err(self.error_message())
        }
    }

    pub fn error_message(&self) -> String {
        let mut buf = [0 as c_char; 256];
        call!(self, get_error_message, buf.as_mut_ptr());
        ffi::c_string(&buf)
    }

    pub fn channels(&self) -> Result<(i32, i32), AsioError> {
        let (mut inputs, mut outputs) = (0, 0);
        check(call!(self, get_channels, &mut inputs, &mut outputs))?;
        Ok((inputs, outputs))
    }

    pub fn latencies(&self) -> Result<(i32, i32), AsioError> {
        let (mut input, mut output) = (0, 0);
        check(call!(self, get_latencies, &mut input, &mut output))?;
        Ok((input, output))
    }

    pub fn buffer_sizes(&self) -> Result<BufferSizes, AsioError> {
        let (mut min, mut max, mut preferred, mut granularity) = (0, 0, 0, 0);
        check(call!(
            self,
            get_buffer_size,
            &mut min,
            &mut max,
            &mut preferred,
            &mut granularity
        ))?;
        Ok(BufferSizes {
            min,
            max,
            preferred,
            granularity,
        })
    }

    pub fn sample_rate(&self) -> Result<f64, AsioError> {
        let mut rate = 0.0;
        check(call!(self, get_sample_rate, &mut rate))?;
        Ok(rate)
    }

    pub fn set_sample_rate(&self, rate: f64) -> Result<(), AsioError> {
        check(call!(self, set_sample_rate, rate))
    }

    pub fn can_sample_rate(&self, rate: f64) -> bool {
        call!(self, can_sample_rate, rate) == ffi::ASE_OK
    }

    pub fn channel_info(&self, channel: i32, input: bool) -> Result<AsioChannelInfo, AsioError> {
        let mut info = AsioChannelInfo {
            channel,
            is_input: i32::from(input),
            is_active: 0,
            channel_group: 0,
            sample_type: 0,
            name: [0; 32],
        };
        check(call!(self, get_channel_info, &mut info))?;
        Ok(info)
    }

    pub fn start(&self) -> Result<(), AsioError> {
        check(call!(self, start))
    }

    pub fn control_panel(&self) -> Result<(), AsioError> {
        check(call!(self, control_panel))
    }

    pub fn stop(&self) -> Result<(), AsioError> {
        check(call!(self, stop))
    }

    pub fn dispose_buffers(&self) -> Result<(), AsioError> {
        check(call!(self, dispose_buffers))
    }

    pub fn output_ready(&self) -> AsioError {
        call!(self, output_ready)
    }

    pub fn create_buffers(
        &self,
        infos: &mut [ffi::AsioBufferInfo],
        frames: i32,
        callbacks: &'static ffi::AsioCallbacks,
    ) -> Result<(), AsioError> {
        check(call!(
            self,
            create_buffers,
            infos.as_mut_ptr(),
            infos.len() as i32,
            frames,
            callbacks
        ))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        call!(self, release);
    }
}

pub fn check(error: AsioError) -> Result<(), AsioError> {
    match error {
        ffi::ASE_OK | ffi::ASE_SUCCESS => Ok(()),
        other => Err(other),
    }
}
