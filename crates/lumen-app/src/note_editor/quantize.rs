//! Quantize and its tools in the clip editor (M5 slice 5c; the owner chose
//! ghost trails and comet sliders from the "Lumen Quantize Specimens").
//!
//! - [`Options`] are the popover's settings (Shift+Q); Q applies the last
//!   ones. They become the pure `edit::quantize::Settings` once the grid is
//!   known (`Options::settings`).
//! - While the popover is open the editor shows what Apply would do: the
//!   clip is drawn with the quantized notes (a stand-in clip with the notes
//!   in their old order, so picks keep their indices) and each note that
//!   moves leaves a dashed outline where it was and a line to where it went
//!   ([`Trail`]). The document is not touched until Apply.
//! - The tools beside it (humanize, fixed length, scale constrain, reverse)
//!   run on the picked notes, or on every note when none are picked, with
//!   fixed amounts (see `Tools`).

use std::sync::Arc;

use eframe::egui::{self, Id as EguiId, Ui, vec2};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::edit::{
    self,
    note_tools::{Humanize, Scale},
    quantize::{self, Mode},
};
use lumen_project::model::{Clip, ClipContent, Note};

use super::{NoteEditor, notes_of};
use crate::commands::Command;
use crate::widgets::{self, Choice, Dropdown, SliderKind, raised};
use crate::{App, theme};

/// Width of the popover's content.
const WIDTH: f32 = 280.0;
/// The label column.
const LABEL: f32 = 76.0;

/// A note value for the grid, as a fraction of a whole note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Division {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
}

impl Division {
    pub const ALL: [Division; 5] = [
        Division::Quarter,
        Division::Eighth,
        Division::Sixteenth,
        Division::ThirtySecond,
        Division::SixtyFourth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Division::Quarter => "1/4",
            Division::Eighth => "1/8",
            Division::Sixteenth => "1/16",
            Division::ThirtySecond => "1/32",
            Division::SixtyFourth => "1/64",
        }
    }

    fn ticks(self) -> i64 {
        PPQ / match self {
            Division::Quarter => 1,
            Division::Eighth => 2,
            Division::Sixteenth => 4,
            Division::ThirtySecond => 8,
            Division::SixtyFourth => 16,
        }
    }
}

/// Straight, triplet or dotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Kind {
    #[default]
    Straight,
    Triplet,
    Dotted,
}

/// The popover's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Options {
    /// The grid; `None` follows the editing grid (the owner's C5).
    pub grid: Option<Division>,
    pub kind: Kind,
    pub mode: Mode,
    pub strength: f32,
    pub swing: f32,
    /// 1 is off.
    pub window: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            grid: None,
            kind: Kind::Straight,
            mode: Mode::Start,
            strength: 1.0,
            swing: 0.0,
            window: 1.0,
        }
    }
}

impl Options {
    /// The pure settings, with `editing` the editing grid's step.
    pub fn settings(&self, editing: Ticks) -> quantize::Settings {
        let base = self.grid.map_or(editing.0, Division::ticks);
        let step = match self.kind {
            Kind::Straight => base,
            Kind::Triplet => base * 2 / 3,
            Kind::Dotted => base * 3 / 2,
        };
        quantize::Settings {
            step: Ticks(step.max(1)),
            mode: self.mode,
            strength: self.strength,
            swing: self.swing,
            window: self.window,
        }
    }

    /// What Q will do, short: the grid ("1/16", "1/8T", "1/16."), then only
    /// what is not the default: the mode, strength and swing.
    pub fn summary(&self, editing: Ticks) -> String {
        let base = self
            .grid
            .map_or_else(|| step_name(editing), |d| d.label().to_owned());
        let mut text = match self.kind {
            Kind::Straight => base,
            Kind::Triplet => format!("{base}T"),
            Kind::Dotted => format!("{base}."),
        };
        match self.mode {
            Mode::Start => {}
            Mode::End => text.push_str(" · ends"),
            Mode::StartAndEnd => text.push_str(" · both"),
            Mode::Length => text.push_str(" · length"),
        }
        if self.strength < 1.0 {
            text.push_str(&format!(" · {:.0}%", self.strength * 100.0));
        }
        if self.swing > 0.0 {
            text.push_str(&format!(" · swing {:.0}%", self.swing * 100.0));
        }
        text
    }

    /// The grid as the popover's dropdown names it.
    fn grid_label(&self, editing: Ticks) -> String {
        match self.grid {
            Some(d) => d.label().to_owned(),
            None => format!("Editing grid ({})", step_name(editing)),
        }
    }
}

/// A step in ticks as a note value, for the dropdown's "Editing grid".
fn step_name(step: Ticks) -> String {
    Division::ALL
        .iter()
        .find(|d| d.ticks() == step.0)
        .map_or_else(|| "custom".to_owned(), |d| d.label().to_owned())
}

/// The amounts the tools use (a popover for them would need a mock-up).
pub(crate) const HUMANIZE_TIMING: Ticks = Ticks(PPQ / 16);
pub(crate) const HUMANIZE_VELOCITY: u8 = 8;

/// A note that the preview moves: where it was and where Apply puts it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Trail {
    pub from: Note,
    pub to: Note,
}

/// What the popover shows of the clip while it is open.
#[derive(Clone)]
pub(crate) struct Shown {
    /// The clip with its notes quantized, in their old order.
    pub clip: Arc<Clip>,
    pub trails: Arc<Vec<Trail>>,
    /// How many notes were looked at, and the largest move in ticks.
    pub looked_at: usize,
    pub largest: Ticks,
}

struct Cache {
    clip: Arc<Clip>,
    targets: Vec<usize>,
    settings: quantize::Settings,
    shown: Shown,
}

/// The quantize state of the editor.
#[derive(Default)]
pub(crate) struct QuantizeState {
    pub options: Options,
    /// The toolbar button's id: Shift+Q opens the popover through it.
    pub button: Option<EguiId>,
    cache: Option<Cache>,
    /// Counts humanize presses, so each one gives different offsets.
    pub humanized: u64,
    /// The root (0 is C) and scale of "Constrain to scale".
    pub root: u8,
    pub scale: ScaleChoice,
}

/// A scale, as a `Default` for the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScaleChoice(pub Scale);

impl Default for ScaleChoice {
    fn default() -> Self {
        Self(Scale::Major)
    }
}

const ROOTS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The notes an edit works on: the picked ones, or every note.
pub(crate) fn targets(picked: &[usize], clip: &Clip) -> Vec<usize> {
    if picked.is_empty() {
        edit::notes::all(notes_of(clip))
    } else {
        picked.to_vec()
    }
}

/// Quantizes `notes` one by one (each note stands alone in the pure
/// function), so the result keeps their order.
fn quantized(clip: &Clip, targets: &[usize], s: &quantize::Settings) -> Shown {
    let notes = notes_of(clip);
    let length = edit::content_length(clip);
    let mut out = notes.to_vec();
    let mut trails = Vec::new();
    let mut largest = Ticks::ZERO;
    for &i in targets {
        let Some(&from) = notes.get(i) else { continue };
        let mut one = vec![from];
        quantize::quantize(&mut one, &[0], length, s);
        let to = one[0];
        if to != from {
            let by = Ticks((to.start() - from.start()).0.abs())
                .max(Ticks((to.end() - from.end()).0.abs()));
            largest = largest.max(by);
            trails.push(Trail { from, to });
        }
        out[i] = to;
    }
    // The clip itself with other notes: its place, loop and events stay, so
    // the roll draws it where the clip is.
    let mut stand_in = clip.clone();
    if let ClipContent::Midi { notes, .. } = &mut stand_in.content {
        *notes = out;
    }
    Shown {
        clip: Arc::new(stand_in),
        trails: Arc::new(trails),
        looked_at: targets.len(),
        largest,
    }
}

impl QuantizeState {
    /// The clip as Apply would leave it, while the popover is open; `None`
    /// when it is closed. Rebuilt only when the clip, the picks or the
    /// settings changed.
    pub fn shown(
        &mut self,
        open: bool,
        clip: &Arc<Clip>,
        picked: &[usize],
        editing: Ticks,
    ) -> Option<Shown> {
        if !open || !matches!(clip.content, ClipContent::Midi { .. }) {
            self.cache = None;
            return None;
        }
        let settings = self.options.settings(editing);
        let targets = targets(picked, clip);
        if let Some(c) = &self.cache
            && Arc::ptr_eq(&c.clip, clip)
            && c.targets == targets
            && c.settings == settings
        {
            return Some(c.shown.clone());
        }
        let shown = quantized(clip, &targets, &settings);
        self.cache = Some(Cache {
            clip: clip.clone(),
            targets,
            settings,
            shown: shown.clone(),
        });
        Some(shown)
    }
}

impl App {
    /// The editing grid's step where the clip starts.
    pub(super) fn editing_step(&self, ed: &NoteEditor, clip: &Clip) -> Ticks {
        let (a, b) = self.grid_cell_at_zoom(clip.start, ed.time.zoom);
        (b - a).max(edit::notes::MIN_NOTE)
    }

    /// Shift+Q: opens the options.
    pub(crate) fn open_quantize_options(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.note_editor.quantize.button {
            widgets::Menu::open_for(ctx, id);
        }
    }

    /// The Quantize button and its popover, in the editor's toolbar.
    pub(super) fn quantize_button(&mut self, row: &mut Ui, ed: &mut NoteEditor, clip: &Clip) {
        let response = raised::button(row, "Quantize");
        ed.quantize.button = Some(response.id);
        let keys = self.keymap.keys(Command::QuantizeOptions);
        let response = widgets::tip(
            response,
            "Quantize the notes",
            keys.as_ref(),
            Some("Q applies the last settings; this shows what it will do first"),
        );
        let editing = self.editing_step(ed, clip);
        // What Q does is told beside the button, so it is known with the
        // popover closed (owner, MT-MIDI-6: "what subdivision is it?").
        let summary = ed.quantize.options.summary(editing);
        row.label(
            egui::RichText::new(&summary)
                .monospace()
                .size(theme::size::CAPTION)
                .color(theme::p().text_secondary),
        )
        .on_hover_text(format!("Q quantizes to {summary}"));
        let id = response.id;
        let mut apply = false;
        widgets::Menu::below(&response).staying().show(|ui| {
            apply = self.quantize_popover(ui, ed, clip, editing, id);
        });
        if apply {
            widgets::Menu::close_for(&response.ctx, id);
            self.apply_note_op(ed, super::NoteOp::Quantize);
        }
    }

    /// The popover's rows. Returns whether Apply was pressed.
    fn quantize_popover(
        &mut self,
        ui: &mut Ui,
        ed: &mut NoteEditor,
        clip: &Clip,
        editing: Ticks,
        id: EguiId,
    ) -> bool {
        ui.set_width(WIDTH);
        let look = theme::controls(false);
        let o = &mut ed.quantize.options;
        let mut changed = false;

        // Grid: a dropdown and straight / triplet / dotted.
        ui.horizontal(|ui| {
            label(ui, "Grid");
            let mut names = vec![format!("Editing grid ({})", step_name(editing))];
            names.extend(Division::ALL.iter().map(|d| d.label().to_owned()));
            let chosen = o.grid.map_or(0, |g| {
                1 + Division::ALL.iter().position(|d| *d == g).unwrap_or(0)
            });
            let shown = o.grid_label(editing);
            if let Some(pick) = Dropdown::new(
                "Quantize grid",
                &shown,
                WIDTH - LABEL - theme::space::S,
            )
            .show(ui, id.with("grid"), &names, Some(chosen))
            {
                o.grid = pick.checked_sub(1).map(|i| Division::ALL[i]);
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            label(ui, "");
            let choices = [
                Choice::new(Kind::Straight, "Straight").named("Straight grid"),
                Choice::new(Kind::Triplet, "Triplet").named("Triplet grid"),
                Choice::new(Kind::Dotted, "Dotted").named("Dotted grid"),
            ];
            if let Some(kind) = widgets::segmented(ui, id.with("kind"), o.kind, &choices) {
                o.kind = kind;
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Quantize");
            let choices = [
                Choice::new(Mode::Start, "Start").named("Quantize note starts"),
                Choice::new(Mode::End, "End").named("Quantize note ends"),
                Choice::new(Mode::StartAndEnd, "Both").named("Quantize starts and ends"),
                Choice::new(Mode::Length, "Length").named("Quantize note lengths"),
            ];
            if let Some(mode) = widgets::segmented(ui, id.with("mode"), o.mode, &choices) {
                o.mode = mode;
                changed = true;
            }
        });
        for (name, default, value) in [
            ("Strength", 1.0, &mut o.strength),
            ("Swing", 0.0, &mut o.swing),
            ("Window", 1.0, &mut o.window),
        ] {
            ui.horizontal(|ui| {
                label(ui, name);
                let (rect, _) = ui.allocate_exact_size(
                    vec2(WIDTH - LABEL - theme::space::S, theme::size::CONTROL),
                    egui::Sense::hover(),
                );
                let out = widgets::comet_slider(
                    ui,
                    rect,
                    id.with(("slider", name)),
                    *value,
                    SliderKind::Percent { default },
                    name,
                    look,
                );
                if out != *value {
                    *value = out;
                    changed = true;
                }
            });
        }
        if changed {
            widgets::note_change(ui);
        }

        // What it will do.
        let picked = ed.picked().to_vec();
        let (moved, total, largest) = match ed.quantize.cache.as_ref() {
            Some(c) => (c.shown.trails.len(), c.shown.looked_at, c.shown.largest),
            None => (0, targets(&picked, clip).len(), Ticks::ZERO),
        };
        let ms = self.ticks_to_ms(largest);
        ui.add_space(theme::space::S);
        ui.label(
            egui::RichText::new(format!("{moved} of {total} notes move, largest {ms} ms"))
                .monospace()
                .size(theme::size::MONO_SMALL),
        );
        ui.label(
            egui::RichText::new(if picked.is_empty() {
                "Applies to every note in the clip"
            } else {
                "Applies to the selected notes"
            })
            .small()
            .weak(),
        );
        if changes_lengths(ed.quantize.options.mode) {
            ui.label(
                egui::RichText::new("This changes note lengths; none gets shorter than one step")
                    .small()
                    .weak(),
            );
        }
        ui.add_space(theme::space::S);
        let mut apply = false;
        ui.horizontal(|ui| {
            if raised::button(ui, "Reset").clicked() {
                ed.quantize.options = Options::default();
                widgets::note_change(ui);
            }
            if raised::primary_button(ui, "Apply").clicked() {
                apply = true;
            }
        });
        apply
    }

    /// Milliseconds a span of ticks lasts at the project's tempo here.
    fn ticks_to_ms(&self, span: Ticks) -> u32 {
        let bpm = self
            .document
            .project()
            .tempo
            .first()
            .map_or(120.0, |p| p.bpm)
            .max(1.0);
        let beats = span.0 as f64 / PPQ as f64;
        (beats * 60_000.0 / bpm).round() as u32
    }

    /// The tools' submenu entries in the roll's context menu: a row per tool
    /// and the scale choices.
    pub(super) fn tools_menu(&mut self, ui: &mut Ui, ed: &mut NoteEditor) -> Option<super::NoteOp> {
        let mut op = None;
        let keys = |s: &Self, c| s.keymap.keys(c);
        let (quantize, humanize, fixed, reverse) = (
            keys(self, Command::Quantize),
            keys(self, Command::HumanizeNotes),
            keys(self, Command::FixedLengthNotes),
            keys(self, Command::ReverseNotes),
        );
        let mut row = |ui: &mut Ui, label: &str, keys, value| {
            if widgets::menu_item(ui, label, keys, true).clicked() {
                op = Some(value);
            }
        };
        row(ui, "Quantize", quantize.as_ref(), super::NoteOp::Quantize);
        row(ui, "Humanize", humanize.as_ref(), super::NoteOp::Humanize);
        row(
            ui,
            "Fixed length",
            fixed.as_ref(),
            super::NoteOp::FixedLength,
        );
        row(ui, "Reverse", reverse.as_ref(), super::NoteOp::Reverse);
        let mut scale_op = None;
        widgets::submenu(ui, "Constrain to scale", |ui| {
            for scale in Scale::ALL {
                widgets::submenu(ui, scale.name(), |ui| {
                    for (root, name) in ROOTS.iter().enumerate() {
                        if widgets::menu_item(ui, name, None, true).clicked() {
                            scale_op = Some((root as u8, scale));
                        }
                    }
                });
            }
        });
        if let Some((root, scale)) = scale_op {
            ed.quantize.root = root;
            ed.quantize.scale = ScaleChoice(scale);
            op = Some(super::NoteOp::ScaleConstrain);
        }
        op
    }

    /// The seed for the next humanize.
    pub(super) fn humanize_with(&self, ed: &mut NoteEditor) -> Humanize {
        ed.quantize.humanized += 1;
        Humanize {
            timing: HUMANIZE_TIMING,
            velocity: HUMANIZE_VELOCITY,
            seed: ed.quantize.humanized,
        }
    }
}

/// End, Both and Length change how long notes are.
fn changes_lengths(mode: Mode) -> bool {
    mode != Mode::Start
}

fn label(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(LABEL, theme::size::CONTROL), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(theme::size::BODY),
        theme::p().text_secondary,
    );
}
