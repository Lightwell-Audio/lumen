//! What one host said, and what became of its device (M6 slice 2c): its
//! link, a failure, keys and text from its window, the state it read back,
//! values that moved, and the edits it made inside itself.

use super::*;

impl Hosting {
    /// What one host said.
    pub(super) fn hear(
        &mut self,
        event: lumen_hosting::Event,
        mixer: &mut MixerHandle,
        changed: &mut Changed,
    ) {
        let id = event.device;
        let Some(t) = self.tracked.get_mut(&id) else {
            return;
        };
        match event.happened {
            Happened::Linked(linked) => {
                let linked = *linked;
                let guard = Guard::new(Box::new(linked.link), linked.latency);
                t.status = Some(guard.status());
                t.log = Some(linked.log.clone());
                t.latency = linked.latency;
                t.tail = linked.tail;
                t.rate = self.rate;
                t.frames = linked.frames;
                t.state = State::Ready;
                t.linked_at = Some(std::time::Instant::now());
                t.bypass = false;
                t.relinking = false;
                t.inputs = linked.buses.0.clone();
                t.outputs = linked.buses.1.clone();
                t.aux = linked.aux;
                if !t.params.linked_before {
                    t.params.linked_before = true;
                    // A fresh host has no window; a new link of the same
                    // host (the buffer size changed) keeps its own.
                    t.editor = EditorState::Closed;
                    t.editor_hidden = false;
                    t.params.base = linked.params.iter().map(|p| (p.id, p.value)).collect();
                    t.params.values = t.params.base.clone();
                    for (&id, &v) in &t.params.started_with {
                        t.params.values.insert(id, v);
                        // A value recovered from before a failure is the
                        // plugin's now, not the project's: it is not
                        // something to put back.
                        if !t.params.held.contains_key(&id) {
                            t.params.base.insert(id, v);
                        }
                    }
                    t.params.last_checkpoint = Some(std::time::Instant::now());
                    t.params.list = linked.params;
                    t.params.total = linked.params_total;
                    t.params.units = linked.units;
                    t.params.programs = linked.programs;
                }
                if t.instrument {
                    self.instruments.insert(
                        t.track.0,
                        InstrumentSpec {
                            kind: HOSTED_KIND,
                            preset: 0,
                            values: Vec::new(),
                            instrument: Box::new(HostedInstrument::new(guard)),
                        },
                    );
                    changed.midi = true;
                } else if let Err(guard) = mixer.attach_host(id, guard) {
                    self.waiting.push((id, guard));
                }
            }
            Happened::Failed(why) => {
                t.editor = EditorState::Closed;
                t.editor_hidden = false;
                t.relinking = false;
                note_failure(&mut t.failures, &t.state);
                t.state = State::Failed(why.to_string());
            }
            Happened::Key { code, down } => self.keys.push((code, down)),
            Happened::Text { id, value, text } => {
                // A value that is a step of a stepped parameter is also
                // one of its choices' words.
                if let Some(steps) = t
                    .params
                    .list
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| i.steps)
                    .filter(|s| *s > 1)
                {
                    let at = value * f64::from(steps);
                    if (at - at.round()).abs() < 1e-6 {
                        let words = t.params.choices.entry(id).or_default();
                        let index = at.round().max(0.0) as usize;
                        if words.len() <= index {
                            words.resize(index + 1, String::new());
                        }
                        words[index] = text.clone();
                    }
                }
                t.params.remember_text(id, value, text);
            }
            Happened::Editor(news) => match news {
                EditorEvent::Opened(b) | EditorEvent::Moved(b) => {
                    if t.editor != EditorState::NotResponding
                        || matches!(news, EditorEvent::Opened(_))
                    {
                        t.editor = EditorState::Open;
                    }
                    if matches!(news, EditorEvent::Opened(_)) {
                        // A window comes up shown.
                        t.editor_hidden = false;
                    }
                    self.places.push((t.class.clone(), b));
                }
                EditorEvent::Closed(place) => {
                    t.editor = EditorState::Closed;
                    t.editor_hidden = false;
                    if let Some(b) = place {
                        self.places.push((t.class.clone(), b));
                    }
                }
                EditorEvent::NotResponding => t.editor = EditorState::NotResponding,
                EditorEvent::Responding => {
                    if t.editor == EditorState::NotResponding {
                        t.editor = EditorState::Open;
                    }
                }
                EditorEvent::Refused(why) => t.editor = EditorState::Refused(why),
            },
            Happened::Typed { id, text, value } => {
                t.params.typed.push((id, text, value));
            }
            Happened::State(state) => {
                let p = &mut t.params;
                let at = p.capturing.take().unwrap_or(0);
                match state {
                    Ok(bytes) => {
                        p.captured = Some(bytes);
                        p.captured_when = Some(std::time::Instant::now());
                        p.captured_values = p.capturing_values.take().unwrap_or_default();
                        p.captured_at = at;
                        p.capture_failed = false;
                        if at == p.edit_count {
                            p.dirty = false;
                        }
                    }
                    Err(_) => {
                        // The state read before stays the last good one.
                        p.capturing_values = None;
                        p.capture_failed = true;
                    }
                }
            }
            Happened::Values(all) => {
                let p = &mut t.params;
                // What the plugin calls its parameters now: a patch may name
                // its controls anew (`kParamTitlesChanged`). Its words for
                // values and steps may have changed with them: they are
                // asked for again.
                if p.forget_texts_if_renamed(&all) {
                    self.texts_forgotten += 1;
                }
                for now in &all {
                    if let Some(known) = p.list.iter_mut().find(|i| i.id == now.id) {
                        known.title.clone_from(&now.title);
                        known.units.clone_from(&now.units);
                        known.steps = now.steps;
                        known.default = now.default;
                        known.flags = now.flags;
                        known.unit = now.unit;
                    }
                }
                // What the plugin holds now: a value that moved without a
                // word (a program chosen in its window, a patch) is an edit
                // of the project, as if the plugin had said so.
                for (id, value) in all.into_iter().map(|p| (p.id, p.value)) {
                    let moves = p
                        .list
                        .iter()
                        .find(|i| i.id == id)
                        .is_none_or(|i| !i.read_only() && !i.bypass() && !i.hidden());
                    let Some(&old) = p.values.get(&id) else {
                        return;
                    };
                    // A parameter a lane drives moves by itself: that is not
                    // an edit of the project.
                    if p.driven.contains(&id) {
                        continue;
                    }
                    if moves && (old - value).abs() > 1e-6 {
                        p.base.entry(id).or_insert(old);
                        p.values.insert(id, value);
                        p.touch();
                        p.edits.push((
                            Edit {
                                id,
                                value,
                                kind: EditKind::Perform,
                            },
                            false,
                        ));
                    }
                }
            }
            Happened::Loaded(answer) => {
                let p = &mut t.params;
                p.quiet_until = Some(std::time::Instant::now() + QUIET);
                let Some(load) = p.loads.pop_front() else {
                    return;
                };
                match answer {
                    Ok(values) => {
                        // The plugin's values are the state's own now: the
                        // baseline an undone change goes back to.
                        let values: BTreeMap<u32, f64> = values.into_iter().collect();
                        p.base = values.clone();
                        p.values = values;
                        p.captured = Some(load.state.clone());
                        p.captured_when = Some(std::time::Instant::now());
                        p.captured_values = p.values.clone();
                        p.captured_at = p.edit_count;
                        p.dirty = false;
                        p.capture_failed = false;
                        if let Some(manager) = self.manager.as_mut() {
                            manager.keep_state(id, load.state.clone());
                        }
                        if let Some(name) = load.preset {
                            self.presets.push(PresetNews::Ready {
                                device: Id(id),
                                name,
                                component: load.state.component,
                                controller: load.state.controller,
                            });
                        }
                    }
                    Err(why) => {
                        if load.preset.is_some() && load.tries < PRESET_RETRIES {
                            // Try again in a moment before giving up.
                            p.retry = Some((
                                std::time::Instant::now() + PRESET_RETRY_WAIT,
                                Load {
                                    tries: load.tries + 1,
                                    ..load
                                },
                            ));
                        } else if let Some(name) = load.preset {
                            self.presets.push(PresetNews::Refused {
                                device: Id(id),
                                name,
                                why,
                            });
                        }
                    }
                }
            }
            Happened::Edits(taken) => {
                t.params.ignored = t.params.ignored.saturating_add(taken.ignored);
                let quiet = t
                    .params
                    .quiet_until
                    .is_some_and(|at| std::time::Instant::now() < at);
                for edit in taken.list {
                    if quiet && !matches!(edit.kind, EditKind::Output | EditKind::Retitled) {
                        return;
                    }
                    let p = &mut t.params;
                    match edit.kind {
                        EditKind::Begin => {
                            if let Some(&now) = p.values.get(&edit.id) {
                                p.base.entry(edit.id).or_insert(now);
                            }
                            p.touch();
                            p.open = Some((edit.id, std::time::Instant::now()));
                            p.edits.push((edit, true));
                        }
                        EditKind::Perform => {
                            let before = p.values.insert(edit.id, edit.value);
                            if let Some(before) = before {
                                p.base.entry(edit.id).or_insert(before);
                            }
                            p.touch();
                            let inside = p.open.is_some_and(|(id, _)| id == edit.id);
                            if inside {
                                p.open = Some((edit.id, std::time::Instant::now()));
                            }
                            p.edits.push((edit, inside));
                        }
                        EditKind::End => {
                            p.open = None;
                            p.edits.push((edit, true));
                        }
                        // What the processor reports is the plugin's to
                        // show, not an edit of the project.
                        EditKind::Output => {
                            p.values.insert(edit.id, edit.value);
                        }
                        // The plugin changed inside, where no parameter
                        // shows it: its state is ahead of the project's
                        // (read once it rests), and the project has
                        // unsaved changes. A plugin says as much while it
                        // loads, which is not the user's change.
                        EditKind::Dirty => {
                            if t.linked_at.is_some_and(|at| at.elapsed() >= SETTLE) {
                                p.touch();
                                self.inside_changes = true;
                            }
                        }
                        // Its controls were named anew (a patch): the list
                        // is read again, and the card and panel follow.
                        EditKind::Retitled => {
                            if let Some(manager) = self.manager.as_ref() {
                                p.retitled = true;
                                manager.refresh(id);
                            }
                        }
                    }
                }
            }
        }
    }
}
