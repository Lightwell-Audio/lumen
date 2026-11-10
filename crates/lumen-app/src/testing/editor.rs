//! Test seams for the clip editor (M5): the piano roll, its controller lane,
//! note edits and grid cells.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::NoteEdit;
use crate::App;
use crate::timeview::TimeView;

impl App {
    /// Edits the notes of a clip as the piano roll will (M5 slice 3a's
    /// `Document` methods), for tests of notes committed while playing.
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

    /// The clip open in the editor.
    pub fn editor_open_clip(&self) -> Option<Id> {
        self.note_editor.clip
    }

    /// The draw tool is chosen (else the pointer).
    pub fn editor_draw_tool(&self) -> bool {
        self.note_editor.tool == crate::note_editor::Tool::Draw
    }

    /// The picked notes, as indices into the open clip's notes.
    pub fn editor_picked(&self) -> Vec<usize> {
        self.note_editor.picked().to_vec()
    }

    /// The editor is in its own window.
    pub fn editor_popped(&self) -> bool {
        self.note_editor.popped
    }

    /// A gesture is in progress (a button is down in the notes).
    pub fn editor_gesture(&self) -> bool {
        self.note_editor.gesture.is_some()
    }

    /// The detail panel shows the Clip tab.
    pub fn clip_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Clip
    }

    /// The detail panel shows the Devices tab.
    pub fn devices_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Devices
    }

    /// The detail panel shows the Mixer tab.
    pub fn mixer_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Mixer
    }

    /// The track the detail panel is about (DP1).
    pub fn detail_subject(&self) -> Option<Id> {
        self.subject()
    }

    /// The clip open in the Clip tab, and whether it is an audio clip.
    pub fn opened_clip_in_detail(&self) -> Option<(Id, bool)> {
        self.opened_clip()
            .map(|id| (id, self.opened_audio() == Some(id)))
    }

    /// Where the notes are drawn, if the editor has been.
    pub fn editor_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_view.map(|v| v.body)
    }

    /// The clip editor's own horizontal view (its zoom and scroll), where it
    /// drew last: separate from [`App::view`], the arrangement's (ADR-0048).
    pub fn editor_view(&self) -> Option<TimeView> {
        self.note_editor.last_view.map(|v| v.time)
    }

    /// The point at the start of clip time `local`, in the row of `pitch`
    /// (the row's middle), where the editor drew it last.
    pub fn editor_point(&self, local: Ticks, pitch: u8) -> Option<egui::Pos2> {
        let v = self.note_editor.last_view?;
        let y = v.row_top(i32::from(pitch)) + v.row_height / 2.0;
        Some(egui::pos2(v.x_of(local), y))
    }

    /// The lane under the roll is showing (it was drawn last frame).
    pub fn lane_shown(&self) -> bool {
        self.note_editor.last_lane.is_some() && !self.note_editor.lane.hidden
    }

    /// The name of the lane's tab chosen ("Velocity", "Mod wheel", ...).
    pub fn lane_name(&self) -> String {
        self.note_editor.lane.kind.name()
    }

    /// The lane's plotting area, where it drew last.
    pub fn lane_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_lane.map(|v| v.body)
    }

    /// The point where the open lane shows `value` at clip time `local`.
    pub fn lane_point(&self, local: Ticks, value: i32) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        Some(egui::pos2(v.x_of(local), v.y_of(value)))
    }

    /// Where the bend handle's dot is, when a curve was just drawn.
    pub fn lane_bend_dot(&self) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        let b = self.note_editor.lane.bend?;
        Some(crate::note_editor::lane::paint::bend_dot(&v, &b))
    }

    /// A gesture is in progress in the lane.
    pub fn lane_gesture(&self) -> bool {
        self.note_editor.lane.gesture.is_some()
    }

    /// The picked points of the controller lane (indices into the clip's
    /// events).
    pub fn lane_picked(&self) -> Vec<usize> {
        self.note_editor.lane.picked.clone()
    }

    /// Makes `pitch` visible in the editor, as scrolling would.
    pub fn editor_show_pitch(&mut self, pitch: u8) {
        if let Some(v) = self.note_editor.last_view {
            self.note_editor.scroll = crate::note_editor::view::scroll_to_centre(
                i32::from(pitch),
                v.row_height,
                v.body.height(),
            );
        }
    }

    /// [`Self::grid_cell_at`] as the audio editor's own zoom sets it.
    pub fn audio_editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.audio_editor.last.as_ref()?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }

    /// The grid step that `t` falls in now (`[start, end)`).
    pub fn grid_cell_at(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid_cell(t)
    }

    /// [`Self::grid_cell_at`] as the piano roll's own zoom sets it.
    pub fn editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.note_editor.last_view?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }
}

/// The piano roll's body drawn on its own with a sample clip (slice 3b),
/// for the shots harness: the real roll arrives with slice 3c.
pub struct PianoRollDemo {
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
}

impl PianoRollDemo {
    /// The sample clip in track colour `colour` (an index of
    /// [`clip_colour`]'s palette).
    pub fn new(colour: usize) -> Self {
        Self {
            boxes: std::sync::Arc::new(crate::piano_roll::demo::sample_boxes(
                crate::theme::CLIP_COLORS[colour],
            )),
        }
    }

    /// Installs the typefaces and the theme (`light` or dark), as the app
    /// does at start, in a context that has no `App`.
    pub fn prepare(ctx: &egui::Context, light: bool) {
        crate::fonts::install(ctx);
        let appearance = if light {
            crate::theme::Appearance::Light
        } else {
            crate::theme::Appearance::Dark
        };
        crate::theme::apply(ctx, appearance, 0.0);
    }

    /// Draws the roll over all of `ui`'s free area.
    pub fn show(&self, ui: &mut egui::Ui, points_per_beat: f64, row_height: f32) {
        crate::piano_roll::demo::show(ui, &self.boxes, points_per_beat, row_height);
    }
}

/// What the audio clip editor drew last frame (M9.5 S1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEditorShown {
    /// The wave area.
    pub body: egui::Rect,
    /// The editor's own view.
    pub view: TimeView,
    /// The beats the clip plays, and the beats its file covers.
    pub window: (f64, f64),
    pub file: (f64, f64),
    /// The wave was drawn sample by sample.
    pub samples: bool,
    /// The bright parts: first, and after a wrap.
    pub played: ((f64, f64), Option<(f64, f64)>),
    /// Where the clip's handles were.
    pub handles: Option<AudioHandlesShown>,
    /// The header's layout, by name, and the properties strip's.
    pub layout: &'static str,
    pub strip: &'static str,
    /// The loop brace's band under the ruler, and its ends while the clip
    /// loops.
    pub band: egui::Rect,
    pub brace: Option<(f32, f32)>,
}

/// What detection has found for the audio editor's file (M9.5 S5a).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionShown {
    /// A worker is reading the file.
    pub working: bool,
    /// The reading failed.
    pub failed: bool,
    /// The findings are in.
    pub ready: bool,
    /// The tempo found, if there is one.
    pub tempo: Option<f64>,
    /// How many transients the sensitivity offers.
    pub transients: usize,
    /// The sensitivity, 0 to 100.
    pub sensitivity: u8,
    /// The file key the findings are for (the reversed twin's for a
    /// reversed clip).
    pub file: Option<u64>,
    /// Where the first transient on offer is, in seconds of the file shown.
    pub first: Option<f64>,
}

/// Where the audio editor's handles were last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioHandlesShown {
    /// The x of the clip's start and end, which may be out of view.
    pub start: f32,
    pub end: f32,
    pub fade_in: egui::Pos2,
    pub fade_out: egui::Pos2,
    pub bend_in: Option<egui::Pos2>,
    pub bend_out: Option<egui::Pos2>,
    pub gain_y: f32,
}

impl App {
    /// Where and what the audio editor drew last, if it has.
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
                crate::audio_editor::strip::Layout::FoldFades => "fold fades",
            },
            band: d.band,
            brace: d.brace,
        })
    }

    /// Where the audio editor's warp strip and its markers' flags were drawn
    /// last frame: `None` for a clip that is not warped (M9.5 S5b).
    pub fn audio_warp_strip(&self) -> Option<(egui::Rect, Vec<f32>)> {
        let strip = self.audio_editor.markers.drawn.as_ref()?;
        Some((strip.rect, strip.flags.clone()))
    }

    /// How many imported files are waiting to be listened to for a loop's
    /// tempo, or are being.
    pub fn follow_listening(&self) -> usize {
        self.follow_jobs.busy()
    }

    /// What detection has found for the opened clip's file.
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

    /// Whether Follow is paused, as a scroll or an editor drag pauses it.
    pub fn follow_paused(&self) -> bool {
        self.transport.follow_paused
    }

    /// The file frame the audio editor draws at beat `q`, if it draws one
    /// there.
    pub fn audio_editor_frame_at(&self, q: f64) -> Option<f64> {
        self.audio_editor.file.as_ref()?.file.frame_at(q)
    }

    /// The loop the engine was last told to play: start, end, enabled.
    pub fn engine_loop_sent(&self) -> (Ticks, Ticks, bool) {
        let r = self.transport.sent_loop();
        (r.start, r.end, r.enabled)
    }

    /// What Loop clip repeats, if it is on: start and end in the song. The
    /// project's own loop (`engine_loop`) is not this.
    pub fn clip_loop_played(&self) -> Option<(Ticks, Ticks)> {
        self.transport
            .clip_looping()
            .then(|| self.transport.played_loop())
            .map(|r| (r.start, r.end))
    }

    /// Whether the audio editor's ruler is scrubbing.
    pub fn audio_editor_scrubbing(&self) -> bool {
        self.audio_editor.scrub_sent.is_some()
    }

    /// Whether the audio editor is in its own window.
    pub fn audio_editor_popped(&self) -> bool {
        self.detail.audio_popped
    }

    /// Lets Follow run again, as Play or the transport's Follow button does.
    pub fn resume_follow(&mut self) {
        self.transport.follow_paused = false;
    }

    /// Moves the playhead to `at`, as a click on a ruler does.
    pub fn locate_to(&mut self, at: Ticks) {
        self.transport.locate(at);
    }

    /// The audio editor's edit cursor, if it has one.
    pub fn audio_editor_cursor(&self) -> Option<Ticks> {
        self.audio_editor.cursor
    }

    /// The envelope the audio editor shows: "gain", "pan" or none.
    pub fn audio_editor_envelope(&self) -> Option<&'static str> {
        self.audio_editor.pill.map(|k| match k {
            lumen_project::envelope::Kind::Gain => "gain",
            lumen_project::envelope::Kind::Pan => "pan",
        })
    }

    /// The audio editor's tool is Draw.
    pub fn audio_editor_draws(&self) -> bool {
        self.audio_editor.tool == crate::note_editor::Tool::Draw
    }

    /// The envelope points picked in the audio editor.
    pub fn audio_editor_picked(&self) -> Vec<usize> {
        self.audio_editor
            .hands
            .picks
            .as_ref()
            .map(|p| p.indices.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The points picked in the arrangement's automation lanes.
    pub fn arrangement_lane_picked(&self) -> Vec<usize> {
        self.arrangement
            .auto
            .picks
            .as_ref()
            .map(|p| p.indices.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Where the audio editor draws the shown envelope's point `index`, and
    /// the y of `value` on it.
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

    /// The y the audio editor draws `value` of the shown envelope at.
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

    /// A read for zero crossings was asked for and has not answered yet.
    pub fn audio_crossings_waiting(&self) -> bool {
        self.audio_editor.crossings.reader.waiting()
    }

    /// Turns Zero crossings on or off, as its header toggle does.
    pub fn set_zero_crossings(&mut self, on: bool) {
        self.settings.editing.zero_crossings = on;
    }

    /// The audio editor's stop with the keyboard that is not a handle, by
    /// name ("wave", "envelope", "loop", "loop start", "loop end").
    pub fn audio_stop_focused(&self, ctx: &egui::Context) -> Option<&'static str> {
        crate::app::audio_editor::stops::focused(ctx).map(|stop| stop.key())
    }

    /// The audio editor's handle with the keyboard, by name ("fade in").
    pub fn audio_handle_focused(&self, ctx: &egui::Context) -> Option<&'static str> {
        crate::app::audio_editor::keys::focused_stop(ctx).map(|part| part.key())
    }
}

impl App {
    /// Says a button is down in a pop-out editor's window, as that window's
    /// frame does: the history keeps a gesture open while it is.
    pub fn hold_popped_pointer(&mut self) {
        self.popped_pointer_down = true;
    }

    /// One drag step on `track`'s volume lane: a lane of one point at beat
    /// `at` and `db`, merged into the gesture in progress.
    pub fn drag_volume_point(&mut self, track: lumen_project::model::Id, at: i64, db: f32) {
        use lumen_core::automation::{Lane, Point};
        let lane = Lane::new(vec![Point::new(Ticks::quarters(at), db)]).expect("a lane");
        let _ =
            self.document
                .set_lane(track, lumen_project::automation::Target::Volume, lane, true);
    }
}
