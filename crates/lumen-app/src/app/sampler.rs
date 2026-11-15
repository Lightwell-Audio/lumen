use eframe::egui;
use lumen_project::model::{Face, Id, PluginRef};

use crate::App;
use crate::app::detail::DetailTab;
use crate::builtin::samples;
use crate::chain::place::{Place, place};

impl App {
    pub(crate) fn chosen_sampler(&self) -> Option<Id> {
        let card = self.chain.selected?;
        self.document.sampler_sound(card).is_some().then_some(card)
    }

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

    pub(crate) fn load_sample_files(&mut self, device: Id, paths: Vec<std::path::PathBuf>) {
        let paths: Vec<std::path::PathBuf> =
            paths.into_iter().filter(|path| path.is_file()).collect();
        if paths.is_empty() {
            return;
        }
        let rate = self.lanes.rate();
        let mut wanted: Vec<(Id, String)> = Vec::new();
        for path in paths {
            let id = self.document.fresh_id();
            self.lanes.samples.load(id, path.clone(), rate);
            wanted.push((id, samples::file_name(&path)));
        }
        self.sampler_load.start(device, wanted);
    }

    pub(crate) fn poll_sampler(&mut self, ctx: &egui::Context) {
        self.lanes.samples.update(self.lanes.rate());
        if !self.sampler_load.is_loading() {
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
                Ok(used) => {
                    let what = match (face, used) {
                        (Face::OneShot, _) => "the sample".to_owned(),
                        (Face::Zones, 1) => "1 zone".to_owned(),
                        (Face::Zones, many) => format!("{many} zones"),
                        (Face::Pads, 1) => "1 pad".to_owned(),
                        (Face::Pads, many) => format!("{many} pads"),
                    };
                    let mut words = format!("Loaded {what} into Shutter.");

                    let skipped = count.saturating_sub(used);
                    if skipped > 0 {
                        words.push_str(&format!(
                            " {skipped} {} skipped: One shot plays one sample. \
                             Choose Zones to lay them across the keyboard.",
                            if skipped == 1 {
                                "file was"
                            } else {
                                "files were"
                            }
                        ));
                    }
                    self.say(words);
                }
                Err(refusal) => self.say(refusal.to_string()),
            }
        }
        for why in arrived.failed {
            self.say(why);
        }
        ctx.request_repaint();
    }

    pub(crate) fn next_sampler_face(&mut self) {
        let Some(device) = self.chosen_sampler() else {
            return;
        };
        match self.document.next_sampler_face(device) {
            Ok(face) => self.say(format!("Shutter shows {}.", face.name())),
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    pub(crate) fn sampler_loops(&self) -> bool {
        self.chosen_sampler()
            .and_then(|device| self.document.sampler_sound(device))
            .is_some_and(|held| held.one_shot.looping)
    }

    pub(crate) fn sampler_reversed(&self) -> bool {
        self.chosen_sampler()
            .and_then(|device| self.document.sampler_sound(device))
            .is_some_and(|held| held.one_shot.reversed)
    }

    pub(crate) fn set_sampler_loop(&mut self, on: bool) {
        if let Some(device) = self.chosen_sampler()
            && let Err(refusal) = self.document.set_sampler_loop(device, on)
        {
            self.say(refusal.to_string());
        }
    }

    pub(crate) fn set_sampler_reverse(&mut self, on: bool) {
        if let Some(device) = self.chosen_sampler()
            && let Err(refusal) = self.document.set_sampler_reverse(device, on)
        {
            self.say(refusal.to_string());
        }
    }

    pub(crate) fn clear_sampler(&mut self) {
        let Some(device) = self.chosen_sampler() else {
            return;
        };
        if let Err(refusal) = self.document.clear_sampler(device) {
            self.say(refusal.to_string());
        }
    }

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

        assert!(doc.add_device(audio, plugin.clone(), Slot::End).is_err());
        let device = doc
            .add_device(midi, plugin, Slot::End)
            .expect("a MIDI track");
        assert!(doc.sampler_sound(device).is_some());
        assert_eq!(doc.project().tracks[1].devices[0].id, device);
    }
}
