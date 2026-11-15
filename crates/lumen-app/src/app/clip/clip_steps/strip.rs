use super::*;

impl App {
    pub(super) fn steps_strip(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        clip: &Clip,
        setup: Setup,
        layout: &GridLayout,
    ) -> Option<Setup> {
        let p = theme::p();
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, p.header);
        painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(theme::stroke::HAIRLINE, p.border_strong),
        );
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, p.border),
        );
        let inner = rect.shrink2(vec2(theme::space::M, 0.0));
        let clip_rect = rect.intersect(ui.clip_rect());
        let mut next = setup;
        let id = ui.id().with(("steps strip", clip.id.0));
        let caption = |ui: &mut Ui, text: &str| {
            ui.label(
                RichText::new(text)
                    .size(theme::size::CAPTION)
                    .color(theme::p().text_secondary),
            );
        };

        let mut right = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::right_to_left(Align::Center)),
        );
        right.set_clip_rect(clip_rect);
        right.spacing_mut().item_spacing.x = theme::space::S;
        if layout.pages > 1 {
            let page = setup.page.min(layout.pages - 1);
            right.label(
                RichText::new(format!("of {}", layout.pages))
                    .monospace()
                    .size(theme::size::SCALE)
                    .color(p.text_hint),
            );
            if layout.pages <= NUMBERED_PAGES {
                let choices: Vec<Choice<'_, u32>> = (0..layout.pages)
                    .map(|n| {
                        Choice::new(n, PAGE_NUMBERS[n as usize])
                            .named(PAGE_NAMES[n as usize])
                            .tip("Show this page of the clip's steps")
                    })
                    .collect();
                if let Some(chosen) =
                    widgets::segmented(&mut right, id.with("pages"), page, &choices)
                {
                    next = next.with_page(i64::from(chosen), layout.pages);
                }
            } else {
                let forward =
                    raised::button(&mut right, "Next").on_hover_text("Show the next page of steps");
                if forward.clicked() {
                    next = next.with_page(i64::from(page) + 1, layout.pages);
                }
                let back = raised::button(&mut right, "Previous")
                    .on_hover_text("Show the previous page of steps");
                if back.clicked() {
                    next = next.with_page(i64::from(page) - 1, layout.pages);
                }
                right.label(
                    RichText::new(format!("{} of {}", page + 1, layout.pages))
                        .monospace()
                        .size(theme::size::SCALE)
                        .color(p.text_hint),
                );
            }
            caption(&mut right, "Page");
        }
        let used = right.min_rect();
        let end = if used.width() > 0.0 {
            used.left() - theme::space::M
        } else {
            inner.right()
        };
        let room = Rect::from_x_y_ranges(inner.left()..=end.max(inner.left()), inner.y_range());
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(room)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(Rect::from_x_y_ranges(
            clip_rect.left()..=end.max(clip_rect.left()),
            clip_rect.y_range(),
        ));
        row.spacing_mut().item_spacing.x = theme::space::M;

        let counts: Vec<Choice<'_, u32>> = COUNTS
            .iter()
            .zip(COUNT_NAMES)
            .map(|(n, (label, name))| {
                Choice::new(*n, label)
                    .named(name)
                    .tip("How many steps a page shows")
            })
            .collect();
        if let Some(count) = widgets::segmented(&mut row, id.with("count"), setup.count, &counts) {
            next.count = count;
            next.page = 0;
        }
        caption(&mut row, "steps");
        let resolution = RESOLUTIONS[setup.resolution.min(RESOLUTIONS.len() - 1)];
        let button =
            raised::button(&mut row, &resolution.label()).on_hover_text("How long a step is");
        widgets::Menu::below(&button).show(|ui| {
            for (n, r) in RESOLUTIONS.iter().enumerate() {
                if widgets::menu_toggle(ui, &r.label(), None, n == setup.resolution).clicked() {
                    next.resolution = n;
                    next.page = 0;
                    ui.close();
                }
            }
        });
        caption(&mut row, "grid");
        let accent = raised::button(&mut row, &format!("Accent {}", setup.accent))
            .on_hover_text("The velocity an accent sets (Shift-click a step)");
        widgets::Menu::below(&accent).show(|ui| {
            for a in ACCENTS {
                if widgets::menu_toggle(ui, &format!("{a}"), None, a == setup.accent).clicked() {
                    next.accent = a;
                    ui.close();
                }
            }
        });
        (next != setup).then_some(next)
    }
}

const PAGE_NAMES: [&str; NUMBERED_PAGES as usize] = [
    "Page 1", "Page 2", "Page 3", "Page 4", "Page 5", "Page 6", "Page 7", "Page 8",
];
