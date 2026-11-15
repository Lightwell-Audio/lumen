use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use lumen_project::model::Id;

use crate::App;
use crate::document::find_device;

impl App {
    pub fn callback_report(&self) -> Option<Result<(String, bool, u64), String>> {
        self.callback_report.as_ref().map(|result| match result {
            Ok(r) => Ok((r.text(), r.pass(), r.callbacks)),
            Err(why) => Err(why.clone()),
        })
    }

    pub fn device_preset(&self, device: Id) -> Option<(String, bool)> {
        let look = self.field_look(device)?;
        look.name.map(|name| (name, look.changed))
    }

    pub fn device_has_preset_field(&self, device: Id) -> bool {
        self.field_look(device).is_some()
    }

    pub fn give_device_sidechain(
        &mut self,
        device: Id,
        track: Id,
        level: f32,
        silent_seconds: f32,
    ) {
        self.hosting.pretend_sidechain_input(device);
        self.chain.sidechain.pretend(track, level, silent_seconds);
    }

    pub fn give_device_outputs(&mut self, device: Id, extra: &[&str]) {
        self.hosting.pretend_outputs(device, extra);
    }

    pub fn sidechain_light(&self, track: Id) -> f32 {
        self.chain.sidechain.level_of(track)
    }

    pub fn revealed_folders(&self) -> Vec<PathBuf> {
        self.chain.presets.revealed.clone()
    }

    pub fn revealed_files(&self) -> Vec<PathBuf> {
        self.media_pool.revealed.clone()
    }

    pub fn safe_mode(&self) -> (bool, bool) {
        (self.safe.on, self.safe.offer)
    }

    pub fn plugin_hosts(&self) -> usize {
        self.hosting.host_count()
    }

    pub fn load_one(&mut self, device: Id) {
        self.load_one_plugin(device);
    }

    pub fn device_state(&self, device: Id) -> Option<String> {
        use crate::hosting::State;
        self.hosting.state(device).map(|s| match s {
            State::Loading => "loading".to_owned(),
            State::Ready => "ready".to_owned(),
            State::Failed(why) => format!("failed: {why}"),
            State::Missing => "missing".to_owned(),
            State::Safe => "safe".to_owned(),
        })
    }

    pub fn device_promised_frames(&self, device: Id) -> Option<u32> {
        self.hosting.promised(device)
    }

    pub fn pretend_budget(
        &mut self,
        block_us: u64,
        used_us: u64,
        plugins: &[(&str, &str, u64, u64, u32)],
    ) {
        use crate::transport::budget::{Share, Snapshot};
        let us = Duration::from_micros;
        let plugins: Vec<Share> = plugins
            .iter()
            .enumerate()
            .map(|(i, &(name, place, work, waited, missed))| Share {
                device: Id(i as u64 + 1),
                name: name.into(),
                place: place.into(),
                work: us(work),
                waited: us(waited),
                missed,
            })
            .collect();
        let waited: Duration = plugins.iter().map(|p| p.waited).sum();
        let (part, whole) = lumen_engine::HOST_SHARE;
        self.transport.budget.pretend(Snapshot {
            block: us(block_us),
            used: us(used_us),
            deadline: us(block_us * part / whole),
            lumen: us(used_us).saturating_sub(waited),
            plugins,
        });
    }

    pub fn pretend_device_state(&mut self, device: Id, state: &str) {
        use crate::hosting::State;
        self.hosting.pretend(
            device,
            match state {
                "loading" => State::Loading,
                "ready" => State::Ready,
                "failed" => State::Failed(
                    "Crashed while loading: access violation, exception 0xc0000005.".into(),
                ),
                "safe" => State::Safe,
                _ => State::Missing,
            },
        );
    }

    pub fn give_device_presets(
        &mut self,
        device: Id,
        programs: &[&str],
        chosen: &str,
        changed: bool,
        files: &[(Option<&str>, &str, bool)],
    ) {
        self.hosting.pretend_programs(device, "Factory", programs);
        let params = find_device(self.document.project(), device)
            .map(|(_, d)| d.params.clone())
            .unwrap_or_default();
        let baseline = if changed {
            let mut other = params.clone();
            other.insert(9999, 1.0);
            other
        } else {
            params
        };
        self.chain
            .presets
            .pretend(device, chosen, Some(baseline), files);
    }

    pub fn open_preset_menu_of(&mut self, ctx: &egui::Context, device: Id) {
        self.open_preset_menu(ctx, device, false);
    }

    pub fn pretend_failures(&mut self, device: Id, count: usize, age_seconds: Option<u64>) {
        self.hosting.pretend_failures(
            device,
            count,
            age_seconds.map(std::time::Duration::from_secs),
        );
    }

    pub fn pretend_safe_mode(&mut self, on: bool, offer: bool) {
        self.safe.on = on;
        self.safe.offer = offer;
        self.hosting.set_safe(on);
    }

    pub fn give_device_picture(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        (width, height): (usize, usize),
        pixel: impl Fn(usize, usize) -> [u8; 4],
    ) {
        let class = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.devices.iter())
            .find(|d| d.id == device)
            .and_then(|d| match &d.plugin {
                lumen_project::model::PluginRef::Vst3 { class_id, .. } => Some(class_id.clone()),
                _ => None,
            });
        let Some(class) = class else { return };
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend(pixel(x, y));
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
        let wake = ctx.clone();
        self.chain
            .pictures
            .get_or_insert_with(|| crate::chain::Pictures::new(move || wake.request_repaint()))
            .put(ctx, &class, image);
    }

    pub fn device_picture(&self, device: Id) -> &'static str {
        let class = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.devices.iter())
            .find(|d| d.id == device)
            .and_then(|d| match &d.plugin {
                lumen_project::model::PluginRef::Vst3 { class_id, .. } => Some(class_id.clone()),
                _ => None,
            });
        match (class, &self.chain.pictures) {
            (Some(class), Some(pictures)) => pictures.state(&class),
            _ => "none",
        }
    }

    pub fn device_latency(&self, device: Id) -> Option<u32> {
        self.hosting.latency(device)
    }

    pub fn device_late_blocks(&self, device: Id) -> Option<u32> {
        self.hosting.late_blocks(device)
    }

    pub fn reload_device(&mut self, device: Id) {
        self.hosting.reload(device);
    }

    pub fn reload_device_clean(&mut self, device: Id) {
        self.hosting.reload_clean(device);
    }

    pub fn end_device_host(&mut self, device: Id) {
        self.hosting.end_plugin(device);
    }

    pub fn set_checkpoint_every(&mut self, every: Duration) {
        self.hosting.set_checkpoint_every(every);
    }

    pub fn device_keeps_failing(&self, device: Id) -> bool {
        self.hosting.keeps_failing(device)
    }

    pub fn device_last_good_age(&self, device: Id) -> Option<Duration> {
        self.hosting.last_good_age(device)
    }

    pub fn remove_device(&mut self, track: Id, device: Id) {
        let _ = self.document.remove_device(track, device);
    }

    pub fn fold_device(&mut self, device: Id) {
        self.toggle_fold_of(device);
    }

    pub fn move_device(&mut self, from: Id, device: Id, to: Id, after: Option<Id>) {
        let slot = after.map_or(crate::document::Slot::End, crate::document::Slot::After);
        let _ = self.document.move_device(from, device, to, slot);
    }

    pub fn copy_device(&mut self, from: Id, device: Id, to: Id, after: Option<Id>) -> Option<Id> {
        let slot = after.map_or(crate::document::Slot::End, crate::document::Slot::After);
        self.document.copy_device(from, device, to, slot).ok()
    }

    pub fn set_device_param(&mut self, device: Id, id: u32, value: f64) {
        self.document.override_param(device, id);
        self.set_device_param_at_rest(device, id, value);
    }

    pub fn set_device_param_at_rest(&mut self, device: Id, id: u32, value: f64) {
        let _ = self.document.set_device_params(
            device,
            &[(id, value)],
            "Change parameter",
            Some("test-parameter"),
        );
    }

    pub fn project_tail(&self) -> u64 {
        crate::builtin::tail_frames(self.project(), 48_000)
    }

    pub fn track_tail(&self, track: Id) -> u64 {
        crate::builtin::track_tail_frames(self.project(), track, 48_000)
    }

    pub fn device_programs(&self, device: Id) -> Vec<(String, Vec<String>)> {
        self.hosting
            .programs(device)
            .iter()
            .map(|l| (l.name.clone(), l.programs.clone()))
            .collect()
    }

    pub fn choose_program(&mut self, device: Id, list: usize, program: usize) {
        self.load_program(device, list, program);
    }

    pub fn load_preset_from_bytes(
        &mut self,
        device: Id,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let preset = lumen_hosting::Preset::parse(bytes).map_err(|e| e.to_string())?;
        self.load_preset(device, name, preset);
        Ok(())
    }

    pub fn device_params(&self, device: Id) -> Vec<(u32, f64)> {
        crate::document::find_device(self.document.project(), device)
            .map(|(_, d)| d.params.iter().map(|(&k, &v)| (k, v)).collect())
            .unwrap_or_default()
    }

    pub fn give_device_parameters(&mut self, device: Id, list: &[(&str, f64, &str)]) {
        self.hosting.pretend_parameters(device, list);
    }

    pub fn give_device_stepped(&mut self, device: Id, list: &[(&str, i32, f64, &[&str])]) {
        self.hosting.pretend_stepped(device, list);
    }

    pub fn strip_parameters(&self, device: Id) -> Vec<u32> {
        self.strip_ids(device)
    }

    pub fn device_choices(&self, device: Id, parameter: u32) -> Vec<String> {
        match find_device(self.document.project(), device) {
            Some((_, d)) => self.parameter_choices(d, parameter),
            None => Vec::new(),
        }
    }

    pub fn builtin_attached(&self, device: Id) -> bool {
        self.builtin.attached(device)
    }

    pub fn is_builtin(&self, device: Id) -> bool {
        find_device(self.document.project(), device)
            .is_some_and(|(_, d)| crate::builtin::is_builtin(d))
    }

    pub fn pretend_spectrum(&mut self, device: Id, db: &[f32]) {
        self.chain.pretend_spectrum = Some((device, db.to_vec()));
    }

    pub fn pretend_track_level(&mut self, peak_db: Option<f32>) {
        self.chain.pretend_level = peak_db;
    }

    pub fn pick_band(&mut self, device: Id, index: usize) {
        self.chain.bands.insert(device, index);
    }

    pub fn pretend_reduction(&mut self, device: Id, decibels: f32, level_db: f32) {
        self.builtin.pretend_reduction(
            device,
            lumen_devices::MeterReading {
                reduction_db: decibels,
                input_db: level_db,
                output_db: level_db - decibels,
            },
        );
    }

    pub fn has_meter(&self, device: Id) -> bool {
        self.builtin.has_meter(device)
    }

    pub fn builtin_reduction(&self, device: Id) -> f32 {
        self.builtin
            .reading(device)
            .map_or(0.0, |reading| reading.reduction_db)
    }

    pub fn show_on_strip(&mut self, device: Id, ids: &[u32]) {
        self.chain.shown.insert(device, ids.to_vec());
    }

    pub fn poke_plugin(&mut self, device: Id, id: u32, value: f64) {
        self.hosting.poke_param(device, id, value);
    }

    pub fn values_sent_to(&self, device: Id) -> u32 {
        self.hosting.values_sent(device)
    }

    pub fn editor_hidden(&self, device: Id) -> bool {
        self.hosting.editor_hidden(device)
    }

    pub fn editor_place(&self, device: Id) -> Option<(i32, i32, u32, u32)> {
        let class = self.hosting.class(device)?;
        let p = self.settings.plugins.editors.get(class)?;
        Some((p.x, p.y, p.width, p.height))
    }

    pub fn set_editor_place(&mut self, device: Id, place: (i32, i32, u32, u32)) {
        if let Some(class) = self.hosting.class(device).map(str::to_owned) {
            self.settings.plugins.remember_editor(
                &class,
                crate::plugins::EditorPlace {
                    x: place.0,
                    y: place.1,
                    width: place.2,
                    height: place.3,
                    dpi: 96,
                },
            );
        }
    }

    pub fn give_device_editor(&mut self, device: Id, state: &str, learn: bool) {
        use crate::hosting::EditorState;
        let state = match state {
            "open" => EditorState::Open,
            "opening" => EditorState::Opening,
            "stuck" => EditorState::NotResponding,
            "none" => EditorState::Refused("This plugin has no editor.".into()),
            _ => EditorState::Closed,
        };
        self.hosting.pretend_editor(device, state);
        if learn {
            self.chain.learning = Some(device);
        }
    }

    pub fn learning(&self, device: Id) -> bool {
        self.chain.learning == Some(device)
    }

    pub fn unread_plugins(&self) -> usize {
        self.hosting.unread()
    }

    pub fn device_param_known(&self, device: Id, id: u32) -> Option<f64> {
        self.hosting.value(device, id)
    }

    pub fn device_param_titles(&self, device: Id) -> Vec<String> {
        self.hosting
            .parameters(device)
            .iter()
            .map(|p| p.title.clone())
            .collect()
    }

    pub fn set_device_bypass(&mut self, track: Id, device: Id, bypass: bool) {
        let _ = self.document.set_device_bypass(track, device, bypass);
    }

    pub fn plugins_busy(&self) -> bool {
        self.plugins.busy()
    }

    pub fn plugin_status(&self) -> String {
        format!("{:?}", self.plugins.status())
    }

    pub fn plugin_scan_waiting(&self) -> Option<Vec<std::path::PathBuf>> {
        self.plugins.waiting().map(|w| w.folders().to_vec())
    }

    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins
            .catalog()
            .devices()
            .iter()
            .filter(|d| d.source == lumen_plugin_catalog::catalog::Source::Vst3)
            .map(|d| d.name.clone())
            .collect()
    }

    pub fn blocked_plugins(&self) -> Vec<String> {
        self.plugins
            .catalog()
            .blocked()
            .iter()
            .map(|b| b.name.clone())
            .collect()
    }

    pub fn favorite_devices(&self) -> Vec<String> {
        self.settings.plugins.favorites.clone()
    }

    pub fn choose_card(&mut self, device: Id) {
        self.chain.selected = Some(device);
    }

    pub fn chosen_card(&self) -> Option<Id> {
        self.chain.selected
    }

    pub fn recent_devices(&self) -> Vec<String> {
        self.settings.plugins.recents.clone()
    }

    pub fn pretend_plugins(&mut self, modules: &[(&str, &[(&str, &str)])], blocked: &[&str]) {
        use lumen_plugin_catalog::cache::{Cache, ClassRecord, Entry, Reason, Result};
        use lumen_plugin_catalog::stamp::Stamp;
        let stamp = Stamp {
            len: 1,
            modified_ns: 1,
        };
        let mut cache = Cache::new();
        let mut n = 0u8;
        for (vendor, classes) in modules {
            let path = format!("/pretend/{vendor}.vst3");
            let classes = classes
                .iter()
                .map(|(name, sub)| {
                    n += 1;
                    ClassRecord {
                        id: lumen_plugin_catalog::catalog::class_id(&[n; 16]),
                        name: (*name).into(),
                        category: "Audio Module Class".into(),
                        vendor: (*vendor).into(),
                        subcategories: (*sub).into(),
                        version: "1.0.0".into(),
                        sdk_version: String::new(),
                    }
                })
                .collect();
            cache.set(Entry {
                path: path.clone(),
                binary: path,
                stamp,
                result: Result::Scanned {
                    vendor: (*vendor).into(),
                    url: String::new(),
                    load_ms: 1,
                    classes,
                },
            });
        }
        for name in blocked {
            let path = format!("/pretend/{name}.vst3");
            cache.set(Entry {
                path: path.clone(),
                binary: path,
                stamp,
                result: Result::Blocked {
                    reason: Reason::TimedOut { seconds: 30 },
                    at: 1,
                },
            });
        }
        self.plugins.pretend(cache);
    }

    pub fn selected_device(&self) -> Option<String> {
        match &self.browser.view.selected {
            Some(crate::browser::view::Selection::Device(id)) => Some(id.clone()),
            _ => None,
        }
    }
}
