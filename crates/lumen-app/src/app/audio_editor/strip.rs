//! The audio editor's properties strip (M9.5 S3, `docs/milestones/M9.5.md`
//! §10): one 32 pt row under the shared header, which holds what the clip
//! card held, for the opened clip. Left to right: the Clip chip (colour and
//! name), Gain, Mute and Lock, the two Fades, Reverse, Polarity and Normalize,
//! and at the right the File chip. When the row is too narrow its groups give
//! way in the owner's order (`audio_editor::strip`): Processing folds into the
//! ⋯ menu first, then the Fades.
//!
//! The controls are the card's own (`app::clip_controls`), so the two cannot
//! differ. Gain, the fades, Mute and Lock edit the opened clip; Processing
//! selects it first and runs the command the menus and keys run (D30).

use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, UiBuilder, vec2};
use eframe::emath::GuiRounding as _;
use lumen_core::time::TempoMap;
use lumen_project::edit::Edge;
use lumen_project::model::{Clip, ClipContent, Id};

use super::strip_chips::{clip_chip_widths, file_chip_width, text_width};
use crate::App;
use crate::app::clip_card::{CARD_WIDTH, row as card_row};
use crate::app::clip_controls::fade_label;
use crate::app::clip_header::icon_toggle;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::strip::{self, Group, Layout, Widths};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets::{self, Menu, raised};

/// An icon on a strip button.
const GLYPH: f32 = 14.0;
/// The ⋯ button's dots: their radius and how far apart.
const DOTS: f32 = 1.25;
const DOT_GAP: f32 = 4.0;

/// The three processing controls, in the strip's order.
#[derive(Clone, Copy, PartialEq)]
enum Processing {
    Reverse,
    Polarity,
    Normalize,
}

impl Processing {
    const ALL: [Self; 3] = [Self::Reverse, Self::Polarity, Self::Normalize];

    fn label(self) -> &'static str {
        match self {
            Self::Reverse => "Reverse",
            Self::Polarity => "Polarity",
            Self::Normalize => "Normalize",
        }
    }

    /// The row in the ⋯ menu.
    fn menu(self) -> &'static str {
        match self {
            Self::Reverse => "Reverse",
            Self::Polarity => "Invert polarity",
            Self::Normalize => "Normalize",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Reverse => Icon::Reverse,
            Self::Polarity => Icon::Polarity,
            Self::Normalize => Icon::Normalize,
        }
    }

    fn command(self) -> Command {
        match self {
            Self::Reverse => Command::ReverseClips,
            Self::Polarity => Command::InvertPolarity,
            Self::Normalize => Command::NormalizeClips,
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Reverse => "Play this clip backwards",
            Self::Polarity => "Invert this clip's polarity",
            Self::Normalize => "Raise this clip's gain so its loudest sample reaches full scale",
        }
    }

    /// Whether the clip has it on (Normalize is an action, never on).
    fn on(self, clip: &Clip) -> bool {
        match self {
            Self::Reverse => clip.reversed,
            Self::Polarity => clip.inverted,
            Self::Normalize => false,
        }
    }
}

/// A raised button that does something (Normalize), the size of the strip's
/// toggle buttons.
fn action_button(ui: &mut Ui, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        theme::p().text,
    );
    let size = vec2(
        galley.size().x + 2.0 * theme::space::S,
        theme::size::CONTROL,
    );
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let face = raised::face_of(ui, &response).enabled(enabled);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = raised::paint(
            ui.painter(),
            rect,
            theme::radius::for_height(rect.height()),
            &face,
        );
        raised::centre_galley(ui.painter(), body, galley, face.text());
        raised::focus_ring(ui.painter(), &response, rect);
    }
    response
}

/// Mute as the letter the track headers use.
fn mute_glyph(painter: &egui::Painter, centre: egui::Pos2, ink: Color32) {
    let galley =
        painter.layout_no_wrap("M".to_owned(), crate::fonts::medium(theme::size::BODY), ink);
    painter.galley(centre - galley.size() / 2.0, galley, ink);
}

/// A label in the strip's caption type, centred on the row.
fn caption(row: &mut Ui, text: &str) {
    let galley = row.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::size::CAPTION),
        theme::p().text_secondary,
    );
    let (rect, _) = row.allocate_exact_size(galley.size(), Sense::hover());
    row.painter()
        .galley(rect.min, galley, theme::p().text_secondary);
}

/// `add` in a child of `row` that has an id of its own, so the controls in
/// it keep theirs when a group before it appears or goes.
fn group<R>(row: &mut Ui, name: &'static str, add: impl FnOnce(&mut Ui) -> R) -> R {
    row.scope_builder(
        UiBuilder::new().id_salt(("audio strip group", name)),
        |ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            add(ui)
        },
    )
    .inner
}

/// Why the opened clip's gain, fades and processing cannot be changed.
enum Fixed {
    No,
    Locked,
    Frozen(String),
}

impl Fixed {
    fn why(&self) -> Option<String> {
        match self {
            Self::No => None,
            Self::Locked => Some("Locked: unlock it to change gain and fades".to_owned()),
            Self::Frozen(track) => Some(format!(
                "{track} is frozen: unfreeze it to change this clip"
            )),
        }
    }
}

impl App {
    /// Why the clip `id` cannot be edited now.
    fn strip_fixed(&self, id: Id) -> Fixed {
        for track in &self.document.project().tracks {
            if let Some(clip) = track.clips.iter().find(|c| c.id == id) {
                if track.frozen.is_some() {
                    return Fixed::Frozen(track.name.clone());
                }
                return if clip.locked {
                    Fixed::Locked
                } else {
                    Fixed::No
                };
            }
        }
        Fixed::No
    }

    /// Whether Properties (Alt+Enter) belongs to the strip: the audio editor
    /// is shown with its keys live, and the clip it would name is the
    /// opened one, or none is selected (D32).
    pub(crate) fn properties_in_strip(&self, ctx: &egui::Context) -> bool {
        let Some(opened) = self.detail_audio_clip() else {
            return false;
        };
        self.audio_editor_live(ctx)
            && self
                .first_selected_clip()
                .is_none_or(|first| first == opened)
    }

    /// Properties: the strip's Gain takes the keyboard when the strip is
    /// where the owner works, else the floating card opens for the clip.
    pub(crate) fn run_clip_properties(&mut self, ctx: &egui::Context) {
        if self.properties_in_strip(ctx) {
            self.audio_editor.focus_strip = true;
            ctx.request_repaint();
        } else if let Some(clip) = self.first_selected_clip() {
            self.open_clip_card(clip);
        }
    }

    /// A Processing control was used: the opened clip becomes the selection,
    /// then the command runs as the menus and keys run it, with its notes.
    fn strip_command(&mut self, ctx: &egui::Context, clip: Id, command: Command) {
        self.select_opened_clip(clip);
        match self.availability(ctx, command) {
            Ok(()) => self.run(ctx, command),
            Err(why) => self.say(why.to_owned()),
        }
    }

    /// What the parts measure this frame.
    fn strip_widths(&self, ui: &Ui, room: f32, clip: &Clip, file: &str) -> Widths {
        let s = theme::space::S;
        let font = FontId::proportional(theme::size::CAPTION);
        let (chip, chip_dot) = clip_chip_widths(ui, &clip.name);
        let gain_label = text_width(ui, "Gain", font.clone()) + s;
        let fade = |edge| {
            text_width(ui, fade_label(edge), font.clone())
                + s
                + theme::size::STRIP_FADE
                + s
                + theme::size::STRIP_SHAPE
        };
        let words: f32 = Processing::ALL
            .iter()
            .map(|p| widgets::toggle_button_width(ui, p.label()))
            .sum::<f32>()
            + 2.0 * s;
        Widths {
            room,
            chip,
            chip_dot,
            gain: gain_label + theme::size::STRIP_GAIN,
            gain_label,
            toggles: 2.0 * theme::size::CONTROL + s,
            fades: fade(Edge::Start) + s + fade(Edge::End),
            processing_words: words,
            processing_icons: 3.0 * theme::size::CONTROL + 2.0 * s,
            more: theme::size::CONTROL,
            file: file_chip_width(ui, file),
        }
    }

    /// The strip for the opened audio `clip` in `rect`.
    pub(super) fn audio_strip(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        (clip, colour): (&Clip, Color32),
        ed: &mut AudioEditor,
    ) {
        let ClipContent::Audio { media, .. } = &clip.content else {
            return;
        };
        let media = *media;
        let palette = *theme::p();
        // The strip is where the work is while the pointer is over it or a
        // control in it has the keyboard (D32): Properties focuses Gain.
        let ctx = ui.ctx().clone();
        let in_strip = ctx.memory(|m| m.focused()).is_some_and(|id| {
            ctx.read_response(id)
                .is_some_and(|r| rect.intersects(r.rect))
        });
        if in_strip || ui.rect_contains_pointer(rect) {
            ed.hovered = true;
        }
        ui.painter().rect_filled(rect, 0.0, palette.app);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        let inner = rect.shrink2(vec2(theme::space::M, 0.0));
        // The chip shows what it will say: the file's name, or "Missing".
        let file_name = self.file_chip_text(media);
        let widths = self.strip_widths(ui, inner.width(), clip, &file_name);
        let layout = strip::choose(widths);
        ed.strip = layout;
        let ends = strip::ends(layout, widths);
        // The File chip has the right end; the row has what is left.
        let file_rect = Rect::from_min_max(
            egui::pos2(inner.right() - ends.file, inner.top()),
            inner.right_bottom(),
        )
        .shrink2(vec2(0.0, (inner.height() - theme::size::CONTROL) / 2.0));
        let row_room = Rect::from_min_max(
            inner.min,
            egui::pos2(
                (file_rect.left() - strip::GROUP_GAP).max(inner.left()),
                inner.bottom(),
            ),
        );
        let mut row = ui.new_child(
            UiBuilder::new()
                .id_salt("audio strip row")
                .max_rect(row_room)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.set_clip_rect(row_room.intersect(ui.clip_rect()));
        row.spacing_mut().item_spacing.x = strip::GROUP_GAP;

        let tempo = self.transport.timeline.tempo.clone();
        let fixed = self.strip_fixed(clip.id);
        let base = ui.id().with(("audio strip", clip.id.0));
        let focus = std::mem::take(&mut ed.focus_strip);

        group(&mut row, "clip", |ui| {
            self.clip_chip(ui, (clip, colour), layout.names_chip(), ed);
        });
        group(&mut row, "gain", |ui| {
            self.strip_gain(ui, clip, &fixed, (ends.gain, ends.label), (base, focus));
        });
        group(&mut row, "state", |ui| {
            self.strip_state(ui, clip, (&fixed, focus));
        });
        let mut now: Option<Group> = None;
        if layout.shows_fades() {
            group(&mut row, "fades", |ui| {
                if self.strip_fades(ui, clip, &tempo, &fixed, base) {
                    now = Some(Group::Fades);
                }
            });
        }
        if layout.shows_processing() {
            group(&mut row, "processing", |ui| {
                if self.strip_processing(ui, &ctx, clip, layout, &fixed) {
                    now = Some(Group::Processing);
                }
            });
        }
        if layout.has_menu() {
            group(&mut row, "more", |ui| {
                let more = self.strip_more(ui, &ctx, clip, &tempo, &fixed, layout, ed);
                // A group that folded while it had the keyboard leaves it on
                // the ⋯ button (D36).
                if let Some(was) = ed.strip_group
                    && !layout.shows(was)
                {
                    more.request_focus();
                }
            });
        }
        ed.strip_group = now;
        let mut file_ui = ui.new_child(
            UiBuilder::new()
                .id_salt("audio strip file")
                .max_rect(file_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.file_chip(&mut file_ui, file_rect, media);
    }

    /// Gain: its label (when there is room) and slider.
    fn strip_gain(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        fixed: &Fixed,
        (width, label): (f32, bool),
        (base, focus): (egui::Id, bool),
    ) {
        let slider = if label {
            caption(ui, "Gain");
            width
                - text_width(ui, "Gain", FontId::proportional(theme::size::CAPTION))
                - theme::space::S
        } else {
            width
        };
        let id = base.with("gain");
        let (rect, _) = ui.allocate_exact_size(
            vec2(slider.max(0.0), theme::size::CONTROL_SMALL),
            Sense::hover(),
        );
        self.disabled_why(ui, rect, id, fixed);
        ui.add_enabled_ui(fixed.why().is_none(), |ui| {
            self.clip_gain_slider(ui, rect, clip, id);
        });
        if focus && fixed.why().is_none() {
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// On a clip that cannot change, says why over the control in `rect`.
    fn disabled_why(&self, ui: &mut Ui, rect: Rect, id: egui::Id, fixed: &Fixed) {
        if let Some(why) = fixed.why() {
            ui.interact(rect, id.with("why"), Sense::hover())
                .on_hover_text(why);
        }
    }

    /// Mute and Lock, which stay live on a locked clip.
    fn strip_state(&mut self, ui: &mut Ui, clip: &Clip, (fixed, focus): (&Fixed, bool)) {
        let keys = self.keymap.keys(Command::MuteClips);
        let mute = icon_toggle(ui, "Mute clip", clip.muted, mute_glyph);
        if focus && fixed.why().is_some() {
            // Gain is off: the keyboard starts at Mute (D32).
            mute.request_focus();
        }
        if widgets::tip(
            mute,
            "Mute clip",
            keys.as_ref(),
            Some("The clip stays where it is and plays nothing"),
        )
        .clicked()
        {
            self.document.set_clips_muted(&[clip.id], !clip.muted);
        }
        let lock = icon_toggle(ui, "Lock clip", clip.locked, |painter, centre, ink| {
            painter.extend(icons::shapes(
                Icon::Lock,
                Rect::from_center_size(centre, vec2(GLYPH, GLYPH)),
                ink,
            ));
        });
        let keys = self.keymap.keys(Command::LockClips);
        if widgets::tip(
            lock,
            "Lock clip",
            keys.as_ref(),
            Some("A locked clip cannot be moved, trimmed, faded or deleted"),
        )
        .clicked()
        {
            self.document.set_clips_locked(&[clip.id], !clip.locked);
        }
    }

    /// The two fades. True when one has the keyboard.
    fn strip_fades(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        tempo: &TempoMap,
        fixed: &Fixed,
        base: egui::Id,
    ) -> bool {
        let mut focused = false;
        for (i, edge) in [Edge::Start, Edge::End].into_iter().enumerate() {
            caption(ui, fade_label(edge));
            let id = base.with(("fade", i));
            let (rect, _) = ui.allocate_exact_size(
                vec2(theme::size::STRIP_FADE, theme::size::CONTROL_SMALL),
                Sense::hover(),
            );
            self.disabled_why(ui, rect, id, fixed);
            ui.add_enabled_ui(fixed.why().is_none(), |ui| {
                self.clip_fade_slider(ui, rect, (clip, tempo), edge, id.with("length"));
                self.clip_fade_shape(ui, clip, edge, (id.with("shape"), theme::size::STRIP_SHAPE));
            });
            focused |=
                ui.memory(|m| m.has_focus(id.with("length")) || m.has_focus(id.with("shape")));
        }
        focused
    }

    /// Reverse, Polarity and Normalize, with their words or as icons. True
    /// when one has the keyboard.
    fn strip_processing(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        clip: &Clip,
        layout: Layout,
        fixed: &Fixed,
    ) -> bool {
        let working = self.normalizing.is_some();
        let mut focused = false;
        for p in Processing::ALL {
            let busy = p == Processing::Normalize && working;
            let enabled = fixed.why().is_none() && !busy;
            let on = p.on(clip);
            let response = ui
                .add_enabled_ui(enabled, |ui| match (layout.words(), p) {
                    (true, Processing::Normalize) => action_button(ui, p.label()),
                    (true, _) => {
                        let mut shown = on;
                        widgets::toggle_button(ui, &mut shown, p.label())
                    }
                    (false, Processing::Normalize) => {
                        raised::icon_button(ui, p.label(), |painter, centre, ink| {
                            painter.extend(icons::shapes(
                                p.icon(),
                                Rect::from_center_size(centre, vec2(GLYPH, GLYPH)),
                                ink,
                            ));
                        })
                    }
                    (false, _) => icon_toggle(ui, p.label(), on, |painter, centre, ink| {
                        painter.extend(icons::shapes(
                            p.icon(),
                            Rect::from_center_size(centre, vec2(GLYPH, GLYPH)),
                            ink,
                        ));
                    }),
                })
                .inner;
            focused |= response.has_focus();
            let keys = self.keymap.keys(p.command());
            let hint = fixed
                .why()
                .or_else(|| busy.then(|| "Normalizing is under way".to_owned()))
                .unwrap_or_else(|| "Acts on this clip, which becomes the selection".to_owned());
            let clicked = widgets::tip(response, p.tip(), keys.as_ref(), Some(&hint)).clicked();
            if clicked && enabled {
                self.strip_command(ctx, clip.id, p.command());
            }
        }
        focused
    }

    /// The ⋯ button and its menu, which holds the folded groups. Returns the
    /// button's response.
    #[allow(clippy::too_many_arguments)]
    fn strip_more(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        clip: &Clip,
        tempo: &TempoMap,
        fixed: &Fixed,
        layout: Layout,
        ed: &mut AudioEditor,
    ) -> egui::Response {
        let more = raised::icon_button(ui, "More clip properties", |painter, centre, ink| {
            for step in [-DOT_GAP, 0.0, DOT_GAP] {
                painter.circle_filled(centre + vec2(step, 0.0), DOTS, ink);
            }
        });
        let more = widgets::tip(
            more,
            "More",
            None,
            Some(if layout.shows_fades() {
                "Reverse, Polarity and Normalize"
            } else {
                "Fades, Reverse, Polarity and Normalize"
            }),
        );
        let enabled = fixed.why().is_none();
        let page_fades = ed.strip_fades;
        let was_open = Menu::is_open_for(ctx, more.id);
        let mut to_fades = None;
        // The menu stays open across its two pages; a Processing row closes it
        // itself.
        Menu::below(&more).staying().show(|ui| {
            if page_fades {
                ui.set_width(CARD_WIDTH);
                if widgets::menu_item(ui, "Back", None, true).clicked() {
                    to_fades = Some(false);
                }
                ui.separator();
                ui.add_enabled_ui(enabled, |ui| {
                    for (i, edge) in [Edge::Start, Edge::End].into_iter().enumerate() {
                        let label = fade_label(edge);
                        card_row(ui, label, |ui| {
                            let ids = (
                                ui.id().with(("strip fade page", i)),
                                theme::size::STRIP_SHAPE,
                            );
                            self.clip_fade_row(ui, (clip, tempo), edge, ids);
                        });
                    }
                });
                if let Some(why) = fixed.why() {
                    ui.label(RichText::new(why).small().color(theme::p().text_hint));
                }
                return;
            }
            if !layout.shows_processing() {
                for p in Processing::ALL {
                    let keys = self.keymap.keys(p.command());
                    let on = p.on(clip);
                    let clicked = ui
                        .add_enabled_ui(enabled, |ui| {
                            if p == Processing::Normalize {
                                widgets::menu_item(ui, p.menu(), keys.as_ref(), true)
                            } else {
                                widgets::menu_toggle(ui, p.menu(), keys.as_ref(), on)
                            }
                        })
                        .inner
                        .clicked();
                    if clicked {
                        self.strip_command(ctx, clip.id, p.command());
                        ui.close();
                    }
                }
            }
            if !layout.shows_fades() {
                if !layout.shows_processing() {
                    ui.separator();
                }
                if widgets::menu_item(ui, "Fades\u{2026}", None, true).clicked() {
                    to_fades = Some(true);
                }
            }
        });
        if let Some(page) = to_fades {
            // A plain row closes its menu: the page change keeps it open.
            ed.strip_fades = page;
            Menu::open_for(ctx, more.id);
        }
        if !Menu::is_open_for(ctx, more.id) {
            ed.strip_fades = false;
            // Closing gives the keyboard back to the button that opened the
            // menu, unless a click elsewhere has already taken it.
            if was_open && !ctx.input(|i| i.pointer.any_pressed()) {
                more.request_focus();
            }
        }
        more
    }
}
