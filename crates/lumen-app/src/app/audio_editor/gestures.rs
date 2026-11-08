//! What the pointer does on the audio editor's wave (M9.5 S2a): trims,
//! fades, the gain line and slip, as the arrangement's own edits
//! (`app::handles`, M9.5 D10) on the editor's view. A press on a handle wins
//! over the wave; Ctrl+Alt slips anywhere; the dimmed file slips; a drag on
//! the bright part picks a range and a click there places the edit cursor
//! (S2b, `range`).

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Modifiers, PointerButton, Pos2, Rect, Response};
use lumen_project::model::{Clip, Id};

use crate::App;
use crate::app::handles::{GestureFrame, TrimStep};
use crate::app::info::{Held, Info, Mouse};
use crate::arrangement::handles::Handle;
use crate::arrangement::select::{Edge, Gesture};
use crate::audio_editor::handles::{self, EditorHandles, Part, WaveHit};
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::{AudioEditor, Fit};
use crate::commands::Command;
use crate::timeview::{Limits, TimeView};

/// The wave as the pointer meets it this frame.
pub(super) struct WaveArea<'a> {
    pub body: Rect,
    pub file: &'a WholeFile,
    pub limits: Limits,
    pub rate: u32,
    /// The file's audio and its media key (a reversed twin's when reversed):
    /// what zero crossings read.
    pub audio: &'a lumen_engine::ClipAudio,
    pub key: u64,
}

impl App {
    /// The opened clip becomes the selection, as a handle gesture on the
    /// arrangement makes it (M9.5 D12).
    pub(super) fn select_opened_clip(&mut self, id: Id) {
        self.arrangement.clips = BTreeSet::from([id]);
        self.arrangement.range = None;
        self.selected_clips_now();
    }

    /// Whether the clip `id` takes edits; says why not when it does not.
    pub(super) fn opened_clip_takes_edits(&mut self, id: Id) -> bool {
        let project = self.document.project();
        let found = project.tracks.iter().find_map(|track| {
            let clip = track.clips.iter().find(|c| c.id == id)?;
            Some((track.frozen.is_some(), track.name.clone(), clip.locked))
        });
        match found {
            Some((true, track, _)) => {
                self.say_frozen(&track);
                false
            }
            Some((false, _, true)) => {
                self.say(
                    "This clip is locked, so it can't be changed. \
                     Unlock it from its right-click menu first"
                        .to_owned(),
                );
                false
            }
            Some((false, _, false)) => true,
            None => false,
        }
    }

    /// The clip `id` cannot be edited now: locked, or on a frozen track.
    pub(super) fn opened_clip_fixed(&self, id: Id) -> bool {
        self.document.project().tracks.iter().any(|track| {
            track
                .clips
                .iter()
                .any(|c| c.id == id && (c.locked || track.frozen.is_some()))
        })
    }

    /// The wave's pointer work, before anything is drawn: drags start,
    /// continue and end here, a click places the edit cursor, and the
    /// right-click menu opens. `taken`: the envelope took this press. Returns
    /// what the pointer is on.
    pub(super) fn audio_wave_pointer(
        &mut self,
        response: &Response,
        id: Id,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
        taken: bool,
    ) -> Option<WaveHit> {
        let ctx = response.ctx.clone();
        let clip = self.shown_clip(id)?;
        let handles = EditorHandles::of(&clip, area.body, ed.time)?.hiding_gain(ed.pill);
        if !taken
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
        {
            let modifiers = ctx.input(|i| i.modifiers);
            let slip = modifiers.command && modifiers.alt;
            ed.gesture = match handles.hit(origin, area.file, ed.time) {
                // The part played: a range from here (M9.5 WD A).
                WaveHit::Played if !slip => self.start_wave_range(id, origin, area, ed),
                _ => self.start_wave_gesture(&clip, &handles, origin, modifiers, area, ed.time),
            };
        }
        if !taken
            && response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            match handles.hit(p, area.file, ed.time) {
                WaveHit::Played => self.place_wave_cursor(&ctx, id, p, area, ed),
                WaveHit::Part(_) => {}
                _ => {
                    ed.cursor = None;
                    ed.anchor = None;
                }
            }
        }
        if let Some(gesture) = ed.gesture.take()
            && ctx.input(|i| i.pointer.primary_down())
            && let Some(p) = ctx.input(|i| i.pointer.latest_pos())
        {
            ed.gesture = self.continue_wave_gesture(&ctx, gesture, p, area, ed);
        }
        let pointer = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| area.body.contains(*p));
        // The handles as they are now: a gesture above may have moved them.
        let handles = self
            .shown_clip(id)
            .and_then(|c| EditorHandles::of(&c, area.body, ed.time))?
            .hiding_gain(ed.pill);
        let hit = pointer.map(|p| handles.hit(p, area.file, ed.time));
        if response.double_clicked()
            && let Some(WaveHit::Part(Part::Handle(handle))) = hit
            && self.opened_clip_takes_edits(id)
        {
            self.select_opened_clip(id);
            self.reset_clip_handle(id, handle);
        }
        if response.secondary_clicked() {
            // A range under the pointer stays: the menu then acts on it.
            let in_range = response.interact_pointer_pos().is_some_and(|p| {
                self.range_in_editor(id, area.file)
                    .is_some_and(|(a, b)| (a..b).contains(&ed.time.beat_at(area.body.left(), p.x)))
            });
            if !in_range && !self.arrangement.clips.contains(&id) {
                self.select_opened_clip(id);
            }
            self.menu_for = crate::app::editing::MenuFor::Clips;
            ed.menu_part = match hit {
                Some(WaveHit::Part(part)) => Some(part),
                _ => None,
            };
        }
        let shapes = ed
            .menu_part
            .and_then(Part::fade_edge)
            .filter(|_| !self.opened_clip_fixed(id));
        crate::widgets::Menu::context(response).show(|ui| match shapes {
            Some(edge) => self.fade_shape_rows(ui, id, edge),
            None => self.lanes_menu(ui),
        });
        hit
    }

    /// What a drag starting at `origin` does, when it does anything.
    fn start_wave_gesture(
        &mut self,
        clip: &Clip,
        handles: &EditorHandles,
        origin: Pos2,
        modifiers: Modifiers,
        area: &WaveArea<'_>,
        time: TimeView,
    ) -> Option<Gesture> {
        let hit = handles.hit(origin, area.file, time);
        let slip = modifiers.command && modifiers.alt;
        let frame = GestureFrame::new(time, area.body);
        if !slip && !matches!(hit, WaveHit::Part(_) | WaveHit::File) {
            return None;
        }
        if !self.opened_clip_takes_edits(clip.id) {
            return None;
        }
        let gesture = match hit {
            WaveHit::Part(Part::Trim(edge)) if !slip => self.trim_gesture(
                clip,
                edge,
                edge == Edge::End && modifiers.shift,
                origin.x,
                frame,
            ),
            WaveHit::Part(Part::Handle(handle)) if !slip => {
                self.handle_gesture(clip, handle, &handles.fades, origin)?
            }
            _ => Gesture::Slip {
                base: Arc::new(self.document.project().clone()),
                clip: clip.id,
                at: frame.ticks_at(origin.x),
            },
        };
        self.select_opened_clip(clip.id);
        Some(gesture)
    }

    /// One frame of a drag at `p`: the view first, when the pointer is at its
    /// side (M9.5 D29), then the edit, redone from where the drag began for
    /// the view as it is now.
    fn continue_wave_gesture(
        &mut self,
        ctx: &egui::Context,
        gesture: Gesture,
        p: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) -> Option<Gesture> {
        if matches!(
            gesture,
            Gesture::Trim { .. } | Gesture::Fade { .. } | Gesture::Range { .. }
        ) {
            let dt = ctx.input(|i| i.stable_dt);
            let from = ctx.input(|i| i.pointer.press_origin()).map_or(p.x, |o| o.x);
            let pull = handles::edge_scroll(p.x, from, area.body.x_range(), dt);
            let moved = ed.time.scrolled(pull, area.limits);
            if moved != ed.time {
                ed.time = moved;
                // Looking around pauses Follow, as in the arrangement.
                self.transport.follow_paused = true;
                ctx.request_repaint();
            }
        }
        let frame = GestureFrame::new(ed.time, area.body);
        // Over the wave, which is drawn after this.
        let overlay = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("audio editor drag"),
            ))
            .with_clip_rect(area.body);
        match gesture {
            Gesture::Trim {
                base,
                clip,
                edge,
                looping,
                grab,
            } => {
                let free = ctx.input(|i| i.modifiers.alt);
                self.step_audio_trim(
                    ctx,
                    &overlay,
                    &TrimStep {
                        base: &base,
                        clip,
                        edge,
                        looping,
                        grab,
                    },
                    p,
                    (frame, free),
                    (area, ed),
                );
                Some(Gesture::Trim {
                    base,
                    clip,
                    edge,
                    looping,
                    grab,
                })
            }
            Gesture::Slip { base, clip, at } => {
                self.step_slip(ctx, &base, clip, at, p, frame);
                Some(Gesture::Slip { base, clip, at })
            }
            Gesture::Range { at, track } => {
                self.step_wave_range(ctx, &overlay, (at, track), p, area, ed);
                Some(Gesture::Range { at, track })
            }
            handle @ (Gesture::Fade { .. } | Gesture::Bend { .. } | Gesture::Gain { .. }) => self
                .continue_handle_gesture(ctx, handle, p, frame, &overlay)
                .map(|step| step.gesture),
            other => Some(other),
        }
    }

    /// One frame of a trim: the arrangement's own step, except that the
    /// edge then settles on a zero crossing when that is on (M9.5 ZC A).
    fn step_audio_trim(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        step: &TrimStep<'_>,
        p: Pos2,
        (frame, free): (GestureFrame, bool),
        (area, ed): (&WaveArea<'_>, &mut AudioEditor),
    ) {
        let at = frame.ticks_at(p.x) + step.grab;
        let snapped = self.snapped_at_zoom(at, free, frame.view.zoom);
        let (to, _) = self.crossing_at(ctx, ed, area, snapped, free);
        self.document.trim_clip(
            step.base,
            step.clip,
            step.edge,
            to,
            step.looping,
            Some("clip trim"),
        );
        if let Some(c) = self.clip(step.clip) {
            let side = match step.edge {
                Edge::Start => c.start,
                Edge::End => c.start + c.length,
            };
            crate::app::grid::draw_snap_guide(painter, frame.x(side), frame.lane);
        }
        ctx.set_cursor_icon(if step.looping {
            CursorIcon::ResizeEast
        } else {
            CursorIcon::ResizeHorizontal
        });
    }

    /// The cursor and the info line over the wave (M9.5 D18).
    pub(super) fn audio_wave_info(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        hit: Option<WaveHit>,
        p: Pos2,
        area: &WaveArea<'_>,
        time: TimeView,
    ) {
        let q = time.beat_at(area.body.left(), p.x);
        let place = area.file.frame_at(q).map_or_else(
            || "Outside the file".to_owned(),
            |frame| {
                format!(
                    "File {}",
                    paint::seconds_text(frame / f64::from(area.rate.max(1)), 0.001)
                )
            },
        );
        if self.opened_clip_fixed(id) {
            self.set_info(Info::new(format!(
                "{place} \u{00B7} this clip can't change now"
            )));
            return;
        }
        let slipping = ctx.input(|i| i.modifiers.command && i.modifiers.alt);
        if slipping {
            ctx.set_cursor_icon(CursorIcon::ResizeColumn);
        }
        let info = match hit {
            Some(WaveHit::Part(Part::Trim(edge))) if !slipping => {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                let side = match edge {
                    Edge::Start => "Clip start",
                    Edge::End => "Clip end",
                };
                let info = Info::new(side).on(Held::NONE, Mouse::Drag, "trim").on(
                    Held::ALT,
                    Mouse::Drag,
                    "ignore the grid",
                );
                match edge {
                    Edge::Start => info,
                    Edge::End => info.on(Held::SHIFT, Mouse::Drag, "loop to here"),
                }
            }
            Some(WaveHit::Part(Part::Handle(handle))) if !slipping => {
                self.handle_info(ctx, id, handle).unwrap_or_default()
            }
            Some(WaveHit::File) => {
                ctx.set_cursor_icon(CursorIcon::ResizeColumn);
                Info::new(place)
                    .on(Held::NONE, Mouse::Drag, "slide the audio in the clip")
                    .on(Held::NONE, Mouse::RightClick, "clip menu")
            }
            Some(WaveHit::Played) => Info::new(place)
                .on(Held::NONE, Mouse::Click, "place the edit cursor")
                .on(Held::NONE, Mouse::Drag, "pick a range")
                .on(Held::ALT, Mouse::Drag, "ignore the grid")
                .on(Held::CTRL_ALT, Mouse::Drag, "slide the audio in the clip")
                .on(Held::NONE, Mouse::RightClick, "clip menu"),
            _ => Info::new(place)
                .key(Command::ZoomIn, "zoom in")
                .key(Command::ZoomToSelection, "zoom to the clip")
                .key(Command::AudioEditorWholeFile, "whole file"),
        };
        self.set_info(info);
    }

    /// What is hot in the wave: what is being dragged, else under the
    /// pointer, else what has the keyboard.
    pub(super) fn audio_hot_part(
        ed: &AudioEditor,
        hit: Option<WaveHit>,
        focused: Option<Part>,
    ) -> Option<Part> {
        let dragged = ed.gesture.as_ref().and_then(|g| match g {
            Gesture::Trim { edge, .. } => Some(Part::Trim(*edge)),
            Gesture::Fade { edge, .. } => Some(Part::Handle(Handle::Fade(*edge))),
            Gesture::Bend { edge, .. } => Some(Part::Handle(Handle::Bend(*edge))),
            Gesture::Gain { .. } => Some(Part::Handle(Handle::Gain)),
            _ => None,
        });
        dragged
            .or(match hit {
                Some(WaveHit::Part(part)) => Some(part),
                _ => None,
            })
            .or(focused)
    }

    /// Asks the opened audio clip's editor for `fit` on its next frame, and
    /// shows it.
    pub(crate) fn audio_editor_fit(&mut self, fit: Fit) {
        self.audio_editor.fit = Some(fit);
        self.show_detail_tab(crate::app::detail::DetailTab::Clip);
    }
}
