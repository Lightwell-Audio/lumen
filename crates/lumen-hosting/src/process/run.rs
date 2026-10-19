//! The thread itself: start, load, configure, link, then serve requests.

use super::*;

pub(super) struct Run {
    pub(super) start: Start,
    pub(super) gate: Arc<Gate>,
    pub(super) tag: Tag,
    pub(super) out: Sender<(Tag, Happened)>,
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
    pub(super) requests: Receiver<Command>,
    pub(super) ended: Arc<AtomicBool>,
    pub(super) counts: Arc<Counts>,
    /// The auxiliary buses to turn on at the next set-up.
    pub(super) aux: Cell<Aux>,
}

/// Where a crashed host said it died. The host writes the line as it goes
/// down and Lumen reads it from the pipe a moment later, so look for it for
/// a short while.
fn crash_detail(host: &HostProcess, error: &HostError) -> Option<String> {
    if !matches!(
        error,
        HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
    ) {
        return None;
    }
    let log = host.log();
    for _ in 0..20 {
        if let Some(line) = log.crash() {
            return Some(line);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

/// What the thread knows of the editor window between polls.
struct EditorTrack {
    /// Believed open (asked for and not seen closed).
    open: bool,
    /// Opened has been said.
    announced: bool,
    /// Its host did not answer the last time it was asked.
    hung: bool,
    last: Option<EditorBounds>,
    polled: Instant,
}

impl Default for EditorTrack {
    fn default() -> Self {
        Self {
            open: false,
            announced: false,
            hung: false,
            last: None,
            polled: Instant::now(),
        }
    }
}

/// Whether `error` means the host's window code is not answering (as against
/// a refusal or a host that is gone).
fn not_answering(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Timeout
            | HostError::Failed {
                kind: lumen_plugin_host::protocol::FailureKind::Unresponsive,
                ..
            }
    )
}

struct Up {
    host: HostProcess,
    name: String,
    vendor: String,
    generation: u32,
    params: Vec<ParamInfo>,
    params_total: u32,
    units: Vec<UnitInfo>,
    programs: Vec<ProgramList>,
}

impl Run {
    fn say(&self, what: Happened) {
        let _ = self.out.send((self.tag, what));
        (self.wake)();
    }

    pub(super) fn run(self) {
        let loaded = {
            let _place = self.gate.permit();
            self.load()
        };
        let mut up = match loaded {
            Ok((up, linked)) => {
                self.say(Happened::Linked(Box::new(linked)));
                up
            }
            Err(failure) => {
                self.say(Happened::Failed(failure));
                return;
            }
        };
        let d = self.start.deadlines;
        let mut polled = Instant::now();
        let mut editor = EditorTrack::default();
        loop {
            let mut commands = match self.requests.recv_timeout(Duration::from_millis(10)) {
                Ok(first) => vec![first],
                Err(RecvTimeoutError::Timeout) => Vec::new(),
                Err(RecvTimeoutError::Disconnected) => vec![Command::Quit],
            };
            while let Ok(more) = self.requests.try_recv() {
                commands.push(more);
            }
            for command in coalesce(commands) {
                match command {
                    Command::Relink(rate, frames, aux) => {
                        if let Some(aux) = aux {
                            self.aux.set(aux);
                        }
                        match self.relink(&mut up, rate, frames) {
                            Ok(linked) => self.say(Happened::Linked(Box::new(linked))),
                            Err(failure) => {
                                self.ended.store(true, Ordering::Release);
                                up.host.kill();
                                self.say(Happened::Failed(failure));
                                return;
                            }
                        }
                    }
                    Command::Set(id, value) => {
                        let _ = up.host.set_param(id, value, QUICK);
                    }
                    Command::Text(id, value) => {
                        if let Ok(text) = up.host.param_text(id, value, QUICK) {
                            self.say(Happened::Text { id, value, text });
                        }
                    }
                    Command::Texts(id, values) => {
                        for value in values {
                            if let Ok(text) = up.host.param_text(id, value, QUICK) {
                                self.say(Happened::Text { id, value, text });
                            }
                        }
                    }
                    Command::Typed(id, text) => {
                        let value = up.host.param_value(id, &text, QUICK).ok();
                        self.say(Happened::Typed { id, text, value });
                    }
                    Command::OpenEditor(bounds) => {
                        if !editor.open {
                            self.open_editor(&mut up, &mut editor, bounds);
                        }
                    }
                    Command::CloseEditor => self.close_editor(&mut up, &mut editor),
                    Command::ShowEditor(visible) => {
                        if editor.open {
                            // A window that does not answer is found out by
                            // the poll; nothing is said here.
                            let _ = up.host.show_editor(visible, EDITOR_WAIT);
                        }
                    }
                    Command::End => {
                        self.ended.store(true, Ordering::Release);
                        up.host.kill();
                        self.say(Happened::Failed(Failure {
                            step: Step::Running,
                            error: HostError::Failed {
                                kind: lumen_plugin_host::protocol::FailureKind::Unresponsive,
                                message:
                                    "the plugin's window stopped answering and the plugin was ended"
                                        .into(),
                            },
                            detail: None,
                        }));
                        return;
                    }
                    Command::Capture => {
                        let state = up
                            .host
                            .state(CAPTURE)
                            .map(|component| Captured {
                                component,
                                // The controller's own state is a courtesy: a
                                // plugin that cannot give it still has a state.
                                controller: up
                                    .host
                                    .controller_state(CAPTURE)
                                    .ok()
                                    .filter(|c| !c.is_empty()),
                            })
                            .map_err(|e| e.to_string());
                        self.say(Happened::State(state));
                    }
                    Command::Refresh => {
                        if let Ok((_, list)) = up.host.parameters(CAPTURE) {
                            self.say(Happened::Values(
                                list.iter().map(|p| (p.id, p.value)).collect(),
                            ));
                        }
                    }
                    Command::Load {
                        state,
                        controller,
                        values,
                    } => {
                        let answer = self.load_state(&mut up, state, controller, &values);
                        self.say(Happened::Loaded(answer));
                    }
                    Command::Keys(keys) => {
                        let _ = up.host.capture_keys(keys, QUICK);
                    }
                    Command::Quit => {
                        up.host.quit(d.quit);
                        return;
                    }
                }
            }
            if polled.elapsed() >= EDIT_POLL && !up.host.has_ended() {
                polled = Instant::now();
                if let Ok(taken) = up.host.take_edits(QUICK)
                    && (!taken.list.is_empty() || taken.dropped > 0 || taken.ignored > 0)
                {
                    self.say(Happened::Edits(taken));
                }
            }
            if editor.open && editor.polled.elapsed() >= EDITOR_POLL && !up.host.has_ended() {
                editor.polled = Instant::now();
                self.watch_editor(&mut up, &mut editor);
            }
            if up.host.has_ended() {
                self.ended.store(true, Ordering::Release);
                let how = up
                    .host
                    .ended()
                    .unwrap_or(lumen_plugin_host::client::ExitKind::Killed);
                let error = HostError::Ended(how);
                let detail = crash_detail(&up.host, &error);
                self.say(Happened::Failed(Failure {
                    step: Step::Running,
                    error,
                    detail,
                }));
                return;
            }
        }
    }

    fn open_editor(&self, up: &mut Up, editor: &mut EditorTrack, bounds: Option<EditorBounds>) {
        match up.host.open_editor(bounds, EDITOR_WAIT) {
            Ok(status) => {
                editor.open = status.open;
                editor.hung = false;
                editor.last = status.bounds;
                if let Some(b) = status.bounds.filter(|_| status.open) {
                    editor.announced = true;
                    self.say(Happened::Editor(EditorEvent::Opened(b)));
                }
            }
            Err(error) if not_answering(&error) => {
                // The window is being made by a plugin that has stopped
                // answering: it is open as far as anyone can say, and the
                // poll finds out when it answers.
                editor.open = true;
                editor.hung = true;
                editor.announced = false;
                self.say(Happened::Editor(EditorEvent::NotResponding));
            }
            Err(error) => self.say(Happened::Editor(EditorEvent::Refused(error.to_string()))),
        }
    }

    fn close_editor(&self, up: &mut Up, editor: &mut EditorTrack) {
        if !editor.open {
            return;
        }
        match up.host.close_editor(EDITOR_WAIT) {
            Ok(status) => {
                editor.open = false;
                editor.hung = false;
                self.say(Happened::Editor(EditorEvent::Closed(
                    status.bounds.or(editor.last),
                )));
            }
            Err(error) if not_answering(&error) => {
                if !editor.hung {
                    editor.hung = true;
                    self.say(Happened::Editor(EditorEvent::NotResponding));
                }
            }
            Err(_) => {}
        }
    }

    /// Asks an open editor where it is, and says what changed.
    fn watch_editor(&self, up: &mut Up, editor: &mut EditorTrack) {
        match up.host.editor_status(EDITOR_WAIT) {
            Ok(status) => {
                if editor.hung {
                    editor.hung = false;
                    self.say(Happened::Editor(EditorEvent::Responding));
                }
                if !status.open {
                    editor.open = false;
                    self.say(Happened::Editor(EditorEvent::Closed(
                        status.bounds.or(editor.last),
                    )));
                    return;
                }
                let Some(b) = status.bounds else {
                    return;
                };
                if !editor.announced {
                    editor.announced = true;
                    editor.last = Some(b);
                    self.say(Happened::Editor(EditorEvent::Opened(b)));
                } else if editor.last != Some(b) {
                    editor.last = Some(b);
                    self.say(Happened::Editor(EditorEvent::Moved(b)));
                }
            }
            Err(error) if not_answering(&error) => {
                if !editor.hung {
                    editor.hung = true;
                    self.say(Happened::Editor(EditorEvent::NotResponding));
                }
            }
            Err(_) => {}
        }
    }

    /// Loads a state into the running plugin and reads every parameter's
    /// value afterwards. A plugin that refuses the state keeps its own.
    fn load_state(
        &self,
        up: &mut Up,
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: &[(u32, f64)],
    ) -> Result<Vec<(u32, f64)>, String> {
        let d = self.start.deadlines;
        up.host
            .set_state(state, d.state)
            .map_err(|e| e.to_string())?;
        if let Some(controller) = controller {
            // The controller's view of its own state is not worth failing for.
            let _ = up.host.set_controller_state(controller, d.state);
        }
        for &(id, value) in values {
            up.host
                .set_param(id, value, d.state)
                .map_err(|e| e.to_string())?;
        }
        let (_, list) = up.host.parameters(d.state).map_err(|e| e.to_string())?;
        Ok(list.iter().map(|p| (p.id, p.value)).collect())
    }

    fn fail(&self, host: &mut HostProcess, step: Step, error: HostError) -> Failure {
        self.ended.store(true, Ordering::Release);
        host.kill();
        let detail = crash_detail(host, &error);
        Failure {
            step,
            error,
            detail,
        }
    }

    fn load(&self) -> Result<(Up, Linked), Failure> {
        let s = &self.start;
        let env: Vec<(&str, &str)> = s
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut host = HostProcess::spawn(&s.host, &env).map_err(|e| Failure {
            step: Step::Start,
            error: HostError::Failed {
                kind: lumen_plugin_host::protocol::FailureKind::Load,
                message: e.to_string(),
            },
            detail: None,
        })?;
        // Keys pressed in the plugin's window go to the app as they come, not
        // at this thread's next look.
        let (out, wake, tag) = (self.out.clone(), self.wake.clone(), self.tag);
        host.on_notice(Box::new(move |news| {
            if let Response::Key { code, down } = news {
                let _ = out.send((tag, Happened::Key { code, down }));
                wake();
            }
        }));
        let class = host
            .load(&s.module, Some(s.class), s.deadlines.load)
            .map_err(|e| self.fail(&mut host, Step::Load, e))?;
        if !s.state.is_empty() {
            host.set_state(s.state.clone(), s.deadlines.state)
                .map_err(|e| self.fail(&mut host, Step::State, e))?;
        }
        if !s.controller_state.is_empty() {
            let _ = host.set_controller_state(s.controller_state.clone(), s.deadlines.state);
        }
        // The parameters are for the interface: a plugin whose controller
        // will not list them still plays. They are listed before the project's
        // own values are set, so their values are what the saved state holds:
        // the baseline an undone change goes back to.
        let (params_total, params) = host
            .parameters(s.deadlines.state)
            .unwrap_or((0, Vec::new()));
        let units = host.units(s.deadlines.state).unwrap_or_default();
        let programs = host.programs(s.deadlines.state).unwrap_or_default();
        for &(id, value) in &s.values {
            host.set_param(id, value, s.deadlines.state)
                .map_err(|e| self.fail(&mut host, Step::State, e))?;
        }
        let mut up = Up {
            host,
            name: class.name,
            vendor: class.vendor,
            generation: 0,
            params,
            params_total,
            units,
            programs,
        };
        let linked = self.configure_and_link(&mut up, s.rate, s.frames)?;
        Ok((up, linked))
    }

    /// Configures at `rate` (a new generation) and opens a link. The plugin
    /// is promised blocks of at most `frames` (the device's), never the
    /// protocol's limit: some plugins work in proportion to it. The link has
    /// room for the engine's longest block; a longer block than promised is
    /// given to the plugin in pieces by the host.
    fn configure_and_link(&self, up: &mut Up, rate: f64, frames: u32) -> Result<Linked, Failure> {
        let s = &self.start;
        let inputs = if s.instrument { 0 } else { 2 };
        let frames = frames.clamp(1, MOST_FRAMES as u32);
        let aux = self.aux.get();
        // The buses the plugin declares; one that will not list them is
        // played on its main buses.
        let buses = up.host.buses(s.deadlines.configure).unwrap_or_default();
        let configured = up
            .host
            .configure_buses(rate, frames, (inputs, 2), aux, s.deadlines.configure)
            .map_err(|e| self.fail(&mut up.host, Step::Configure, e))?;
        let (latency, tail) = (configured.latency, configured.tail);
        up.generation = up.host.generation();
        let block = up
            .host
            .attach(
                Layout::new(MOST_FRAMES as u32, configured.inputs, configured.outputs),
                s.wake,
                s.deadlines.attach,
            )
            .map_err(|e| self.fail(&mut up.host, Step::Attach, e))?;
        let pairs = output_pairs(&buses.1, aux.outputs, configured.outputs);
        Ok(Linked {
            link: Link::new(
                block,
                up.generation,
                tail,
                pairs,
                self.ended.clone(),
                self.counts.clone(),
            ),
            buses,
            aux,
            name: up.name.clone(),
            vendor: up.vendor.clone(),
            latency,
            tail,
            frames,
            params: up.params.clone(),
            params_total: up.params_total,
            units: up.units.clone(),
            programs: up.programs.clone(),
            pid: up.host.id(),
            log: up.host.log(),
        })
    }

    fn relink(&self, up: &mut Up, rate: f64, frames: u32) -> Result<Linked, Failure> {
        self.configure_and_link(up, rate, frames)
    }
}

/// Where the output buses that are on sit in a block: the main bus first (2
/// channels, as Lumen sets it up), then each auxiliary bus of `mask` in bus
/// order. A list that does not add up to the `total` the host carries is
/// played on its main bus alone.
fn output_pairs(buses: &[BusInfo], mask: u32, total: u8) -> Vec<Pair> {
    let mut pairs = vec![Pair {
        first: 0,
        channels: 2,
    }];
    let mut first = 2;
    for bus in (1..buses.len().min(MAX_BUSES)).filter(|bus| mask & (1 << bus) != 0) {
        let channels = usize::from(buses[bus].channels).clamp(1, 2);
        pairs.push(Pair { first, channels });
        first += channels;
    }
    if first == usize::from(total) {
        pairs
    } else {
        vec![Pair {
            first: 0,
            channels: usize::from(total).clamp(1, 2),
        }]
    }
}
