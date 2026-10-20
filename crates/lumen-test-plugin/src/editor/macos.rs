//! Small native VST3 view used to exercise macOS NSView attachment,
//! size changes, detach, and content-scale callbacks.

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::ptr::null_mut;

use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeNSView, kResultFalse, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use super::{BASE_HEIGHT, BASE_WIDTH};
use crate::faults::Faults;

pub(super) fn create(_faults: &Faults) -> *mut IPlugView {
    let view = ComWrapper::new(Editor {
        native: RefCell::new(None),
        text_field: RefCell::new(None),
        frame: RefCell::new(None),
        size: RefCell::new((BASE_WIDTH, BASE_HEIGHT)),
        scale: RefCell::new(1.0),
        self_ptr: Cell::new(0),
    });
    let Some(ptr) = view.to_com_ptr::<IPlugView>() else {
        return null_mut();
    };
    view.self_ptr.set(ptr.as_ptr() as usize);
    ptr.into_raw()
}

struct Editor {
    native: RefCell<Option<Retained<NSView>>>,
    text_field: RefCell<Option<Retained<NSTextField>>>,
    frame: RefCell<Option<ComPtr<IPlugFrame>>>,
    size: RefCell<(i32, i32)>,
    scale: RefCell<f32>,
    self_ptr: Cell<usize>,
}

impl Class for Editor {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

impl IPlugViewTrait for Editor {
    unsafe fn isPlatformTypeSupported(&self, platform: *const c_char) -> tresult {
        if platform.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: VST3 supplies a NUL-terminated platform type string.
        let (provided, supported) = unsafe {
            (
                std::ffi::CStr::from_ptr(platform),
                std::ffi::CStr::from_ptr(kPlatformTypeNSView),
            )
        };
        if provided == supported {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, platform: *const c_char) -> tresult {
        // SAFETY: the platform string is supplied by the host per the VST3 ABI.
        if parent.is_null() || unsafe { self.isPlatformTypeSupported(platform) } != kResultTrue {
            return kInvalidArgument;
        }
        if self.native.borrow().is_some() {
            return kResultFalse;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return kResultFalse;
        };
        let native = NSView::new(mtm);
        let (width, height) = *self.size.borrow();
        native.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width as f64, height as f64),
        ));
        let field = NSTextField::new(mtm);
        field.setPlaceholderString(Some(&NSString::from_str("Type here to test editor focus")));
        field.setEditable(true);
        field.setSelectable(true);
        field.setBezeled(true);
        field.setFrame(NSRect::new(
            NSPoint::new(16.0, 16.0),
            NSSize::new((f64::from(width) - 32.0).max(1.0), 28.0),
        ));
        native.addSubview(&field);
        // SAFETY: the host passed its live NSView on this AppKit thread.
        let parent = unsafe { &*(parent as *const NSView) };
        parent.addSubview(&native);
        *self.native.borrow_mut() = Some(native);
        *self.text_field.borrow_mut() = Some(field);
        let frame = self.frame.borrow().clone();
        if let Some(frame) = frame {
            let mut requested = ViewRect {
                left: 0,
                top: 0,
                right: BASE_WIDTH + 40,
                bottom: BASE_HEIGHT + 30,
            };
            // SAFETY: the host's frame is live and the request uses this view's COM identity.
            unsafe {
                frame.resizeView(self.self_ptr.get() as *mut IPlugView, &mut requested);
            }
        }
        kResultOk
    }

    unsafe fn removed(&self) -> tresult {
        if let Some(native) = self.native.borrow_mut().take() {
            native.removeFromSuperview();
        }
        self.text_field.borrow_mut().take();
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        let (width, height) = *self.size.borrow();
        // SAFETY: the host passes a valid output pointer.
        unsafe {
            *size = ViewRect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
        }
        kResultOk
    }

    unsafe fn onSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid rectangle.
        let rect = unsafe { &*size };
        let size = clamp(rect.right - rect.left, rect.bottom - rect.top);
        *self.size.borrow_mut() = size;
        if let Some(native) = self.native.borrow().as_ref() {
            native.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(size.0 as f64, size.1 as f64),
            ));
        }
        if let Some(field) = self.text_field.borrow().as_ref() {
            field.setFrame(NSRect::new(
                NSPoint::new(16.0, 16.0),
                NSSize::new((f64::from(size.0) - 32.0).max(1.0), 28.0),
            ));
        }
        kResultOk
    }

    unsafe fn onFocus(&self, _state: u8) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        // SAFETY: the host gives a live frame or null.
        let frame = unsafe { ComRef::from_raw(frame) }.map(|r| r.to_com_ptr());
        *self.frame.borrow_mut() = frame;
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }
    unsafe fn checkSizeConstraint(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid in/out rectangle.
        let rect = unsafe { &mut *size };
        let (width, height) = clamp(rect.right - rect.left, rect.bottom - rect.top);
        rect.right = rect.left + width;
        rect.bottom = rect.top + height;
        kResultOk
    }
}

impl IPlugViewContentScaleSupportTrait for Editor {
    unsafe fn setContentScaleFactor(&self, scale: f32) -> tresult {
        if !scale.is_finite() || scale <= 0.0 {
            return kInvalidArgument;
        }
        *self.scale.borrow_mut() = scale;
        kResultOk
    }
}

fn clamp(width: i32, height: i32) -> (i32, i32) {
    (width.clamp(240, 1600), height.clamp(160, 1200))
}
