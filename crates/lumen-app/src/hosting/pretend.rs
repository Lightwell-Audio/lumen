use super::*;

impl Hosting {
    pub(crate) fn late_blocks(&self, device: Id) -> Option<u32> {
        self.tracked
            .get(&device.0)
            .and_then(|t| t.status.as_ref())
            .map(|status| status.late_blocks())
    }

    pub fn pretend(&mut self, device: Id, state: State) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.state = state;
        }
    }

    pub fn pretend_sidechain_input(&mut self, device: Id) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            let bus = |name: &str, main: bool| BusInfo {
                name: name.into(),
                channels: 2,
                main,
                default_active: main,
            };
            t.inputs = vec![bus("Input", true), bus("Sidechain", false)];
        }
    }

    pub fn pretend_outputs(&mut self, device: Id, extra: &[&str]) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            let bus = |name: &str, main: bool| BusInfo {
                name: name.into(),
                channels: 2,
                main,
                default_active: main,
            };
            t.outputs = std::iter::once(bus("Main", true))
                .chain(extra.iter().map(|name| bus(name, false)))
                .collect();
        }
    }

    pub fn pretend_edits(&mut self, device: Id, edits: Vec<(Edit, bool)>) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.edits.extend(edits);
        }
    }

    pub fn pretend_parameters(&mut self, device: Id, list: &[(&str, f64, &str)]) {
        let Some(t) = self.tracked.get_mut(&device.0) else {
            return;
        };
        for (i, (title, value, text)) in list.iter().enumerate() {
            let id = i as u32 + 1;
            t.params.list.push(ParamInfo {
                id,
                title: (*title).to_owned(),
                units: String::new(),
                steps: 0,
                default: 0.5,
                flags: 1,
                unit: 0,
                value: *value,
            });
            t.params.values.insert(id, *value);
            t.params.remember_text(id, *value, (*text).to_owned());
        }
        t.params.total = list.len() as u32;
    }

    pub fn pretend_programs(&mut self, device: Id, name: &str, programs: &[&str]) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.programs.push(ProgramList {
                id: t.params.programs.len() as i32,
                name: name.to_owned(),
                unit: 0,
                parameter: Some(90),
                programs: programs.iter().map(|p| (*p).to_owned()).collect(),
            });
        }
    }

    pub fn pretend_failures(&mut self, device: Id, count: usize, age: Option<std::time::Duration>) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.failures = vec![std::time::Instant::now(); count];
            t.params.captured_when = age.and_then(|age| std::time::Instant::now().checked_sub(age));
        }
    }

    pub fn pretend_stepped(&mut self, device: Id, list: &[(&str, i32, f64, &[&str])]) {
        let Some(t) = self.tracked.get_mut(&device.0) else {
            return;
        };
        for (title, steps, value, choices) in list {
            let id = t.params.list.len() as u32 + 1;
            t.params.list.push(ParamInfo {
                id,
                title: (*title).to_owned(),
                units: String::new(),
                steps: *steps,
                default: 0.0,
                flags: 1,
                unit: 0,
                value: *value,
            });
            t.params.values.insert(id, *value);
            let at = (value * f64::from((*steps).max(1))).round() as usize;
            let text = choices.get(at).copied().unwrap_or("");
            t.params.remember_text(id, *value, text.to_owned());
            t.params
                .choices
                .insert(id, choices.iter().map(|c| (*c).to_owned()).collect());
            t.params.choices_asked.insert(id);
        }
        t.params.total = t.params.list.len() as u32;
    }

    pub fn pretend_editor(&mut self, device: Id, state: EditorState) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.editor = state;
        }
    }
}
