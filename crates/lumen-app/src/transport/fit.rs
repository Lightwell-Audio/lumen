//! How the transport bar fits its room: one rule for every width.
//!
//! The bar's essentials are the buttons, tempo, meter, the position (bars
//! and time) and the audio status dot; the MIDI chip and the panel toggles
//! sit apart at the right and always stay. Everything else is optional, and
//! gives way in a fixed order as the window narrows, so nothing is ever cut
//! or drawn over:
//!
//! 1. the device text shortens, then goes (the dot's tooltip has it all);
//! 2. the computer keyboard's readout goes (the info line carries it);
//! 3. the CPU readout goes;
//! 4. the delay compensation readout goes;
//! 5. the dropout chip goes (the dot's tooltip and the audio panel say it);
//! 6. only then the essentials themselves tighten (compact: tempo without
//!    " BPM", closer spacing).
//!
//! The pieces are measured, not guessed: each frame the bar notes how wide
//! its essentials came out, and the next frame spends the room that is left
//! on the optional pieces, in that order. The first frame, and the frame
//! after a change of mode, are the only ones that can be a step behind.

use eframe::egui::{self, Ui};

use crate::theme;

/// The least room worth showing the device text in; less and it goes.
const MIN_DEVICE: f32 = 72.0;
/// Room kept free after the optional pieces, against rounding.
const SLACK: f32 = 2.0;

/// What the bar shows this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Plan {
    /// The essentials tighten: no " BPM", closer spacing.
    pub compact: bool,
    pub dropout: bool,
    pub compensation: bool,
    pub cpu: bool,
    pub keyboard: bool,
    /// The room the device text may take; 0 when it is not shown.
    pub device: f32,
}

/// The widths the optional pieces want, 0 for one that has nothing to show.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wants {
    pub dropout: f32,
    pub compensation: f32,
    pub cpu: f32,
    pub keyboard: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Fit {
    pub plan: Plan,
    /// Where this frame's bar starts, and what its optional pieces took.
    start: f32,
    drawn: f32,
    /// The width of the essentials in the mode drawn last frame.
    base: Option<f32>,
    /// Their width in the roomy mode, last known.
    roomy: Option<f32>,
    /// How much the compact mode saves, once both have been drawn.
    saves: Option<f32>,
}

impl Fit {
    /// Decides what shows, given the room from here to the bar's end.
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
        // Until the essentials have been measured nothing optional shows.
        if let Some(base) = base {
            let mut left = room - base - SLACK;
            let mut take = |width: f32| {
                let fits = left >= width;
                if fits {
                    left -= width;
                } else {
                    // Strict order: what is lower waits for what is higher.
                    left = -1.0;
                }
                fits
            };
            plan.dropout = take(wants.dropout);
            plan.compensation = take(wants.compensation);
            plan.cpu = take(wants.cpu);
            plan.keyboard = take(wants.keyboard);
            plan.device = if left >= MIN_DEVICE { left } else { 0.0 };
        }
        self.plan = plan;
        plan
    }

    /// Notes that an optional piece took the room since `before`
    /// (the cursor's left edge then).
    pub fn took(&mut self, ui: &Ui, before: f32) {
        self.drawn += (ui.cursor().left() - before).max(0.0);
    }

    /// Ends the frame: measures the essentials.
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

/// How wide `text` is in `font`.
pub fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
        .size()
        .x
}

/// The gap between two pieces in the bar.
pub fn gap(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.x
}

/// The compact spacing between the bar's pieces.
pub const COMPACT_GAP: f32 = theme::space::XS;
