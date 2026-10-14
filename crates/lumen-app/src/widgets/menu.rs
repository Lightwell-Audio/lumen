//! Menus that close gently (the owner's C, after MT-POL-2): a menu with
//! settings in it stays open while the pointer is on it, so a change can be
//! followed by another and its motion seen; once something has changed and
//! the pointer leaves, it waits a moment and fades. Esc, a click elsewhere
//! and commands close it at once, and every menu fades out (`BASE`) instead
//! of vanishing. egui's menus cannot fade out, so Lumen keeps their open
//! state and shows a closing menu, untouchable, while it fades.

use std::sync::Arc;

use eframe::egui::{
    self, Id as EguiId, LayerId, Popup, PopupCloseBehavior, Pos2, Response, Ui,
    epaint::ClippedShape, vec2,
};

use super::raised;
use crate::{motion, theme};

/// How long after the pointer leaves a changed menu it starts to fade.
const LEAVE_DELAY: f64 = 0.4;
/// Room around a menu the pointer may stray into without leaving it.
const LEAVE_SLOP: f32 = 8.0;
/// How far a closing menu drifts up as it fades.
const LIFT: f32 = 4.0;

/// The key a change inside a menu is noted under this frame.
fn change_key() -> EguiId {
    EguiId::new("lumen menu changed")
}

/// Notes that a setting in the menu being drawn changed (switches,
/// segmented controls, choice rows and menu toggles call it), so the menu
/// fades once the pointer leaves it.
pub(crate) fn note_change(ui: &Ui) {
    ui.ctx().data_mut(|d| d.insert_temp(change_key(), true));
}

/// Where the menus drawn this pass are noted, so a staying menu can tell
/// that a click landed in a menu opened from inside it (a dropdown in a
/// popover), not outside.
fn drawn_id() -> EguiId {
    EguiId::new("lumen menus drawn")
}

fn note_drawn(ctx: &egui::Context, id: EguiId) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let drawn = d.get_temp_mut_or_default::<(u64, Vec<EguiId>)>(drawn_id());
        if drawn.0 != pass {
            *drawn = (pass, Vec::new());
        }
        drawn.1.push(id);
    });
}

/// Whether a menu other than `id` was drawn this pass.
fn other_drawn(ctx: &egui::Context, id: EguiId) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| {
        d.get_temp::<(u64, Vec<EguiId>)>(drawn_id())
            .is_some_and(|(at, ids)| at == pass && ids.iter().any(|i| *i != id))
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    open: bool,
    /// Where a context menu opened.
    at: Option<Pos2>,
    /// When it started to fade out.
    closing: Option<f64>,
    /// Whether a setting in it changed since it opened.
    changed: bool,
    /// When the pointer left it, after a change.
    left: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Opens {
    /// Under the control, by a click on it.
    Below,
    /// At the pointer, by a right-click.
    Context,
}

/// A menu opened from a control's `response`.
pub struct Menu<'r> {
    response: &'r Response,
    opens: Opens,
    keep_open: bool,
    /// Stays open when the pointer leaves after a change (a popover whose
    /// effect is looked at elsewhere: the quantize options).
    stays: bool,
}

impl<'r> Menu<'r> {
    /// A menu under the control, opened and closed by clicking it. Clicks
    /// inside leave it open: its rows close it themselves.
    pub fn below(response: &'r Response) -> Self {
        Self {
            response,
            opens: Opens::Below,
            keep_open: true,
            stays: false,
        }
    }

    /// A menu at the pointer, opened by right-clicking the control. A click
    /// inside closes it, unless [`Self::keeping_open`].
    pub fn context(response: &'r Response) -> Self {
        Self {
            response,
            opens: Opens::Context,
            keep_open: false,
            stays: false,
        }
    }

    /// Clicks inside leave the menu open (a menu with settings in it).
    pub fn keeping_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// The menu stays open when the pointer leaves it after a change; only
    /// Esc, a click elsewhere or a command closes it.
    pub fn staying(mut self) -> Self {
        self.keep_open = true;
        self.stays = true;
        self
    }

    /// Opens the menu of the control with `response_id` from code (a key).
    pub fn open_for(ctx: &egui::Context, response_id: EguiId) {
        let id = state_id(response_id);
        let state = load(ctx, id);
        if !state.open || state.closing.is_some() {
            store(
                ctx,
                id,
                State {
                    open: true,
                    ..State::default()
                },
            );
            ctx.request_repaint();
        }
    }

    /// Closes the menu of the control with `response_id` from code.
    pub fn close_for(ctx: &egui::Context, response_id: EguiId) {
        let id = state_id(response_id);
        let mut state = load(ctx, id);
        if state.open && state.closing.is_none() {
            state.closing = Some(ctx.input(|i| i.time));
            store(ctx, id, state);
            ctx.request_repaint();
        }
    }

    /// Whether the menu of the control with `response_id` is open (not
    /// fading out): for lighting the control before it is drawn.
    pub fn is_open_for(ctx: &egui::Context, response_id: EguiId) -> bool {
        let state = load(ctx, state_id(response_id));
        state.open && state.closing.is_none()
    }

    fn id(&self) -> EguiId {
        state_id(self.response.id)
    }

    /// Shows the menu while it is open; `content` runs only then. A
    /// closing menu is a picture of its last frame, fading: nothing in it
    /// can be clicked, focused or read out.
    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let ctx = self.response.ctx.clone();
        let id = self.id();
        let now = ctx.input(|i| i.time);
        let mut state = load(&ctx, id);
        let fade_time = f64::from(motion::time(motion::BASE));
        let begin_closing = |state: &mut State| {
            if fade_time > 0.0 {
                state.closing.get_or_insert(now);
                // The fade's first frame: an idle window would otherwise
                // draw the next one only after it was over (owner,
                // MT-POL-2: menus vanished).
                ctx.request_repaint();
            } else {
                *state = State::default();
            }
        };
        let fresh = |at: Option<Pos2>| State {
            open: true,
            at,
            ..State::default()
        };
        let mut opened_now = false;
        match self.opens {
            Opens::Below => {
                if self.response.clicked() {
                    if state.open && state.closing.is_none() {
                        begin_closing(&mut state);
                    } else {
                        state = fresh(None);
                        opened_now = true;
                    }
                }
            }
            Opens::Context => {
                if self.response.secondary_clicked() {
                    let at = self
                        .response
                        .interact_pointer_pos()
                        .or_else(|| ctx.pointer_hover_pos());
                    state = fresh(at);
                    opened_now = true;
                } else if self.response.clicked() && state.open && state.closing.is_none() {
                    begin_closing(&mut state);
                }
            }
        }
        // egui is told the menu is open while it is (not while it fades), so
        // keys such as Esc go to it and its button can light.
        let popup_id = Popup::default_response_id(self.response);
        if state.open && state.closing.is_none() {
            if !Popup::is_any_open(&ctx) || Popup::is_id_open(&ctx, popup_id) {
                Popup::open_id(&ctx, popup_id);
            }
        } else if Popup::is_id_open(&ctx, popup_id) {
            Popup::close_id(&ctx, popup_id);
        }
        if let Some(since) = state.closing {
            let left = 1.0 - (now - since) / fade_time.max(1e-6);
            if left <= 0.0 {
                store(&ctx, id, State::default());
                return None;
            }
            // Ease-in on the way out: it lingers, then goes.
            let shown = motion::ease_out(left as f32);
            paint_fading(&ctx, id, shown, LIFT * (1.0 - shown));
            ctx.request_repaint();
            store(&ctx, id, state);
            return None;
        }
        if !state.open {
            store(&ctx, id, state);
            return None;
        }
        let popup = match self.opens {
            Opens::Below => Popup::menu(self.response),
            Opens::Context => Popup::menu(self.response).at_position(state.at.unwrap_or_default()),
        };
        let mut open = true;
        note_drawn(&ctx, id);
        ctx.data_mut(|d| d.remove::<bool>(change_key()));
        let shown_menu = popup
            .open_bool(&mut open)
            .close_behavior(if self.keep_open {
                PopupCloseBehavior::CloseOnClickOutside
            } else {
                PopupCloseBehavior::CloseOnClick
            })
            .show(content);
        let changed = ctx.data_mut(|d| d.remove_temp::<bool>(change_key())) == Some(true);
        let Some(shown_menu) = shown_menu else {
            store(&ctx, id, State::default());
            return None;
        };
        // The edge light along the menu's top inside edge (the design
        // system's Menu card: a floating surface has one), painted before the
        // picture is taken so the fade keeps it.
        let layer = shown_menu.response.layer_id;
        raised::lip(
            &ctx.layer_painter(layer),
            shown_menu.response.rect,
            theme::radius::LARGE,
            theme::p().edge_light,
        );
        // What it looks like now, to fade from when it closes.
        let picture: Vec<ClippedShape> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| list.all_entries().cloned().collect())
                .unwrap_or_default()
        });
        ctx.data_mut(|d| {
            d.insert_temp(
                id.with("picture"),
                Picture {
                    layer,
                    shapes: Arc::new(picture),
                },
            )
        });
        state.changed |= changed;
        if !open && !opened_now && self.stays && other_drawn(&ctx, id) {
            // A click in a menu opened from inside this one (a dropdown in
            // a popover) is not a click elsewhere.
        } else if !open && !opened_now {
            // Esc, a click elsewhere or a command closed it.
            begin_closing(&mut state);
            Popup::close_id(&ctx, popup_id);
        } else if state.changed && !self.stays {
            // Once changed, it closes when the pointer leaves it.
            let area = shown_menu.response.rect.expand(LEAVE_SLOP);
            let on_it = ctx.pointer_hover_pos().is_some_and(|p| area.contains(p))
                || submenu_open(&ctx, &shown_menu.response);
            if on_it {
                state.left = None;
            } else {
                let since = *state.left.get_or_insert(now);
                let waited = now - since;
                if waited >= LEAVE_DELAY {
                    begin_closing(&mut state);
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        LEAVE_DELAY - waited,
                    ));
                }
            }
        }
        store(&ctx, id, state);
        Some(shown_menu.inner)
    }
}

/// A menu's last frame, as painted.
#[derive(Clone)]
struct Picture {
    layer: LayerId,
    shapes: Arc<Vec<ClippedShape>>,
}

/// Paints the closing menu `id`'s picture at `shown` strength, `lift`
/// points up.
fn paint_fading(ctx: &egui::Context, id: EguiId, shown: f32, lift: f32) {
    let Some(picture) = ctx.data(|d| d.get_temp::<Picture>(id.with("picture"))) else {
        return;
    };
    let mut painter = ctx.layer_painter(picture.layer);
    painter.multiply_opacity(shown);
    for ClippedShape { clip_rect, shape } in picture.shapes.iter() {
        let mut shape = shape.clone();
        shape.translate(vec2(0.0, -lift));
        painter
            .with_clip_rect(clip_rect.translate(vec2(0.0, -lift)))
            .add(shape);
    }
}

/// Whether a submenu of `menu` is open: the pointer is then on it, not
/// gone (the app menu's "Open recent").
fn submenu_open(ctx: &egui::Context, menu: &Response) -> bool {
    !egui::containers::menu::MenuState::is_deepest_open_sub_menu(ctx, menu.id)
}

fn state_id(response_id: EguiId) -> EguiId {
    response_id.with("lumen menu")
}

fn load(ctx: &egui::Context, id: EguiId) -> State {
    ctx.data(|d| d.get_temp(id)).unwrap_or_default()
}

fn store(ctx: &egui::Context, id: EguiId, state: State) {
    ctx.data_mut(|d| d.insert_temp(id, state));
}
