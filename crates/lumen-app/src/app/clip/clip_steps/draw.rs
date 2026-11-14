//! The step grid drawn: its cells, the pointer and keys on them, and the name
//! the grid gives assistive technology (M8 slice D3).

use super::*;

impl App {
    /// Draws the grid: the name column, the strip, the cells and what the
    /// pointer and the keyboard do on them.
    pub(crate) fn draw_steps(&mut self, ui: &mut Ui, frame: StepsFrame<'_>) -> StepsOut {
        let StepsFrame {
            ed,
            clip,
            lanes,
            map_id,
            map_name,
            sounds,
            editable,
            area,
            strip,
            keys,
            body,
            surface,
        } = frame;
        let ctx = ui.ctx().clone();
        let response = ui.interact(body, grid_id(clip.id), Sense::click_and_drag());
        let blank = |response: egui::Response| StepsOut {
            response,
            over_body: None,
            hover: None,
        };
        let content = lumen_project::edit::content_length(clip);
        let meter = self.transport.timeline.meter.meter_at(clip.start);
        let mut setup = ed.drum.steps.setup(clip.id);
        let Some(layout) = setup.layout(content, meter) else {
            return blank(response);
        };
        // The page the layout kept inside the clip is the page.
        let page = (layout.first / layout.count as i64) as u32;
        if page != setup.page {
            setup.page = page;
            ed.drum.steps.set_setup(clip.id, setup);
        }
        let pitches: Vec<u8> = lanes.iter().map(|lane| lane.input).collect();

        // The geometry: the page fills the body, so the editor's own
        // `x_of` agrees with the cells.
        let mut view = EditorView {
            body,
            time: geometry::time_view(body.width(), clip.start, &layout),
            clip_start: clip.start,
            row_height: ROW,
            scroll: ed.scroll,
            rows: Rows::lanes(pitches.iter().copied()),
            lanes: true,
        };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let layer = ui.layer_id();
        let on_top = |p: Pos2| ctx.layer_id_at(p) == Some(layer);
        let over_area = pointer.filter(|p| area.contains(*p) && on_top(*p));
        if over_area.is_some() {
            let (delta, ctrl) = ctx.input(|i| (i.smooth_scroll_delta, i.modifiers.command));
            if delta.y != 0.0 && !ctrl {
                view.scroll -= delta.y;
            }
        }
        view.clamp_scroll();
        ed.scroll = view.scroll;
        ed.time = view.time;
        ed.zooms.clear();
        ed.hovered = over_area.is_some();
        let over_body = pointer.filter(|p| body.contains(*p) && on_top(*p));
        ed.pointer_at = over_body.map(|p| (view.local_at(p.x), view.pitch_at(p.y)));
        if response.clicked() || response.drag_started() {
            ed.lane.focus = false;
        }

        let grid = Grid {
            body,
            layout,
            scroll: view.scroll,
            pitches: &pitches,
        };
        ed.drum.steps.drawn = Some(Drawn {
            layout,
            body,
            row_height: ROW,
            scroll: view.scroll,
            rows: pitches.clone(),
        });

        // The ground, the names, and the empty state.
        self.drum_surface(
            ui,
            &ctx,
            surface,
            None,
            crate::app::clip::clip_drum::DrumFrame {
                ed,
                view: &view,
                clip,
                lanes,
                map_id,
                map_name,
                sounds,
                keys,
                steps: true,
            },
        );

        // The strip: the grid's own controls.
        if let Some(next) = self.steps_strip(ui, strip, clip, setup, &layout) {
            ed.drum.steps.set_setup(clip.id, next);
            ed.drum.steps.gesture = None;
        }

        if lanes.is_empty() {
            return blank(response);
        }

        let notes = notes_of(clip);
        let cells = ed.drum.steps.cells_for(clip, notes, &pitches, &layout);

        // The pointer: a press on a step starts a gesture; a vertical drag
        // sets its velocity; the release commits one undo step.
        let modifiers = ctx.input(|i| i.modifiers);
        let (pressed, down, latest, escape) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
                i.key_pressed(Key::Escape),
            )
        });
        let hover_cell = over_body.and_then(|p| grid.cell_at(p));
        if pressed
            && editable
            && ed.drum.steps.gesture.is_none()
            && let Some(p) = over_body
            && let Some((row, column)) = hover_cell
            && !grid.past_end(grid.index_of(column))
        {
            let target = Target {
                pitch: pitches[row],
                index: grid.index_of(column),
            };
            let cell = cells
                .get(row * layout.count + column)
                .copied()
                .unwrap_or_default();
            ed.drum.steps.gesture = Some(Gesture::press(
                target,
                row,
                column,
                &cell,
                p,
                modifiers.shift,
                setup.accent,
            ));
            ed.drum.steps.cursor = Some(target);
            response.request_focus();
        }
        let mut live = None;
        let mut chip = None;
        if let Some(mut gesture) = ed.drum.steps.gesture {
            if escape || !editable {
                ed.drum.steps.gesture = None;
            } else if down {
                if let Some(p) = latest {
                    gesture.drag(p);
                }
                if gesture.is_drag() {
                    live = Some(Live {
                        row: gesture.row,
                        column: gesture.column,
                        velocity: gesture.velocity(),
                    });
                    chip = Some(paint::velocity_chip(gesture.velocity(), setup.accent));
                }
                ed.drum.steps.gesture = Some(gesture);
                ctx.request_repaint();
            } else {
                ed.drum.steps.gesture = None;
                self.commit_step(ed, clip, layout.step, gesture.release(setup.accent));
            }
        }

        // What the frame shows (the document may have changed above).
        let cells = if ed.drum.steps.gesture.is_none() {
            let now = self.editor_clip(clip.id).map(|(c, _)| c);
            match now {
                Some(now) if !Arc::ptr_eq(&now, clip) => {
                    ed.drum
                        .steps
                        .cells_for(&now, notes_of(&now), &pitches, &layout)
                }
                _ => cells,
            }
        } else {
            cells
        };
        let focused = response.has_focus();
        if focused {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..EventFilter::default()
                    },
                );
            });
            if ed.drum.steps.cursor.is_none() {
                ed.drum.steps.cursor = Some(Target {
                    pitch: pitches[0],
                    index: layout.first,
                });
            }
        }
        ed.drum.steps.focused = focused;
        let cursor_cell = ed
            .drum
            .steps
            .cursor
            .filter(|_| focused)
            .and_then(|c| Some((grid.row_of(c.pitch)?, grid.column_of(c.index)?)));
        let picked = picked_cells(&grid, notes, ed.picked());
        let playing = self
            .transport
            .state()
            .playing
            .then(|| {
                let local = self.transport.playhead() - clip.start;
                if local < Ticks::ZERO {
                    return None;
                }
                let index = local.0 / layout.step.0;
                (!grid.past_end(index))
                    .then(|| grid.column_of(index))
                    .flatten()
            })
            .flatten();
        ed.drum.steps.playing = playing.map(|column| grid.index_of(column));
        let scene = Scene {
            grid,
            lanes,
            cells: &cells,
            accent: setup.accent,
            playing,
            hover: hover_cell.filter(|_| ed.drum.steps.gesture.is_none() || live.is_some()),
            cursor: cursor_cell,
            picked: &picked,
            live,
        };
        let painter = ui.painter().clone();
        paint::grid(&painter, &scene);
        paint::overlay(&painter, &scene);
        if let (Some(chip), Some(p)) = (&chip, latest) {
            widgets::draw_hint(&painter, p + vec2(14.0, -16.0), chip, body);
        }
        if playing.is_some() {
            ctx.request_repaint();
        }

        // The accessible name says where the keyboard's step is and what it
        // holds; the info line says what the pointer can do.
        let name = self.step_name(
            clip,
            lanes,
            &grid,
            ed.drum.steps.cursor,
            &cells,
            setup.accent,
        );
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, name.clone()));
        if over_body.is_some() && !Popup::is_any_open(&ctx) {
            let mut info = Info::new("Step grid")
                .on(Held::NONE, Mouse::Click, "toggle a step")
                .on(Held::NONE, Mouse::Drag, "set its velocity")
                .on(Held::SHIFT, Mouse::Click, "toggle its accent")
                .on(Held::NONE, Mouse::RightClick, "menu");
            if let Some((row, column)) = hover_cell
                && let Some(lane) = lanes.get(row)
            {
                info = Info::new(format!("{}, step {}", lane.name, grid.index_of(column) + 1))
                    .on(Held::NONE, Mouse::Click, "toggle")
                    .on(Held::NONE, Mouse::Drag, "set velocity")
                    .on(Held::SHIFT, Mouse::Click, "toggle accent")
                    .on(Held::NONE, Mouse::RightClick, "menu");
            }
            self.set_info(info);
        }
        if over_body.is_some() || ed.drum.steps.gesture.is_some() {
            ctx.set_cursor_icon(if ed.drum.steps.gesture.is_some_and(|g| g.is_drag()) {
                egui::CursorIcon::ResizeVertical
            } else if hover_cell.is_some() && editable {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            });
        }
        // The note under the pointer, for the right-click menu.
        let hover = hover_cell.and_then(|(row, column)| {
            let target = Target {
                pitch: pitches[row],
                index: grid.index_of(column),
            };
            model::owned(notes, target, layout.step)
                .find(|&i| notes[i].pitch() == target.pitch && !notes[i].muted())
                .or_else(|| {
                    model::owned(notes, target, layout.step)
                        .find(|&i| notes[i].pitch() == target.pitch)
                })
                .map(|i| (i, Zone::Body))
        });
        ed.last_view = Some(view);
        StepsOut {
            response,
            over_body,
            hover,
        }
    }

    /// "Step grid, Beat. Snare, step 5 of 16, on, velocity 100, accent": what
    /// the grid says to assistive technology, for the step the keyboard is on.
    pub(super) fn step_name(
        &self,
        clip: &Clip,
        lanes: &[Lane],
        grid: &Grid<'_>,
        cursor: Option<Target>,
        cells: &[Cell],
        accent: u8,
    ) -> String {
        let head = format!("Step grid: {}", clip.name);
        let Some(cursor) = cursor else {
            return head;
        };
        let (Some(row), Some(column)) = (grid.row_of(cursor.pitch), grid.column_of(cursor.index))
        else {
            return head;
        };
        let cell = cells
            .get(row * grid.layout.count + column)
            .copied()
            .unwrap_or_default();
        let sound = lanes.get(row).map_or("", |lane| lane.name.as_str());
        let mut text = format!(
            "{head}. {sound}, step {} of {}",
            cursor.index + 1,
            grid.layout.in_clip
        );
        if cell.on {
            text.push_str(&format!(", on, velocity {}", cell.velocity));
            if cell.velocity >= accent {
                text.push_str(", accent");
            }
            if cell.off_grid {
                text.push_str(", off the grid");
            }
        } else if cell.held {
            text.push_str(", off, hit kept muted");
        } else {
            text.push_str(", off");
        }
        text
    }
}
