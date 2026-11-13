//! The shown track's devices as rows: what each card shows, how its plugin is
//! running, and where Reload would start it and how long ago that was.

use super::*;
use crate::chain::{faces, outputs, presets, sidechain};

/// A device's row in the chain, with what the card needs (no state blobs).
pub(super) struct Row {
    pub(super) id: Id,
    pub(super) name: String,
    pub(super) vendor: String,
    pub(super) identity: String,
    /// A VST3 plugin: its class id is the identity.
    pub(super) vst3: bool,
    /// Lumen's own effect (M8 slice B0): no host, no window, a table.
    pub(super) builtin: bool,
    /// The built-in's effect is running in the mixer; false when the pool had
    /// no room for it.
    pub(super) running: bool,
    pub(super) instrument: bool,
    pub(super) bypass: bool,
    pub(super) face: OwnedFace,
    pub(super) latency_ms: f32,
    /// The strip's knobs and how many parameters the plugin offers.
    pub(super) slots: Vec<Option<strip::Slot>>,
    pub(super) parameters: usize,
    /// The plugin's own window and Learn.
    pub(super) editor: strip::Editor,
    pub(super) learning: bool,
    /// The preset field of a running plugin.
    pub(super) field: Option<presets::FieldLook>,
    /// The sidechain row of a plugin effect with a sidechain input.
    pub(super) sidechain: Option<sidechain::Look>,
    /// The outputs row of a plugin instrument with several outputs.
    pub(super) outputs: Option<outputs::Look>,
    /// The drawn face of a built-in that has one (M8 slice B1).
    pub(super) drawn: Option<faces::eq::Look>,
    /// The band its face has picked, 0-based.
    pub(super) picked: usize,
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
    /// The shown track's devices as rows, with how each is running.
    pub(super) fn rows_of(&self, track: &Track, rate: u32) -> Vec<Row> {
        let playing = self.transport.state().playing;
        track
            .devices
            .iter()
            .map(|d| {
                let mut row = self.row_of(d, rate);
                row.sidechain = self.sidechain_look(track.id, d, playing);
                row.outputs = self.outputs_look(d);
                row.drawn = self.drawn_face(d, rate);
                row.picked = self.chain.bands.get(&d.id).copied().unwrap_or(0);
                row
            })
            .collect()
    }

    /// The drawn face of `d`, when it is a built-in that has one, with the
    /// values the project holds and the analyser's last picture (M8 slice B1).
    fn drawn_face(&self, d: &Device, rate: u32) -> Option<faces::eq::Look> {
        if crate::builtin::name_of(d) != lumen_devices::eq::NAME {
            return None;
        }
        let table = lumen_devices::eq::TABLE;
        let value = |id: u32| lumen_devices::params::value_of_param(table, id, &d.params);
        let bands: [faces::eq::Band; lumen_devices::eq::BANDS] = std::array::from_fn(|index| {
            let number = index + 1;
            let kind = if number == 1 {
                lumen_devices::eq::Kind::LowCut
            } else if number == lumen_devices::eq::BANDS {
                lumen_devices::eq::Kind::HighCut
            } else {
                lumen_devices::eq::Kind::of_choice(
                    value(lumen_devices::eq::type_of(number)).max(0.0) as usize,
                )
            };
            faces::eq::Band {
                number,
                kind,
                on: value(lumen_devices::eq::on_of(number)) >= 0.5,
                freq: value(lumen_devices::eq::freq_of(number)),
                gain: if kind.has_gain() {
                    value(lumen_devices::eq::gain_of(number))
                } else {
                    0.0
                },
                q: if kind.has_q() {
                    value(lumen_devices::eq::q_of(number))
                } else {
                    1.0
                },
                slope: if kind.has_slope() {
                    value(lumen_devices::eq::gain_of(number))
                } else {
                    12.0
                },
            }
        });
        let values = crate::builtin::values(d);
        let mut response = vec![0.0; faces::eq::POINTS];
        lumen_devices::eq::response(
            &values,
            rate.max(1),
            &mut response,
            (faces::eq::LOW, faces::eq::HIGH),
        );
        Some(faces::eq::Look {
            bands,
            response,
            spectrum: self
                .chain
                .pretend_spectrum
                .as_ref()
                .filter(|(id, _)| *id == d.id)
                .map(|(_, db)| db.clone())
                .or_else(|| self.chain.analyzers.get(&d.id).cloned()),
            listening: self.chain.analyzers.contains_key(&d.id),
        })
    }

    /// Reads the analyser of every built-in whose face is drawn this frame,
    /// and silences every other (M8 slice B1): the mist runs only while its
    /// card is whole and on the shown track.
    pub(super) fn pump_spectra(&mut self, track: Id, rate: u32) {
        let mut live: std::collections::BTreeSet<Id> = std::collections::BTreeSet::new();
        if !self.chain.compact
            && let Some(showing) = all_tracks(self.document.project()).find(|t| t.id == track)
        {
            for device in &showing.devices {
                if crate::builtin::name_of(device) == lumen_devices::eq::NAME
                    && !self.chain.folded.contains(&device.id)
                {
                    live.insert(device.id);
                }
            }
        }
        let App { builtin, chain, .. } = self;
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
            let picture = chain
                .analyzers
                .entry(id)
                .or_insert_with(|| vec![lumen_devices::analyzer::MIN_DB; faces::eq::POINTS]);
            if reader.read() {
                reader.points(rate.max(1), (faces::eq::LOW, faces::eq::HIGH), picture);
            }
        }
    }

    fn row_of(&self, d: &Device, rate: u32) -> Row {
        let builtin = crate::builtin::is_builtin(d);
        let (name, vendor, identity) = match &d.plugin {
            PluginRef::Vst3 {
                class_id,
                name,
                vendor,
                ..
            } => (name.clone(), vendor.clone(), class_id.clone()),
            // A built-in has a personality: its own name, with what it is
            // under it (M8 slice B0, DESIGN §7k). A device without one (the
            // synth) keeps its plain name and the product's.
            PluginRef::Builtin { name } => match crate::builtin::identity(name) {
                Some(identity) => (
                    identity.name.to_owned(),
                    identity.kind.to_owned(),
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
            // What the device's own build reported, as the routing has it.
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
        let (slots, parameters) = if (d.is_vst3() || builtin) && matches!(face, OwnedFace::Ready) {
            self.strip_of(d)
        } else {
            (Vec::new(), 0)
        };
        let editor = if builtin {
            // A built-in's controls are its card and All parameters: there is
            // no separate window.
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
            builtin,
            running: !(builtin && self.builtin.refused(d.id)),
            instrument: d.is_instrument(),
            bypass: d.bypass,
            face,
            latency_ms,
            sidechain: None,
            outputs: None,
            drawn: None,
            picked: 0,
        }
    }
}

impl App {
    /// Where Reload would start a stopped plugin from, in words: the sound
    /// last read from it and how long ago, or what the project holds.
    fn reload_source(&self, d: &Device) -> String {
        match self.hosting.last_good_age(d.id) {
            Some(age) => format!("its sound from {} ago", ago(age)),
            None if d.state.is_empty() => "the plugin as it was added".to_owned(),
            None => "the sound saved in the project".to_owned(),
        }
    }
}

/// How long ago, roughly: "a moment", "40 s", "3 min", "2 h".
pub(super) fn ago(age: std::time::Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0..5 => "a moment".to_owned(),
        5..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3600),
    }
}
