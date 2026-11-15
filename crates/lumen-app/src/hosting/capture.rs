use super::*;

impl Hosting {
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

    pub fn take_inside_changes(&mut self) -> bool {
        std::mem::take(&mut self.inside_changes)
    }

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
        if t.params.capturing.is_none() && !t.params.fresh() {
            manager.capture(device.0);
            t.params.asked();
        }
        true
    }

    pub fn fresh_state(&self, device: Id) -> Option<Captured> {
        let p = &self.tracked.get(&device.0)?.params;
        (p.capturing.is_none() && p.fresh())
            .then(|| p.captured.clone())
            .flatten()
    }

    pub fn capture_failed(&self, device: Id) -> bool {
        self.tracked
            .get(&device.0)
            .is_some_and(|t| t.params.capture_failed)
    }

    pub fn captures_pending(&self) -> bool {
        self.tracked
            .values()
            .any(|t| t.state == State::Ready && t.params.capturing.is_some())
    }

    pub fn unread(&self) -> usize {
        self.tracked
            .values()
            .filter(|t| t.state == State::Ready && t.params.dirty && t.params.capture_failed)
            .count()
    }

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
                    if let Some(captured) = &t.params.captured {
                        device.state = captured.component.clone();
                        if let Some(controller) = &captured.controller {
                            device.controller_state = controller.clone();
                        }
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
