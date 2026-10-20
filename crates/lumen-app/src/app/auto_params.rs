//! The chooser for a plugin's parameters (M7 slice A3): the plugin first
//! (one of the track's, as a pill when there are several), then the
//! parameter, found by searching the plugin's own list. A plugin may list
//! thousands, so the list draws only the rows in view and the search is run
//! once for each change of its words.

use eframe::egui::{self, Align2, FontId, Key, Sense, Ui, pos2, vec2};
use lumen_hosting::{ParamInfo, UnitInfo};
use lumen_project::automation::Target;
use lumen_project::model::{Id, Project};

use crate::arrangement::auto_params::{automatable, group_name, groups, matching};
use crate::arrangement::auto_state::ParamChooser;
use crate::widgets::{Dialog, pill_toggle};
use crate::{App, theme};

/// A row of the list.
const ROW: f32 = 28.0;

/// What a click or Enter in the chooser asked for.
enum Chosen {
    /// This parameter of the plugin.
    Param(u32),
    /// Another of the track's plugins.
    Plugin(Id),
}

impl App {
    /// Tells the arrangement what the plugins call the parameters its lanes
    /// drive, for the lanes of the tracks shown: "Filter · Cutoff".
    pub(crate) fn lane_names(&mut self) {
        let project = self.document.project();
        let shown = &self.arrangement.auto.shown;
        let mut names = std::collections::BTreeMap::new();
        for track in project.tracks.iter().filter(|t| shown.contains_key(&t.id)) {
            // The lanes the track has, for its chooser, and the one it shows
            // (which has no lane until a point is added).
            let targets = track
                .automation
                .iter()
                .map(|l| l.target)
                .chain(shown.get(&track.id).copied());
            for target in targets {
                let Target::Device { device, param } = target else {
                    continue;
                };
                if let Some(title) = self
                    .hosting
                    .parameters(device)
                    .iter()
                    .find(|p| p.id == param)
                    .map(|p| p.title.clone())
                    && let Some((_, d)) = crate::document::find_device(project, device)
                {
                    names.insert(
                        (device, param),
                        format!("{} \u{00B7} {title}", crate::document::device_name(d)),
                    );
                }
            }
        }
        if self.arrangement.auto.names != names {
            self.arrangement.auto.names = names;
        }
    }

    /// The chooser, when open. Choosing a parameter shows its lane on the
    /// track (the lane itself exists once a point is added) and closes it.
    pub(crate) fn lane_parameters(&mut self, ctx: &egui::Context) {
        let Some(mut chooser) = self.arrangement.auto.chooser.take() else {
            return;
        };
        let project = self.document.project().clone();
        // Gone with its track or its plugin.
        let Some(track) = project.tracks.iter().find(|t| t.id == chooser.track) else {
            return;
        };
        let plugins: Vec<Id> = track
            .devices
            .iter()
            .filter(|d| d.is_vst3())
            .map(|d| d.id)
            .collect();
        if !plugins.contains(&chooser.device) {
            return;
        }
        let list: Vec<ParamInfo> = self.hosting.parameters(chooser.device).to_vec();
        let units: Vec<UnitInfo> = self.hosting.units(chooser.device).to_vec();
        let base = automatable(&list);
        let title = format!(
            "Automate {}",
            crate::document::find_device(&project, chooser.device).map_or_else(
                || "a plugin".to_owned(),
                |(_, d)| crate::document::device_name(d)
            )
        );
        let mut chosen = None;
        let close = Dialog::new("automation parameters", &title, 400.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                self.chooser_body(
                    ui,
                    &mut chooser,
                    (&project, &plugins),
                    (&list, &units, &base),
                    &mut chosen,
                );
            });
        match chosen {
            Some(Chosen::Param(param)) => {
                let target = Target::Device {
                    device: chooser.device,
                    param,
                };
                self.arrangement.auto.shown.insert(chooser.track, target);
                self.arrangement.auto.picks = None;
                return;
            }
            Some(Chosen::Plugin(device)) => {
                chooser = ParamChooser::new(chooser.track, device);
            }
            None => {}
        }
        if !close {
            self.arrangement.auto.chooser = Some(chooser);
        }
    }

    fn chooser_body(
        &self,
        ui: &mut Ui,
        chooser: &mut ParamChooser,
        (project, plugins): (&Project, &[Id]),
        (list, units, base): (&[ParamInfo], &[UnitInfo], &[usize]),
        chosen: &mut Option<Chosen>,
    ) {
        let palette = *theme::p();
        if plugins.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (i, &device) in plugins.iter().enumerate() {
                    let name = crate::document::find_device(project, device)
                        .map_or_else(String::new, |(_, d)| crate::document::device_name(d));
                    if pill_toggle(
                        ui,
                        egui::Id::new(("automation plugin", i)),
                        &name,
                        device == chooser.device,
                        "Choose this plugin's parameters",
                    ) && device != chooser.device
                    {
                        *chosen = Some(Chosen::Plugin(device));
                    }
                }
            });
            ui.add_space(theme::space::XS);
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut chooser.search)
                .id(egui::Id::new("automation parameter search"))
                .hint_text(format!("Search {} parameters", base.len()))
                .desired_width(f32::INFINITY),
        );
        search.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                "Search parameters to automate",
            )
        });
        if chooser.focus {
            search.request_focus();
            chooser.focus = false;
        }
        let group_list = groups(list, units, base);
        if chooser
            .group
            .is_some_and(|g| !group_list.iter().any(|(u, ..)| *u == g))
        {
            chooser.group = None;
        }
        if !group_list.is_empty() {
            ui.add_space(theme::space::XS);
            ui.horizontal_wrapped(|ui| {
                let all = pill_toggle(
                    ui,
                    egui::Id::new("automation group all"),
                    &format!("All {}", base.len()),
                    chooser.group.is_none(),
                    "Show every group",
                );
                if all {
                    chooser.group = None;
                    chooser.cursor = 0;
                }
                for (unit, name, count) in &group_list {
                    if pill_toggle(
                        ui,
                        egui::Id::new(("automation group", *unit)),
                        &format!("{name} {count}"),
                        chooser.group == Some(*unit),
                        "Show these parameters",
                    ) {
                        chooser.group = Some(*unit);
                        chooser.cursor = 0;
                    }
                }
            });
        }
        ui.add_space(theme::space::XS);
        let rows = matching(list, units, base, &chooser.search, chooser.group);
        chooser.cursor = chooser.cursor.min(rows.len().saturating_sub(1));
        let (down, up, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
            )
        });
        if down && chooser.cursor + 1 < rows.len() {
            chooser.cursor += 1;
        }
        if up {
            chooser.cursor = chooser.cursor.saturating_sub(1);
        }
        if enter && let Some(&at) = rows.get(chooser.cursor) {
            *chosen = Some(Chosen::Param(list[at].id));
        }
        if base.is_empty() {
            ui.add_space(theme::space::M);
            ui.label(
                egui::RichText::new(
                    "This plugin lists no parameters yet. They appear once it has loaded.",
                )
                .color(palette.text_hint),
            );
            ui.add_space(theme::space::M);
        } else if rows.is_empty() {
            ui.add_space(theme::space::M);
            ui.label(
                egui::RichText::new("No parameter matches. Clear the search to see them all.")
                    .color(palette.text_hint),
            );
            ui.add_space(theme::space::M);
        }
        let track = project.tracks.iter().find(|t| t.id == chooser.track);
        let here = self.arrangement.auto.shown.get(&chooser.track).copied();
        egui::ScrollArea::vertical()
            .max_height(ROW * 9.0)
            .auto_shrink([false, true])
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                for r in range {
                    let p = &list[rows[r]];
                    let target = Target::Device {
                        device: chooser.device,
                        param: p.id,
                    };
                    let points = track.and_then(|t| t.lane(target)).map(|l| l.lane.len());
                    let look = RowLook {
                        p,
                        group: group_name(units, p.unit),
                        points,
                        shown: here == Some(target),
                        under_cursor: r == chooser.cursor,
                    };
                    if let Some(param) = parameter_row(ui, chooser.device, &look) {
                        *chosen = Some(Chosen::Param(param));
                    }
                }
            });
        ui.add_space(theme::space::XS);
        ui.label(
            egui::RichText::new(format!("{} of {}", rows.len(), base.len()))
                .font(FontId::monospace(theme::size::MONO_SMALL))
                .color(palette.text_hint),
        );
    }
}

/// What one row shows.
struct RowLook<'a> {
    p: &'a ParamInfo,
    group: String,
    /// How many points its lane holds, if it has one.
    points: Option<usize>,
    /// Its lane is the one the track shows.
    shown: bool,
    under_cursor: bool,
}

/// One row: the parameter's name, its group and its lane's points. The
/// parameter's id when the row was clicked.
fn parameter_row(ui: &mut Ui, device: Id, look: &RowLook<'_>) -> Option<u32> {
    let palette = *theme::p();
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let id = egui::Id::new(("automation parameter row", device.0, look.p.id));
    let response = ui.interact(row, id, Sense::click());
    let name = format!("Automate {}", look.p.title);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let painter = ui.painter();
    if look.under_cursor || response.hovered() {
        painter.rect_filled(row, 4.0, palette.accent_muted);
    }
    let ink = if look.shown {
        palette.accent
    } else {
        palette.text
    };
    painter.text(
        pos2(row.left() + 8.0, row.center().y),
        Align2::LEFT_CENTER,
        &look.p.title,
        FontId::proportional(theme::size::BODY),
        ink,
    );
    let mut right = row.right() - 8.0;
    if let Some(n) = look.points {
        let text = match n {
            1 => "1 point".to_owned(),
            n => format!("{n} points"),
        };
        let galley = painter.layout_no_wrap(
            text,
            FontId::monospace(theme::size::MONO_SMALL),
            palette.text_secondary,
        );
        right -= galley.size().x;
        painter.galley(
            pos2(right, row.center().y - galley.size().y * 0.5),
            galley,
            palette.text_secondary,
        );
        right -= 12.0;
    }
    if look.group != "General" {
        let galley = painter.layout_no_wrap(
            look.group.clone(),
            FontId::proportional(theme::size::CAPTION),
            palette.text_hint,
        );
        painter.galley(
            pos2(
                right - galley.size().x,
                row.center().y - galley.size().y * 0.5,
            ),
            galley,
            palette.text_hint,
        );
    }
    response.clicked().then_some(look.p.id)
}
