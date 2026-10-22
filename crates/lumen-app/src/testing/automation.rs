//! Test seams for automation (M7): lanes, taking a lane over, recording a pass,
//! controller mappings, clip envelopes and the tempo and meter lanes.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    /// Gives track `track` a lane for `target` (M7 slice A0: the lane
    /// interface arrives in slice A1, so end-to-end tests make lanes here).
    /// Returns why not when the lane is refused.
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

    /// Switches a lane on or off, as the lane's switch in slice A4 will.
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

    /// Whether the user has taken `track`'s lane for `target` over by moving
    /// the control it drives.
    pub fn lane_taken_over(&self, track: Id, target: lumen_project::automation::Target) -> bool {
        self.document.overrides().contains(track, target)
    }

    /// Moves a track's fader, as a drag does: a lane that drives it is taken
    /// over.
    pub fn set_track_volume(&mut self, track: Id, db: f32) {
        self.document.set_volume(track, db);
    }

    /// Sets what a track does with the controls that move while the song plays:
    /// "read", "touch", "latch" or "write".
    pub fn set_automation_mode(&mut self, track: Id, mode: &str) {
        use crate::auto_record::Mode;
        let mode = Mode::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(mode))
            .expect("a mode: read, touch, latch or write");
        self.arrangement.auto.set_mode(track, mode);
    }

    /// The control waiting for a controller to move, to be mapped (M7 slice A5).
    pub fn controller_learning(&self) -> Option<(Id, lumen_project::automation::Target)> {
        self.controllers.learning
    }

    /// Maps controller `number` on `channel` to `track`'s `target`, as learning
    /// it would.
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

    /// Makes `track`'s `target` wait for a controller, as its menu would.
    pub fn begin_learning(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.start_learning(track, target);
    }

    /// Whether the Mappings window is open.
    pub fn mappings_window_open(&self) -> bool {
        self.controllers.window
    }

    /// How many controller messages there was no room for.
    pub fn controllers_dropped(&self) -> u64 {
        self.controllers.dropped()
    }

    /// What a track does with the controls that move while the song plays.
    pub fn automation_mode(&self, track: Id) -> &'static str {
        self.arrangement.auto.mode_of(track).name()
    }

    /// Whether the controls and the chip show a lane as overridden: taken over
    /// and not being written.
    pub fn shows_lane_overridden(
        &self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> bool {
        lumen_project::routing::all_tracks(self.document.project())
            .find(|t| t.id == track)
            .is_some_and(|t| self.arrangement.taken_over(&self.document, t, target))
    }

    /// Whether a lane is being written now.
    pub fn recording_automation(
        &self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> bool {
        self.arrangement.auto.recording.contains(&(track, target))
    }

    /// Gives a plugin's parameter edits as if its own window made them:
    /// `(parameter, value, "begin" | "perform" | "end")` each, in order.
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

    /// Takes a lane over, as moving the control it drives does.
    pub fn take_over_lane(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.document.override_lane(track, target);
    }

    /// Lets a lane taken over play again, as the chip's Re-enable does.
    pub fn reenable_lane(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.document.reenable_lane(track, target);
    }

    /// Takes a lane away, as the lane chip's menu does.
    pub fn take_lane_away(
        &mut self,
        track: Id,
        target: lumen_project::automation::Target,
    ) -> Result<(), &'static str> {
        self.document
            .remove_lane(track, target)
            .map_err(|refusal| refusal.reason())
    }

    /// The parameter `track`'s shown lane draws, if it shows one (M7 slice A1).
    pub fn automation_shown(&self, track: Id) -> Option<lumen_project::automation::Target> {
        self.arrangement.auto.shown.get(&track).copied()
    }

    /// Where `track`'s shown lane is on screen: its whole row.
    pub fn automation_row(&self, ctx: &egui::Context, track: Id) -> Option<egui::Rect> {
        self.lane_frame_of(ctx, track).map(|(_, frame)| frame.row)
    }

    /// Where a point at `quarters` of `value` would be drawn on `track`'s
    /// shown lane, for pressing there.
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

    /// Where point `index` of `track`'s shown lane is drawn.
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

    /// The points picked on a lane: its track and their places in the lane.
    pub fn automation_picked(&self) -> Option<(Id, Vec<usize>)> {
        self.arrangement
            .auto
            .picks
            .as_ref()
            .map(|p| (p.track, p.list()))
    }

    /// The live value drawn beside the lane gesture under way, if any.
    pub fn automation_tip(&self) -> Option<String> {
        self.arrangement.auto.tip.clone()
    }

    /// The tool the lanes have in hand: "Pointer", "Pencil" or a shape's name.
    pub fn automation_tool(&self) -> String {
        use crate::arrangement::auto_state::{Tool, wave_name};
        match self.arrangement.auto.tool {
            Tool::Pointer => "Pointer".to_owned(),
            Tool::Pencil => "Pencil".to_owned(),
            Tool::Shape(wave) => wave_name(wave).to_owned(),
        }
    }

    /// Where the dot that bends the segment leaving point `index` of
    /// `track`'s shown lane is drawn, if it has one.
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

    /// Shows `target` as the lane `track` draws, as the chip's menu would.
    pub fn show_clip_envelope(&mut self, track: Id, target: lumen_project::automation::Target) {
        self.arrangement.auto.show(track, target);
    }

    /// Whether the song locks envelopes: clip edits leave automation alone.
    pub fn lock_envelopes(&self) -> bool {
        self.settings.editing.lock_envelopes
    }

    /// Whether the lanes are shown (fully, once they have settled).
    pub fn song_lanes_shown(&self) -> bool {
        self.song.shown
    }

    /// Where a tempo of `bpm` at `quarters` is drawn in the tempo lane.
    pub fn song_tempo_spot(&self, quarters: f64, bpm: f64) -> Option<egui::Pos2> {
        let (band, scale) = self.song.drawn?;
        Some(egui::pos2(
            band.tempo.x(crate::ruler::ticks_at(quarters)),
            band.tempo.y(scale, bpm),
        ))
    }

    /// The middle of the chip of time-signature change `index`.
    pub fn song_chip_spot(&self, index: usize) -> Option<egui::Pos2> {
        let (band, _) = self.song.drawn?;
        let point = self.document.project().meter.get(index).copied()?;
        Some(crate::song_lanes::scale::chip(&band.meter, &point).center())
    }

    /// An empty place in the meter lane at `quarters`.
    pub fn song_meter_spot(&self, quarters: f64) -> Option<egui::Pos2> {
        let (band, _) = self.song.drawn?;
        Some(egui::pos2(
            band.meter.x(crate::ruler::ticks_at(quarters)),
            band.meter.rect.center().y,
        ))
    }

    /// The change picked in the lanes: which lane (`"tempo"` or `"meter"`)
    /// and its index.
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

    /// Whether a box to type a tempo or a time signature is open.
    pub fn song_typing_open(&self) -> bool {
        self.song.typing.is_some()
    }

    /// Gives the song tempo changes `(quarters, bpm)` and time-signature
    /// changes `(quarters, beats, unit)`, as the lanes would make them. The
    /// maps are tidied, so a change off a bar line moves to one.
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

    /// Makes tempo change `index` ramp to the next, as its menu does.
    pub fn ramp_song_tempo(&mut self, index: usize) {
        self.document.set_tempo_ramp(index, true);
    }

    /// Shows or hides the tempo and meter lanes, as T does.
    pub fn show_song_lanes(&mut self, shown: bool) {
        if self.song.shown != shown {
            self.song_lanes_toggle();
        }
    }
}
