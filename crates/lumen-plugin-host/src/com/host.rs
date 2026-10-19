//! The host context a plugin is given: the application object, attribute
//! lists and messages.

use super::*;

/// An attribute's name in a list.
type AttrID = *const c_char;

pub struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the plugin passes a String128 buffer; "Lumen\0" fits.
        unsafe {
            for (i, unit) in "Lumen".encode_utf16().chain(Some(0)).enumerate() {
                (*name)[i] = unit;
            }
        }
        kResultOk
    }

    /// Plugins with a separate controller ask the host for a message and an
    /// attribute list to talk to it with, and not all of them look at what
    /// they get back. These two are the only objects a host has to make.
    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if obj.is_null() || cid.is_null() || iid.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: valid out pointer, and valid ids from the caller.
        let (cid, iid) = unsafe {
            *obj = std::ptr::null_mut();
            (*cid, *iid)
        };
        let same = |id: TUID, wanted: [u8; 16]| id.map(|c| c as u8) == wanted;
        let made: Option<*mut c_void> = if same(cid, IMessage::IID) && same(iid, IMessage::IID) {
            ComWrapper::new(HostMessage::new())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw().cast())
        } else if same(cid, IAttributeList::IID) && same(iid, IAttributeList::IID) {
            ComWrapper::new(HostAttributes::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw().cast())
        } else {
            None
        };
        match made {
            Some(raw) => {
                // SAFETY: the out pointer is valid; the plugin owns the
                // reference `into_raw` handed over.
                unsafe { *obj = raw };
                kResultOk
            }
            None => kNotImplemented,
        }
    }
}

/// A value in an attribute list.
enum Attribute {
    Int(i64),
    Float(f64),
    /// UTF-16 units, without the terminator.
    Text(Vec<u16>),
    Binary(Vec<u8>),
}

/// An attribute list: what a message carries.
#[derive(Default)]
pub struct HostAttributes {
    values: Mutex<HashMap<String, Attribute>>,
}

impl HostAttributes {
    fn set(&self, id: AttrID, value: Attribute) -> tresult {
        if id.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, value);
        kResultOk
    }

    fn get<R>(&self, id: AttrID, read: impl FnOnce(&Attribute) -> Option<R>) -> Option<R> {
        if id.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        values.get(id.as_ref()).and_then(read)
    }
}

impl Class for HostAttributes {
    type Interfaces = (IAttributeList,);
}

impl IAttributeListTrait for HostAttributes {
    unsafe fn setInt(&self, id: AttrID, value: i64) -> tresult {
        self.set(id, Attribute::Int(value))
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut i64) -> tresult {
        if value.is_null() {
            return kInvalidArgument;
        }
        match self.get(id, |a| match a {
            Attribute::Int(v) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                // SAFETY: a valid out pointer.
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attribute::Float(value))
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        if value.is_null() {
            return kInvalidArgument;
        }
        match self.get(id, |a| match a {
            Attribute::Float(v) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                // SAFETY: a valid out pointer.
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut units = Vec::new();
        // SAFETY: a NUL-terminated UTF-16 string from the plugin.
        unsafe {
            let mut at = string;
            while *at != 0 {
                units.push(*at as u16);
                at = at.add(1);
            }
        }
        self.set(id, Attribute::Text(units))
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: u32) -> tresult {
        if string.is_null() || size_in_bytes < 2 {
            return kInvalidArgument;
        }
        let Some(units) = self.get(id, |a| match a {
            Attribute::Text(t) => Some(t.clone()),
            _ => None,
        }) else {
            return kResultFalse;
        };
        let room = (size_in_bytes as usize / 2).saturating_sub(1);
        // SAFETY: the caller's buffer holds `size_in_bytes` bytes.
        unsafe {
            for (i, unit) in units.iter().take(room).enumerate() {
                *string.add(i) = *unit as TChar;
            }
            *string.add(units.len().min(room)) = 0;
        }
        kResultOk
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: u32) -> tresult {
        if data.is_null() && size_in_bytes != 0 {
            return kInvalidArgument;
        }
        let bytes = if size_in_bytes == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin vouches for `size_in_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size_in_bytes as usize) }
                .to_vec()
        };
        self.set(id, Attribute::Binary(bytes))
    }

    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut u32,
    ) -> tresult {
        if data.is_null() || size_in_bytes.is_null() || id.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        match values.get(id.as_ref()) {
            Some(Attribute::Binary(bytes)) => {
                // SAFETY: valid out pointers; the bytes live as long as the
                // list holds the value, which is how long the plugin may use
                // them.
                unsafe {
                    *data = bytes.as_ptr().cast();
                    *size_in_bytes = bytes.len() as u32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// A message: an id and an attribute list.
pub struct HostMessage {
    id: Mutex<CString>,
    attributes: ComPtr<IAttributeList>,
    // Keeps the list's object alive; the message hands out its pointer.
    _list: ComWrapper<HostAttributes>,
}

impl HostMessage {
    fn new() -> Self {
        let list = ComWrapper::new(HostAttributes::default());
        let attributes = list
            .to_com_ptr::<IAttributeList>()
            .expect("the list implements its interface");
        Self {
            id: Mutex::new(CString::default()),
            attributes,
            _list: list,
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> *const c_char {
        // The string lives in the message; the plugin reads it before the id
        // is set again, as with any host.
        self.id.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
    }

    unsafe fn setMessageID(&self, id: *const c_char) {
        if id.is_null() {
            return;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_owned();
        *self.id.lock().unwrap_or_else(|e| e.into_inner()) = id;
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attributes.as_ptr()
    }
}

pub fn host_application() -> ComPtr<IHostApplication> {
    ComWrapper::new(HostApplication)
        .to_com_ptr::<IHostApplication>()
        .expect("interface listed")
}
