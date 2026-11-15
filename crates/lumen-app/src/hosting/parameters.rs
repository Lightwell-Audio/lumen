use super::*;

impl Hosting {
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.list.as_slice())
    }

    pub fn units(&self, device: Id) -> &[UnitInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.units.as_slice())
    }

    pub fn value(&self, device: Id, id: u32) -> Option<f64> {
        self.tracked.get(&device.0)?.params.values.get(&id).copied()
    }

    pub fn ask_text(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_text(device.0, id, value);
        }
    }

    pub fn poke_param(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.set_param(device.0, id, value);
        }
    }

    pub fn ask_value(&self, device: Id, id: u32, text: String) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_value(device.0, id, text);
        }
    }

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

    pub fn text(&self, device: Id, id: u32, value: f64) -> Option<&str> {
        self.texts(device, id)
            .iter()
            .find(|(at, _)| same_value(*at, value))
            .map(|(_, text)| text.as_str())
    }

    pub fn texts_forgotten(&self) -> u64 {
        self.texts_forgotten
    }

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

    pub fn choices(&self, device: Id, id: u32) -> &[String] {
        self.tracked
            .get(&device.0)
            .and_then(|t| t.params.choices.get(&id))
            .map_or(&[], Vec::as_slice)
    }

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

    pub fn values_sent(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.sent)
    }

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

    pub fn gesture_open(&self) -> bool {
        self.tracked.values().any(|t| t.params.gesture_is_open())
    }
}

#[derive(Default)]
pub(super) struct Parameters {
    pub(super) list: Vec<ParamInfo>,
    pub(super) total: u32,
    pub(super) units: Vec<UnitInfo>,
    pub(super) values: BTreeMap<u32, f64>,
    pub(super) base: BTreeMap<u32, f64>,
    pub(super) driven: std::collections::BTreeSet<u32>,
    pub(super) released: Vec<(u32, std::time::Instant)>,
    pub(super) started_with: BTreeMap<u32, f64>,
    pub(super) texts: HashMap<u32, Vec<(f64, String)>>,
    pub(super) choices: HashMap<u32, Vec<String>>,
    pub(super) choices_asked: std::collections::HashSet<u32>,
    pub(super) retitled: bool,
    pub(super) typed: Vec<(u32, String, Option<f64>)>,
    pub(super) edits: Vec<(Edit, bool)>,
    pub(super) open: Option<(u32, std::time::Instant)>,
    pub(super) ignored: u32,
    pub(super) sent: u32,
    pub(super) linked_before: bool,
    pub(super) dirty: bool,
    pub(super) edit_count: u64,
    pub(super) last_edit: Option<std::time::Instant>,
    pub(super) captured: Option<Captured>,
    pub(super) captured_when: Option<std::time::Instant>,
    pub(super) captured_values: BTreeMap<u32, f64>,
    pub(super) capturing_values: Option<BTreeMap<u32, f64>>,
    pub(super) last_checkpoint: Option<std::time::Instant>,
    pub(super) captured_at: u64,
    pub(super) capturing: Option<u64>,
    pub(super) capture_failed: bool,
    pub(super) held: BTreeMap<u32, f64>,
    pub(super) programs: Vec<ProgramList>,
    pub(super) state_rev: u32,
    pub(super) loads: std::collections::VecDeque<Load>,
    pub(super) retry: Option<(std::time::Instant, Load)>,
    pub(super) quiet_until: Option<std::time::Instant>,
    pub(super) program: Option<(String, std::time::Instant)>,
    pub(super) program_fresh: bool,
}

pub(super) struct Load {
    pub(super) state: Captured,
    pub(super) preset: Option<String>,
    pub(super) tries: u8,
}
impl Parameters {
    pub(super) fn touch(&mut self) {
        self.dirty = true;
        self.edit_count += 1;
        self.last_edit = Some(std::time::Instant::now());
        self.capture_failed = false;
    }

    pub(super) fn fresh(&self) -> bool {
        self.captured.is_some() && self.captured_at == self.edit_count
    }

    fn settable(&self, id: u32) -> bool {
        self.list
            .iter()
            .find(|i| i.id == id)
            .is_none_or(|i| !i.read_only() && !i.bypass())
    }

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

    pub(super) fn gesture_is_open(&self) -> bool {
        self.open
            .is_some_and(|(_, since)| since.elapsed() < GESTURE_LIMIT)
            || self
                .program
                .as_ref()
                .is_some_and(|(_, since)| since.elapsed() < PROGRAM_WINDOW)
    }

    pub(super) fn asked(&mut self) {
        self.capturing = Some(self.edit_count);
        self.capturing_values = Some(self.values.clone());
    }

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

    pub(super) fn remember_text(&mut self, id: u32, value: f64, text: String) {
        let kept = self.texts.entry(id).or_default();
        kept.retain(|(at, _)| !same_value(*at, value));
        if kept.len() >= TEXTS_KEPT {
            kept.remove(0);
        }
        kept.push((value, text));
    }
}

const TEXTS_KEPT: usize = 8;

pub(crate) fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}
impl Parameters {
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
