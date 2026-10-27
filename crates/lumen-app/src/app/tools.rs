//! The six global arrangement tools and their switch (M7b E0).

use eframe::egui::{self, Rect};

use crate::icons::Icon;
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

    /// What the switch and its menu show for the tool (the interface
    /// audit's M6).
    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Pointer => Icon::Pointer,
            Self::Draw => Icon::Pencil,
            Self::Erase => Icon::Eraser,
            Self::Split => Icon::Split,
            Self::Mute => Icon::MuteCircle,
            Self::Zoom => Icon::ZoomIn,
        }
    }

    /// The command whose keys pick the tool: Draw's is B, which switches
    /// between Draw and Pointer, so the menu shows B rather than nothing.
    pub(crate) const fn shortcut(self) -> Command {
        match self {
            Self::Draw => Command::DrawTool,
            other => other.command(),
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

    /// The tool piece of the ruler corner's well (the interface audit's
    /// H1): the tool's icon and name, and its menu on click. The name goes
    /// when the header column is too narrow for it.
    pub(crate) fn arrangement_tool_switch(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let tool = self.arrangement.tool;
        let current = tool;
        let label = format!("Arrangement tool: {}", tool.name());
        let switch_id = ui.id().with("arrangement tool switch");
        // Quiet at rest: only a tool other than Pointer is a mode worth
        // lighting.
        let lit = motion::glide(
            ui.ctx(),
            switch_id.with("lit"),
            f32::from(tool != ArrangementTool::Pointer),
            true,
        );
        let (response, ink) =
            widgets::well::button(ui, rect, switch_id, (lit, theme::Tone::Accent), true);
        let keys = self.keymap.keys(tool.shortcut());
        let response = widgets::tip(
            response,
            &label,
            keys.as_ref(),
            Some("Choose an arrangement tool"),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
        });
        let painter = ui.painter().with_clip_rect(rect);
        let side = theme::size::CAPTION + 3.0;
        let galley = painter.layout_no_wrap(
            tool.name().to_owned(),
            egui::FontId::proportional(theme::size::CAPTION),
            ink,
        );
        let gap = theme::space::XS;
        let needed = side + gap + galley.size().x + gap + widgets::ARROW_SIZE;
        let room = rect.width() - 2.0 * gap;
        let named = needed <= room;
        let content = if named { needed } else { side };
        let left = rect.center().x - content / 2.0;
        let icon = Rect::from_center_size(
            egui::pos2(left + side / 2.0, rect.center().y),
            egui::vec2(side, side),
        );
        painter.extend(crate::icons::shapes(tool.icon(), icon, ink));
        if named {
            let text_at = egui::pos2(icon.right() + gap, rect.center().y - galley.size().y / 2.0);
            let arrow_x = text_at.x + galley.size().x + gap + widgets::ARROW_SIZE / 2.0;
            painter.galley(text_at, galley, ink);
            widgets::draw_arrow(
                &painter,
                egui::pos2(arrow_x, rect.center().y),
                1.0,
                theme::p().text_hint,
            );
        }

        // One click picks and closes; the chosen tool is marked while open.
        let mut picked = None;
        widgets::Menu::below(&response).show(|ui| {
            for tool in ArrangementTool::ALL {
                let keys = self.keymap.keys(tool.shortcut());
                let row = widgets::menu_toggle_icon(
                    ui,
                    tool.icon(),
                    tool.name(),
                    keys.as_ref(),
                    tool == current,
                );
                if row.clicked() {
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
