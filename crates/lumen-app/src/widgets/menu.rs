use std::sync::Arc;

use eframe::egui::{
    self, Id as EguiId, LayerId, Popup, PopupCloseBehavior, Pos2, Response, Ui,
    epaint::ClippedShape, vec2,
};

use super::raised;
use crate::{motion, theme};

const LEAVE_DELAY: f64 = motion::MENU_LEAVE_DELAY;
const LEAVE_SLOP: f32 = 8.0;
const LIFT: f32 = 4.0;

fn change_key() -> EguiId {
    EguiId::new("lumen menu changed")
}

pub(crate) fn note_change(ui: &Ui) {
    ui.ctx().data_mut(|d| d.insert_temp(change_key(), true));
}

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
    at: Option<Pos2>,
    closing: Option<f64>,
    changed: bool,
    left: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Opens {
    Below,
    Context,
}

pub struct Menu<'r> {
    response: &'r Response,
    opens: Opens,
    keep_open: bool,
    stays: bool,
}

impl<'r> Menu<'r> {
    pub fn below(response: &'r Response) -> Self {
        Self {
            response,
            opens: Opens::Below,
            keep_open: true,
            stays: false,
        }
    }

    pub fn context(response: &'r Response) -> Self {
        Self {
            response,
            opens: Opens::Context,
            keep_open: false,
            stays: false,
        }
    }

    pub fn keeping_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    pub fn staying(mut self) -> Self {
        self.keep_open = true;
        self.stays = true;
        self
    }

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

    pub fn close_for(ctx: &egui::Context, response_id: EguiId) {
        let id = state_id(response_id);
        let mut state = load(ctx, id);
        if state.open && state.closing.is_none() {
            state.closing = Some(ctx.input(|i| i.time));
            store(ctx, id, state);
            ctx.request_repaint();
        }
    }

    pub fn is_open_for(ctx: &egui::Context, response_id: EguiId) -> bool {
        let state = load(ctx, state_id(response_id));
        state.open && state.closing.is_none()
    }

    fn id(&self) -> EguiId {
        state_id(self.response.id)
    }

    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let ctx = self.response.ctx.clone();
        let id = self.id();
        let now = ctx.input(|i| i.time);
        let mut state = load(&ctx, id);
        let fade_time = f64::from(motion::time(motion::BASE));
        let begin_closing = |state: &mut State| {
            if fade_time > 0.0 {
                state.closing.get_or_insert(now);
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
            .show(|ui| {
                let height = ui.ctx().content_rect().height() - theme::space::M * 2.0;
                ui.set_max_height(height);
                ui.spacing_mut().scroll.floating = false;
                ui.spacing_mut().scroll.bar_width = theme::space::XS;
                ui.spacing_mut().scroll.foreground_color = false;
                egui::ScrollArea::vertical()
                    .id_salt("menu scroll")
                    .max_height(height)
                    .show(ui, content)
                    .inner
            });
        let changed = ctx.data_mut(|d| d.remove_temp::<bool>(change_key())) == Some(true);
        let Some(shown_menu) = shown_menu else {
            store(&ctx, id, State::default());
            return None;
        };
        let layer = shown_menu.response.layer_id;
        raised::lip(
            &ctx.layer_painter(layer),
            shown_menu.response.rect,
            theme::radius::LARGE,
            theme::p().edge_light,
        );
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
        } else if !open && !opened_now {
            begin_closing(&mut state);
            Popup::close_id(&ctx, popup_id);
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            }
        } else if state.changed && !self.stays {
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

#[derive(Clone)]
struct Picture {
    layer: LayerId,
    shapes: Arc<Vec<ClippedShape>>,
}

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
