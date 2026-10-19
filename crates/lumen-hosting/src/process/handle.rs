//! The handle to a device's thread.

use super::*;

/// Which device, and which start of it, a message is from: the manager drops
/// what a replaced or removed device's old thread still says.
pub type Tag = (u64, u64);

/// The handle to a device's thread. Dropping it asks the host to quit and
/// does not wait.
pub struct Process {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Process {
    /// Starts the thread; its messages go to `out` tagged with `tag`.
    pub(crate) fn start(
        start: Start,
        gate: Arc<Gate>,
        tag: Tag,
        out: Sender<(Tag, Happened)>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let (commands, requests) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("plugin-host".into())
            .spawn(move || {
                let ended = Arc::new(AtomicBool::new(false));
                let run = Run {
                    aux: Cell::new(start.aux),
                    start,
                    gate,
                    tag,
                    out: out.clone(),
                    wake: Arc::clone(&wake),
                    requests,
                    ended: Arc::clone(&ended),
                    counts: Arc::new(Counts::default()),
                };
                // A panic here (a bug, not a plugin: nothing a plugin sends
                // can cause one) ends the host with the thread's state and
                // is said, so the device fails and Reload is offered.
                catching(
                    || run.run(),
                    || {
                        ended.store(true, Ordering::Release);
                        let failure = Failure {
                            step: Step::Running,
                            error: HostError::Protocol(
                                "the app's thread for this plugin failed".into(),
                            ),
                            detail: None,
                        };
                        let _ = out.send((tag, Happened::Failed(failure)));
                        wake();
                    },
                );
            })?;
        Ok(Self {
            commands,
            thread: Some(thread),
        })
    }

    /// Asks for a fresh link at `rate`, promising the plugin blocks of at
    /// most `frames`.
    pub fn relink(&self, rate: f64, frames: u32) {
        let _ = self.commands.send(Command::Relink(rate, frames, None));
    }

    /// As [`Process::relink`], with these auxiliary buses on.
    pub fn relink_with(&self, rate: f64, frames: u32, aux: Aux) {
        let _ = self.commands.send(Command::Relink(rate, frames, Some(aux)));
    }

    /// Sets a parameter of the plugin (normalized, 0 to 1).
    pub fn set_param(&self, id: u32, value: f64) {
        let _ = self.commands.send(Command::Set(id, value));
    }

    /// Asks the host for the plugin's state; the answer comes as
    /// [`Happened::State`], within the state deadline or as a failure.
    pub fn capture(&self) {
        let _ = self.commands.send(Command::Capture);
    }

    /// Asks for every parameter's value; the answer comes as
    /// [`Happened::Values`].
    pub fn refresh(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    /// Loads a state into the plugin; the answer comes as
    /// [`Happened::Loaded`].
    pub fn load_state(&self, state: Vec<u8>, controller: Option<Vec<u8>>, values: Vec<(u32, f64)>) {
        let _ = self.commands.send(Command::Load {
            state,
            controller,
            values,
        });
    }

    /// Asks for the plugin's text for a value; the answer comes as
    /// [`Happened::Text`].
    pub fn param_text(&self, id: u32, value: f64) {
        let _ = self.commands.send(Command::Text(id, value));
    }

    /// Asks for the plugin's text for each of `values` of one parameter; the
    /// answers come as [`Happened::Text`], one each.
    pub fn param_texts(&self, id: u32, values: Vec<f64>) {
        let _ = self.commands.send(Command::Texts(id, values));
    }

    /// Asks the plugin to read `text` the user typed for a parameter; the
    /// answer comes as [`Happened::Typed`].
    pub fn param_value(&self, id: u32, text: String) {
        let _ = self.commands.send(Command::Typed(id, text));
    }

    /// Opens the plugin's editor window (at `bounds` if given); the answer
    /// comes as [`Happened::Editor`].
    pub fn open_editor(&self, bounds: Option<EditorBounds>) {
        let _ = self.commands.send(Command::OpenEditor(bounds));
    }

    /// Closes the editor window; the answer comes as [`Happened::Editor`].
    pub fn close_editor(&self) {
        let _ = self.commands.send(Command::CloseEditor);
    }

    /// Hides the editor window without closing it (the plugin keeps its view
    /// and its place), or shows it again.
    pub fn show_editor(&self, visible: bool) {
        let _ = self.commands.send(Command::ShowEditor(visible));
    }

    /// Ends the host (a plugin whose window stopped answering); the device
    /// then reads as failed, as any host that stopped.
    pub fn end(&self) {
        let _ = self.commands.send(Command::End);
    }

    /// Takes `keys` (virtual-key codes) from the plugin's window when pressed
    /// alone; each press and release comes as [`Happened::Key`]. Empty gives
    /// them back.
    pub fn capture_keys(&self, keys: Vec<u8>) {
        let _ = self.commands.send(Command::Keys(keys));
    }

    /// Asks the host to quit and waits for the thread, as long as it takes.
    pub fn join(mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Asks the host to quit, without waiting.
    pub fn ask_to_quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }

    /// Whether the thread has ended (its host with it).
    pub fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // The thread quits the host on its own and ends; nothing waits for
        // it here.
        let _ = self.commands.send(Command::Quit);
    }
}
