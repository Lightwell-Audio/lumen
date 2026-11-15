use std::ffi::c_void;

use vst3::Steinberg::{
    FIDString, FUnknown, IPluginFactory, IPluginFactory2, IPluginFactory2Trait,
    IPluginFactoryTrait, PClassInfo, PClassInfo_, PClassInfo2, PFactoryInfo, PFactoryInfo_, TUID,
    int32, kInvalidArgument, kResultOk, tresult,
};
use vst3::{Class, ComWrapper, uid};

use crate::component::Processor;
use crate::controller::Controller;
use crate::faults::{self, Faults, Kind as Fault};
use crate::params::Kind;
use crate::text::copy_cstring;

pub const INSTRUMENT: TUID = uid(0x4C54_5001, 0x0000_0001, 0x4C55_4D45, 0x4E54_5354);
pub const INSTRUMENT_CONTROLLER: TUID = uid(0x4C54_5001, 0x0000_0002, 0x4C55_4D45, 0x4E54_5354);
pub const EFFECT: TUID = uid(0x4C54_5002, 0x0000_0001, 0x4C55_4D45, 0x4E54_5354);
pub const EFFECT_CONTROLLER: TUID = uid(0x4C54_5002, 0x0000_0002, 0x4C55_4D45, 0x4E54_5354);

const VENDOR: &str = "Test Vendor";

struct Entry {
    cid: TUID,
    category: &'static str,
    name: &'static str,
    sub_categories: &'static str,
}

const CLASSES: [Entry; 4] = [
    Entry {
        cid: INSTRUMENT,
        category: "Audio Module Class",
        name: "Deterministic Test Instrument",
        sub_categories: "Instrument|Synth",
    },
    Entry {
        cid: INSTRUMENT_CONTROLLER,
        category: "Component Controller Class",
        name: "Deterministic Test Instrument",
        sub_categories: "",
    },
    Entry {
        cid: EFFECT,
        category: "Audio Module Class",
        name: "Deterministic Test Effect",
        sub_categories: "Fx",
    },
    Entry {
        cid: EFFECT_CONTROLLER,
        category: "Component Controller Class",
        name: "Deterministic Test Effect",
        sub_categories: "",
    },
];

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring("", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some(class) = usize::try_from(index).ok().and_then(|i| CLASSES.get(i)) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = class.cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(class.category, &mut info.category);
        copy_cstring(class.name, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        let cid = unsafe { *cid.cast::<TUID>() };
        let faults = Faults::from_env();
        let instance = if cid == INSTRUMENT {
            component(Kind::Instrument, faults).to_com_ptr::<FUnknown>()
        } else if cid == EFFECT {
            component(Kind::Effect, faults).to_com_ptr::<FUnknown>()
        } else if cid == INSTRUMENT_CONTROLLER {
            controller(Kind::Instrument, faults).to_com_ptr::<FUnknown>()
        } else if cid == EFFECT_CONTROLLER {
            controller(Kind::Effect, faults).to_com_ptr::<FUnknown>()
        } else {
            None
        };
        let Some(instance) = instance else {
            return kInvalidArgument;
        };
        let raw = instance.as_ptr();
        unsafe { ((*(*raw).vtbl).queryInterface)(raw, iid.cast_mut().cast::<TUID>(), obj) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some(class) = usize::try_from(index).ok().and_then(|i| CLASSES.get(i)) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = class.cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        info.classFlags = 0;
        copy_cstring(class.category, &mut info.category);
        copy_cstring(class.name, &mut info.name);
        copy_cstring(class.sub_categories, &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring("1.0.0", &mut info.version);
        copy_cstring("VST 3.8.0", &mut info.sdkVersion);
        kResultOk
    }
}

pub fn component(kind: Kind, faults: Faults) -> ComWrapper<Processor> {
    let paired = match kind {
        Kind::Instrument => INSTRUMENT_CONTROLLER,
        Kind::Effect => EFFECT_CONTROLLER,
    };
    ComWrapper::new(Processor::new(kind, paired, faults))
}

pub fn controller(kind: Kind, faults: Faults) -> ComWrapper<Controller> {
    ComWrapper::new(Controller::new(kind, faults))
}

pub fn factory() -> vst3::ComPtr<IPluginFactory> {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .expect("Factory implements IPluginFactory")
}

pub fn plugin_factory() -> *mut IPluginFactory {
    let faults = Faults::from_env();
    if faults.has(Fault::FactoryCrash) {
        faults::crash();
    }
    if faults.has(Fault::FactoryHang) {
        faults::hang();
    }
    if faults.has(Fault::Prints) {
        println!("test plugin: stray output from the factory");
    }
    if faults.has(Fault::FactoryNull) {
        return std::ptr::null_mut();
    }
    factory().into_raw()
}
