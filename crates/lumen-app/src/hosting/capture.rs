//! Reading every running plugin's state (M6 slices 4 and 5, ADR-0076,
//! ADR-0077): before a save, in checkpoints, and when a plugin changed inside.

use super::*;

impl Hosting {
    /// Asks every running plugin for its state now, because a save is about
    /// to be written. Every plugin, not only those that reported a change: a
    /// plugin keeps most of what the user sets (a patch, a preset, its own
    /// pages) without telling the host, so nothing says it has changed. True
    /// when a read is in flight, to be waited for ([`Hosting::captures_pending`]).
    pub fn request_captures(&mut self) -> bool {
        let Some(manager) = self.manager.as_ref() else {
            return false;
        };
        let mut any = false;
        for (&id, t) in &mut self.tracked {
            if t.state != State::Ready {
                continue;
            }
            if t.params.capturing.is_none() {
                manager.capture(id);
                t.params.asked();
            }
            any = true;
        }
        any
    }

    /// Whether a plugin has said, since the last call, that something changed
    /// inside it that no parameter shows (a patch, a preset). The project then
    /// has unsaved changes, though no edit.
    pub fn take_inside_changes(&mut self) -> bool {
        std::mem::take(&mut self.inside_changes)
    }

    /// Asks one running plugin for its state now (a preset to be saved). Not
    /// asked again while a read is in flight. False when the plugin is not
    /// running.
    pub fn capture_one(&mut self, device: Id) -> bool {
        let Some(manager) = self.manager.as_ref() else {
            return false;
        };
        let Some(t) = self
            .tracked
            .get_mut(&device.0)
            .filter(|t| t.state == State::Ready)
        else {
            return false;
        };
        // TEMP TRACE (remove before merge)
        trace_app(&format!(
            "capture_one: capturing={} fresh={} edit_count={} captured_at={}",
            t.params.capturing.is_some(),
            t.params.fresh(),
            t.params.edit_count,
            t.params.captured_at
        ));
        if t.params.capturing.is_none() && !t.params.fresh() {
            manager.capture(device.0);
            t.params.asked();
        }
        true
    }

    /// The plugin's state as it is now: the last read, when nothing has changed
    /// in the plugin since. None while it has changed or a read is in flight.
    pub fn fresh_state(&self, device: Id) -> Option<Captured> {
        let p = &self.tracked.get(&device.0)?.params;
        (p.capturing.is_none() && p.fresh())
            .then(|| p.captured.clone())
            .flatten()
    }

    /// Whether the last read of the plugin's state failed.
    pub fn capture_failed(&self, device: Id) -> bool {
        self.tracked
            .get(&device.0)
            .is_some_and(|t| t.params.capture_failed)
    }

    /// Whether a plugin's state is still being read.
    pub fn captures_pending(&self) -> bool {
        self.tracked
            .values()
            .any(|t| t.state == State::Ready && t.params.capturing.is_some())
    }

    /// How many plugins have changed and could not be read, so a save holds
    /// the state they were loaded with and the parameter values the project
    /// keeps, but nothing else that changed inside them.
    pub fn unread(&self) -> usize {
        self.tracked
            .values()
            .filter(|t| t.state == State::Ready && t.params.dirty && t.params.capture_failed)
            .count()
    }

    /// Stamps what the running plugins reported into `project`, to be saved.
    pub fn stamp(&self, project: &mut Project) {
        for track in project
            .tracks
            .iter_mut()
            .chain(std::iter::once(&mut project.master))
        {
            if !track.devices.iter().any(|d| {
                self.tracked
                    .get(&d.id.0)
                    .is_some_and(|t| t.state == State::Ready)
            }) {
                continue;
            }
            let track = Arc::make_mut(track);
            for device in &mut track.devices {
                if let Some(t) = self.tracked.get(&device.id.0)
                    && t.state == State::Ready
                    && t.rate != 0
                {
                    device.reported = Some(Reported {
                        latency: t.latency,
                        tail: t.tail,
                        rate: t.rate,
                    });
                    // A state read since the plugin last changed is what it
                    // is now, with every value in it: it replaces the saved
                    // state and the values the project held over it.
                    if let Some(captured) = &t.params.captured {
                        device.state = captured.component.clone();
                        if let Some(controller) = &captured.controller {
                            device.controller_state = controller.clone();
                        }
                        // A state read since the plugin last changed is what it
                        // is now, with every value in it. An older one (a
                        // checkpoint, or a read that failed since) has the
                        // values that moved after it set over it.
                        device.params = if t.params.fresh() {
                            BTreeMap::new()
                        } else {
                            t.params.catch_up(&device.params)
                        };
                    }
                }
            }
        }
    }
}
