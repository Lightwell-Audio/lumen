use super::*;
use crate::chain::{outputs, presets, sidechain};

pub(super) struct Row {
    pub(super) id: Id,
    pub(super) name: String,
    pub(super) vendor: String,
    pub(super) identity: String,
    pub(super) vst3: bool,
    pub(super) builtin: bool,
    pub(super) running: bool,
    pub(super) instrument: bool,
    pub(super) bypass: bool,
    pub(super) face: OwnedFace,
    pub(super) latency_ms: f32,
    pub(super) slots: Vec<Option<strip::Slot>>,
    pub(super) parameters: usize,
    pub(super) editor: strip::Editor,
    pub(super) learning: bool,
    pub(super) field: Option<presets::FieldLook>,
    pub(super) sidechain: Option<sidechain::Look>,
    pub(super) outputs: Option<outputs::Look>,
    pub(super) drawing: Option<DrawingRow>,
}

pub(in crate::chain) struct DrawingRow {
    pub(in crate::chain) drawing: &'static crate::chain::face::Drawing,
    pub(in crate::chain) parts: Option<&'static crate::chain::face::Parts>,
    pub(in crate::chain) picked: usize,
    pub(in crate::chain) spectrum: Option<Vec<f32>>,
    pub(in crate::chain) reading: Option<lumen_devices::MeterReading>,
    pub(in crate::chain) slots: Vec<strip::Slot>,
    pub(in crate::chain) table: &'static [lumen_devices::params::ParamInfo],
}

pub(super) enum OwnedFace {
    Ready,
    Loading,
    Failed {
        words: String,
        from: String,
        keeps: bool,
    },
    Missing(usize),
    Safe(usize),
}

impl OwnedFace {
    pub(super) fn face(&self) -> Face<'_> {
        match self {
            OwnedFace::Ready => Face::Ready,
            OwnedFace::Loading => Face::Loading,
            OwnedFace::Failed { words, from, keeps } => Face::Failed(Failure {
                words,
                from,
                keeps: *keeps,
            }),
            OwnedFace::Missing(saved) => Face::Missing { saved: *saved },
            OwnedFace::Safe(saved) => Face::Safe { saved: *saved },
        }
    }
}

impl App {
    pub(super) fn rows_of(&self, track: &Track, rate: u32) -> Vec<Row> {
        let playing = self.transport.state().playing;
        track
            .devices
            .iter()
            .map(|d| {
                let mut row = self.row_of(d, rate);
                row.sidechain = self.sidechain_look(track.id, d, playing);
                row.outputs = self.outputs_look(d);
                row
            })
            .collect()
    }

    pub(super) fn silence_spectra(&mut self) {
        for id in self.builtin.spectrum_ids() {
            if let Some(reader) = self.builtin.spectrum(id) {
                reader.set_on(false);
            }
            self.chain.analyzers.remove(&id);
        }
    }

    pub(super) fn pump_spectra(&mut self, ctx: &egui::Context, track: Id, rate: u32) {
        let heard = mist_runs(
            crate::motion::reduced(),
            self.transport.state().playing || self.chain.pretend_spectrum.is_some(),
            crate::chain::card::level_of(self.arrangement.meters.reading(track.0).peak),
        );
        let mut live: std::collections::BTreeSet<Id> = std::collections::BTreeSet::new();
        if heard && let Some(showing) = all_tracks(self.document.project()).find(|t| t.id == track)
        {
            for device in &showing.devices {
                if lumen_devices::watches_spectrum(crate::builtin::name_of(device))
                    && !self.chain.folded.contains(&device.id)
                {
                    live.insert(device.id);
                }
            }
        }
        let App { builtin, chain, .. } = self;
        let dt = ctx.input(|input| input.stable_dt);
        for id in builtin.spectrum_ids() {
            let on = live.contains(&id);
            let Some(reader) = builtin.spectrum(id) else {
                continue;
            };
            reader.set_on(on);
            if !on {
                chain.analyzers.remove(&id);
                continue;
            }
            let picture = chain.analyzers.entry(id).or_insert_with(|| {
                lumen_devices::analyzer::Display::new((
                    crate::chain::face::eq::LOW,
                    crate::chain::face::eq::HIGH,
                ))
            });
            if reader.read() == lumen_devices::Read::Window {
                picture.update(reader.spectrum(), rate.max(1), dt);
            } else {
                picture.advance(dt);
            }
        }
        if !live.is_empty() && self.chain.pretend_spectrum.is_none() {
            ctx.request_repaint_after(crate::motion::METER_FRAME);
        }
    }

    fn row_of(&self, d: &Device, rate: u32) -> Row {
        let builtin = crate::builtin::is_builtin(d);
        let controls = crate::builtin::has_table(d);
        let (name, vendor, identity) = match &d.plugin {
            PluginRef::Vst3 {
                class_id,
                name,
                vendor,
                ..
            } => (name.clone(), vendor.clone(), class_id.clone()),
            PluginRef::Builtin { name } => match crate::builtin::identity(name) {
                Some(identity) => (
                    identity.name.to_owned(),
                    identity.subtitle.to_owned(),
                    name.clone(),
                ),
                None => (
                    crate::document::device_name(d),
                    PRODUCT_NAME.to_owned(),
                    name.clone(),
                ),
            },
        };
        let (face, frames) = if d.is_vst3() {
            let face = match self.hosting.state(d.id) {
                Some(State::Ready) => OwnedFace::Ready,
                Some(State::Failed(words)) => OwnedFace::Failed {
                    words: words.clone(),
                    from: self.reload_source(d),
                    keeps: self.hosting.keeps_failing(d.id),
                },
                Some(State::Missing) => {
                    OwnedFace::Missing(d.state.len() + d.controller_state.len())
                }
                Some(State::Safe) => OwnedFace::Safe(d.state.len() + d.controller_state.len()),
                Some(State::Loading) | None => OwnedFace::Loading,
            };
            let frames = self
                .hosting
                .latency(d.id)
                .or_else(|| d.reported.map(|r| r.latency_at(rate)));
            (face, frames.unwrap_or(0))
        } else if builtin {
            let frames = self.lanes.devices.builtin.get(&d.id).copied().unwrap_or(0);
            (OwnedFace::Ready, frames)
        } else {
            (OwnedFace::Ready, 0)
        };
        let latency_ms = if rate == 0 {
            0.0
        } else {
            frames as f32 / rate as f32 * 1000.0
        };
        let (slots, parameters) = if (d.is_vst3() || controls) && matches!(face, OwnedFace::Ready) {
            self.strip_of(d)
        } else {
            (Vec::new(), 0)
        };
        let drawing = if builtin && matches!(face, OwnedFace::Ready) {
            self.drawing_of(d)
        } else {
            None
        };
        let editor = if controls {
            strip::Editor::Unavailable("This device's controls are on its card".to_owned())
        } else {
            self.editor_look(d)
        };
        Row {
            id: d.id,
            slots,
            parameters,
            editor,
            learning: self.chain.learning == Some(d.id),
            field: self.field_look(d.id),
            name,
            vendor,
            identity,
            vst3: d.is_vst3(),
            builtin: controls,
            running: !(builtin && self.builtin.refused(d.id)),
            instrument: d.is_instrument(),
            bypass: d.bypass,
            face,
            latency_ms,
            sidechain: None,
            outputs: None,
            drawing,
        }
    }
}

impl App {
    fn reload_source(&self, d: &Device) -> String {
        match self.hosting.last_good_age(d.id) {
            Some(age) => format!("its sound from {} ago", ago(age)),
            None if d.state.is_empty() => "the plugin as it was added".to_owned(),
            None => "the sound saved in the project".to_owned(),
        }
    }
}

pub(super) fn ago(age: std::time::Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0..5 => "a moment".to_owned(),
        5..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3600),
    }
}

pub(super) fn mist_runs(reduced: bool, playing: bool, level: f32) -> bool {
    !reduced && (playing || level > 0.0)
}

pub(super) fn personal_of(
    row: &Row,
    step: crate::chain::geometry::Step,
    level: f32,
    rate: u32,
    bpm: f32,
) -> Option<card::Personal<'_>> {
    let identity = crate::builtin::identity(&row.identity).filter(|_| row.builtin)?;
    let drawing = row.drawing.as_ref()?;
    Some(card::Personal {
        hue: identity.colour,
        drawing: drawing.drawing,
        slots: &drawing.slots,
        table: drawing.table,
        step,
        level,
        parts: drawing.parts,
        picked: drawing.picked,
        spectrum: drawing.spectrum.as_deref(),
        rate,
        bpm,
        reading: drawing.reading,
    })
}
