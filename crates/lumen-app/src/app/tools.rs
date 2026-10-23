//! The six global arrangement tools and their switch (M7b E0).

use eframe::egui::{self, Rect, Sense};

use crate::{App, commands::Command, motion, theme, widgets};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ArrangementTool {
    #[default]
    Pointer,
    Draw,
    Erase,
    Split,
    Mute,
    Zoom,
}

impl ArrangementTool {
    pub(crate) const ALL: [Self; 6] = [
        Self::Pointer,
        Self::Draw,
        Self::Erase,
        Self::Split,
        Self::Mute,
        Self::Zoom,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer",
            Self::Draw => "Draw",
            Self::Erase => "Erase",
            Self::Split => "Split",
            Self::Mute => "Mute",
            Self::Zoom => "Zoom",
        }
    }

    pub(crate) const fn command(self) -> Command {
        match self {
            Self::Pointer => Command::ToolPointer,
            Self::Draw => Command::ToolDraw,
            Self::Erase => Command::ToolErase,
            Self::Split => Command::ToolSplit,
            Self::Mute => Command::ToolMute,
            Self::Zoom => Command::ToolZoom,
        }
    }
}

impl App {
    pub(crate) fn choose_arrangement_tool(&mut self, tool: ArrangementTool) {
        self.arrangement.tool = tool;
        self.arrangement.gesture = None;
        self.arrangement.tool_keys = true;
    }

    pub(crate) fn active_arrangement_tool_name(&self) -> &'static str {
        self.arrangement.tool.name()
    }

    /// Compact tool switch in the header corner, directly above track names.
    pub(crate) fn arrangement_tool_switch(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let tool = self.arrangement.tool;
        let current = tool;
        let label = format!("Arrangement tool: {}", tool.name());
        let switch_id = ui.id().with("arrangement tool switch");
        let response = ui.interact(rect, switch_id, Sense::click());
        let keys = self.keymap.keys(tool.command());
        let response = widgets::tip(
            response,
            &label,
            keys.as_ref(),
            Some("Choose an arrangement tool"),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
        });
        // Quiet at rest: only a tool other than Pointer is a mode worth
        // lighting.
        let lit = motion::glide(
            ui.ctx(),
            response.id.with("lit"),
            f32::from(tool != ArrangementTool::Pointer),
            true,
        );
        let face = widgets::raised::face_of(ui, &response).lit(lit, theme::Tone::Accent);
        let body = widgets::raised::paint(
            ui.painter(),
            rect,
            theme::radius::for_height(rect.height()),
            &face,
        );
        let galley = ui.painter().layout_no_wrap(
            tool.name().to_owned(),
            egui::FontId::proportional(theme::size::CAPTION),
            face.text(),
        );
        let text_body = Rect::from_min_max(body.min, body.max - egui::vec2(theme::space::S, 0.0));
        let painter = ui.painter().with_clip_rect(rect);
        widgets::raised::centre_galley(&painter, text_body, galley, face.text());
        widgets::draw_arrow(
            &painter,
            egui::pos2(body.right() - theme::space::S, body.center().y),
            1.0,
            face.text(),
        );
        widgets::raised::focus_ring(ui.painter(), &response, rect);

        // One click picks and closes; the chosen tool is marked while open.
        let mut picked = None;
        widgets::Menu::below(&response).show(|ui| {
            for tool in ArrangementTool::ALL {
                let keys = self.keymap.keys(tool.command());
                if widgets::menu_toggle(ui, tool.name(), keys.as_ref(), tool == current).clicked() {
                    picked = Some(tool);
                }
            }
        });
        if let Some(tool) = picked {
            self.choose_arrangement_tool(tool);
            widgets::Menu::close_for(ui.ctx(), switch_id);
        }
    }
}
