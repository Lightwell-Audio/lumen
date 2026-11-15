use super::*;

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
        unsafe {
            for (i, unit) in "Lumen".encode_utf16().chain(Some(0)).enumerate() {
                (*name)[i] = unit;
            }
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if obj.is_null() || cid.is_null() || iid.is_null() {
            return kInvalidArgument;
        }
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
                unsafe { *obj = raw };
                kResultOk
            }
            None => kNotImplemented,
        }
    }
}

enum Attribute {
    Int(i64),
    Float(f64),
    Text(Vec<u16>),
    Binary(Vec<u8>),
}

#[derive(Default)]
pub struct HostAttributes {
    values: Mutex<HashMap<String, Attribute>>,
}

impl HostAttributes {
    fn set(&self, id: AttrID, value: Attribute) -> tresult {
        if id.is_null() {
            return kInvalidArgument;
        }
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
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    #[allow(
        clippy::unnecessary_cast,
        reason = "`TChar` is `u16` on Windows and a narrower character type elsewhere, so the cast one platform needs is a no-op on the other"
    )]
    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut units = Vec::new();
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
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        match values.get(id.as_ref()) {
            Some(Attribute::Binary(bytes)) => {
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

pub struct HostMessage {
    id: Mutex<CString>,
    attributes: ComPtr<IAttributeList>,
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
        self.id.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
    }

    unsafe fn setMessageID(&self, id: *const c_char) {
        if id.is_null() {
            return;
        }
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
