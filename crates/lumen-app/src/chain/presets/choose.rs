//! The sounds a plugin offers and the one a card has: what the field shows, the
//! list, the arrows, and a frame's worth of files read, chosen and saved.

use super::state::{Choice, Current, Source};
use super::*;

impl App {
    /// What the field of `device`'s card shows; none for a card without one (a
    /// device that cannot keep presets).
    pub(crate) fn field_look(&self, device: Id) -> Option<FieldLook> {
        let (_, d) = find_device(self.document.project(), device)?;
        let usable = crate::builtin::is_builtin(d)
            || (d.is_vst3() && matches!(self.hosting.state(device), Some(State::Ready)));
        if !usable {
            return None;
        }
        let current = self.chain.presets.current.get(&device);
        Some(FieldLook {
            name: current.map(|c| c.name.clone()),
            changed: current
                .and_then(|c| c.baseline.as_ref())
                .is_some_and(|baseline| *baseline != d.params),
        })
    }

    /// The folders that may hold the presets of `device`: a plugin's under its
    /// vendor and name, a built-in's in Lumen's own folder (M8 slice B0). A
    /// built-in's folder is its stable saved name, never its personality's:
    /// renaming a personality must not orphan every preset a person saved
    /// (review L5).
    pub(super) fn preset_folders(&self, device: Id) -> Vec<(PathBuf, bool)> {
        let Some((_, d)) = find_device(self.document.project(), device) else {
            return Vec::new();
        };
        match &d.plugin {
            PluginRef::Vst3 { name, vendor, .. } => self.chain.presets.places.folders(vendor, name),
            PluginRef::Builtin { name } => self.chain.presets.places.builtin_folders(name),
        }
    }

    /// Where a sound of `device` is saved.
    pub(super) fn preset_folder(&self, device: Id) -> Option<PathBuf> {
        let (_, d) = find_device(self.document.project(), device)?;
        match &d.plugin {
            PluginRef::Vst3 { name, vendor, .. } => {
                self.chain.presets.places.user_folder(vendor, name)
            }
            PluginRef::Builtin { name } => self.chain.presets.places.builtin_folder(name),
        }
    }

    /// Asks for the preset files of a plugin that has not been asked about.
    pub(in crate::chain) fn ensure_listed(&mut self, ctx: &egui::Context, device: Id) {
        if self.chain.presets.asked.contains(&device) {
            return;
        }
        let folders = self.preset_folders(device);
        if !folders.is_empty() {
            self.chain.presets.relist(ctx, device, folders);
        }
    }

    /// The sounds that can be chosen for `device`, in the menu's order: the
    /// factory programs, then the user's files, then the system's.
    pub(super) fn choices(&self, device: Id) -> Vec<Choice> {
        let mut out = Vec::new();
        let lists = self.hosting.programs(device);
        let selectable = lists.iter().filter(|l| l.parameter.is_some()).count();
        for (list, chosen) in lists.iter().enumerate() {
            if chosen.parameter.is_none() {
                continue;
            }
            for (index, name) in chosen.programs.iter().enumerate() {
                out.push(Choice::Program {
                    list,
                    index,
                    name: name.clone(),
                    group: (selectable > 1).then(|| chosen.name.clone()),
                });
            }
        }
        if let Some(files) = self.chain.presets.listed.get(&device) {
            out.extend(files.iter().map(|e| Choice::File {
                path: e.path.clone(),
                user: e.user,
                name: e.name.clone(),
                group: e.group.clone(),
            }));
        }
        out
    }

    /// Chooses a sound for `device`.
    pub(super) fn choose_preset(&mut self, ctx: &egui::Context, device: Id, choice: Choice) {
        match choice {
            Choice::Program {
                list, index, name, ..
            } => {
                let rev =
                    find_device(self.document.project(), device).map_or(0, |(_, d)| d.state_rev);
                self.load_program(device, list, index);
                self.chain
                    .presets
                    .chose_program(device, &name, list, index, rev);
            }
            Choice::File {
                path, user, name, ..
            } => {
                self.chain
                    .presets
                    .reading
                    .insert(device, (name, path.clone(), user));
                self.chain.presets.files.read(ctx, device, path);
            }
        }
    }

    /// The previous or next sound after the one `device` has (the card's
    /// arrows), wrapping round. With none chosen, the first or the last.
    pub(in crate::chain) fn step_preset(&mut self, ctx: &egui::Context, device: Id, forward: bool) {
        let choices = self.choices(device);
        if choices.is_empty() {
            self.say("This plugin lists no presets. Open its window to choose one.".to_owned());
            return;
        }
        let at = self
            .chain
            .presets
            .current
            .get(&device)
            .and_then(|current| choices.iter().position(|c| c.is_current(current)));
        let last = choices.len() - 1;
        let to = match (at, forward) {
            (Some(i), true) => (i + 1) % choices.len(),
            (Some(0), false) => last,
            (Some(i), false) => i - 1,
            (None, true) => 0,
            (None, false) => last,
        };
        self.choose_preset(ctx, device, choices[to].clone());
    }

    /// Once a frame: files found, read and written; a save waiting for its
    /// plugin; the names of the sounds kept honest.
    pub(crate) fn poll_presets(&mut self, ctx: &egui::Context) {
        if let Some(why) = self.chain.presets.files.take_failure() {
            self.say(why);
        }
        for done in self.chain.presets.files.poll() {
            match done {
                Done::Listed { device, entries } => {
                    self.chain.presets.listed.insert(device, entries);
                }
                Done::Read {
                    device,
                    name,
                    result,
                } => match result {
                    Ok(Sound::Vst3(preset)) => self.load_preset(device, &name, preset),
                    Ok(Sound::Builtin {
                        device: for_device,
                        values,
                    }) => self.load_builtin_preset(device, &name, &for_device, values),
                    Err(why) => {
                        self.chain.presets.refused(device);
                        self.say(format!("\u{201c}{name}\u{201d} cannot be used: {why}."));
                    }
                },
                Done::Folder { folder, exists, .. } => self.folder_found(folder, exists),
                Done::Wrote {
                    device,
                    name,
                    result,
                } => match result {
                    Ok(path) => self.saved(ctx, device, path),
                    Err(why) => self.say(format!("Could not save \u{201c}{name}\u{201d}: {why}.")),
                },
            }
        }
        self.drive_save(ctx);
        self.keep_names_honest();
        if let Some(wait) = self.hosting.next_retry() {
            ctx.request_repaint_after(wait);
        }
    }

    /// A file was written: it is the card's sound now, and listed.
    fn saved(&mut self, ctx: &egui::Context, device: Id, path: PathBuf) {
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let rev = find_device(self.document.project(), device).map_or(0, |(_, d)| d.state_rev);
        self.say(format!("Saved \u{201c}{name}\u{201d}."));
        self.chain.presets.current.insert(
            device,
            Current {
                name,
                source: Source::File { path, user: true },
                baseline: None,
                rev,
            },
        );
        let folders = self.preset_folders(device);
        if !folders.is_empty() {
            self.chain.presets.relist(ctx, device, folders);
        }
    }

    /// Drops the name of a sound an undo took away, and notes the values a
    /// sound settled at so a change after it shows.
    fn keep_names_honest(&mut self) {
        let project = self.document.project();
        let hosting = &self.hosting;
        self.chain.presets.current.retain(|&device, current| {
            let Some((_, d)) = find_device(project, device) else {
                return false;
            };
            if matches!(current.source, Source::File { .. }) && d.state_rev != current.rev {
                return false;
            }
            // A program's answering edits are part of it for a moment; a
            // file is the whole sound at once.
            let settled = matches!(current.source, Source::File { .. })
                || hosting.program_loading(device).is_none();
            if current.baseline.is_none() && settled {
                current.baseline = Some(d.params.clone());
            }
            true
        });
    }

    /// Whether the chosen device has a preset field (a running plugin).
    pub(crate) fn chosen_has_presets(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| self.field_look(d).is_some())
    }
}
