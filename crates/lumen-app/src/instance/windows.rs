use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use super::protocol::{self, Reply, Request};
use super::{Deliver, IO_TIMEOUT, Place};

fn name(place: &Place) -> Vec<u16> {
    wide(&format!(
        r"\\.\pipe\{}-{}",
        lumen_core::product::PRODUCT_NAME,
        place.key
    ))
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

unsafe impl Send for Handle {}

pub(super) fn serve(place: &Place, deliver: Deliver) -> io::Result<()> {
    let security = Security::current_user()?;
    let name = name(place);
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            &security.attributes(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let pipe = Handle(pipe);
    std::thread::Builder::new()
        .name("lumen-instance".into())
        .spawn(move || listen(pipe, deliver))?;
    Ok(())
}

fn listen(pipe: Handle, deliver: Deliver) {
    loop {
        let connected = overlapped(pipe.0, None, |ov| unsafe { ConnectNamedPipe(pipe.0, ov) });
        if let Err(e) = connected {
            crate::trace::log(|| format!("instance: the pipe failed ({e})"));
            return;
        }
        let mut conn = PipeIo {
            handle: pipe.0,
            timeout: IO_TIMEOUT,
        };
        let deadline = Instant::now() + IO_TIMEOUT * 2;
        if let Err(e) = protocol::serve(&mut conn, &*deliver, deadline) {
            crate::trace::log(|| format!("instance: a request failed ({e})"));
        }
        let mut rest = [0u8; 64];
        let _ = conn.read(&mut rest);
        unsafe { DisconnectNamedPipe(pipe.0) };
    }
}

pub(super) fn send(place: &Place, request: &Request, left: Duration) -> io::Result<Reply> {
    let name = name(place);
    let deadline = Instant::now() + left;
    let pipe = loop {
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break Handle(handle);
        }
        if unsafe { GetLastError() } != ERROR_PIPE_BUSY || Instant::now() >= deadline {
            return Err(io::Error::last_os_error());
        }
        unsafe { WaitNamedPipeW(name.as_ptr(), 500) };
    };
    let mut pid = 0u32;
    if unsafe { GetNamedPipeServerProcessId(pipe.0, &mut pid) } != 0 {
        unsafe { AllowSetForegroundWindow(pid) };
    }
    let mut conn = PipeIo {
        handle: pipe.0,
        timeout: deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(100)),
    };
    protocol::ask(&mut conn, request, deadline)
}

struct PipeIo {
    handle: HANDLE,
    timeout: Duration,
}

impl Read for PipeIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize) as u32;
        let ptr = buf.as_mut_ptr();
        let handle = self.handle;
        let read = overlapped(handle, Some(self.timeout), |ov| unsafe {
            ReadFile(handle, ptr, len, std::ptr::null_mut(), ov)
        });
        match read {
            Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            other => other.map(|n| n as usize),
        }
    }
}

impl Write for PipeIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize) as u32;
        let ptr = buf.as_ptr();
        let handle = self.handle;
        overlapped(handle, Some(self.timeout), |ov| unsafe {
            WriteFile(handle, ptr, len, std::ptr::null_mut(), ov)
        })
        .map(|n| n as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn overlapped(
    handle: HANDLE,
    timeout: Option<Duration>,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<u32> {
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    let event = Handle(event);
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event.0;
    if start(&mut ov) == 0 {
        match unsafe { GetLastError() } {
            ERROR_IO_PENDING => {}
            ERROR_PIPE_CONNECTED => return Ok(0),
            _ => return Err(io::Error::last_os_error()),
        }
    }
    let ms = timeout.map_or(INFINITE, |t| {
        t.as_millis().min(u128::from(INFINITE - 1)) as u32
    });
    let waited = unsafe { WaitForSingleObject(event.0, ms) };
    let mut n = 0u32;
    if waited == WAIT_OBJECT_0 {
        if unsafe { GetOverlappedResult(handle, &ov, &mut n, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(n);
    }
    unsafe {
        CancelIoEx(handle, &ov);
        GetOverlappedResult(handle, &ov, &mut n, 1);
    }
    if waited == WAIT_TIMEOUT {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Err(io::Error::last_os_error())
    }
}

struct Security(PSECURITY_DESCRIPTOR);

impl Security {
    fn current_user() -> io::Result<Self> {
        let sid = current_user_sid()?;
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);
    let mut len = 0u32;
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    if unsafe { GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    let mut text: *mut u16 = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    };
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}
