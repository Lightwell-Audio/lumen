//! The COM objects a host gives a plugin: the host context, the handler that
//! hears about edits and restarts, event lists, parameter changes and a
//! memory stream for state. Built on the `vst3` crate's `ComWrapper`; the
//! methods take `&self`, so what changes lives behind a lock or an atomic.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::Vst::{
    Event, IAttributeList, IAttributeListTrait, IComponentHandler, IComponentHandler2,
    IComponentHandler2Trait, IComponentHandlerTrait, IEventList, IEventListTrait, IHostApplication,
    IHostApplicationTrait, IMessage, IMessageTrait, IParamValueQueue, IParamValueQueueTrait,
    IParameterChanges, IParameterChangesTrait, ParamID, ParamValue, String128, TChar,
};
use vst3::Steinberg::{
    FIDString, IBStream, IBStreamTrait, TBool, TUID, int32, int64, kInvalidArgument,
    kNotImplemented, kResultFalse, kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper, Interface};

use crate::protocol::{Edit, EditKind, MAX_EDITS, ParamQueue};

mod events;
mod handler;
mod host;
mod stream;

#[cfg(test)]
mod tests;

pub use events::*;
pub use handler::*;
pub use host::*;
pub use stream::*;
