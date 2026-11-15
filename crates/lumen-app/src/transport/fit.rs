use eframe::egui::{self, Ui};

use crate::theme;

const MIN_DEVICE: f32 = 72.0;
const SLACK: f32 = 2.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Plan {
    pub compact: bool,
    pub dropout: bool,
    pub compensation: bool,
    pub tap: bool,
    pub cpu: bool,
    pub keyboard: bool,
    pub device: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Wants {
    pub dropout: f32,
    pub compensation: f32,
    pub tap: f32,
    pub cpu: f32,
    pub keyboard: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Fit {
    pub plan: Plan,
    start: f32,
    drawn: f32,
    base: Option<f32>,
    roomy: Option<f32>,
    saves: Option<f32>,
}

impl Fit {
    pub fn begin(&mut self, ui: &Ui, wants: Wants) -> Plan {
        self.start = ui.cursor().left();
        self.drawn = 0.0;
        let room = (ui.max_rect().right() - self.start).max(0.0);
        let roomy_base = match (self.plan.compact, self.base) {
            (false, base) => base,
            (true, Some(base)) => self.saves.map(|saves| base + saves).or(self.roomy),
            (true, None) => None,
        };
        let compact = roomy_base.is_some_and(|base| base > room);
        let base = if compact {
            self.base.filter(|_| self.plan.compact)
        } else {
            roomy_base
        };
        let mut plan = Plan {
            compact,
            ..Plan::default()
        };
        if let Some(base) = base {
            let mut left = room - base - SLACK;
            let mut take = |width: f32| {
                let fits = left >= width;
                if fits {
                    left -= width;
                } else {
                    left = -1.0;
                }
                fits
            };
            plan.dropout = take(wants.dropout);
            plan.compensation = take(wants.compensation);
            plan.tap = take(wants.tap);
            plan.cpu = take(wants.cpu);
            plan.keyboard = take(wants.keyboard);
            plan.device = if left >= MIN_DEVICE { left } else { 0.0 };
        }
        self.plan = plan;
        plan
    }

    pub fn took(&mut self, ui: &Ui, before: f32) {
        self.drawn += (ui.cursor().left() - before).max(0.0);
    }

    pub fn spent(&mut self, width: f32) {
        self.drawn += width.max(0.0);
    }

    pub fn finish(&mut self, ui: &Ui) {
        let base = (ui.cursor().left() - self.start - self.drawn).max(0.0);
        if self.plan.compact {
            if let Some(roomy) = self.roomy {
                self.saves = Some((roomy - base).max(0.0));
            }
        } else {
            self.roomy = Some(base);
        }
        self.base = Some(base);
    }
}

pub fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
        .size()
        .x
}

pub fn gap(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.x
}

pub fn divider(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(theme::stroke::HAIRLINE));
}

pub const COMPACT_GAP: f32 = theme::space::XS;
