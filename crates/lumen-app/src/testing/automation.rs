use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    pub fn set_pass_pacing(&mut self, every: std::time::Duration, autosave: std::time::Duration) {
        self.lane_commits.set_pacing(every, autosave);
    }

    pub fn give_lane(
        &mut self,
        track: Id,
        target: lumen_project::automation::Target,
        lane: lumen_core::automation::Lane,
    ) -> Result<(), &'static str> {
        self.document
            .set_lane(track, target, lane, false)
            .map_err(|refusal| refusal.reason())
    }

    pub fn switch_lane(
        &mut self,
        track: Id,
        target: lumen_project::automation::Target,
        on: bool,
    ) -> Result<(), &'static str> {
        self.document
            .set_lane_enabled(track, target, on)
            .map_err(|refusal| refusal.reason())
    }

    pub fn lane_taken_over(&self, track: Id, target: lumen_project::automation::Target) -> bool {
        self.document.overrides().contains(track, target)
    }

    pub fn set_track_volume(&mut self, track: Id, db: f32) {
        self.document.set_volume(track, db);
    }

    pub fn set_automation_mode(&mut self, track: Id, mode: &str) {
        use crate::recording::auto_record::Mode;
        let mode = Mode::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(mode))
            .expect("a mode: read, touch, latch or write");
        self.arrangement.auto.set_mode(track, mode);
    }

    pub fn controller_learning(&self) -> Option<(Id, lumen_project::automation::Target)> {
        self.controllers.learning
    }

    pub fn give_mapping(
        &mut self,
        track: Id,
        target: lumen_project::automation::Target,
        channel: u8,
        number: u8,
    ) -> Result<(), &'static str> {
        self.document
            .set_mapping(track, target, channel, number)
            .map_err(|refusal| refusal.reason())
    }

    pub fn begin_learning(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.start_learning(track, target);
    }

    pub fn mappings_window_open(&self) -> bool {
        self.controllers.window
    }

    pub fn controllers_dropped(&self) -> u64 {
        self.controllers.dropped()
    }

    pub fn automation_mode(&self, track: Id) -> &'static str {
        self.arrangement.auto.mode_of(track).name()
    }

    pub fn shows_lane_overridden(
        &self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> bool {
        lumen_project::routing::all_tracks(self.document.project())
            .find(|t| t.id == track)
            .is_some_and(|t| self.arrangement.taken_over(&self.document, t, target))
    }

    pub fn recording_automation(
        &self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> bool {
        self.arrangement.auto.recording.contains(&(track, target))
    }

    pub fn pretend_plugin_edits(&mut self, device: Id, edits: &[(u32, f64, &str)]) {
        use lumen_hosting::{Edit, EditKind};
        let edits = edits
            .iter()
            .map(|&(id, value, kind)| {
                let kind = match kind {
                    "begin" => EditKind::Begin,
                    "perform" => EditKind::Perform,
                    "end" => EditKind::End,
                    other => panic!("an edit: begin, perform or end, not {other}"),
                };
                (Edit { id, value, kind }, true)
            })
            .collect();
        self.hosting.pretend_edits(device, edits);
    }

    pub fn take_over_lane(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.document.override_lane(track, target);
    }

    pub fn reenable_lane(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.document.reenable_lane(track, target);
    }

    pub fn take_lane_away(
        &mut self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> Result<(), &'static str> {
        self.document
            .remove_lane(track, target)
            .map_err(|refusal| refusal.reason())
    }

    pub fn automation_shown(&self, track: Id) -> Option<lumen_project::automation::Target> {
        self.arrangement.auto.shown.get(&track).copied()
    }

    pub fn automation_row(&self, ctx: &egui::Context, track: Id) -> Option<egui::Rect> {
        self.lane_frame_of(ctx, track).map(|(_, frame)| frame.row)
    }

    pub fn automation_spot(
        &self,
        ctx: &egui::Context,
        track: Id,
        quarters: f64,
        value: f32,
    ) -> Option<egui::Pos2> {
        let (target, frame) = self.lane_frame_of(ctx, track)?;
        Some(egui::pos2(
            frame.x(crate::ruler::ticks_at(quarters)),
            frame.y(target, value),
        ))
    }

    pub fn automation_point_pos(
        &self,
        ctx: &egui::Context,
        track: Id,
        index: usize,
    ) -> Option<egui::Pos2> {
        let (target, frame) = self.lane_frame_of(ctx, track)?;
        let held = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)?;
        let point = held.lane_view(target)?.lane.points().get(index).copied()?;
        Some(frame.pos(target, &point))
    }

    pub fn automation_picked(&self) -> Option<(Id, Vec<usize>)> {
        self.arrangement
            .auto
            .picks
            .as_ref()
            .map(|p| (p.track, p.list()))
    }

    pub fn automation_tip(&self) -> Option<String> {
        self.arrangement.auto.tip.clone()
    }

    pub fn automation_tool(&self) -> String {
        use crate::arrangement::auto_state::{Tool, wave_name};
        match self.arrangement.auto.tool {
            Tool::Pointer => "Pointer".to_owned(),
            Tool::Pencil => "Pencil".to_owned(),
            Tool::Shape(wave) => wave_name(wave).to_owned(),
        }
    }

    pub fn automation_bend_pos(
        &self,
        ctx: &egui::Context,
        track: Id,
        index: usize,
    ) -> Option<egui::Pos2> {
        let (target, frame) = self.lane_frame_of(ctx, track)?;
        let held = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)?;
        frame.bend_dot(target, &held.lane_view(target)?.lane, index)
    }

    fn lane_frame_of(
        &self,
        ctx: &egui::Context,
        track: Id,
    ) -> Option<(
        lumen_project::automation::Target,
        crate::arrangement::auto_view::Frame,
    )> {
        self.arrangement.lane_frame(
            ctx,
            self.canvas,
            self.document.project(),
            self.lanes.pending().len(),
            self.nav.view,
            track,
        )
    }

    pub fn show_clip_envelope(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.arrangement.auto.show(track, target);
    }

    pub fn lock_envelopes(&self) -> bool {
        self.settings.editing.lock_envelopes
    }

    pub fn song_lanes_shown(&self) -> bool {
        self.song.shown
    }

    pub fn song_tempo_spot(&self, quarters: f64, bpm: f64) -> Option<egui::Pos2> {
        let (band, scale) = self.song.drawn?;
        Some(egui::pos2(
            band.tempo.x(crate::ruler::ticks_at(quarters)),
            band.tempo.y(scale, bpm),
        ))
    }

    pub fn song_chip_spot(&self, index: usize) -> Option<egui::Pos2> {
        let (band, _) = self.song.drawn?;
        let point = self.document.project().meter.get(index).copied()?;
        Some(crate::song_lanes::scale::chip(&band.meter, &point).center())
    }

    pub fn song_meter_spot(&self, quarters: f64) -> Option<egui::Pos2> {
        let (band, _) = self.song.drawn?;
        Some(egui::pos2(
            band.meter.x(crate::ruler::ticks_at(quarters)),
            band.meter.rect.center().y,
        ))
    }

    pub fn song_picked(&self) -> Option<(&'static str, usize)> {
        let p = self.song.picked?;
        Some((
            match p.which {
                crate::song_lanes::Which::Tempo => "tempo",
                crate::song_lanes::Which::Meter => "meter",
            },
            p.index,
        ))
    }

    pub fn song_typing_open(&self) -> bool {
        self.song.typing.is_some()
    }

    pub fn give_song_maps(&mut self, tempos: &[(i64, f64)], meters: &[(i64, u32, u32)]) {
        for &(quarters, bpm) in tempos {
            self.document
                .put_tempo_change(Ticks::quarters(quarters), bpm, None);
        }
        for &(quarters, beats, unit) in meters {
            self.document
                .put_meter_change(Ticks::quarters(quarters), beats, unit);
        }
    }

    pub fn ramp_song_tempo(&mut self, index: usize) {
        self.document.set_tempo_ramp(index, true);
    }

    pub fn show_song_lanes(&mut self, shown: bool) {
        if self.song.shown != shown {
            self.song_lanes_toggle();
        }
    }
}
