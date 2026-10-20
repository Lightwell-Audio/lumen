//! CoreFoundation support for native VST3 bundles.
//!
//! CFBundle selects the executable named by `CFBundleExecutable`, including
//! the appropriate architecture slice, and supplies the `CFBundleRef` that
//! the VST3 `bundleEntry` contract requires.

use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use super::ModuleError;

type CFTypeRef = *const c_void;
type CFAllocatorRef = CFTypeRef;
type CFURLRef = *mut c_void;
type CFStringRef = *const c_void;
type CFBundleRef = *mut c_void;
type CFErrorRef = *mut c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFAllocatorRef,
        buffer: *const u8,
        buf_len: isize,
        is_directory: bool,
    ) -> CFURLRef;
    fn CFURLGetFileSystemRepresentation(
        url: CFURLRef,
        resolve_against_base: bool,
        buffer: *mut u8,
        max_buf_len: isize,
    ) -> bool;
    fn CFBundleCreate(allocator: CFAllocatorRef, url: CFURLRef) -> CFBundleRef;
    fn CFBundleLoadExecutableAndReturnError(bundle: CFBundleRef, error: *mut CFErrorRef) -> bool;
    fn CFBundleCopyExecutableURL(bundle: CFBundleRef) -> CFURLRef;
    fn CFBundleGetFunctionPointerForName(bundle: CFBundleRef, name: CFStringRef) -> *mut c_void;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        c_string: *const c_char,
        encoding: u32,
    ) -> *mut c_void;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFErrorCopyDescription(error: CFErrorRef) -> CFStringRef;
    fn CFRelease(value: CFTypeRef);
}

const UTF8: u32 = 0x0800_0100;
const PATH_CAPACITY: usize = 32_768;

/// A process-lifetime bundle reference and its executable, both retained by
/// the caller for as long as any factory or plugin object can use them.
pub(super) struct Bundle {
    raw: NonNull<c_void>,
}

impl Bundle {
    pub(super) fn load(path: &Path) -> Result<Self, ModuleError> {
        let url = create_url(path)?;
        // SAFETY: `url` is a live file URL for the bundle path.
        let raw = unsafe { CFBundleCreate(std::ptr::null(), url) };
        // SAFETY: CreateFromFileSystemRepresentation returned a +1 reference.
        unsafe { CFRelease(url) };
        let raw = NonNull::new(raw).ok_or_else(|| {
            ModuleError::Load(format!(
                "CoreFoundation could not create a bundle for {}",
                path.display()
            ))
        })?;
        let mut error = std::ptr::null_mut();
        // SAFETY: `raw` is a live CFBundleRef; `error` is a valid out pointer.
        if !unsafe { CFBundleLoadExecutableAndReturnError(raw.as_ptr(), &mut error) } {
            let detail = if let Some(error) = NonNull::new(error) {
                // SAFETY: CFErrorCopyDescription returns a retained string or null.
                let description = unsafe { CFErrorCopyDescription(error.as_ptr()) };
                // SAFETY: the load API returned a retained error object.
                unsafe { CFRelease(error.as_ptr()) };
                if description.is_null() {
                    "CoreFoundation returned no load description".to_owned()
                } else {
                    let mut buffer = vec![0; 2048];
                    // SAFETY: the CFString and writable NUL-terminated buffer are valid.
                    let ok = unsafe {
                        CFStringGetCString(
                            description,
                            buffer.as_mut_ptr().cast(),
                            buffer.len() as isize,
                            UTF8,
                        )
                    };
                    // SAFETY: CFErrorCopyDescription returned a retained string.
                    unsafe { CFRelease(description) };
                    if ok {
                        let end = buffer
                            .iter()
                            .position(|&byte| byte == 0)
                            .unwrap_or(buffer.len());
                        buffer.truncate(end);
                        String::from_utf8_lossy(&buffer).into_owned()
                    } else {
                        "CoreFoundation returned an unreadable load error".to_owned()
                    }
                }
            } else {
                "CoreFoundation supplied no load error".to_owned()
            };
            // Retain failed bundles too: loading may have run initializers,
            // and unloading plugin code is outside the host's policy.
            Box::leak(Box::new(Self { raw }));
            return Err(ModuleError::Load(format!(
                "CoreFoundation could not load the executable in {}: {detail}",
                path.display(),
            )));
        }
        Ok(Self { raw })
    }

    pub(super) fn call_entry(&self) -> Result<(), ModuleError> {
        let entry = self
            .function(b"bundleEntry\0")
            .ok_or(ModuleError::NoBundleEntry)?;
        // SAFETY: VST3 defines bundleEntry as bool bundleEntry(CFBundleRef).
        let entry = unsafe {
            std::mem::transmute::<*mut c_void, unsafe extern "system" fn(CFBundleRef) -> bool>(
                entry,
            )
        };
        // SAFETY: the bundle reference is valid and retained for process life.
        if unsafe { entry(self.raw.as_ptr()) } {
            Ok(())
        } else {
            Err(ModuleError::EntryFailed)
        }
    }

    pub(super) fn function(&self, symbol: &[u8]) -> Option<*mut c_void> {
        let name = create_string(symbol)?;
        // SAFETY: `name` is a live CFStringRef; the bundle is still retained.
        let function = unsafe { CFBundleGetFunctionPointerForName(self.raw.as_ptr(), name) };
        // SAFETY: CreateWithCString returned a +1 reference.
        unsafe { CFRelease(name) };
        NonNull::new(function).map(NonNull::as_ptr)
    }
}

/// Resolve the executable selected by the bundle metadata without loading
/// plugin code. This is used by discovery and accepts only real bundles.
pub(super) fn executable_path(path: &Path) -> Result<PathBuf, String> {
    let url = create_url(path).map_err(|e| e.to_string())?;
    // SAFETY: `url` is a live file URL for the bundle path.
    let bundle = unsafe { CFBundleCreate(std::ptr::null(), url) };
    // SAFETY: CreateFromFileSystemRepresentation returned a +1 reference.
    unsafe { CFRelease(url) };
    let Some(bundle) = NonNull::new(bundle) else {
        return Err("CoreFoundation rejected the bundle".into());
    };
    // SAFETY: the bundle reference is live; CopyExecutableURL returns +1 or null.
    let executable = unsafe { CFBundleCopyExecutableURL(bundle.as_ptr()) };
    // SAFETY: Create returned a +1 reference; release our local reference.
    unsafe { CFRelease(bundle.as_ptr()) };
    let Some(executable) = NonNull::new(executable) else {
        return Err("bundle has no executable named by Info.plist".into());
    };
    let mut bytes = vec![0; PATH_CAPACITY];
    // SAFETY: `executable` is a live URL and `bytes` is writable storage.
    let ok = unsafe {
        CFURLGetFileSystemRepresentation(
            executable.as_ptr(),
            true,
            bytes.as_mut_ptr(),
            bytes.len() as isize,
        )
    };
    // SAFETY: CopyExecutableURL returned a +1 reference.
    unsafe { CFRelease(executable.as_ptr()) };
    if !ok {
        return Err("bundle executable path is not representable".into());
    }
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    bytes.truncate(end);
    use std::os::unix::ffi::OsStringExt;
    let executable = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if !executable.is_file() {
        return Err("bundle executable does not exist".into());
    }
    Ok(executable)
}

fn create_url(path: &Path) -> Result<CFURLRef, ModuleError> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid filesystem path buffer for the duration of the call.
    NonNull::new(unsafe {
        CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            true,
        )
    })
    .map(NonNull::as_ptr)
    .ok_or_else(|| ModuleError::Load(format!("could not form a URL for {}", path.display())))
}

fn create_string(c_string: &[u8]) -> Option<CFStringRef> {
    if c_string.last() != Some(&0) {
        return None;
    }
    // SAFETY: input is NUL-terminated UTF-8 and has static lifetime at call sites.
    NonNull::new(unsafe {
        CFStringCreateWithCString(std::ptr::null(), c_string.as_ptr().cast(), UTF8)
    })
    .map(|string| string.as_ptr() as CFStringRef)
}
