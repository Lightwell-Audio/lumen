//! The browser's Project media tab (M9 slice R4b, ADR-0106): every audio file
//! the project holds, which ones are missing or unused, and the actions on
//! them. It decides nothing about files: `app::media_pool` does that, and
//! this draws the state and says what was clicked.

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Color32, FontId, Id, Key, Modifiers, Painter, Rect, Sense, Ui, pos2, vec2,
};
use lumen_project::model::Id as MediaId;

use super::controls;
use crate::app::media_pool::Filter;
use crate::app::panes::{self, Pane};
use crate::pool::Row;
use crate::relink::{Found, Verdict};
use crate::{App, fonts, motion, theme, widgets};

const ROW_H: f32 = 44.0;
/// Room inside a row, either side.
const PAD: f32 = 6.0;
/// What the strays card needs below the list.
const STRAYS_H: f32 = 84.0;

/// What the pointer or a key did in the list.
#[derive(Default)]
struct Out {
    select: Option<MediaId>,
    /// Double-click or Enter: show where it is used, or locate it.
    open: Option<MediaId>,
    locate: Option<MediaId>,
    reveal: Option<std::path::PathBuf>,
    remove: Option<MediaId>,
    select_clips: Option<MediaId>,
}

impl App {
    /// Draws the Project media tab below the tabs, in `body`.
    pub(super) fn media_tab(&mut self, ui: &mut Ui, body: Rect) {
        let ctx = ui.ctx().clone();
        let side = (theme::space::M - theme::space::S) as i8;

        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(side, 0))
            .show(ui, |ui| self.media_controls(ui, &ctx));

        let pool = &self.media_pool;
        let strays = pool.strays();
        let footer = if strays.files.is_empty() {
            0.0
        } else {
            STRAYS_H
        };
        let top = ui.cursor().top() + theme::space::XS;
        let area = Rect::from_min_max(
            pos2(body.left(), top),
            pos2(body.right(), (body.bottom() - footer).max(top)),
        );
        let shown: Vec<Row> = pool.shown().into_iter().cloned().collect();
        let selected = pool.selected;
        let all_empty = pool.rows().is_empty();
        if shown.is_empty() {
            self.media_empty(ui, area, all_empty);
        } else {
            let keys = self.media_keys(&ctx, &shown);
            let out = self.media_list(ui, area, &shown, selected, keys);
            self.apply_media_list(&ctx, out);
        }
        if footer > 0.0 {
            let rect = Rect::from_min_max(pos2(body.left(), area.bottom()), body.max);
            self.strays_card(ui, rect);
        }
    }

    /// The search, the filter, what is under way and what to do about it.
    fn media_controls(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let searched = controls::search(
            ui,
            &mut self.media_pool.text,
            &mut self.media_pool.focus_search,
            "Search project media",
        );
        if searched.escaped {
            if self.media_pool.text.is_empty() {
                panes::focus(ctx, Pane::Browser);
            } else {
                self.media_pool.text.clear();
            }
        }
        ui.add_space(theme::space::S);
        let (all, missing, unused) = (
            self.media_pool.rows().len(),
            self.media_pool.missing(),
            self.media_pool.unused(),
        );
        let labels = [
            format!("All {all}"),
            format!("Missing {missing}"),
            format!("Unused {unused}"),
        ];
        if let Some(filter) = widgets::segmented(
            ui,
            Id::new("media filter"),
            self.media_pool.filter,
            &[
                widgets::Choice::new(Filter::All, &labels[0])
                    .named("All files")
                    .tip("Every audio file the project holds"),
                widgets::Choice::new(Filter::Missing, &labels[1])
                    .named("Missing files")
                    .tip("Files the project names that are not on this machine"),
                widgets::Choice::new(Filter::Unused, &labels[2])
                    .named("Unused files")
                    .tip("Files no clip and no frozen track plays"),
            ],
        ) {
            self.media_pool.filter = filter;
        }
        ui.add_space(theme::space::S);
        self.media_state(ui, ctx);
        let actions = missing > 0 || unused > 0;
        if actions {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);
                if missing > 0
                    && self.media_pool.search.is_none()
                    && widgets::tip(
                        widgets::raised::button(ui, "Search a folder\u{2026}"),
                        "Search a folder",
                        None,
                        Some("Looks in the folder you choose, and in the folders inside it, for the missing files by name"),
                    )
                    .clicked()
                {
                    self.search_folder_for_media(ctx);
                }
                if unused > 0
                    && widgets::tip(
                        widgets::raised::button(ui, &format!("Remove {unused} unused")),
                        "Remove unused files",
                        None,
                        Some("Takes the files nothing plays out of the project. The files stay where they are, and Undo brings the entries back"),
                    )
                    .clicked()
                {
                    self.remove_unused_media();
                }
            });
            ui.add_space(theme::space::XS);
        }
    }

    /// A search under way, what it found, a file waiting to be confirmed, or
    /// the last thing that happened.
    fn media_state(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        if let Some(search) = &self.media_pool.search {
            let progress = search.control.progress();
            let folder = search.folder.display().to_string();
            let mut stop = false;
            card(ui, |ui| {
                wrapped(ui, &format!("Searching {folder}"));
                bar(ui, progress);
                stop = widgets::raised::button(ui, "Stop").clicked();
            });
            if stop || ctx.input(|i| i.key_pressed(Key::Escape)) && !ctx.text_edit_focused() {
                self.stop_search();
            }
            return;
        }
        if let Some((id, path, frames)) = self.media_pool.confirm.clone() {
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let wanted = self
                .media_pool
                .rows()
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.frames);
            let found = frames.map_or_else(
                || "a length the file does not say".to_owned(),
                |f| format!("{f} frames"),
            );
            let (mut yes, mut no) = (false, false);
            card(ui, |ui| {
                wrapped(
                    ui,
                    &format!(
                        "{name} is {found} long, and the file the project used was {wanted} frames. Clips may play the wrong part."
                    ),
                );
                ui.horizontal_wrapped(|ui| {
                    yes = widgets::raised::primary_button(ui, "Use it anyway").clicked();
                    no = widgets::raised::button(ui, "Cancel").clicked();
                });
            });
            if yes {
                self.accept_confirmed();
            } else if no {
                self.media_pool.confirm = None;
            }
            return;
        }
        if self.media_pool.report.is_some() {
            self.report_card(ui);
            return;
        }
        if let Some(note) = self.media_pool.note.clone() {
            let mut dismiss = false;
            card(ui, |ui| {
                wrapped(ui, &note);
                dismiss = widgets::raised::button(ui, "Dismiss").clicked();
            });
            if dismiss {
                self.media_pool.note = None;
            }
        }
    }

    /// What a folder search found, to look at before anything changes.
    fn report_card(&mut self, ui: &mut Ui) {
        let Some(report) = self.media_pool.report.clone() else {
            return;
        };
        let name = |id: MediaId| {
            self.media_pool
                .rows()
                .iter()
                .find(|r| r.id == id)
                .map_or_else(|| "A file".to_owned(), |r| r.name.clone())
        };
        let (mut apply, mut dismiss) = (false, false);
        let mut use_anyway = None;
        card(ui, |ui| {
            let found = report.found.len();
            wrapped(
                ui,
                &if found == 0 {
                    "No file was found that can stand in for a missing one.".to_owned()
                } else {
                    format!(
                        "Found {found} {}. Nothing changes until you relink them.",
                        if found == 1 { "file" } else { "files" }
                    )
                },
            );
            for f in report.found.iter().take(6) {
                detail(
                    ui,
                    &format!("{} \u{2192} {}", name(f.id), parent_of(&f.path)),
                );
            }
            if found > 6 {
                detail(ui, &format!("and {} more", found - 6));
            }
            for f in &report.different {
                let length = match &f.verdict {
                    Verdict::DifferentLength(Some(frames)) => format!("{frames} frames"),
                    _ => "another length".to_owned(),
                };
                detail(
                    ui,
                    &format!(
                        "{} is {length} in {}: not taken",
                        name(f.id),
                        parent_of(&f.path)
                    ),
                );
                if widgets::raised::button(ui, &format!("Use {} anyway", name(f.id))).clicked() {
                    use_anyway = Some(Found {
                        id: f.id,
                        path: f.path.clone(),
                        verdict: f.verdict.clone(),
                    });
                }
            }
            for (id, paths) in &report.ambiguous {
                detail(
                    ui,
                    &format!(
                        "{} fits {} files equally: not taken",
                        name(*id),
                        paths.len()
                    ),
                );
            }
            if !report.not_found.is_empty() {
                detail(
                    ui,
                    &format!("{} not found", count_files(report.not_found.len())),
                );
            }
            if report.truncated {
                detail(ui, "The folder is very large: the search stopped early");
            }
            ui.horizontal_wrapped(|ui| {
                if found > 0 {
                    apply =
                        widgets::raised::primary_button(ui, &format!("Relink {found}")).clicked();
                }
                dismiss = widgets::raised::button(ui, "Dismiss").clicked();
            });
        });
        if let Some(f) = use_anyway {
            if let Some(report) = &mut self.media_pool.report {
                report.different.retain(|d| d.id != f.id);
            }
            self.relink_media(f.id, f.path);
        }
        if apply {
            self.apply_report();
        } else if dismiss {
            self.media_pool.report = None;
        }
    }

    /// The list is empty: nothing in the project, or nothing that matches.
    fn media_empty(&mut self, ui: &mut Ui, area: Rect, nothing_at_all: bool) {
        let (title, hint) = if nothing_at_all {
            (
                "No audio in this project",
                "Drop an audio file on the arrangement to add one",
            )
        } else if self.media_pool.filter == Filter::Missing && self.media_pool.text.is_empty() {
            ("No file is missing", "Everything the project names is here")
        } else if self.media_pool.filter == Filter::Unused && self.media_pool.text.is_empty() {
            (
                "Every file is used",
                "Nothing here is waiting to be cleared",
            )
        } else {
            ("No files match", "Try other words, or show all files")
        };
        let action = (!nothing_at_all).then_some(widgets::EmptyAction {
            label: "Show all",
            keys: None,
            tip: "Clear the words and show every file again",
        });
        let state = widgets::EmptyState {
            icon: None,
            icon_size: theme::space::L,
            title,
            hint,
            action,
        };
        if widgets::empty_state(ui, area, &state) {
            self.media_pool.text.clear();
            self.media_pool.filter = Filter::All;
        }
    }

    /// The arrows move through the list while the pane has the keyboard.
    /// Returns whether the selection moved (to scroll it into view).
    fn media_keys(&mut self, ctx: &egui::Context, shown: &[Row]) -> bool {
        if panes::focused(ctx) != Some(Pane::Browser) || ctx.text_edit_focused() {
            return false;
        }
        let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        let at = self
            .media_pool
            .selected
            .and_then(|id| shown.iter().position(|r| r.id == id));
        let to = if key(Key::ArrowDown) {
            Some(at.map_or(0, |i| (i + 1).min(shown.len() - 1)))
        } else if key(Key::ArrowUp) {
            Some(at.map_or(shown.len() - 1, |i| i.saturating_sub(1)))
        } else if key(Key::Home) {
            Some(0)
        } else if key(Key::End) {
            Some(shown.len() - 1)
        } else {
            None
        };
        if key(Key::Enter)
            && let Some(row) = at.and_then(|i| shown.get(i))
        {
            self.open_media_row(ctx, row.id, row.missing);
        }
        match to {
            Some(i) if Some(i) != at => {
                self.media_pool.selected = Some(shown[i].id);
                true
            }
            _ => false,
        }
    }

    /// Double-click or Enter: a missing file is located, a used one is
    /// shown where it is used.
    fn open_media_row(&mut self, ctx: &egui::Context, id: MediaId, missing: bool) {
        if missing {
            self.locate_media(ctx, id);
        } else {
            self.select_clips_of(id);
        }
    }

    fn apply_media_list(&mut self, ctx: &egui::Context, out: Out) {
        if let Some(id) = out.select {
            self.media_pool.selected = Some(id);
            panes::focus(ctx, Pane::Browser);
        }
        if let Some(id) = out.open {
            let missing = self
                .media_pool
                .rows()
                .iter()
                .find(|r| r.id == id)
                .is_some_and(|r| r.missing);
            self.open_media_row(ctx, id, missing);
        }
        if let Some(id) = out.locate {
            self.locate_media(ctx, id);
        }
        if let Some(id) = out.select_clips {
            self.select_clips_of(id);
        }
        if let Some(path) = out.reveal {
            crate::crash::reveal(&path);
        }
        if let Some(id) = out.remove {
            let removed = self.document.remove_unused_media(&[id]);
            if removed > 0 {
                self.media_pool.note = Some(
                    "Removed the file from the project. The file itself is untouched. Undo brings it back."
                        .into(),
                );
            }
        }
    }

    /// The rows in view, painted one by one.
    fn media_list(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        shown: &[Row],
        selected: Option<MediaId>,
        scroll_to_selected: bool,
    ) -> Out {
        let mut out = Out::default();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let width = rect.width();
        let total = ROW_H * shown.len() as f32;
        let project_rate = self.document.project().sample_rate;
        let wanted = selected.and_then(|id| shown.iter().position(|r| r.id == id));
        egui::ScrollArea::vertical()
            .id_salt("media list")
            .auto_shrink([false, false])
            .show_viewport(&mut child, |ui, viewport| {
                let origin = ui.cursor().min;
                ui.allocate_space(vec2(width, total));
                for (i, row) in shown.iter().enumerate() {
                    let (top, bottom) = (ROW_H * i as f32, ROW_H * (i + 1) as f32);
                    let scroll = scroll_to_selected && Some(i) == wanted;
                    if !scroll && (bottom < viewport.top() || top > viewport.bottom()) {
                        continue;
                    }
                    let at = Rect::from_min_size(
                        pos2(origin.x + PAD, origin.y + top),
                        vec2(width - 2.0 * PAD - theme::space::XS, ROW_H),
                    );
                    if scroll {
                        ui.scroll_to_rect(at, None);
                    }
                    let file = self.document.media_files.get(&row.id).cloned();
                    media_row(
                        ui,
                        at,
                        row,
                        selected == Some(row.id),
                        file,
                        project_rate,
                        &mut out,
                    );
                }
            });
        out
    }

    /// Files that belong to no project, and the way to move them to the trash.
    fn strays_card(&mut self, ui: &mut Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let strays = self.media_pool.strays();
        let (count, bytes) = (strays.files.len(), strays.bytes());
        let unfinished = strays.files.iter().filter(|s| s.unfinished).count();
        let armed = self.media_pool.armed;
        let working = self.is_working_in_files();
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(theme::space::S, 0.0))));
        let (mut press, mut cancel) = (false, false);
        card(&mut child, |ui| {
            let size = crate::files::describe_size(bytes);
            let verb = if count == 1 { "belongs" } else { "belong" };
            let mut words = format!("{} {verb} to no project ({size}).", count_files(count));
            if unfinished > 0 {
                words.push_str(&format!(" {unfinished} of them an unfinished recording."));
            }
            wrapped(ui, &words);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);
                let label = if armed {
                    format!("Move {} to the trash", count_files(count))
                } else {
                    "Clear\u{2026}".to_owned()
                };
                let button = ui.add_enabled_ui(!working, |ui| {
                    if armed {
                        widgets::raised::primary_button(ui, &label)
                    } else {
                        widgets::raised::button(ui, &label)
                    }
                });
                press = button.inner.clicked();
                if working {
                    button
                        .response
                        .on_hover_text("Wait for the render or recording under way to finish");
                }
                if armed {
                    cancel = widgets::raised::button(ui, "Cancel").clicked();
                }
            });
        });
        if press {
            self.clear_stray_files(&ctx);
        } else if cancel {
            self.media_pool.armed = false;
        }
    }
}

/// One row: the name and where it stands, then what it is.
fn media_row(
    ui: &mut Ui,
    rect: Rect,
    row: &Row,
    selected: bool,
    file: Option<std::path::PathBuf>,
    project_rate: u32,
    out: &mut Out,
) {
    let palette = *theme::p();
    let id = Id::new(("media row", row.id.0));
    let response = ui.interact(rect, id, Sense::click());
    let state = state_word(row);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{}, {state}", row.name),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let response = response.on_hover_text(if row.missing {
        format!("Not found: {}", row.path)
    } else {
        row.path.clone()
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover),
    );

    let (tag, tag_ink) = if row.missing {
        ("Missing".to_owned(), palette.warning)
    } else if row.unused() {
        ("Unused".to_owned(), palette.text_hint)
    } else if row.uses == 1 {
        ("1 clip".to_owned(), palette.text_hint)
    } else {
        (format!("{} clips", row.uses), palette.text_hint)
    };
    let tag = painter.layout_no_wrap(tag, fonts::medium(theme::size::CAPTION), tag_ink);
    let right = rect.right() - PAD - tag.size().x;
    let left = rect.left() + PAD + theme::space::XS;
    let name = fit(
        painter,
        &row.name,
        fonts::medium(theme::size::BODY),
        if row.missing {
            palette.text_secondary
        } else {
            palette.text
        },
        right - left - theme::space::S,
    );
    let first = rect.top() + 7.0;
    painter.galley(pos2(left, first), name, palette.text);
    painter.galley(pos2(right, first + 1.0), tag, tag_ink);
    let second = fit(
        painter,
        &detail_line(row, project_rate),
        FontId::proportional(theme::size::CAPTION),
        palette.text_hint,
        rect.right() - PAD - left,
    );
    painter.galley(
        pos2(left, rect.bottom() - 7.0 - second.size().y),
        second,
        palette.text_hint,
    );
    widgets::raised::focus_ring(painter, &response, rect);

    if response.clicked() {
        out.select = Some(row.id);
    }
    if response.double_clicked() {
        out.open = Some(row.id);
    }
    widgets::Menu::context(&response).show(|ui| {
        out.select = Some(row.id);
        if row.missing && widgets::menu_item(ui, "Locate\u{2026}", None, true).clicked() {
            out.locate = Some(row.id);
            ui.close();
        }
        if row.uses > 0 && widgets::menu_item(ui, "Select its clips", None, true).clicked() {
            out.select_clips = Some(row.id);
            ui.close();
        }
        if let Some(file) = &file
            && !row.missing
            && widgets::menu_item(ui, "Show in folder", None, true).clicked()
        {
            out.reveal = Some(file.clone());
            ui.close();
        }
        if row.unused() && widgets::menu_item(ui, "Remove from project", None, true).clicked() {
            out.remove = Some(row.id);
            ui.close();
        }
    });
}

fn state_word(row: &Row) -> String {
    if row.missing {
        "missing".to_owned()
    } else if row.unused() {
        "unused".to_owned()
    } else {
        format!("used by {} clips", row.uses)
    }
}

/// The second line: where it lives and what it is. A missing file says where
/// the project looked.
fn detail_line(row: &Row, project_rate: u32) -> String {
    if row.missing {
        return format!("Was {}", row.path);
    }
    let seconds = row.frames as f64 / f64::from(row.sample_rate.max(1));
    let channels = match row.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} channels"),
    };
    let mut line = format!(
        "{} \u{00B7} {}:{:02} \u{00B7} {} \u{00B7} {}",
        row.home.label(),
        (seconds / 60.0) as u64,
        (seconds % 60.0) as u64,
        channels,
        crate::files::describe_size(row.size_bytes),
    );
    // The rate is said when it is not the project's: that is the file the
    // engine converts.
    if row.sample_rate != project_rate {
        let khz = f64::from(row.sample_rate) / 1000.0;
        line.push_str(&format!(" \u{00B7} {khz} kHz"));
    }
    line
}

/// A line of `text` cut with "…" to fit `width`.
fn fit(
    painter: &Painter,
    text: &str,
    font: FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, colour));
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    painter.layout_job(job)
}

/// A card on the pane's ground, the width of the pane.
fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::p().raised)
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(egui::Margin::same(theme::space::S as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);
            add(ui);
        });
    ui.add_space(theme::space::XS);
}

/// A sentence that wraps within the card.
fn wrapped(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text),
        )
        .wrap(),
    );
}

/// A quieter line under the sentence.
fn detail(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text_hint),
        )
        .wrap(),
    );
}

/// A thin bar: a sunk trough and the accent for how far.
fn bar(ui: &mut Ui, fraction: f32) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), theme::space::S), Sense::hover());
    widgets::raised::paint_field(ui.painter(), rect, theme::radius::SMALL, 0.0);
    let filled = Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter()
        .rect_filled(filled, theme::radius::SMALL, theme::p().accent);
}

fn count_files(n: usize) -> String {
    if n == 1 {
        "1 file".to_owned()
    } else {
        format!("{n} files")
    }
}

/// The folder a found file is in, for the report.
fn parent_of(path: &std::path::Path) -> String {
    path.parent()
        .map_or_else(|| path.display().to_string(), |p| p.display().to_string())
}
