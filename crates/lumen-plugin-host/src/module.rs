use std::ffi::{c_char, c_void};
use std::fmt;
use std::path::{Path, PathBuf};

use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, PFactoryInfo, kResultOk,
};

use crate::protocol::{ClassId, ClassInfo, MAX_CLASSES};
use crate::scan::{ScanClass, ScanReport};

#[cfg(target_os = "macos")]
#[path = "module/macos.rs"]
mod macos;

#[derive(Debug)]
pub enum ModuleError {
    NotFound(PathBuf),
    Load(String),
    NoFactory,
    EmptyFactory,
    NoBundleEntry,
    EntryFailed,
    Unsupported(&'static str),
}

fn load_error(e: libloading::Error) -> ModuleError {
    let mut text = e.to_string();
    if let Some(why) = std::error::Error::source(&e) {
        text = format!("{text} ({why})");
    }
    ModuleError::Load(text)
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleError::NotFound(p) => write!(f, "no module at {}", p.display()),
            ModuleError::Load(e) => write!(f, "could not load the module: {e}"),
            ModuleError::NoFactory => write!(f, "the library has no GetPluginFactory export"),
            ModuleError::EmptyFactory => write!(f, "GetPluginFactory returned no factory"),
            ModuleError::NoBundleEntry => write!(f, "the bundle has no bundleEntry export"),
            ModuleError::EntryFailed => write!(f, "the module's entry point reported failure"),
            ModuleError::Unsupported(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for ModuleError {}

pub struct Module {
    pub(crate) factory: ComPtr<IPluginFactory>,
    #[cfg(not(target_os = "macos"))]
    _library: &'static libloading::Library,
    #[cfg(target_os = "macos")]
    _library: Option<&'static libloading::Library>,
    #[cfg(target_os = "macos")]
    _bundle: Option<&'static macos::Bundle>,
}

pub fn binary_path(path: &Path) -> Result<PathBuf, ModuleError> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if path.is_dir() {
        let (arch, extension) = if cfg!(windows) {
            ("x86_64-win", "vst3")
        } else if cfg!(target_os = "linux") {
            ("x86_64-linux", "so")
        } else {
            #[cfg(target_os = "macos")]
            {
                return macos::executable_path(path)
                    .map_err(|_| ModuleError::NotFound(path.to_path_buf()));
            }
            #[cfg(not(target_os = "macos"))]
            {
                ("MacOS", "")
            }
        };
        let inside = path.join("Contents").join(arch);
        if let Ok(entries) = std::fs::read_dir(&inside) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && (extension.is_empty() || p.extension().is_some_and(|e| e == extension))
                })
                .collect();
            found.sort();
            if let Some(binary) = found.into_iter().next() {
                return Ok(binary);
            }
        }
    }
    Err(ModuleError::NotFound(path.to_path_buf()))
}

fn open(binary: &Path) -> Result<(libloading::Library, *mut c_void), ModuleError> {
    #[cfg(windows)]
    {
        use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
        unsafe { Library::load_with_flags(binary, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .map(|library| (library.into(), std::ptr::null_mut()))
            .map_err(load_error)
    }
    #[cfg(unix)]
    {
        use libloading::os::unix::Library;
        let library = unsafe { Library::new(binary) }.map_err(load_error)?;
        let handle = library.into_raw();
        let library = unsafe { Library::from_raw(handle) };
        Ok((library.into(), handle))
    }
}

fn text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Module {
    pub fn load(path: &Path) -> Result<Self, ModuleError> {
        let binary = binary_path(path)?;
        #[cfg(target_os = "macos")]
        {
            if path.is_dir() {
                let bundle = Box::leak(Box::new(macos::Bundle::load(path)?));
                bundle.call_entry()?;
                let get_factory = bundle
                    .function(b"GetPluginFactory\0")
                    .ok_or(ModuleError::NoFactory)?;
                let get_factory = unsafe {
                    std::mem::transmute::<
                        *mut c_void,
                        unsafe extern "system" fn() -> *mut IPluginFactory,
                    >(get_factory)
                };
                let factory =
                    unsafe { ComPtr::from_raw(get_factory()) }.ok_or(ModuleError::EmptyFactory)?;
                return Ok(Self {
                    factory,
                    _library: None,
                    _bundle: Some(bundle),
                });
            }
            let (library, _) = open(&binary)?;
            let library: &'static libloading::Library = Box::leak(Box::new(library));
            let get_factory = unsafe {
                library.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(
                    b"GetPluginFactory\0",
                )
            }
            .map_err(|_| ModuleError::NoFactory)?;
            let factory =
                unsafe { ComPtr::from_raw(get_factory()) }.ok_or(ModuleError::EmptyFactory)?;
            Ok(Self {
                factory,
                _library: Some(library),
                _bundle: None,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let (library, handle) = open(&binary)?;
            let library: &'static libloading::Library = Box::leak(Box::new(library));

            #[cfg(windows)]
            {
                let init =
                    unsafe { library.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") };
                if let Ok(init) = init {
                    if !unsafe { init() } {
                        return Err(ModuleError::EntryFailed);
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(entry) = unsafe {
                    library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"ModuleEntry\0")
                } {
                    if !unsafe { entry(handle) } {
                        return Err(ModuleError::EntryFailed);
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            let _ = handle;
            let get_factory = unsafe {
                library.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(
                    b"GetPluginFactory\0",
                )
            }
            .map_err(|_| ModuleError::NoFactory)?;
            let factory =
                unsafe { ComPtr::from_raw(get_factory()) }.ok_or(ModuleError::EmptyFactory)?;
            Ok(Self {
                factory,
                _library: library,
            })
        }
    }

    pub fn classes(&self) -> Vec<ClassInfo> {
        unsafe {
            let count = (self.factory.countClasses().max(0) as usize).min(MAX_CLASSES);
            let second = self.factory.cast::<IPluginFactory2>();
            (0..count as i32)
                .filter_map(|i| {
                    if let Some(f2) = &second {
                        let mut info: PClassInfo2 = std::mem::zeroed();
                        if f2.getClassInfo2(i, &mut info) == kResultOk {
                            return Some(ClassInfo {
                                id: tuid(&info.cid),
                                name: text(&info.name),
                                category: text(&info.category),
                                vendor: text(&info.vendor),
                                subcategories: text(&info.subCategories),
                            });
                        }
                    }
                    let mut info: PClassInfo = std::mem::zeroed();
                    (self.factory.getClassInfo(i, &mut info) == kResultOk).then(|| ClassInfo {
                        id: tuid(&info.cid),
                        name: text(&info.name),
                        category: text(&info.category),
                        vendor: String::new(),
                        subcategories: String::new(),
                    })
                })
                .collect()
        }
    }
}

impl Module {
    pub fn scan_report(&self, load_ms: u32) -> ScanReport {
        unsafe {
            let mut info: PFactoryInfo = std::mem::zeroed();
            let (vendor, url) = if self.factory.getFactoryInfo(&mut info) == kResultOk {
                (text(&info.vendor), text(&info.url))
            } else {
                (String::new(), String::new())
            };
            let count = (self.factory.countClasses().max(0) as usize).min(MAX_CLASSES);
            let second = self.factory.cast::<IPluginFactory2>();
            let classes = (0..count as i32)
                .filter_map(|i| {
                    if let Some(f2) = &second {
                        let mut info: PClassInfo2 = std::mem::zeroed();
                        if f2.getClassInfo2(i, &mut info) == kResultOk {
                            return Some(ScanClass {
                                id: tuid(&info.cid),
                                name: text(&info.name),
                                category: text(&info.category),
                                vendor: text(&info.vendor),
                                subcategories: text(&info.subCategories),
                                version: text(&info.version),
                                sdk_version: text(&info.sdkVersion),
                            });
                        }
                    }
                    let mut info: PClassInfo = std::mem::zeroed();
                    (self.factory.getClassInfo(i, &mut info) == kResultOk).then(|| ScanClass {
                        id: tuid(&info.cid),
                        name: text(&info.name),
                        category: text(&info.category),
                        vendor: String::new(),
                        subcategories: String::new(),
                        version: String::new(),
                        sdk_version: String::new(),
                    })
                })
                .collect();
            ScanReport {
                vendor,
                url,
                classes,
                load_ms,
            }
        }
    }
}

fn tuid(cid: &[c_char; 16]) -> ClassId {
    cid.map(|c| c as u8)
}

pub(crate) fn to_tuid(id: &ClassId) -> [c_char; 16] {
    id.map(|b| b as c_char)
}

pub const AUDIO_CLASS: &str = "Audio Module Class";
