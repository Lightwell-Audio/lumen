//! Clean-room description of the ASIO 2 driver ABI on 64-bit Windows.
//!
//! Written from the publicly documented interface (vtable order, struct
//! layouts, constants), without the Steinberg SDK. Shipping this needs the
//! licensing decision in ADR-0006. x86-64 only: on 32-bit Windows the driver
//! methods would use `thiscall`.

use std::ffi::{c_char, c_void};

use windows_sys::core::{GUID, HRESULT};

pub type AsioBool = i32;
pub type AsioError = i32;
pub type AsioSampleType = i32;

pub const ASIO_TRUE: AsioBool = 1;

pub const ASE_OK: AsioError = 0;
pub const ASE_SUCCESS: AsioError = 0x3f48_47a0;
pub const ASE_NOT_PRESENT: AsioError = -1000;
pub const ASE_HW_MALFUNCTION: AsioError = -999;
pub const ASE_INVALID_PARAMETER: AsioError = -998;
pub const ASE_INVALID_MODE: AsioError = -997;
pub const ASE_SP_NOT_ADVANCING: AsioError = -996;
pub const ASE_NO_CLOCK: AsioError = -995;
pub const ASE_NO_MEMORY: AsioError = -994;

// asioMessage selectors.
pub const K_ASIO_SELECTOR_SUPPORTED: i32 = 1;
pub const K_ASIO_ENGINE_VERSION: i32 = 2;
pub const K_ASIO_RESET_REQUEST: i32 = 3;
#[allow(dead_code)] // part of the ABI; not handled yet
pub const K_ASIO_BUFFER_SIZE_CHANGE: i32 = 4;
pub const K_ASIO_RESYNC_REQUEST: i32 = 5;
pub const K_ASIO_LATENCIES_CHANGED: i32 = 6;
pub const K_ASIO_SUPPORTS_TIME_INFO: i32 = 7;
#[allow(dead_code)] // part of the ABI; not handled yet
pub const K_ASIO_SUPPORTS_TIME_CODE: i32 = 8;
#[allow(dead_code)] // part of the ABI; not handled yet
pub const K_ASIO_OVERLOAD: i32 = 15;

// AsioTimeInfo flags.
pub const K_SAMPLE_POSITION_VALID: u32 = 1 << 1;

// Sample types (little-endian ones are what Windows drivers use).
pub const ST_INT16_LSB: AsioSampleType = 16;
pub const ST_INT24_LSB: AsioSampleType = 17;
pub const ST_INT32_LSB: AsioSampleType = 18;
pub const ST_FLOAT32_LSB: AsioSampleType = 19;
pub const ST_FLOAT64_LSB: AsioSampleType = 20;
pub const ST_INT32_LSB16: AsioSampleType = 24;
pub const ST_INT32_LSB18: AsioSampleType = 25;
pub const ST_INT32_LSB20: AsioSampleType = 26;
pub const ST_INT32_LSB24: AsioSampleType = 27;

pub fn sample_type_name(t: AsioSampleType) -> &'static str {
    match t {
        0 => "Int16MSB",
        1 => "Int24MSB",
        2 => "Int32MSB",
        3 => "Float32MSB",
        4 => "Float64MSB",
        ST_INT16_LSB => "Int16LSB",
        ST_INT24_LSB => "Int24LSB",
        ST_INT32_LSB => "Int32LSB",
        ST_FLOAT32_LSB => "Float32LSB",
        ST_FLOAT64_LSB => "Float64LSB",
        ST_INT32_LSB16 => "Int32LSB16",
        ST_INT32_LSB18 => "Int32LSB18",
        ST_INT32_LSB20 => "Int32LSB20",
        ST_INT32_LSB24 => "Int32LSB24",
        _ => "unknown",
    }
}

pub fn error_name(e: AsioError) -> &'static str {
    match e {
        ASE_OK => "ASE_OK",
        ASE_SUCCESS => "ASE_SUCCESS",
        ASE_NOT_PRESENT => "ASE_NotPresent",
        ASE_HW_MALFUNCTION => "ASE_HWMalfunction",
        ASE_INVALID_PARAMETER => "ASE_InvalidParameter",
        ASE_INVALID_MODE => "ASE_InvalidMode",
        ASE_SP_NOT_ADVANCING => "ASE_SPNotAdvancing",
        ASE_NO_CLOCK => "ASE_NoClock",
        ASE_NO_MEMORY => "ASE_NoMemory",
        _ => "unknown error",
    }
}

/// 64-bit value split into two 32-bit halves (Windows has no native int64 here).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AsioSamples {
    pub hi: u32,
    pub lo: u32,
}

impl AsioSamples {
    pub fn get(self) -> u64 {
        (u64::from(self.hi) << 32) | u64::from(self.lo)
    }
}

pub type AsioTimeStamp = AsioSamples;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AsioClockSource {
    pub index: i32,
    pub associated_channel: i32,
    pub associated_group: i32,
    pub is_current_source: AsioBool,
    pub name: [c_char; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AsioChannelInfo {
    pub channel: i32,
    pub is_input: AsioBool,
    pub is_active: AsioBool,
    pub channel_group: i32,
    pub sample_type: AsioSampleType,
    pub name: [c_char; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AsioBufferInfo {
    pub is_input: AsioBool,
    pub channel_num: i32,
    pub buffers: [*mut c_void; 2],
}

// The SDK forces 4-byte packing for these on Windows.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTimeInfo {
    pub speed: f64,
    pub system_time: AsioTimeStamp,
    pub sample_position: AsioSamples,
    pub sample_rate: f64,
    pub flags: u32,
    pub reserved: [c_char; 12],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTimeCode {
    pub speed: f64,
    pub time_code_samples: AsioSamples,
    pub flags: u32,
    pub future: [c_char; 64],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTime {
    pub reserved: [i32; 4],
    pub time_info: AsioTimeInfo,
    pub time_code: AsioTimeCode,
}

const _: () = assert!(size_of::<AsioClockSource>() == 48);
const _: () = assert!(size_of::<AsioChannelInfo>() == 52);
const _: () = assert!(size_of::<AsioBufferInfo>() == 24);
const _: () = assert!(size_of::<AsioTimeInfo>() == 48);
const _: () = assert!(size_of::<AsioTimeCode>() == 84);
const _: () = assert!(size_of::<AsioTime>() == 148);

#[repr(C)]
pub struct AsioCallbacks {
    pub buffer_switch: unsafe extern "C" fn(index: i32, direct_process: AsioBool),
    pub sample_rate_did_change: unsafe extern "C" fn(rate: f64),
    pub asio_message:
        unsafe extern "C" fn(selector: i32, value: i32, message: *mut c_void, opt: *mut f64) -> i32,
    pub buffer_switch_time_info: unsafe extern "C" fn(
        params: *mut AsioTime,
        index: i32,
        direct_process: AsioBool,
    ) -> *mut AsioTime,
}

/// The driver object: a COM object whose vtable extends `IUnknown`.
#[repr(C)]
pub struct IAsio {
    pub vtbl: *const IAsioVtbl,
}

#[repr(C)]
pub struct IAsioVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut IAsio, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut IAsio) -> u32,
    pub release: unsafe extern "system" fn(*mut IAsio) -> u32,
    pub init: unsafe extern "system" fn(*mut IAsio, sys_handle: *mut c_void) -> AsioBool,
    pub get_driver_name: unsafe extern "system" fn(*mut IAsio, name: *mut c_char),
    pub get_driver_version: unsafe extern "system" fn(*mut IAsio) -> i32,
    pub get_error_message: unsafe extern "system" fn(*mut IAsio, text: *mut c_char),
    pub start: unsafe extern "system" fn(*mut IAsio) -> AsioError,
    pub stop: unsafe extern "system" fn(*mut IAsio) -> AsioError,
    pub get_channels: unsafe extern "system" fn(*mut IAsio, *mut i32, *mut i32) -> AsioError,
    pub get_latencies: unsafe extern "system" fn(*mut IAsio, *mut i32, *mut i32) -> AsioError,
    pub get_buffer_size:
        unsafe extern "system" fn(*mut IAsio, *mut i32, *mut i32, *mut i32, *mut i32) -> AsioError,
    pub can_sample_rate: unsafe extern "system" fn(*mut IAsio, f64) -> AsioError,
    pub get_sample_rate: unsafe extern "system" fn(*mut IAsio, *mut f64) -> AsioError,
    pub set_sample_rate: unsafe extern "system" fn(*mut IAsio, f64) -> AsioError,
    pub get_clock_sources:
        unsafe extern "system" fn(*mut IAsio, *mut AsioClockSource, *mut i32) -> AsioError,
    pub set_clock_source: unsafe extern "system" fn(*mut IAsio, i32) -> AsioError,
    pub get_sample_position:
        unsafe extern "system" fn(*mut IAsio, *mut AsioSamples, *mut AsioTimeStamp) -> AsioError,
    pub get_channel_info: unsafe extern "system" fn(*mut IAsio, *mut AsioChannelInfo) -> AsioError,
    pub create_buffers: unsafe extern "system" fn(
        *mut IAsio,
        *mut AsioBufferInfo,
        num_channels: i32,
        buffer_size: i32,
        callbacks: *const AsioCallbacks,
    ) -> AsioError,
    pub dispose_buffers: unsafe extern "system" fn(*mut IAsio) -> AsioError,
    pub control_panel: unsafe extern "system" fn(*mut IAsio) -> AsioError,
    pub future: unsafe extern "system" fn(*mut IAsio, i32, *mut c_void) -> AsioError,
    pub output_ready: unsafe extern "system" fn(*mut IAsio) -> AsioError,
}

/// Reads a NUL-terminated driver string, tolerating missing terminators.
pub fn c_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
