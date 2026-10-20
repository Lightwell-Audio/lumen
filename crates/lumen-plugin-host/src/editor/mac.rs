//! Native VST3 editor windows on macOS. AppKit, the plugin and its view all
//! stay on the host's main thread; the view is attached to an NSView owned by
//! this child process, never to Lumen's window.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApp, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent, NSEventMask,
    NSEventType, NSFloatingWindowLevel, NSNormalWindowLevel, NSRunningApplication, NSTextView,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeNSView, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

use super::{Editors, Notify};
use crate::protocol::{EditorBounds, EditorStatus, MAX_EDITOR_SIDE, Response};
use crate::server::{Pump, Waker};

const EVENT_POLL: Duration = Duration::from_millis(8);

thread_local! {
    static EDITOR: RefCell<Option<OpenEditor>> = const { RefCell::new(None) };
    static LAST: RefCell<Option<EditorBounds>> = const { RefCell::new(None) };
}

struct OpenEditor {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    _frame: ComWrapper<FrameObject>,
    window: Retained<NSWindow>,
    content: Retained<NSView>,
    attached: bool,
    hidden: bool,
    floating: bool,
    last_view_size: (i32, i32),
    content_scale: f32,
    captured: Vec<u8>,
    captured_down: Vec<u8>,
    notify: Option<Notify>,
}

struct FrameObject;

impl Class for FrameObject {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for FrameObject {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        if size.is_null() || view.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a non-null rectangle supplied for the duration of this callback.
        let Some((width, height)) = rectangle_size(unsafe { &*size }) else {
            return kInvalidArgument;
        };
        let view_to_notify = EDITOR.with(|slot| {
            let mut slot = slot.borrow_mut();
            let editor = slot.as_mut()?;
            if editor.view.as_ptr() != view {
                return None;
            }
            let view = editor.view.clone();
            editor
                .window
                .setContentSize(NSSize::new(width as f64, height as f64));
            editor.content.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(width as f64, height as f64),
            ));
            editor.last_view_size = (width, height);
            Some((view, width, height))
        });
        if let Some((view, width, height)) = view_to_notify {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            // SAFETY: the view was cloned from the live editor on this thread.
            unsafe { view.onSize(&mut rect) };
            kResultTrue
        } else {
            kInvalidArgument
        }
    }
}

fn constrained_size(width: i32, height: i32) -> (i32, i32) {
    (
        width.clamp(64, MAX_EDITOR_SIDE as i32),
        height.clamp(64, MAX_EDITOR_SIDE as i32),
    )
}

fn rectangle_size(rect: &ViewRect) -> Option<(i32, i32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    (width > 0 && height > 0).then(|| constrained_size(width, height))
}

/// The editor operations are called only by the AppKit main thread.
#[derive(Default)]
pub struct MacEditors {
    captured: Vec<u8>,
    notify: Option<Notify>,
}

impl Editors for MacEditors {
    fn open(
        &mut self,
        title: &str,
        view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String> {
        let mtm = MainThreadMarker::new().ok_or("AppKit editor requested off the main thread")?;
        if EDITOR.with(|e| e.borrow().is_some()) {
            return Err("an editor is already open".into());
        }
        let view = view.ok_or("the plugin has no editor")?;
        // SAFETY: a live view owned by the plugin host's main thread.
        if unsafe { view.isPlatformTypeSupported(kPlatformTypeNSView) } != kResultTrue {
            return Err("the plugin editor does not support NSView".into());
        }

        let app = NSApp(mtm);
        if app.activationPolicy() != NSApplicationActivationPolicy::Accessory
            && !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory)
        {
            return Err("AppKit could not use accessory activation policy".into());
        }

        let initial = view_size(&view);
        let (width, height) = (initial.0.max(64) as u32, initial.1.max(64) as u32);
        let rect = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(width), f64::from(height)),
        );
        // SAFETY: the plugin view is live and owned on this main thread.
        let can_resize = unsafe { view.canResize() } == kResultTrue;
        let mut style = NSWindowStyleMask::Titled
            .union(NSWindowStyleMask::Closable)
            .union(NSWindowStyleMask::Miniaturizable);
        if can_resize {
            style = style.union(NSWindowStyleMask::Resizable);
        }
        // SAFETY: constructing a normal AppKit window on its marked main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(title));
        // AppKit windows are retained explicitly; close must not invalidate
        // the Rust owner before lifecycle cleanup calls `removed`.
        // SAFETY: the Rust owner explicitly keeps the window retained.
        unsafe { window.setReleasedWhenClosed(false) };
        let content = window
            .contentView()
            .ok_or("AppKit did not create a window content view")?;
        content.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(width), f64::from(height)),
        ));

        let scale = view.cast::<IPlugViewContentScaleSupport>();
        let factor = window.backingScaleFactor() as f32;
        if let Some(scale) = &scale {
            // SAFETY: this is the optional VST3 scale interface of the view.
            unsafe { scale.setContentScaleFactor(factor) };
        }
        let initial = view_size(&view);
        let initial = if let Some(bounds) = wanted {
            let per_point = f64::from(bounds.dpi.max(1)) / 72.0;
            let requested = (
                (f64::from(bounds.width) / per_point).round() as i32,
                (f64::from(bounds.height) / per_point).round() as i32,
            );
            if can_resize {
                constrain_view_size(&view, requested)
            } else {
                initial
            }
        } else {
            initial
        };
        window.setContentSize(NSSize::new(initial.0 as f64, initial.1 as f64));
        content.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(initial.0 as f64, initial.1 as f64),
        ));
        let frame = ComWrapper::new(FrameObject);
        // SAFETY: the COM frame remains owned in OpenEditor until detached.
        let frame_ptr = frame
            .to_com_ptr::<IPlugFrame>()
            .ok_or("could not create the plugin editor frame")?;
        // SAFETY: a valid frame pointer with the host's retained reference.
        if unsafe { view.setFrame(frame_ptr.as_ptr()) } != kResultOk {
            // SAFETY: clear a frame even if the plugin reports setup failure.
            unsafe { view.setFrame(std::ptr::null_mut()) };
            return Err("the plugin refused the editor frame".into());
        }

        if let Some(bounds) = wanted {
            set_window_position(&window, bounds, mtm);
        } else {
            window.center();
        }
        EDITOR.with(|slot| {
            *slot.borrow_mut() = Some(OpenEditor {
                view: view.clone(),
                scale,
                _frame: frame,
                window: window.clone(),
                content: content.clone(),
                attached: false,
                hidden: false,
                floating: false,
                last_view_size: initial,
                content_scale: factor,
                captured: self.captured.clone(),
                captured_down: Vec::new(),
                notify: self.notify.clone(),
            });
        });
        let parent = &*content as *const NSView as *mut c_void;
        // SAFETY: AppKit content view is a live NSView on this main thread.
        let attached = unsafe { view.attached(parent, kPlatformTypeNSView) };
        if attached != kResultOk {
            close_editor(mtm);
            return Err(format!(
                "the plugin could not attach its editor ({attached})"
            ));
        }
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                editor.attached = true;
            }
        });
        bring_forward(app, &window);
        stack_editor();
        Ok(editor_status(mtm))
    }

    fn close(&mut self) -> EditorStatus {
        let mtm = MainThreadMarker::new();
        let Some(mtm) = mtm else {
            return EditorStatus::default();
        };
        close_editor(mtm);
        EditorStatus {
            bounds: LAST.with(|last| *last.borrow()),
            ..EditorStatus::default()
        }
    }

    fn set_visible(&mut self, visible: bool) -> EditorStatus {
        let Some(mtm) = MainThreadMarker::new() else {
            return EditorStatus::default();
        };
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                editor.hidden = !visible;
                if visible {
                    bring_forward(NSApp(mtm), &editor.window);
                } else {
                    editor.window.orderOut(None);
                    release_captured(editor);
                }
            }
        });
        editor_status(mtm)
    }

    fn status(&mut self) -> EditorStatus {
        let Some(mtm) = MainThreadMarker::new() else {
            return EditorStatus::default();
        };
        if EDITOR.with(|e| {
            e.borrow()
                .as_ref()
                .is_some_and(|e| !e.hidden && !e.window.isVisible())
        }) {
            close_editor(mtm);
        }
        editor_status(mtm)
    }

    fn capture_keys(&mut self, keys: Vec<u8>) {
        self.captured = keys.clone();
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                let removed: Vec<u8> = editor
                    .captured_down
                    .iter()
                    .copied()
                    .filter(|c| !keys.contains(c))
                    .collect();
                if let Some(notify) = &editor.notify {
                    for code in removed {
                        notify(Response::Key { code, down: false });
                    }
                }
                editor.captured_down.retain(|c| keys.contains(c));
                editor.captured = keys;
            }
        });
    }

    fn set_notify(&mut self, notify: Notify) {
        self.notify = Some(notify.clone());
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                editor.notify = Some(notify);
            }
        });
    }
}

fn view_size(view: &ComPtr<IPlugView>) -> (i32, i32) {
    let mut rect = ViewRect {
        left: 0,
        top: 0,
        right: 320,
        bottom: 200,
    };
    // SAFETY: valid out pointer to a live VST3 view.
    unsafe { view.getSize(&mut rect) };
    rectangle_size(&rect).unwrap_or((320, 200))
}

fn constrain_view_size(view: &ComPtr<IPlugView>, (width, height): (i32, i32)) -> (i32, i32) {
    let mut rect = ViewRect {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    // SAFETY: a valid in/out rectangle for the live editor view.
    if unsafe { view.checkSizeConstraint(&mut rect) } == kResultOk {
        rectangle_size(&rect).unwrap_or_else(|| constrained_size(width, height))
    } else {
        constrained_size(width, height)
    }
}

fn editor_status(mtm: MainThreadMarker) -> EditorStatus {
    EDITOR.with(|slot| {
        let slot = slot.borrow();
        let Some(editor) = slot.as_ref() else {
            return EditorStatus {
                bounds: LAST.with(|last| *last.borrow()),
                ..EditorStatus::default()
            };
        };
        let frame = editor.window.frame();
        let content = editor.content.frame();
        let dpi = (editor.window.backingScaleFactor() * 72.0).round().max(1.0) as u32;
        let scale = f64::from(dpi) / 72.0;
        let screen_top = primary_screen_top(mtm);
        let bounds = EditorBounds {
            x: (frame.origin.x * scale).round() as i32,
            y: ((screen_top - frame.origin.y - frame.size.height) * scale).round() as i32,
            width: (content.size.width * scale).round().max(0.0) as u32,
            height: (content.size.height * scale).round().max(0.0) as u32,
            dpi,
        };
        LAST.with(|last| *last.borrow_mut() = Some(bounds));
        EditorStatus {
            open: true,
            visible: !editor.hidden && editor.window.isVisible(),
            bounds: Some(bounds),
        }
    })
}

fn set_window_position(window: &NSWindow, bounds: EditorBounds, mtm: MainThreadMarker) {
    let dpi = bounds.dpi.max(1) as f64;
    let scale = dpi / 72.0;
    let frame = window.frame();
    let screen_top = primary_screen_top(mtm);
    window.setFrameOrigin(NSPoint::new(
        f64::from(bounds.x) / scale,
        screen_top - f64::from(bounds.y) / scale - frame.size.height,
    ));
    // A saved editor may belong to a display that has since been removed.
    // AppKit keeps its title bar reachable on one of the current displays.
    window.setFrame_display(
        window.constrainFrameRect_toScreen(window.frame(), None),
        false,
    );
}

fn close_editor(mtm: MainThreadMarker) {
    let editor = EDITOR.with(|slot| slot.borrow_mut().take());
    if let Some(mut editor) = editor {
        let frame = editor.window.frame();
        let dpi = (editor.window.backingScaleFactor() * 72.0).round().max(1.0) as u32;
        let scale = f64::from(dpi) / 72.0;
        if editor.attached {
            // SAFETY: this thread created and owns the view and its window.
            unsafe {
                editor.view.removed();
            }
            editor.attached = false;
        }
        // SAFETY: stop callbacks before the frame owner is dropped.
        unsafe {
            editor.view.setFrame(std::ptr::null_mut());
        }
        release_captured(&mut editor);
        let screen_top = primary_screen_top(mtm);
        let content = editor.content.frame();
        LAST.with(|last| {
            *last.borrow_mut() = Some(EditorBounds {
                x: (frame.origin.x * scale).round() as i32,
                y: ((screen_top - frame.origin.y - frame.size.height) * scale).round() as i32,
                width: (content.size.width * scale).round().max(0.0) as u32,
                height: (content.size.height * scale).round().max(0.0) as u32,
                dpi,
            })
        });
        editor.window.close();
    }
}

/// AppKit reports screen frames from the primary display's bottom-left
/// coordinate system. The protocol stores a top-left Y relative to that same
/// origin; negative values correctly represent displays above the primary.
fn primary_screen_top(mtm: MainThreadMarker) -> f64 {
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    if screens.count() == 0 {
        return 0.0;
    }
    let frame = screens.objectAtIndex(0).frame();
    frame.origin.y + frame.size.height
}

fn release_captured(editor: &mut OpenEditor) {
    if let Some(notify) = &editor.notify {
        for code in editor.captured_down.drain(..) {
            notify(Response::Key { code, down: false });
        }
    } else {
        editor.captured_down.clear();
    }
}

/// A low-latency AppKit event pump. Short timed waits keep native window
/// events flowing while the channel wakes protocol requests from the reader.
pub struct WindowPump {
    wake: Sender<()>,
    woken: Receiver<()>,
}

impl WindowPump {
    pub fn new() -> Self {
        let (wake, woken) = mpsc::channel();
        Self { wake, woken }
    }
}

impl Default for WindowPump {
    fn default() -> Self {
        Self::new()
    }
}

impl Pump for WindowPump {
    fn waker(&self) -> Waker {
        let wake = self.wake.clone();
        Arc::new(move || {
            let _ = wake.send(());
        })
    }

    fn run(&mut self, drain: &mut dyn FnMut() -> bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = autoreleasepool(|_| {
            let app = NSApp(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            app.finishLaunching();
            app
        });
        loop {
            if !autoreleasepool(|_| drain()) {
                break;
            }
            let has_visible_window = EDITOR.with(|e| {
                e.borrow()
                    .as_ref()
                    .is_some_and(|e| !e.hidden && e.window.isVisible())
            });
            let disconnected = if has_visible_window {
                matches!(
                    self.woken.recv_timeout(EVENT_POLL),
                    Err(RecvTimeoutError::Disconnected)
                )
            } else {
                self.woken.recv().is_err()
            };
            if disconnected {
                break;
            }
            autoreleasepool(|_| {
                while let Some(event) = next_event(&app) {
                    if handle_key_event(&event) {
                        continue;
                    }
                    app.sendEvent(&event);
                }
                EDITOR.with(|slot| {
                    if let Some(editor) = slot.borrow_mut().as_mut()
                        && !editor.window.isKeyWindow()
                    {
                        release_captured(editor);
                    }
                });
                if EDITOR.with(|e| {
                    e.borrow()
                        .as_ref()
                        .is_some_and(|e| !e.hidden && !e.window.isVisible())
                }) {
                    close_editor(mtm);
                }
                stack_editor();
                sync_resize();
                sync_backing_scale();
            });
        }
        close_editor(mtm);
    }
}

/// Puts a plugin window in front of every other window and gives it the
/// keyboard. The host is an accessory process that is not active when its
/// window opens, and an inactive process's window otherwise opens behind the
/// active application's, here Lumen's.
fn bring_forward(app: Retained<NSApplication>, window: &NSWindow) {
    // The cooperative `activate` of newer systems does nothing unless the
    // active application yields, and Lumen does not.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
    window.orderFrontRegardless();
}

/// Keeps the plugin window above Lumen's own windows while Lumen or the
/// window's process is the active application, and drops it to the normal
/// level when another application is active, so it never floats over other
/// work. Only a change is applied.
fn stack_editor() {
    EDITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(editor) = slot.as_mut() else {
            return;
        };
        let parent = std::os::unix::process::parent_id() as i32;
        let wanted = NSRunningApplication::currentApplication().isActive()
            || NSRunningApplication::runningApplicationWithProcessIdentifier(parent)
                .is_some_and(|lumen| lumen.isActive());
        if wanted != editor.floating {
            editor.floating = wanted;
            editor.window.setLevel(if wanted {
                NSFloatingWindowLevel
            } else {
                NSNormalWindowLevel
            });
        }
    });
}

fn next_event(app: &NSApplication) -> Option<Retained<NSEvent>> {
    let past = NSDate::distantPast();
    app.nextEventMatchingMask_untilDate_inMode_dequeue(
        NSEventMask::Any,
        Some(&past),
        // SAFETY: this is the system-owned default run-loop mode constant.
        unsafe { NSDefaultRunLoopMode },
        true,
    )
}

fn sync_resize() {
    let current = EDITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let editor = slot.as_mut()?;
        let bounds = editor.content.bounds();
        let size = (
            bounds.size.width.round() as i32,
            bounds.size.height.round() as i32,
        );
        if size == editor.last_view_size {
            return None;
        }
        Some((
            editor.view.clone(),
            editor.window.clone(),
            editor.content.clone(),
            size,
        ))
    });
    let Some((view, window, content, proposed)) = current else {
        return;
    };
    let size = constrain_view_size(&view, proposed);
    if size != proposed {
        window.setContentSize(NSSize::new(size.0 as f64, size.1 as f64));
        content.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(size.0 as f64, size.1 as f64),
        ));
    }
    EDITOR.with(|slot| {
        if let Some(editor) = slot.borrow_mut().as_mut() {
            editor.last_view_size = size;
        }
    });
    let mut rect = ViewRect {
        left: 0,
        top: 0,
        right: size.0,
        bottom: size.1,
    };
    // SAFETY: the cloned view is attached and belongs to this main thread.
    unsafe { view.onSize(&mut rect) };
}

fn sync_backing_scale() {
    let changed = EDITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let editor = slot.as_mut()?;
        let factor = editor.window.backingScaleFactor() as f32;
        if (factor - editor.content_scale).abs() < f32::EPSILON {
            return None;
        }
        editor.content_scale = factor;
        Some((editor.scale.clone(), editor.view.clone(), factor))
    });
    let Some((scale, view, factor)) = changed else {
        return;
    };
    if let Some(scale) = scale {
        // SAFETY: the scale interface belongs to the active view on this thread.
        unsafe {
            scale.setContentScaleFactor(factor);
        }
        let size = view_size(&view);
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                editor
                    .window
                    .setContentSize(NSSize::new(size.0 as f64, size.1 as f64));
                editor.content.setFrame(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(size.0 as f64, size.1 as f64),
                ));
                editor.last_view_size = size;
            }
        });
    }
}

fn handle_key_event(event: &NSEvent) -> bool {
    let event_type = event.r#type();
    if event_type != NSEventType::KeyDown && event_type != NSEventType::KeyUp {
        return false;
    }
    let Some(code) = windows_keycode(event.keyCode()) else {
        return false;
    };
    let down = event_type == NSEventType::KeyDown;
    EDITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(editor) = slot.as_mut() else {
            return false;
        };
        if !editor.window.isKeyWindow() {
            return false;
        }
        let modifiers = event.modifierFlags().bits();
        let held = editor.captured_down.contains(&code);
        let modified = modifiers & (1 << 17 | 1 << 18 | 1 << 19 | 1 << 20) != 0;
        let text_entry = editor
            .window
            .firstResponder()
            .is_some_and(|responder| responder.isKindOfClass(NSTextView::class()));
        if !editor.captured.contains(&code) || (modified && !held) || (text_entry && !held) {
            return false;
        }
        if let Some(notify) = &editor.notify {
            if down && !editor.captured_down.contains(&code) {
                editor.captured_down.push(code);
                notify(Response::Key { code, down: true });
            } else if !down && editor.captured_down.contains(&code) {
                editor.captured_down.retain(|held| *held != code);
                notify(Response::Key { code, down: false });
            }
        }
        true
    })
}

fn windows_keycode(key: u16) -> Option<u8> {
    // USB keyboard usages for the common note-key range, mapped to the
    // Windows virtual-key values already used by Lumen's computer keyboard.
    let letter = match key {
        0 => b'A',
        11 => b'B',
        8 => b'C',
        2 => b'D',
        14 => b'E',
        3 => b'F',
        5 => b'G',
        4 => b'H',
        34 => b'I',
        38 => b'J',
        40 => b'K',
        37 => b'L',
        46 => b'M',
        45 => b'N',
        31 => b'O',
        35 => b'P',
        12 => b'Q',
        15 => b'R',
        1 => b'S',
        17 => b'T',
        32 => b'U',
        9 => b'V',
        13 => b'W',
        7 => b'X',
        16 => b'Y',
        6 => b'Z',
        18 => b'1',
        19 => b'2',
        20 => b'3',
        21 => b'4',
        23 => b'5',
        22 => b'6',
        26 => b'7',
        28 => b'8',
        25 => b'9',
        29 => b'0',
        49 => b' ',
        36 => 0x0D,
        53 => 0x1B,
        51 => 0x08,
        _ => return None,
    };
    Some(letter)
}

#[cfg(test)]
mod tests {
    use super::{constrained_size, rectangle_size, windows_keycode};
    use vst3::Steinberg::ViewRect;

    #[test]
    fn invalid_plugin_rectangles_are_refused_without_overflow() {
        assert_eq!(
            rectangle_size(&ViewRect {
                left: i32::MIN,
                top: 0,
                right: i32::MAX,
                bottom: 200
            }),
            None
        );
        assert_eq!(
            rectangle_size(&ViewRect {
                left: 20,
                top: 0,
                right: 10,
                bottom: 200
            }),
            None
        );
        assert_eq!(
            rectangle_size(&ViewRect {
                left: 10,
                top: 20,
                right: 330,
                bottom: 220
            }),
            Some((320, 200))
        );
    }

    #[test]
    fn editor_sizes_stay_inside_appkit_and_protocol_limits() {
        assert_eq!(constrained_size(0, 1), (64, 64));
        assert_eq!(constrained_size(800, 600), (800, 600));
        assert_eq!(constrained_size(i32::MAX, i32::MAX), (16_384, 16_384));
    }

    #[test]
    fn mac_hardware_keys_map_to_the_existing_windows_virtual_keys() {
        assert_eq!(windows_keycode(0), Some(b'A'));
        assert_eq!(windows_keycode(6), Some(b'Z'));
        assert_eq!(windows_keycode(18), Some(b'1'));
        assert_eq!(windows_keycode(49), Some(b' '));
        assert_eq!(windows_keycode(999), None);
    }
}
