//! Test seams for the drum editor (M8 D2): where the lanes are, where a hit
//! and a lane's name are, which lanes show, and what the arrangement's drum
//! rows drew last frame.

use eframe::egui::{Pos2, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;
use crate::drum_editor::geometry;

impl App {
    /// The view the open clip's drums show: "lanes" or "keys"; `None` when the
    /// clip is not open or its track reads no drum map.
    pub fn drum_view_shown(&self) -> Option<&'static str> {
        let clip = self.editor_open_clip()?;
        match self.open_drum_view(clip)? {
            crate::drum_editor::View::Lanes => Some("lanes"),
            crate::drum_editor::View::Keys => Some("keys"),
        }
    }

    /// The drum view of `clip`'s track, if it reads a map.
    fn open_drum_view(&self, clip: Id) -> Option<crate::drum_editor::View> {
        self.clip_drum_map(clip)?;
        let track = self.track_of_clip(clip)?;
        Some(self.note_editor.drum.view(track))
    }

    /// The lanes the open clip shows, by sound name, top to bottom.
    pub fn open_drum_lanes(&self) -> Vec<String> {
        let Some(clip) = self.editor_open_clip() else {
            return Vec::new();
        };
        self.drum_lanes(clip)
            .unwrap_or_default()
            .into_iter()
            .map(|lane| lane.name)
            .collect()
    }

    /// The row of the sound `input` in the lanes the open clip shows.
    pub fn drum_lane_row(&self, input: u8) -> Option<usize> {
        let view = self.note_editor.last_view?;
        let clip = self.editor_open_clip()?;
        self.drum_lanes(clip)?
            .iter()
            .position(|lane| lane.input == input)
            .filter(|_| view.rows.row_of(i32::from(input)).is_some())
    }

    /// Where the middle of a hit's diamond is, at clip time `local` in the
    /// lane of `input`, as the lanes drew it last.
    pub fn drum_hit_point(&self, local: Ticks, input: u8) -> Option<Pos2> {
        let view = self.note_editor.last_view?;
        let row = self.drum_lane_row(input)?;
        let clip = self.editor_open_clip()?;
        let (open, _) = self.editor_clip(clip)?;
        let cell = self.grid_cell_at_zoom(open.start, view.time.zoom);
        let radius = geometry::hit_radius(&view, cell.1 - cell.0);
        let top = view.body.top() + row as f32 * view.row_height - view.scroll;
        Some(pos2(view.x_of(local) + radius, top + view.row_height / 2.0))
    }

    /// Where a lane's name is, for a click that auditions it.
    pub fn drum_name_point(&self, input: u8) -> Option<Pos2> {
        let (_, name) = self.drum_lane_name_rect(input)?;
        Some(pos2(name.left() + geometry::NAME_X + 8.0, name.center().y))
    }

    /// Where a lane's M is (`solo` false) or its S is.
    pub fn drum_toggle_point(&self, input: u8, solo: bool) -> Option<Pos2> {
        let (_, name) = self.drum_lane_name_rect(input)?;
        let (mute, solo_button) = geometry::buttons(name);
        Some(if solo {
            solo_button.center()
        } else {
            mute.center()
        })
    }

    /// Where a lane's drawing is (its reorder handle).
    pub fn drum_handle_point(&self, input: u8) -> Option<Pos2> {
        let (_, name) = self.drum_lane_name_rect(input)?;
        Some(geometry::icon_rect(name).center())
    }

    /// A lane's name column row as the lanes drew it last.
    fn drum_lane_name_rect(
        &self,
        input: u8,
    ) -> Option<(crate::note_editor::view::EditorView, eframe::egui::Rect)> {
        let view = self.note_editor.last_view?;
        let row = self.drum_lane_row(input)?;
        let row_rect = geometry::row_rect(&view, row);
        let name = eframe::egui::Rect::from_min_max(
            pos2(view.body.left() - geometry::NAME, row_rect.top()),
            pos2(view.body.left(), row_rect.bottom()),
        );
        Some((view, name))
    }

    /// Whether the open clip's track hides its unused lanes.
    pub fn hides_unused_lanes_shown(&self) -> bool {
        self.hides_unused_lanes()
    }

    /// Whether the lane of `input` is set to show its sound's length.
    pub fn lane_shows_length(&self, input: u8) -> bool {
        let Some(clip) = self.editor_open_clip() else {
            return false;
        };
        let Some(map) = self.clip_drum_map(clip) else {
            return false;
        };
        self.note_editor.drum.shows_length(map.id, input)
    }

    /// Sets the lane of `input` to show its length, as the lane's menu does.
    pub fn toggle_lane_length(&mut self, input: u8) {
        let Some(clip) = self.editor_open_clip() else {
            return;
        };
        let Some(map) = self.clip_drum_map(clip) else {
            return;
        };
        let map = map.id;
        self.note_editor.drum.toggle_length(map, input);
    }

    /// What the mapped clip `clip`'s drum rows drew on the arrangement last
    /// frame: the rows and the cells. `None` when the clip was not in view or
    /// its track reads no map.
    pub fn arrangement_drum_picture(&self, clip: Id) -> Option<(usize, usize)> {
        self.arrangement.drum_pictures.get(&clip).copied()
    }

    /// The view a drum clip opens in without a click: "lanes" by default.
    pub fn drum_view_of_track(&self, track: Id) -> &'static str {
        match self.note_editor.drum.view(track) {
            crate::drum_editor::View::Lanes => "lanes",
            crate::drum_editor::View::Keys => "keys",
        }
    }

    /// Turns the lanes' Hide unused on or off, as its menu row does.
    pub fn hide_the_unused_lanes(&mut self) {
        self.toggle_hide_unused_lanes();
    }

    /// Shows the open drum clip as the piano roll (its Keys view), as the
    /// header switch does.
    pub fn show_the_keys_view(&mut self) {
        self.set_drum_view(crate::drum_editor::View::Keys);
    }

    /// Gives the open clip's sound `input` its own quantize grid, as the map
    /// editor's Quantize column does (one undo step).
    pub fn set_lane_quantize(
        &mut self,
        input: u8,
        size: lumen_core::grid::Size,
        feel: lumen_core::grid::Feel,
    ) {
        let Some(clip) = self.editor_open_clip() else {
            return;
        };
        let Some(map) = self.clip_drum_map(clip) else {
            return;
        };
        let map = map.id;
        self.document.set_sound_quantize(
            map,
            input,
            Some(lumen_project::model::Quantize { size, feel }),
        );
    }

    /// Takes the drum map off the open clip's track, as Detach does.
    pub fn detach_the_drum_map(&mut self) {
        let Some(clip) = self.editor_open_clip() else {
            return;
        };
        let Some(track) = self.track_of_clip(clip) else {
            return;
        };
        self.document.detach_drum_maps(&[track]);
    }
}
