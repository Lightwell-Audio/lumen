//! The six global arrangement tools and their switch (M7b E0).

use eframe::egui::{self, Rect, Sense};

use crate::{App, commands::Command, theme, widgets};

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
        Self::Pointer, Self::Draw, Self::Erase, Self::Split, Self::Mute, Self::Zoom,
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
    }

    pub(crate) fn arrangement_tool_name(&self) -> &'static str {
        self.arrangement.tool.name()
    }

    /// Compact tool switch in the header corner, directly above track names.
    pub(crate) fn arrangement_tool_switch(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let tool = self.arrangement.tool;
        let label = format!("{} tool ▾", tool.name());
        let response = ui.interact(rect, ui.id().with("arrangement tool switch"), Sense::click());
        let keys = self.keymap.keys(tool.command());
        let response = widgets::tip(response, &label, keys.as_ref(), Some("Choose an arrangement tool"));
        if response.clicked() {
            widgets::Menu::open_for(ui.ctx(), response.id);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
        let face = widgets::raised::face_of(ui, &response).lit(true, theme::Tone::On);
        let body = widgets::raised::paint(ui.painter(), rect, theme::radius::for_height(rect.height()), &face);
        let galley = ui.painter().layout_no_wrap(label, egui::FontId::proportional(theme::size::CAPTION), face.text());
        widgets::raised::centre_galley(ui.painter(), body, galley, face.text());
        widgets::raised::focus_ring(ui.painter(), &response, rect);

        widgets::Menu::below(&response).show(|ui| {
            for tool in ArrangementTool::ALL {
                let command = tool.command();
                let keys = self.keymap.keys(command);
                if widgets::menu_item(ui, tool.name(), keys.as_ref(), true).clicked() {
                    self.choose_arrangement_tool(tool);
                }
            }
        });
    }
}
