//! Keeping the hosts in step with the project and the engine, once a frame
//! (ADR-0071): devices that came or went, a new rate or block size, what the
//! hosts said, what the engine learned about the running ones, checkpoints,
//! and the latencies the routing is built with.

use super::*;

impl Hosting {
    /// Keeps the hosts in step with the project and the engine. Call every
    /// frame with the device's rate and block size (0 for unknown).
    /// `latencies` receives each plugin's latency for the routing.
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

    /// Devices that left the project, or changed which plugin they hold, or an
    /// instrument that moved to another track: the engine keeps an instrument by
    /// its track, so it starts again where it now plays (an effect is attached by
    /// its own id and goes with its device).
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

    /// New devices, and missing ones whose plugin has turned up.
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

    /// A new rate or block size: every host configures again for it, so a
    /// plugin is always promised the device's block (ADR-0075).
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

    /// Effects the mixer's queue had no room for last time.
    fn attach_waiting(&mut self, mixer: &mut MixerHandle) {
        let mut still = Vec::new();
        for (id, guard) in std::mem::take(&mut self.waiting) {
            if let Err(guard) = mixer.attach_host(id, guard) {
                still.push((id, guard));
            }
        }
        self.waiting = still;
    }

    /// What the engine learned about a running device, and what the project now
    /// asks of it.
    fn follow(&mut self, w: &Wanted<'_>, changed: &mut Changed) {
        let id = w.device.id.0;
        let Some(t) = self.tracked.get_mut(&id) else {
            return;
        };
        let Some(status) = t.status.clone() else {
            return;
        };
        // A sidechain turns the plugin's sidechain input on, and taking
        // it away turns it off: the plugin is set up again with the buses
        // it should have.
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

    /// The sound the project holds is the plugin's: after a preset was loaded, or
    /// an undo or redo went across one, the plugin is given the project's state.
    /// Its own values come back with the answer. True when the plugin was started
    /// again from nothing, so there is nothing more to do for it this frame.
    fn follow_state(&mut self, w: &Wanted<'_>, id: u64) -> bool {
        let Some(t) = self.tracked.get_mut(&id) else {
            return false;
        };
        // The sound the project holds is the plugin's: after a preset was
        // loaded, or an undo or redo went across one, the plugin is given
        // the project's state. Its own values come back with the answer.
        if t.state == State::Ready
            && t.params.state_rev != w.device.state_rev
            && w.device.state.is_empty()
            && !t.params.loads.iter().any(|l| l.preset.is_some())
            && let Some(manager) = self.manager.as_mut()
        {
            // The project holds no state (a plugin as it was added): a
            // plugin cannot be told to forget, so it is started again from
            // nothing, with the values the project holds over it.
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

    /// The values the project holds are the plugin's, and a plugin that changed
    /// is read once it rests.
    fn follow_values(&mut self, w: &Wanted<'_>, id: u64, changed: &mut Changed) {
        let Some(t) = self.tracked.get_mut(&id) else {
            return;
        };
        // The values the project holds are the plugin's: what an edit,
        // an undo or a redo changed is sent to the host (not while a state
        // is on its way: its values are not known yet).
        // Edits the plugin made itself and the document has not recorded
        // yet make the project the older of the two: nothing is sent
        // until it has caught up, or the old value would go back into the
        // plugin for a moment before the new one.
        if t.state == State::Ready
            && t.params.loads.is_empty()
            && t.params.edits.is_empty()
            && let Some(manager) = self.manager.as_ref()
        {
            // A parameter a lane drives is the lane's: the engine gives the
            // plugin its value with every block, so the value the project
            // holds at rest is not also sent over the control channel
            // (M7 slice A3, ADR-0084). The plugin's value moves with the lane
            // and Lumen is not told, so when the lane lets go (it is switched
            // off or removed) what Lumen knew is stale: the value at rest is
            // sent again, even when it is the one Lumen last knew.
            // A lane the user has taken over drives nothing (M7 slice A4a).
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
            // Once more, a moment later, for what the plan in the engine
            // still had a block or two to say.
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
            // A program parameter first: choosing a program sets other
            // parameters as it goes, and the values the project holds
            // for those must be set after, not overwritten by it.
            let programs: Vec<u32> = t
                .params
                .list
                .iter()
                .filter(|i| i.program_change())
                .map(|i| i.id)
                .collect();
            send.sort_by_key(|(id, _)| !programs.contains(id));
            // What the plugin answers a value of ours with (the values a
            // program holds, a linked parameter) is not the user's edit:
            // unless the user chose a program, whose answer is wanted.
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
        // A plugin that has changed is read once it rests, so the next
        // save holds what it is now.
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
                // And every value, for what moved without a word.
                manager.refresh(id);
                t.params.asked();
                t.params.last_checkpoint = Some(std::time::Instant::now());
            } else {
                let left = (REST - waited).as_millis() as u64 + 1;
                changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
            }
        }
    }

    /// Checkpoints: every running plugin is read now and then, whether or not it
    /// said it changed (a plugin keeps most of what the user sets without a
    /// word), so a plugin that fails, or a crash of Lumen's own, costs the time
    /// since the last one and not the session.
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
                // Staggered by device, so plugins are not all read at once.
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

    /// The routing's picture of each plugin's latency: an effect adds it to its
    /// track's path and an instrument plays that late.
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
