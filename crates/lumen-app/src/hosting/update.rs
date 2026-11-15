use super::*;

impl Hosting {
    pub fn update(
        &mut self,
        project: &Project,
        catalog: &Catalog,
        (rate, block): (u32, u32),
        mixer: &mut MixerHandle,
        latencies: &mut std::collections::BTreeMap<Id, u32>,
    ) -> Changed {
        let mut changed = Changed::default();
        let frames = promised_frames(block);
        let list = wanted(project);
        self.forget_gone(&list, mixer, &mut changed);
        self.track_new(&list, catalog, rate, frames);
        self.reconfigure(rate, frames);
        let events = self.manager.as_mut().map(Manager::poll).unwrap_or_default();
        for event in events {
            self.hear(event, mixer, &mut changed);
        }
        self.attach_waiting(mixer);
        for w in &list {
            self.follow(w, &mut changed);
        }
        self.checkpoint(&mut changed);
        self.publish_latencies(&list, latencies);
        changed
    }

    fn forget_gone(&mut self, list: &[Wanted<'_>], mixer: &mut MixerHandle, changed: &mut Changed) {
        let gone: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(id, t)| {
                !list.iter().any(|w| {
                    w.device.id.0 == **id
                        && w.class == t.class
                        && (!t.instrument || w.track.id == t.track)
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            let Some(t) = self.tracked.remove(&id) else {
                continue;
            };
            if let Some(manager) = self.manager.as_mut() {
                manager.forget(id);
            }
            if t.instrument {
                self.instruments.remove(&t.track.0);
                changed.midi = true;
            } else {
                mixer.detach_host(id);
            }
            self.waiting.retain(|(w, _)| *w != id);
        }
    }

    fn track_new(&mut self, list: &[Wanted<'_>], catalog: &Catalog, rate: u32, frames: u32) {
        for w in list {
            let id = w.device.id.0;
            let known = self.tracked.get(&id).map(|t| t.state.clone());
            if !matches!(known, None | Some(State::Missing | State::Safe)) {
                continue;
            }
            let state = self.start(w, catalog, rate, frames);
            match self.tracked.get_mut(&id) {
                Some(t) => t.state = state,
                None => {
                    self.tracked.insert(
                        id,
                        Tracked {
                            track: w.track.id,
                            class: w.class.to_owned(),
                            instrument: w.instrument,
                            state,
                            status: None,
                            log: None,
                            latency: w.device.reported.map_or(0, |r| r.latency_at(rate.max(1))),
                            tail: w.device.reported.map_or(0, |r| r.tail),
                            rate,
                            frames: 0,
                            bypass: false,
                            params: Parameters {
                                started_with: w.device.params.clone(),
                                state_rev: w.device.state_rev,
                                ..Parameters::default()
                            },
                            editor: EditorState::Closed,
                            linked_at: None,
                            editor_hidden: false,
                            relinking: false,
                            inputs: Vec::new(),
                            outputs: Vec::new(),
                            aux: Aux::default(),
                            failures: Vec::new(),
                        },
                    );
                }
            }
        }
    }

    fn reconfigure(&mut self, rate: u32, frames: u32) {
        if rate != 0 && (rate != self.rate || frames != self.frames) {
            if self.rate != 0
                && let Some(manager) = self.manager.as_mut()
            {
                manager.relink_all(f64::from(rate), frames);
                for t in self.tracked.values_mut() {
                    t.relinking = t.state == State::Ready;
                }
            }
            self.rate = rate;
            self.frames = frames;
        }
    }

    fn attach_waiting(&mut self, mixer: &mut MixerHandle) {
        let mut still = Vec::new();
        for (id, guard) in std::mem::take(&mut self.waiting) {
            if let Err(guard) = mixer.attach_host(id, guard) {
                still.push((id, guard));
            }
        }
        self.waiting = still;
    }

    fn follow(&mut self, w: &Wanted<'_>, changed: &mut Changed) {
        let id = w.device.id.0;
        let Some(t) = self.tracked.get_mut(&id) else {
            return;
        };
        let Some(status) = t.status.clone() else {
            return;
        };
        if t.state == State::Ready && !t.relinking {
            let aux = Aux {
                inputs: if t.instrument {
                    0
                } else {
                    sidechain_mask(w.device.sidechain.is_some(), &t.inputs)
                },
                outputs: if t.instrument {
                    output_mask(&t.outputs)
                } else {
                    0
                },
            };
            if aux != t.aux
                && let Some(manager) = self.manager.as_mut()
            {
                manager.relink_with(id, f64::from(self.rate), self.frames, aux);
                t.relinking = true;
            }
        }
        if t.bypass != w.device.bypass {
            status.set_bypass(w.device.bypass);
            t.bypass = w.device.bypass;
        }
        if let Some(latency) = status.take_news() {
            t.latency = latency;
        }
        if status.take_restart()
            && let Some(manager) = self.manager.as_mut()
        {
            manager.relink(id, f64::from(self.rate), self.frames);
            t.relinking = true;
        }
        if status.failed() && t.state == State::Ready && !t.relinking {
            note_failure(&mut t.failures, &t.state);
            t.state = State::Failed(gave_up_words(t.log.as_deref()));
        }
        if self.follow_state(w, id) {
            return;
        }
        self.follow_values(w, id, changed);
    }

    fn follow_state(&mut self, w: &Wanted<'_>, id: u64) -> bool {
        let Some(t) = self.tracked.get_mut(&id) else {
            return false;
        };
        if t.state == State::Ready
            && t.params.state_rev != w.device.state_rev
            && w.device.state.is_empty()
            && !t.params.loads.iter().any(|l| l.preset.is_some())
            && let Some(manager) = self.manager.as_mut()
        {
            let values: Vec<(u32, f64)> = w.device.params.iter().map(|(&k, &v)| (k, v)).collect();
            let nothing = Captured {
                component: Vec::new(),
                controller: None,
            };
            match manager.reload_from(id, Some(nothing), values) {
                Ok(true) => {
                    t.state = State::Loading;
                    t.status = None;
                    t.log = None;
                    t.params = Parameters {
                        started_with: w.device.params.clone(),
                        held: w.device.params.clone(),
                        state_rev: w.device.state_rev,
                        ..Parameters::default()
                    };
                    return true;
                }
                Ok(false) => {}
                Err(e) => {
                    t.state = State::Failed(format!("The plugin host could not be started: {e}"));
                    return true;
                }
            }
        }
        if t.state == State::Ready
            && t.params.loads.is_empty()
            && t.params
                .retry
                .as_ref()
                .is_some_and(|(at, _)| std::time::Instant::now() >= *at)
            && let Some(manager) = self.manager.as_ref()
            && let Some((_, load)) = t.params.retry.take()
        {
            manager.load_state(
                id,
                load.state.component.clone(),
                load.state.controller.clone(),
                Vec::new(),
            );
            t.params.loads.push_back(load);
            t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
        }
        if t.state == State::Ready
            && t.params.state_rev != w.device.state_rev
            && t.params.retry.is_none()
            && !t.params.loads.iter().any(|l| l.preset.is_some())
            && let Some(manager) = self.manager.as_ref()
        {
            let state = Captured {
                component: w.device.state.clone(),
                controller: Some(w.device.controller_state.clone()).filter(|c| !c.is_empty()),
            };
            manager.load_state(
                id,
                state.component.clone(),
                state.controller.clone(),
                Vec::new(),
            );
            t.params.loads.push_back(Load {
                state,
                preset: None,
                tries: 0,
            });
            t.params.state_rev = w.device.state_rev;
            t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
        }
        false
    }

    fn follow_values(&mut self, w: &Wanted<'_>, id: u64, changed: &mut Changed) {
        let Some(t) = self.tracked.get_mut(&id) else {
            return;
        };
        if t.state == State::Ready
            && t.params.loads.is_empty()
            && t.params.edits.is_empty()
            && let Some(manager) = self.manager.as_ref()
        {
            let driven: std::collections::BTreeSet<u32> = w
                .track
                .driven_params(w.device.id)
                .filter(|&param| {
                    !self.overridden.contains(
                        w.track.id,
                        lumen_project::automation::Target::Device {
                            device: w.device.id,
                            param,
                        },
                    )
                })
                .collect();
            let now = std::time::Instant::now();
            for id in t.params.driven.difference(&driven) {
                t.params.values.insert(*id, f64::NAN);
                t.params.released.push((*id, now + RELEASE_AGAIN));
            }
            let mut waiting = None;
            let mut again = Vec::new();
            t.params.released.retain(|&(id, at)| {
                if driven.contains(&id) {
                    return false;
                }
                if at <= now {
                    again.push(id);
                    return false;
                }
                let left = (at - now).as_millis() as u64 + 1;
                waiting = Some(waiting.map_or(left, |w: u64| w.min(left)));
                true
            });
            for id in again {
                t.params.values.insert(id, f64::NAN);
            }
            if let Some(left) = waiting {
                changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
            }
            t.params.driven = driven;
            let mut send = t.params.differences(&w.device.params);
            send.retain(|&(id, _)| !t.params.driven.contains(&id));
            let programs: Vec<u32> = t
                .params
                .list
                .iter()
                .filter(|i| i.program_change())
                .map(|i| i.id)
                .collect();
            send.sort_by_key(|(id, _)| !programs.contains(id));
            let choosing = t.params.program_fresh;
            if !send.is_empty() {
                t.params.program_fresh = false;
                if !choosing {
                    t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
                }
            }
            t.params.sent = t.params.sent.saturating_add(send.len() as u32);
            for (id, value) in send {
                manager.set_param(w.device.id.0, id, value);
                t.params.values.insert(id, value);
                t.params.touch();
            }
        }
        if t.params.held != w.device.params {
            t.params.held = w.device.params.clone();
        }
        if t.state == State::Ready
            && t.params.dirty
            && t.params.loads.is_empty()
            && t.params.capturing.is_none()
            && !t.params.capture_failed
            && let Some(since) = t.params.last_edit
            && let Some(manager) = self.manager.as_ref()
        {
            let waited = since.elapsed();
            if waited >= REST {
                manager.capture(id);
                manager.refresh(id);
                t.params.asked();
                t.params.last_checkpoint = Some(std::time::Instant::now());
            } else {
                let left = (REST - waited).as_millis() as u64 + 1;
                changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
            }
        }
    }

    fn checkpoint(&mut self, changed: &mut Changed) {
        if let Some(manager) = self.manager.as_ref() {
            let every = self.checkpoint_every;
            let busy = self.tracked.values().any(|t| t.params.gesture_is_open());
            for (&id, t) in &mut self.tracked {
                if t.state != State::Ready {
                    continue;
                }
                let Some(last) = t.params.last_checkpoint else {
                    continue;
                };
                let due = last + every + every / 8 * (id % 4) as u32;
                let now = std::time::Instant::now();
                if now < due {
                    let left = (due - now).as_millis() as u64 + 1;
                    changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
                } else if t.params.capturing.is_none() && t.params.loads.is_empty() && !busy {
                    manager.capture(id);
                    t.params.asked();
                    t.params.last_checkpoint = Some(now);
                }
            }
        }
    }

    fn publish_latencies(
        &self,
        list: &[Wanted<'_>],
        latencies: &mut std::collections::BTreeMap<Id, u32>,
    ) {
        latencies.clear();
        for w in list.iter() {
            if let Some(t) = self.tracked.get(&w.device.id.0) {
                latencies.insert(w.device.id, t.latency);
            }
        }
    }
}
