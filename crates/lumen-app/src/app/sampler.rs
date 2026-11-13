//! Shutter in the app (M8 slice B5): putting it on a track, choosing the
//! files it plays, and the waits around them. The sound itself is built in
//! `builtin::samples` and the edits are `document::sampler`'s; here are the
//! commands and the words.
//!
//! The face is drawn by its own slice: every action below is reachable from
//! the palette, a menu and a command, so the drawn face calls the same
//! methods and nothing else.

use eframe::egui;
use lumen_project::model::{Face, Id, PluginRef};

use crate::App;
use crate::app::detail::DetailTab;
use crate::chain::place::{Place, place};

impl App {
    /// The sampler the chosen card is, when the chosen device is one.
    pub(crate) fn chosen_sampler(&self) -> Option<Id> {
        let card = self.chain.selected?;
        self.document.sampler_sound(card).is_some().then_some(card)
    }

    /// Puts Shutter, the built-in sampler, on the selected track, or on a new
    /// MIDI track when no MIDI track is chosen: one undo step, its card shown
    /// (M8 slice B5).
    pub(crate) fn add_sampler(&mut self) {
        let plugin = PluginRef::Builtin {
            name: lumen_devices::sampler::NAME.to_owned(),
        };
        let values = crate::builtin::defaults(lumen_devices::sampler::NAME);
        let place = place(
            self.document.project(),
            &self.arrangement.selected,
            self.chain.selected,
            lumen_project::model::DeviceKind::Instrument,
        );
        let result = match place {
            Place::Track { track, slot } => self
                .document
                .add_device_with(track, plugin, &values, slot)
                .map(|device| (track, device)),
            Place::NewTrack { after } => self
                .document
                .add_device_on_new_track_with(plugin, &values, after),
        };
        match result {
            Ok((track, device)) => {
                self.arrangement.selected = vec![track];
                self.chain.selected = Some(device);
                self.show_detail = true;
                self.detail_tab = DetailTab::Devices;
                self.chain.grow = true;
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    /// Asks which files the chosen sampler plays. They are decoded off the
    /// audio thread and become its face's sound in one undo step.
    pub(crate) fn choose_samples(&mut self, ctx: &egui::Context) {
        let Some(device) = self.chosen_sampler() else {
            return;
        };
        if self.dialog.is_some() {
            return;
        }
        let owner = self.owner;
        let show = move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Load samples")
                .add_filter("Audio", crate::media::EXTENSIONS);
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            crate::app::projects::FileDialog::SamplerFiles {
                paths: dialog.pick_files().unwrap_or_default(),
                device,
            }
        };
        self.file_dialog(ctx, show, move |paths| {
            crate::app::projects::FileDialog::SamplerFiles { paths, device }
        });
    }

    /// Files chosen for `device`: each gets a media id and decodes, and the
    /// ones that arrive join the sampler as one undo step.
    pub(crate) fn load_sample_files(&mut self, device: Id, paths: Vec<std::path::PathBuf>) {
        let paths: Vec<std::path::PathBuf> =
            paths.into_iter().filter(|path| path.is_file()).collect();
        if paths.is_empty() {
            return;
        }
        let rate = self.lanes.rate();
        let mut ids = Vec::new();
        for path in paths {
            let id = self.document.fresh_id();
            self.lanes.samples.load(id, path, rate);
            ids.push(id);
        }
        self.sampler_load.start(device, ids);
    }

    /// Reads what the sample decoders answered, and joins the files to their
    /// sampler once all have. Called every frame.
    pub(crate) fn poll_sampler(&mut self, ctx: &egui::Context) {
        self.lanes.samples.update(self.lanes.rate());
        if !self.sampler_load.is_loading() {
            // What a project's own samples decoded to is already held by the
            // library: the list of arrivals is for the file choice only, and
            // is emptied so it cannot grow.
            self.lanes.samples.take_ready();
            return;
        }
        let Some(arrived) = self.sampler_load.take_ready(&mut self.lanes.samples) else {
            ctx.request_repaint();
            return;
        };
        let face = self
            .document
            .sampler_sound(arrived.device)
            .map_or(Face::OneShot, |held| held.face);
        let count = arrived.files.len();
        if count > 0 {
            match self.document.add_samples(arrived.device, &arrived.files) {
                Ok(_) => {
                    let what = match (face, count) {
                        (Face::OneShot, _) => "the sample".to_owned(),
                        (Face::Zones, 1) => "1 zone".to_owned(),
                        (Face::Zones, many) => format!("{many} zones"),
                        (Face::Pads, 1) => "1 pad".to_owned(),
                        (Face::Pads, many) => format!("{many} pads"),
                    };
                    self.say(format!("Loaded {what} into Shutter."));
                }
                Err(refusal) => self.say(refusal.to_string()),
            }
        }
        for why in arrived.failed {
            self.say(why);
        }
        ctx.request_repaint();
    }

    /// Cycles the chosen sampler's face (One shot, Zones, Pads), one undo
    /// step: the behaviour is reachable before the drawn face exists.
    pub(crate) fn next_sampler_face(&mut self) {
        let Some(device) = self.chosen_sampler() else {
            return;
        };
        match self.document.next_sampler_face(device) {
            Ok(face) => self.say(format!("Shutter shows {}.", face.name())),
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    /// Whether One shot loops, read from the chosen sampler.
    pub(crate) fn sampler_loops(&self) -> bool {
        self.chosen_sampler()
            .and_then(|device| self.document.sampler_sound(device))
            .is_some_and(|held| held.one_shot.looping)
    }

    /// Whether One shot plays backwards.
    pub(crate) fn sampler_reversed(&self) -> bool {
        self.chosen_sampler()
            .and_then(|device| self.document.sampler_sound(device))
            .is_some_and(|held| held.one_shot.reversed)
    }

    /// Turns One shot's loop (or its reverse) on or off.
    pub(crate) fn set_sampler_loop(&mut self, on: bool) {
        if let Some(device) = self.chosen_sampler()
            && let Err(refusal) = self.document.set_sampler_loop(device, on)
        {
            self.say(refusal.to_string());
        }
    }

    /// The same for reverse.
    pub(crate) fn set_sampler_reverse(&mut self, on: bool) {
        if let Some(device) = self.chosen_sampler()
            && let Err(refusal) = self.document.set_sampler_reverse(device, on)
        {
            self.say(refusal.to_string());
        }
    }

    /// Empties the chosen sampler's sound: one undo step. Its files stay in
    /// the media pool until they are unused and purged.
    pub(crate) fn clear_sampler(&mut self) {
        let Some(device) = self.chosen_sampler() else {
            return;
        };
        if let Err(refusal) = self.document.clear_sampler(device) {
            self.say(refusal.to_string());
        }
    }

    /// Whether the sampler's own commands can run.
    pub(crate) fn has_sampler(&self) -> bool {
        self.chosen_sampler().is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{Document, Slot};
    use lumen_project::model::{DeviceKind, PluginRef, TrackKind};

    #[test]
    fn the_sampler_goes_on_a_midi_track_and_needs_one() {
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        let midi = doc.add_track(TrackKind::Midi, None);
        let plugin = PluginRef::Builtin {
            name: lumen_devices::sampler::NAME.to_owned(),
        };
        assert_eq!(plugin.kind(), DeviceKind::Instrument);
        // On an audio track it is refused; on a MIDI track it goes in first.
        assert!(doc.add_device(audio, plugin.clone(), Slot::End).is_err());
        let device = doc
            .add_device(midi, plugin, Slot::End)
            .expect("a MIDI track");
        assert!(doc.sampler_sound(device).is_some());
        assert_eq!(doc.project().tracks[1].devices[0].id, device);
    }
}
