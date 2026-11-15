use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::NoteEdit;
use crate::App;
use crate::timeview::TimeView;

impl App {
    pub fn edit_notes(&mut self, clip: Id, selection: &[usize], edit: NoteEdit) {
        use crate::document::Resize;
        use lumen_project::edit::notes::End;
        let base = self.document.project().clone();
        match edit {
            NoteEdit::Move { time, pitch } => {
                self.document
                    .move_notes(&base, clip, selection, time, pitch, None);
            }
            NoteEdit::Delete => {
                self.document.delete_notes(clip, selection);
            }
            NoteEdit::Mute(muted) => {
                self.document.mute_notes(clip, selection, Some(muted));
            }
            NoteEdit::Lengthen(by) => {
                let resize = Resize {
                    end: End::End,
                    by,
                    min: Ticks::ZERO,
                };
                self.document
                    .resize_notes(&base, clip, selection, resize, None);
            }
        }
    }

    pub fn editor_open_clip(&self) -> Option<Id> {
        self.note_editor.clip
    }

    pub fn editor_draw_tool(&self) -> bool {
        self.note_editor.tool == crate::note_editor::Tool::Draw
    }

    pub fn editor_picked(&self) -> Vec<usize> {
        self.note_editor.picked().to_vec()
    }

    pub fn editor_popped(&self) -> bool {
        self.note_editor.popped
    }

    pub fn editor_gesture(&self) -> bool {
        self.note_editor.gesture.is_some()
    }

    pub fn clip_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Clip
    }

    pub fn devices_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Devices
    }

    pub fn mixer_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Mixer
    }

    pub fn detail_subject(&self) -> Option<Id> {
        self.subject()
    }

    pub fn opened_clip_in_detail(&self) -> Option<(Id, bool)> {
        self.opened_clip()
            .map(|id| (id, self.opened_audio() == Some(id)))
    }

    pub fn editor_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_view.map(|v| v.body)
    }

    pub fn editor_view(&self) -> Option<TimeView> {
        self.note_editor.last_view.map(|v| v.time)
    }

    pub fn editor_point(&self, local: Ticks, pitch: u8) -> Option<egui::Pos2> {
        let v = self.note_editor.last_view?;
        let y = v.row_top(i32::from(pitch)) + v.row_height / 2.0;
        Some(egui::pos2(v.x_of(local), y))
    }

    pub fn lane_shown(&self) -> bool {
        self.note_editor.last_lane.is_some() && !self.note_editor.lane.hidden
    }

    pub fn lane_name(&self) -> String {
        self.note_editor.lane.kind.name()
    }

    pub fn lane_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_lane.map(|v| v.body)
    }

    pub fn lane_point(&self, local: Ticks, value: i32) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        Some(egui::pos2(v.x_of(local), v.y_of(value)))
    }

    pub fn editor_row_height(&self) -> f32 {
        self.note_editor.row_height
    }

    pub fn set_editor_row_height(&mut self, height: f32) {
        self.note_editor.row_height = height;
        self.note_editor.framed_for = None;
    }

    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|(text, _)| text.as_str())
    }

    pub fn lane_bend_dot(&self) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        let b = self.note_editor.lane.bend?;
        Some(crate::note_editor::lane::paint::bend_dot(&v, &b))
    }

    pub fn lane_gesture(&self) -> bool {
        self.note_editor.lane.gesture.is_some()
    }

    pub fn lane_picked(&self) -> Vec<usize> {
        self.note_editor.lane.picked.clone()
    }

    pub fn editor_show_pitch(&mut self, pitch: u8) {
        if let Some(v) = self.note_editor.last_view {
            self.note_editor.scroll = crate::note_editor::view::scroll_to_centre(
                i32::from(pitch),
                v.row_height,
                v.body.height(),
            );
        }
    }

    pub fn audio_editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.audio_editor.last.as_ref()?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }

    pub fn grid_cell_at(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid_cell(t)
    }

    pub fn editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.note_editor.last_view?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }
}

pub struct PianoRollDemo {
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
}

impl PianoRollDemo {
    pub fn new(colour: usize) -> Self {
        Self {
            boxes: std::sync::Arc::new(crate::piano_roll::demo::sample_boxes(
                crate::theme::CLIP_COLORS[colour],
            )),
        }
    }

    pub fn prepare(ctx: &egui::Context, light: bool) {
        crate::fonts::install(ctx);
        let appearance = if light {
            crate::theme::Appearance::Light
        } else {
            crate::theme::Appearance::Dark
        };
        crate::theme::apply(ctx, appearance, 0.0);
    }

    pub fn show(&self, ui: &mut egui::Ui, points_per_beat: f64, row_height: f32) {
        crate::piano_roll::demo::show(ui, &self.boxes, points_per_beat, row_height);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEditorShown {
    pub body: egui::Rect,
    pub view: TimeView,
    pub window: (f64, f64),
    pub file: (f64, f64),
    pub samples: bool,
    pub played: ((f64, f64), Option<(f64, f64)>),
    pub handles: Option<AudioHandlesShown>,
    pub layout: &'static str,
    pub strip: &'static str,
    pub band: egui::Rect,
    pub brace: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionShown {
    pub working: bool,
    pub failed: bool,
    pub ready: bool,
    pub tempo: Option<f64>,
    pub transients: usize,
    pub sensitivity: u8,
    pub file: Option<u64>,
    pub first: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioHandlesShown {
    pub start: f32,
    pub end: f32,
    pub fade_in: egui::Pos2,
    pub fade_out: egui::Pos2,
    pub bend_in: Option<egui::Pos2>,
    pub bend_out: Option<egui::Pos2>,
    pub gain_y: f32,
}

impl App {
    pub fn audio_editor_shown(&self) -> Option<AudioEditorShown> {
        use crate::arrangement::select::Edge;
        use crate::audio_editor::header::Layout;
        let layout = match self.audio_editor.layout {
            Layout::Words => "words",
            Layout::Icons => "icons",
            Layout::Compact => "compact",
            Layout::More => "more",
            Layout::Least => "least",
        };
        self.audio_editor.last.map(|d| AudioEditorShown {
            body: d.body,
            view: d.time,
            window: d.window,
            file: d.file,
            samples: d.samples,
            played: (d.played.first, d.played.second),
            handles: d.handles.map(|h| AudioHandlesShown {
                start: h.start,
                end: h.end,
                fade_in: h.fades.handle_pos(Edge::Start),
                fade_out: h.fades.handle_pos(Edge::End),
                bend_in: h.fades.bend_pos(Edge::Start),
                bend_out: h.fades.bend_pos(Edge::End),
                gain_y: h.fades.gain_y(),
            }),
            layout,
            strip: match self.audio_editor.strip {
                crate::audio_editor::strip::Layout::Full => "full",
                crate::audio_editor::strip::Layout::Icons => "icons",
                crate::audio_editor::strip::Layout::Dot => "dot",
                crate::audio_editor::strip::Layout::FoldProcessing => "fold processing",
                crate::audio_editor::strip::Layout::FoldWarp => "fold warp",
                crate::audio_editor::strip::Layout::FoldPitch => "fold pitch",
                crate::audio_editor::strip::Layout::FoldFades => "fold fades",
            },
            band: d.band,
            brace: d.brace,
        })
    }

    pub fn set_audio_cursor(&mut self, at: Ticks) {
        self.audio_editor.cursor = Some(at);
    }

    pub fn audio_warp_strip(&self) -> Option<(egui::Rect, Vec<f32>)> {
        let strip = self.audio_editor.markers.drawn.as_ref()?;
        Some((strip.rect, strip.flags.clone()))
    }

    pub fn follow_listening(&self) -> usize {
        self.follow_jobs.busy()
    }

    pub fn audio_detection(&self) -> DetectionShown {
        use crate::audio_editor::detect::State;
        let detect = &self.audio_editor.detect;
        let found = match &detect.state {
            State::Ready(a) => Some(a),
            _ => None,
        };
        DetectionShown {
            working: matches!(detect.state, State::Working(_)),
            failed: matches!(detect.state, State::Failed(_)),
            ready: found.is_some(),
            tempo: found.and_then(|a| a.tempo).map(|t| t.bpm),
            transients: found.map_or(0, |a| a.transients_at(detect.sensitivity).count()),
            sensitivity: detect.sensitivity,
            file: detect.media,
            first: found
                .and_then(|a| a.transients_at(detect.sensitivity).next())
                .map(|t| t.seconds),
        }
    }

    pub fn follow_paused(&self) -> bool {
        self.transport.follow_paused
    }

    pub fn audio_editor_frame_at(&self, q: f64) -> Option<f64> {
        self.audio_editor.file.as_ref()?.file.frame_at(q)
    }

    pub fn engine_loop_sent(&self) -> (Ticks, Ticks, bool) {
        let r = self.transport.sent_loop();
        (r.start, r.end, r.enabled)
    }

    pub fn clip_loop_played(&self) -> Option<(Ticks, Ticks)> {
        self.transport
            .clip_looping()
            .then(|| self.transport.played_loop())
            .map(|r| (r.start, r.end))
    }

    pub fn audio_editor_scrubbing(&self) -> bool {
        self.audio_editor.scrub_sent.is_some()
    }

    pub fn audio_editor_popped(&self) -> bool {
        self.detail.audio_popped
    }

    pub fn resume_follow(&mut self) {
        self.transport.follow_paused = false;
    }

    pub fn locate_to(&mut self, at: Ticks) {
        self.transport.locate(at);
    }

    pub fn audio_editor_cursor(&self) -> Option<Ticks> {
        self.audio_editor.cursor
    }

    pub fn audio_editor_envelope(&self) -> Option<&'static str> {
        self.audio_editor.pill.map(|k| match k {
            lumen_project::envelope::Kind::Gain => "gain",
            lumen_project::envelope::Kind::Pan => "pan",
            lumen_project::envelope::Kind::Pitch => "pitch",
        })
    }

    pub fn audio_editor_draws(&self) -> bool {
        self.audio_editor.tool == crate::note_editor::Tool::Draw
    }

    pub fn audio_editor_picked(&self) -> Vec<usize> {
        self.audio_editor
            .hands
            .picks
            .as_ref()
            .map(|p| p.indices.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn arrangement_lane_picked(&self) -> Vec<usize> {
        self.arrangement
            .auto
            .picks
            .as_ref()
            .map(|p| p.indices.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn audio_envelope_point(&self, index: usize) -> Option<egui::Pos2> {
        let (_, frame, target) = self.audio_envelope_frame()?;
        let last = self.audio_editor.last.as_ref()?;
        let track = self
            .project()
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == last.clip))?;
        let view = track.lane_view(target)?;
        let point = view.lane.points().get(index)?;
        Some(frame.pos(target, point))
    }

    pub fn audio_envelope_y(&self, value: f32) -> Option<f32> {
        let (_, frame, target) = self.audio_envelope_frame()?;
        Some(frame.y(target, value))
    }

    fn audio_envelope_frame(
        &self,
    ) -> Option<(
        lumen_project::model::Clip,
        crate::arrangement::auto_view::Frame,
        lumen_project::automation::Target,
    )> {
        let last = self.audio_editor.last.as_ref()?;
        let kind = self.audio_editor.pill?;
        let clip = self.shown_clip(last.clip)?;
        let frame = self.envelope_frame(&clip, last.body, last.time);
        Some((
            clip,
            frame,
            lumen_project::automation::Target::envelope(last.clip, kind),
        ))
    }

    pub fn audio_crossings_waiting(&self) -> bool {
        self.audio_editor.crossings.reader.waiting()
    }

    pub fn set_zero_crossings(&mut self, on: bool) {
        self.settings.editing.zero_crossings = on;
    }

    pub fn audio_stop_focused(&self, ctx: &egui::Context) -> Option<&'static str> {
        crate::app::audio_editor::stops::focused(ctx).map(|stop| stop.key())
    }

    pub fn audio_handle_focused(&self, ctx: &egui::Context) -> Option<&'static str> {
        crate::app::audio_editor::keys::focused_stop(ctx).map(|part| part.key())
    }
}

impl App {
    pub fn hold_popped_pointer(&mut self) {
        self.popped_pointer_down = true;
    }

    pub fn drag_volume_point(&mut self, track: lumen_project::model::Id, at: i64, db: f32) {
        use lumen_core::automation::{Lane, Point};
        let lane = Lane::new(vec![Point::new(Ticks::quarters(at), db)]).expect("a lane");
        let _ =
            self.document
                .set_lane(track, lumen_project::automation::Target::Volume, lane, true);
    }
}
