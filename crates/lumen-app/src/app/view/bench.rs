//! The `--bench` and `--bench-editor` drivers: they put the view where the
//! benchmark wants it, frame by frame, and report when it is done
//! (`crate::bench`).

use eframe::egui;

use crate::{App, bench};

impl App {
    /// `--bench-frames`: puts the view where the benchmark wants it this
    /// frame, or reports and exits when it is done (`crate::bench`).
    pub(crate) fn drive_bench(&mut self, ctx: &egui::Context) {
        let Some(bench) = &mut self.bench else {
            return;
        };
        let now = std::time::Instant::now();
        let step = bench.step(now);
        let idle_left = bench.idle_left(now);
        let out = bench.out().to_path_buf();
        let editor = bench.editor();
        match step {
            bench::Step::Drive(view) => {
                self.nav.set(crate::timeview::TimeView {
                    left: f64::from(view.left_beat),
                    zoom: f64::from(view.px_per_beat),
                });
                let content = self.arrangement.height_of_content(
                    ctx,
                    self.document.project(),
                    self.lanes.pending().len(),
                );
                // The lanes take a little over half the window.
                let visible = ctx.content_rect().height() * 0.6;
                self.arrangement.top = view.top * (content - visible).max(0.0);
                if editor {
                    self.bench_editor(view.top);
                }
                ctx.request_repaint();
            }
            bench::Step::Idle => {
                if let Some(bench) = &mut self.bench {
                    for cause in ctx.repaint_causes() {
                        bench.idle_cause(format!("{}:{} {}", cause.file, cause.line, cause.reason));
                    }
                }
                ctx.request_repaint_after(idle_left);
            }
            bench::Step::Done(report, pass) => {
                println!(
                    "{report}
report: {}",
                    out.display()
                );
                std::process::exit(if pass { 0 } else { 3 });
            }
        }
    }

    /// `--bench-editor`: keeps the stress project's dense clip open in the
    /// clip editor, its pitches scrolled to `top` (0 to 1) of their range.
    fn bench_editor(&mut self, top: f32) {
        // The roll has its own view (ADR-0048): the run drives it to the same
        // zoom and scroll as the arrangement, so it is measured zoomed.
        self.note_editor.time = self.nav.view;
        if self.note_editor.clip.is_none() {
            let dense = self
                .document
                .project()
                .tracks
                .last()
                .and_then(|t| t.clips.first())
                .map(|c| c.id);
            if let Some(id) = dense {
                self.open_note_editor(id);
                self.detail_size = 420.0;
            }
        }
        let rows = 128.0 * self.note_editor.row_height;
        self.note_editor.scroll = top * (rows - 300.0).max(0.0);
    }
}
