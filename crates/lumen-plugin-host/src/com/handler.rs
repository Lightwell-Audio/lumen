use super::*;
use crate::protocol::RESTART_TITLES;

#[derive(Default)]
pub struct Handler {
    pub restart: AtomicU32,
    reports: Mutex<Reports>,
    to_processor: Mutex<Vec<(u32, f64)>>,
}

struct Reports {
    list: Vec<Edit>,
    dropped: u32,
}

impl Default for Reports {
    fn default() -> Self {
        Self {
            list: Vec::with_capacity(REPORT_ROOM),
            dropped: 0,
        }
    }
}

const REPORT_ROOM: usize = 256;

impl Handler {
    pub fn report(&self, edit: Edit) {
        let Ok(mut reports) = self.reports.lock() else {
            return;
        };
        if let Some(last) = reports.list.last_mut()
            && edit.kind == last.kind
            && matches!(
                edit.kind,
                EditKind::Perform | EditKind::Output | EditKind::Dirty | EditKind::Retitled
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

    pub fn take_reports(&self) -> (Vec<Edit>, u32) {
        match self.reports.lock() {
            Ok(mut r) => {
                let taken = r.list.to_vec();
                r.list.clear();
                (taken, std::mem::take(&mut r.dropped))
            }
            Err(_) => (Vec::new(), 0),
        }
    }

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

    pub fn is_queued(&self, id: u32) -> bool {
        self.to_processor
            .lock()
            .is_ok_and(|queued| queued.iter().any(|(i, _)| *i == id))
    }

    pub fn drain_to_processor(&self, into: &mut Vec<(u32, f64)>) {
        if let Ok(mut queued) = self.to_processor.lock() {
            into.extend(queued.drain(..));
        }
    }
}

pub const MOST_PENDING: usize = 512;

impl Class for Handler {
    type Interfaces = (IComponentHandler, IComponentHandler2);
}

const RESTART_CHANGED_STATE: u32 = 1 | (1 << 2);

impl Handler {
    fn report_dirty(&self) {
        self.report(Edit {
            id: 0,
            value: 0.0,
            kind: EditKind::Dirty,
        });
    }
}

impl IComponentHandler2Trait for Handler {
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
        if flags as u32 & RESTART_TITLES != 0 {
            self.report(Edit {
                id: 0,
                value: 0.0,
                kind: EditKind::Retitled,
            });
        }
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(id: u32) -> Edit {
        Edit {
            id,
            value: 0.5,
            kind: EditKind::Begin,
        }
    }

    #[test]
    fn taking_the_reports_leaves_the_list_its_room() {
        let handler = Handler::default();
        let room = handler.reports.lock().unwrap().list.capacity();
        assert!(room >= REPORT_ROOM);
        for id in 0..300 {
            handler.report(edit(id));
        }
        let (taken, dropped) = handler.take_reports();
        assert_eq!((taken.len(), dropped), (300, 0));
        let after = handler.reports.lock().unwrap().list.capacity();
        assert!(after >= 300, "the room grown to is kept: {after}");
        handler.report(edit(1));
        assert_eq!(handler.reports.lock().unwrap().list.capacity(), after);
    }
}
