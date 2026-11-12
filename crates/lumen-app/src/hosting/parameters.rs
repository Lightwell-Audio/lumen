//! A plugin's parameters as the bridge keeps them (M6 slice 4, ADR-0076):
//! the lists, values and text, typed values, the plugin's own edits, and what
//! differs between the project and the plugin.

use super::*;

impl Hosting {
    /// The plugin's parameters as listed when it loaded (empty until it has).
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.list.as_slice())
    }

    /// The groups the plugin's parameters sit in.
    pub fn units(&self, device: Id) -> &[UnitInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.units.as_slice())
    }

    /// A parameter's value as far as Lumen knows (normalized).
    pub fn value(&self, device: Id, id: u32) -> Option<f64> {
        self.tracked.get(&device.0)?.params.values.get(&id).copied()
    }

    /// Asks the plugin for its own text for `value` of a parameter; the answer
    /// is [`Hosting::text`] a moment later.
    pub fn ask_text(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_text(device.0, id, value);
        }
    }

    /// Sends a parameter value straight to the device's plugin, as its own
    /// window would deliver it, without an edit of the project. For tests.
    pub fn poke_param(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.set_param(device.0, id, value);
        }
    }

    /// Asks the plugin to read `text` the user typed for a parameter; the
    /// answer is in [`Hosting::take_typed`] a moment later.
    pub fn ask_value(&self, device: Id, id: u32, text: String) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_value(device.0, id, text);
        }
    }

    /// What plugins made of the text typed for them since the last call, in
    /// the order they answered.
    pub fn take_typed(&mut self) -> Vec<Typed> {
        let mut out = Vec::new();
        let mut devices: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(_, t)| !t.params.typed.is_empty())
            .map(|(&id, _)| id)
            .collect();
        devices.sort_unstable();
        for id in devices {
            if let Some(t) = self.tracked.get_mut(&id) {
                for (parameter, text, value) in std::mem::take(&mut t.params.typed) {
                    out.push(Typed {
                        device: Id(id),
                        id: parameter,
                        text,
                        value,
                    });
                }
            }
        }
        out
    }

    /// The plugin's text for `value` of a parameter, if it has answered for
    /// that value (and the answer is still kept).
    pub fn text(&self, device: Id, id: u32, value: f64) -> Option<&str> {
        self.texts(device, id)
            .iter()
            .find(|(at, _)| same_value(*at, value))
            .map(|(_, text)| text.as_str())
    }

    /// Counts the times plugins' words for their values were forgotten (a
    /// plugin named its controls anew): a reader that remembers what it
    /// asked for forgets that when this changes, and asks again.
    pub fn texts_forgotten(&self) -> u64 {
        self.texts_forgotten
    }

    /// The kept answer for the value nearest `value`, to stand for it until
    /// the plugin answers for `value` itself.
    pub fn nearest_text(&self, device: Id, id: u32, value: f64) -> Option<&str> {
        self.texts(device, id)
            .iter()
            .min_by(|(a, _), (b, _)| (a - value).abs().total_cmp(&(b - value).abs()))
            .map(|(_, text)| text.as_str())
    }

    fn texts(&self, device: Id, id: u32) -> &[(f64, String)] {
        self.tracked
            .get(&device.0)
            .and_then(|t| t.params.texts.get(&id))
            .map_or(&[], Vec::as_slice)
    }

    /// The plugin's own word for each step of a stepped parameter, as far as
    /// it has said them.
    pub fn choices(&self, device: Id, id: u32) -> &[String] {
        self.tracked
            .get(&device.0)
            .and_then(|t| t.params.choices.get(&id))
            .map_or(&[], Vec::as_slice)
    }

    /// Asks the plugin for the word of every step of the stepped parameters
    /// `ids` (once each), for the chooser's list.
    pub fn ask_choices(&mut self, device: Id, ids: &[u32]) {
        let (Some(manager), Some(t)) = (self.manager.as_ref(), self.tracked.get_mut(&device.0))
        else {
            return;
        };
        for &id in ids {
            let Some(steps) = t
                .params
                .list
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.steps)
                .filter(|s| (2..=MOST_CHOICES).contains(s))
            else {
                continue;
            };
            if t.params.choices_asked.insert(id) {
                let values = (0..=steps)
                    .map(|step| f64::from(step) / f64::from(steps))
                    .collect();
                manager.param_texts(device.0, id, values);
            }
        }
    }

    /// How many values Lumen has sent the device's plugin to match the project.
    pub fn values_sent(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.sent)
    }

    /// The edits plugins made inside themselves since the last call, oldest
    /// first, for the document to record. Their values are already known here,
    /// so the document's matching change sends nothing back.
    pub fn take_edits(&mut self) -> Vec<DeviceEdits> {
        let mut out: Vec<DeviceEdits> = self
            .tracked
            .iter_mut()
            .filter(|(_, t)| !t.params.edits.is_empty())
            .map(|(&id, t)| DeviceEdits {
                device: Id(id),
                edits: std::mem::take(&mut t.params.edits),
            })
            .collect();
        out.sort_by_key(|e| e.device);
        out
    }

    /// Whether a plugin has a gesture open (a control held in its window): the
    /// document keeps the undo step open until it ends.
    pub fn gesture_open(&self) -> bool {
        self.tracked.values().any(|t| t.params.gesture_is_open())
    }
}

/// What Lumen knows of a plugin's parameters while its host runs (M6 slice 4).
#[derive(Default)]
pub(super) struct Parameters {
    /// The plugin's parameters, capped, as they were when it loaded, with
    /// their values in the saved state.
    pub(super) list: Vec<ParamInfo>,
    /// The plugin's own count, which may be more than the list.
    pub(super) total: u32,
    pub(super) units: Vec<UnitInfo>,
    /// The value each parameter has in the plugin as far as Lumen knows: the
    /// saved state's, then what Lumen set and what the plugin did inside.
    pub(super) values: BTreeMap<u32, f64>,
    /// The value a parameter had before the project first held one for it:
    /// what an undone change goes back to.
    pub(super) base: BTreeMap<u32, f64>,
    /// The parameters a lane drives now (M7 slice A3): the engine gives the
    /// plugin their values, so what Lumen knows of them is not the plugin's
    /// until the lane lets go.
    pub(super) driven: std::collections::BTreeSet<u32>,
    /// Parameters a lane let go of, and when to send each its value at rest
    /// once more: the engine's plan without the lane can reach the audio
    /// thread a block after the first send, and until then the lane's value
    /// goes with every block over it.
    pub(super) released: Vec<(u32, std::time::Instant)>,
    /// The values the project held when the host was started.
    pub(super) started_with: BTreeMap<u32, f64>,
    /// The plugin's own text for the values last asked about, by parameter:
    /// the newest last, at most [`TEXTS_KEPT`] each. More than one is kept
    /// because more than one reader asks (the card for the value it shows,
    /// a lane for the point under the pointer), and one answer must not
    /// stand for another's value.
    pub(super) texts: HashMap<u32, Vec<(f64, String)>>,
    /// The plugin's own word for each step of a stepped parameter, by
    /// parameter (a chooser's list), as far as it has said them.
    pub(super) choices: HashMap<u32, Vec<String>>,
    /// The parameters whose steps have been asked about.
    pub(super) choices_asked: std::collections::HashSet<u32>,
    /// The plugin named its controls anew and the list is being read again:
    /// once it comes, the texts above are forgotten.
    pub(super) retitled: bool,
    /// What the plugin made of text the user typed, waiting for the document:
    /// the parameter, the text, and the value or `None` when it did not
    /// understand.
    pub(super) typed: Vec<(u32, String, Option<f64>)>,
    /// Edits the plugin made inside itself, waiting for the document, each
    /// with whether it came inside a gesture (between a begin and an end).
    pub(super) edits: Vec<(Edit, bool)>,
    /// The parameter whose gesture is open, since when. A gesture that never
    /// ends is let go of after [`GESTURE_LIMIT`].
    pub(super) open: Option<(u32, std::time::Instant)>,
    /// Controller, bend and pressure events no parameter took, in all.
    pub(super) ignored: u32,
    /// Values Lumen has sent the plugin to match the project (a plugin's own
    /// edits need none).
    pub(super) sent: u32,
    /// Whether the host has linked before: a fresh link of the same process
    /// keeps what is known.
    pub(super) linked_before: bool,
    /// Whether the plugin's state is ahead of what the project saved.
    pub(super) dirty: bool,
    /// How many times it has changed, and when last: a state read after the
    /// last change is the plugin's state now.
    pub(super) edit_count: u64,
    pub(super) last_edit: Option<std::time::Instant>,
    /// The plugin's state as last read, and the edit count it was read at. A
    /// read is never replaced by a failed one: this is the last good state, the
    /// one a plugin that fails starts again from.
    pub(super) captured: Option<Captured>,
    /// When it was read, and the plugin's values when it was asked for.
    pub(super) captured_when: Option<std::time::Instant>,
    pub(super) captured_values: BTreeMap<u32, f64>,
    /// The values as they were when the read in flight was asked for.
    pub(super) capturing_values: Option<BTreeMap<u32, f64>>,
    /// When the plugin was last read for a checkpoint (or loaded).
    pub(super) last_checkpoint: Option<std::time::Instant>,
    pub(super) captured_at: u64,
    /// A read in flight, started at this edit count.
    pub(super) capturing: Option<u64>,
    /// The last read failed (hung, crashed, refused); not tried again until
    /// the plugin changes again.
    pub(super) capture_failed: bool,
    /// The values the project holds for the device, as last seen.
    pub(super) held: BTreeMap<u32, f64>,
    /// The programs the plugin offers (M6 slice 5).
    pub(super) programs: Vec<ProgramList>,
    /// The revision of the sound the plugin has, as the project counts them
    /// (`Device::state_rev`): another revision in the project means a state
    /// was loaded, or an undo or redo went across one, and the plugin is given
    /// the project's state.
    pub(super) state_rev: u32,
    /// States sent to the plugin, answers not yet in, oldest first.
    pub(super) loads: std::collections::VecDeque<Load>,
    /// A preset the plugin refused, to be given to it again when the time
    /// comes: a plugin still loading its content may refuse a state it takes
    /// a moment later.
    pub(super) retry: Option<(std::time::Instant, Load)>,
    /// What the plugin does inside itself shortly after a state is loaded is
    /// the loading, not the user: its edits are not taken until this time.
    pub(super) quiet_until: Option<std::time::Instant>,
    /// A program the user chose, by name, and when: the edits the plugin makes
    /// in answer are one undo step with the choice.
    pub(super) program: Option<(String, std::time::Instant)>,
    /// The choice has not been sent to the plugin yet: what the plugin sets in
    /// answer to it is wanted, unlike its answer to any later value of ours.
    pub(super) program_fresh: bool,
}

/// A state sent to a plugin whose answer is awaited.
pub(super) struct Load {
    pub(super) state: Captured,
    /// `Some` for a preset the user chose: the project is edited once the
    /// plugin has taken it (and not if it refuses). `None` for the project's
    /// own state sent again after an undo or redo.
    pub(super) preset: Option<String>,
    /// How many times the plugin has refused it already.
    pub(super) tries: u8,
}
impl Parameters {
    /// The plugin has changed: its state read before is out of date.
    pub(super) fn touch(&mut self) {
        self.dirty = true;
        self.edit_count += 1;
        self.last_edit = Some(std::time::Instant::now());
        self.capture_failed = false;
    }

    /// Whether the state read last is the plugin's state now.
    pub(super) fn fresh(&self) -> bool {
        self.captured.is_some() && self.captured_at == self.edit_count
    }

    /// Whether Lumen may set the parameter: not an output the plugin reports
    /// and not its bypass.
    fn settable(&self, id: u32) -> bool {
        self.list
            .iter()
            .find(|i| i.id == id)
            .is_none_or(|i| !i.read_only() && !i.bypass())
    }

    /// The values a plugin started again from its last good state needs set
    /// over it: what the project holds, and every value that moved since that
    /// state was read (or since the plugin loaded, when none was), by the
    /// plugin's own doing or Lumen's.
    pub(super) fn catch_up(&self, held: &BTreeMap<u32, f64>) -> BTreeMap<u32, f64> {
        let then = if self.captured.is_some() {
            &self.captured_values
        } else {
            &self.base
        };
        let mut all = held.clone();
        for (&id, &value) in &self.values {
            if self.settable(id) && !all.contains_key(&id) && then.get(&id) != Some(&value) {
                all.insert(id, value);
            }
        }
        all
    }

    /// Whether the user holds a control in the plugin's window, or has just
    /// chosen a program: not a moment to read the plugin.
    pub(super) fn gesture_is_open(&self) -> bool {
        self.open
            .is_some_and(|(_, since)| since.elapsed() < GESTURE_LIMIT)
            || self
                .program
                .as_ref()
                .is_some_and(|(_, since)| since.elapsed() < PROGRAM_WINDOW)
    }

    /// Notes that a read is asked for now.
    pub(super) fn asked(&mut self) {
        self.capturing = Some(self.edit_count);
        self.capturing_values = Some(self.values.clone());
    }

    /// Forgets the plugin's words for values and steps when the list `now`
    /// read again names its controls anew (it said so, or a title, unit or
    /// step count differs): a patch that renames them may say their values
    /// otherwise too. True when they were forgotten.
    pub(super) fn forget_texts_if_renamed(&mut self, now: &[ParamInfo]) -> bool {
        let renamed = std::mem::take(&mut self.retitled)
            || now.iter().any(|now| {
                self.list.iter().any(|known| {
                    known.id == now.id
                        && (known.title != now.title
                            || known.units != now.units
                            || known.steps != now.steps)
                })
            });
        if renamed {
            self.texts.clear();
            self.choices.clear();
            self.choices_asked.clear();
        }
        renamed
    }

    /// Keeps the plugin's `text` for `value` of parameter `id`, the newest.
    pub(super) fn remember_text(&mut self, id: u32, value: f64, text: String) {
        let kept = self.texts.entry(id).or_default();
        kept.retain(|(at, _)| !same_value(*at, value));
        if kept.len() >= TEXTS_KEPT {
            kept.remove(0);
        }
        kept.push((value, text));
    }
}

/// How many of a parameter's value texts are kept.
const TEXTS_KEPT: usize = 8;

/// Whether two normalized values are the same value, for their texts.
pub(crate) fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}
impl Parameters {
    /// The values to send so the plugin matches what the project holds: every
    /// value the project has that the plugin does not, and, for a parameter
    /// the project let go of (an undone change), the value it had before.
    pub(super) fn differences(&self, held: &BTreeMap<u32, f64>) -> Vec<(u32, f64)> {
        let mut send: Vec<(u32, f64)> = held
            .iter()
            .filter(|&(id, v)| self.values.get(id) != Some(v))
            .map(|(&id, &v)| (id, v))
            .collect();
        for (&id, &base) in &self.base {
            if !held.contains_key(&id) && self.values.get(&id).is_some_and(|&v| v != base) {
                send.push((id, base));
            }
        }
        send
    }
}
