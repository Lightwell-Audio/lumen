//! The component handler: the edits and restarts a plugin reports.

use super::*;

/// Hears the controller: restart requests (latency or bus changes) and
/// parameter edits made in its own windows.
///
/// An edit is kept twice: once for Lumen, in order and with its gesture marks
/// ([`Handler::take_reports`]), and once for the processor, which hears the
/// latest value of each parameter at the start of its next block
/// ([`Handler::drain_to_processor`]): a host's duty, because a controller
/// only tells the host; the processor knows nothing until it is told.
#[derive(Default)]
pub struct Handler {
    pub restart: AtomicU32,
    reports: Mutex<Reports>,
    to_processor: Mutex<Vec<(u32, f64)>>,
}

#[derive(Default)]
struct Reports {
    list: Vec<Edit>,
    dropped: u32,
}

impl Handler {
    /// Records an edit for Lumen. A value that follows a value of the same
    /// parameter replaces it, so a fast drag between two polls is one entry;
    /// gesture marks are never merged.
    pub fn report(&self, edit: Edit) {
        let Ok(mut reports) = self.reports.lock() else {
            return;
        };
        if let Some(last) = reports.list.last_mut()
            && edit.kind == last.kind
            && matches!(
                edit.kind,
                EditKind::Perform | EditKind::Output | EditKind::Dirty
            )
            && edit.id == last.id
        {
            last.value = edit.value;
            return;
        }
        if reports.list.len() >= MAX_EDITS {
            reports.dropped = reports.dropped.saturating_add(1);
            return;
        }
        reports.list.push(edit);
    }

    /// The edits since the last call, and how many were dropped for want of room.
    pub fn take_reports(&self) -> (Vec<Edit>, u32) {
        match self.reports.lock() {
            Ok(mut r) => (std::mem::take(&mut r.list), std::mem::take(&mut r.dropped)),
            Err(_) => (Vec::new(), 0),
        }
    }

    /// Queues a value for the processor's next block (the latest of each
    /// parameter wins).
    pub fn send_to_processor(&self, id: u32, value: f64) {
        let Ok(mut queued) = self.to_processor.lock() else {
            return;
        };
        match queued.iter().position(|(i, _)| *i == id) {
            Some(at) => queued[at].1 = value,
            None if queued.len() < MOST_PENDING => queued.push((id, value)),
            None => {}
        }
    }

    /// Moves the queued values into `into` (which keeps its room, so a
    /// steady stream of blocks does not allocate).
    pub fn drain_to_processor(&self, into: &mut Vec<(u32, f64)>) {
        if let Ok(mut queued) = self.to_processor.lock() {
            into.extend(queued.drain(..));
        }
    }
}

/// The most distinct parameters waiting for a block, and the most a block
/// is given at its start.
pub const MOST_PENDING: usize = 512;

impl Class for Handler {
    type Interfaces = (IComponentHandler, IComponentHandler2);
}

/// What a restart asks for that says the plugin's own state changed: its
/// parameter values (a program or patch was loaded) or the whole component.
const RESTART_CHANGED_STATE: u32 = 1 | (1 << 2);

impl Handler {
    /// Tells Lumen the plugin changed inside, where no parameter shows it.
    fn report_dirty(&self) {
        self.report(Edit {
            id: 0,
            value: 0.0,
            kind: EditKind::Dirty,
        });
    }
}

impl IComponentHandler2Trait for Handler {
    /// The plugin's state changed (or, with false, was saved by the plugin
    /// itself): only a change matters to Lumen.
    unsafe fn setDirty(&self, state: TBool) -> tresult {
        if state != 0 {
            self.report_dirty();
        }
        kResultOk
    }

    unsafe fn requestOpenEditor(&self, _name: FIDString) -> tresult {
        kResultFalse
    }

    unsafe fn startGroupEdit(&self) -> tresult {
        kResultOk
    }

    unsafe fn finishGroupEdit(&self) -> tresult {
        kResultOk
    }
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        self.report(Edit {
            id,
            value: 0.0,
            kind: EditKind::Begin,
        });
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if value.is_finite() {
            self.report(Edit {
                id,
                value,
                kind: EditKind::Perform,
            });
            self.send_to_processor(id, value);
        }
        kResultOk
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        self.report(Edit {
            id,
            value: 0.0,
            kind: EditKind::End,
        });
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags as u32, Ordering::AcqRel);
        if flags as u32 & RESTART_CHANGED_STATE != 0 {
            self.report_dirty();
        }
        kResultOk
    }
}
