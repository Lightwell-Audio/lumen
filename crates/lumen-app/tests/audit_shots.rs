//! The whole-application look audit (M7b E6): every menu, popover, window
//! and panel, rendered headless to PNG in both themes and several scales,
//! with two mechanical checks on each state (controls without a name, and
//! labelled controls that leave the window). Not a test of behaviour: it
//! is ignored, and runs on demand with a GPU:
//!
//! ```text
//! $env:SHOTS = "<existing folder>"
//! cargo test -p lumen-app --test audit_shots -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The mechanical findings are appended to `<SHOTS>/audit-report.txt`.

use std::io::Write as _;
use std::path::PathBuf;

use eframe::egui::accesskit::{Action, Role};
use eframe::egui::{Event, Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use lumen_app::{App, Launch};
use lumen_project::testing::TempDir;

type H = Harness<'static, App>;

/// A window being audited, and where its pictures and findings go.
struct Scene {
    h: H,
    tag: String,
    dir: TempDir,
    /// Names already listed, so a state lists only what is new in it.
    seen: std::collections::HashSet<String>,
}

fn shots_dir() -> PathBuf {
    PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"))
}

fn report(line: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(shots_dir().join("audit-report.txt"))
        .expect("the report file");
    writeln!(file, "{line}").expect("write the report");
}

impl Scene {
    /// A window of `size` in `appearance` at `scale`, with three tones on
    /// three tracks when `files` is set.
    fn new(name: &str, appearance: &str, scale: f32, size: (f32, f32), files: bool) -> Scene {
        let dir = TempDir::new(&format!("audit-{name}-{appearance}-{scale}"));
        let launch = Launch::for_test(dir.path()).appearing(appearance);
        let mut h = Harness::builder()
            .with_size(vec2(size.0, size.1))
            .with_pixels_per_point(scale)
            .with_theme(if appearance == "light" {
                eframe::egui::Theme::Light
            } else {
                eframe::egui::Theme::Dark
            })
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(10);
        let mut scene = Scene {
            h,
            tag: format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
            dir,
            seen: Default::default(),
        };
        if files {
            scene.drop_files(1.5);
        }
        scene
    }

    /// A window with a fixture project from `tests/projects` open.
    fn with_project(
        name: &str,
        appearance: &str,
        scale: f32,
        size: (f32, f32),
        fixture: &str,
    ) -> Scene {
        let dir = TempDir::new(&format!("audit-{name}-{appearance}-{scale}"));
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/projects")
            .join(fixture);
        let copy = dir.path().join(fixture);
        std::fs::copy(&source, &copy).expect("the fixture project");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .opening(copy);
        let mut h = Harness::builder()
            .with_size(vec2(size.0, size.1))
            .with_pixels_per_point(scale)
            .with_theme(if appearance == "light" {
                eframe::egui::Theme::Light
            } else {
                eframe::egui::Theme::Dark
            })
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(60);
        Scene {
            h,
            tag: format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
            dir,
            seen: Default::default(),
        }
    }

    fn drop_files(&mut self, seconds: f32) {
        let files: Vec<PathBuf> = ["kick", "snare", "bass"]
            .iter()
            .map(|n| wav(&self.dir.path().join(format!("{n}.wav")), seconds))
            .collect();
        self.h.input_mut().dropped_files = files
            .into_iter()
            .map(|p| std::sync::Arc::new(Dropped(p)) as eframe::egui::DroppedFileHandle)
            .collect();
        self.h.run_steps(60);
    }

    fn steps(&mut self, n: usize) {
        self.h.run_steps(n);
    }

    /// Saves a picture and runs the two mechanical checks.
    fn snap(&mut self, what: &str) {
        self.h.run_steps(4);
        let name = format!("{what}-{}", self.tag);
        let image = self.h.render().expect("render");
        image
            .save(shots_dir().join(format!("{name}.png")))
            .expect("save");
        self.check(&name);
        if self.tag.contains("dark-100") {
            self.list_new(what);
        }
    }

    /// The names that appeared since the last state, to learn what a menu
    /// holds.
    fn list_new(&mut self, what: &str) {
        let mut fresh = Vec::new();
        for n in self
            .h
            .query_all(by().predicate(|n| n.label().is_some_and(|l| !l.trim().is_empty())))
        {
            let label = n
                .accesskit_node()
                .label()
                .unwrap_or_default()
                .replace('\n', " ");
            if self.seen.insert(label.clone()) {
                let r = n.rect();
                fresh.push(format!(
                    "{:?} \"{label}\" ({:.0},{:.0})",
                    n.accesskit_node().role(),
                    r.left(),
                    r.top()
                ));
            }
        }
        if !fresh.is_empty() {
            report(&format!("--- NEW in [{what}] {}", fresh.len()));
            for f in fresh {
                report(&format!("    {f}"));
            }
        }
    }

    fn check(&mut self, name: &str) {
        let screen = self.h.ctx.content_rect();
        let ppp = self.h.ctx.pixels_per_point();
        let unnamed: Vec<String> = self
            .h
            .query_all(by().predicate(|node| {
                let data = node.data();
                let acts = data.supports_action(Action::Click)
                    || data.supports_action(Action::Focus)
                    || matches!(
                        node.role(),
                        Role::TextInput | Role::Slider | Role::SpinButton | Role::CheckBox
                    );
                let named = node.label().is_some_and(|l| !l.trim().is_empty())
                    || node.value().is_some_and(|v| !v.trim().is_empty());
                let container = node.children().next().is_some()
                    && !matches!(node.role(), Role::Button | Role::Label);
                acts && !named
                    && !container
                    && !matches!(node.role(), Role::Window | Role::ScrollBar)
            }))
            .map(|n| {
                let r = n.rect();
                format!(
                    "{:?} at ({:.0},{:.0}) {:.0}x{:.0}",
                    n.accesskit_node().role(),
                    r.left(),
                    r.top(),
                    r.width(),
                    r.height()
                )
            })
            .collect();
        for u in unnamed {
            report(&format!("UNNAMED  [{name}] {u}"));
        }
        let outside: Vec<String> = self
            .h
            .query_all(by().predicate(|node| node.label().is_some_and(|l| !l.trim().is_empty())))
            .filter_map(|n| {
                let r = n.rect();
                if r.width() <= 1.0 || r.height() <= 1.0 {
                    return None;
                }
                let slack = 1.5 / ppp;
                let out = r.left() < screen.left() - slack
                    || r.top() < screen.top() - slack
                    || r.right() > screen.right() + slack
                    || r.bottom() > screen.bottom() + slack;
                // Wholly outside is scrolled content; only a control that
                // straddles the edge has left the window.
                (out && r.intersects(screen)).then(|| {
                    format!(
                        "{:?} \"{}\" at ({:.0},{:.0},{:.0},{:.0}) window {:.0}x{:.0}",
                        n.accesskit_node().role(),
                        n.accesskit_node().label().unwrap_or_default(),
                        r.left(),
                        r.top(),
                        r.right(),
                        r.bottom(),
                        screen.width(),
                        screen.height()
                    )
                })
            })
            .collect();
        for o in outside {
            report(&format!("OFFSCREEN [{name}] {o}"));
        }
    }

    /// Every labelled control on screen, for finding names while writing a
    /// scene.
    fn dump(&self, what: &str) {
        let mut seen = Vec::new();
        for n in self
            .h
            .query_all(by().predicate(|n| n.label().is_some_and(|l| !l.trim().is_empty())))
        {
            let r = n.rect();
            seen.push(format!(
                "{:?} \"{}\" ({:.0},{:.0},{:.0},{:.0})",
                n.accesskit_node().role(),
                n.accesskit_node()
                    .label()
                    .unwrap_or_default()
                    .replace('\n', " "),
                r.left(),
                r.top(),
                r.right(),
                r.bottom()
            ));
        }
        report(&format!("--- LABELS [{what}] {}", seen.len()));
        for s in seen {
            report(&format!("    {s}"));
        }
    }

    /// The centre of the first control whose name contains `label`.
    fn find(&self, label: &str) -> Option<Rect> {
        self.h
            .query_all_by_label_contains(label)
            .map(|n| n.rect())
            .next()
    }

    /// Left-clicks the first control whose name contains `label`. A missing
    /// one is a finding.
    fn click(&mut self, label: &str) -> bool {
        match self.find(label) {
            Some(r) => {
                click(&mut self.h, r.center());
                self.h.run_steps(8);
                true
            }
            None => {
                report(&format!(
                    "MISSING  [{}] no control named \"{label}\"",
                    self.tag
                ));
                false
            }
        }
    }

    /// The first control named `label` that is wider than a button, such as a
    /// menu row.
    fn find_row(&self, label: &str) -> Option<Rect> {
        self.h
            .query_all_by_label_contains(label)
            .map(|n| n.rect())
            .find(|r| r.width() > 100.0)
    }

    /// Left-clicks the lowest control named `label` (the detail panel's tab
    /// rather than the browser's of the same name).
    fn click_last(&mut self, label: &str) -> bool {
        let found = self
            .h
            .query_all_by_label_contains(label)
            .map(|n| n.rect())
            .max_by(|a, b| a.top().total_cmp(&b.top()));
        match found {
            Some(r) => {
                click(&mut self.h, r.center());
                self.h.run_steps(8);
                true
            }
            None => {
                report(&format!(
                    "MISSING  [{}] no control named \"{label}\"",
                    self.tag
                ));
                false
            }
        }
    }

    fn right(&mut self, label: &str) -> bool {
        match self.find(label) {
            Some(r) => {
                right_click(&mut self.h, r.center());
                self.h.run_steps(8);
                true
            }
            None => {
                report(&format!(
                    "MISSING  [{}] no control named \"{label}\"",
                    self.tag
                ));
                false
            }
        }
    }

    fn hover(&mut self, label: &str) -> bool {
        match self.find(label) {
            Some(r) => {
                self.h.hover_at(r.center());
                self.h.run_steps(40);
                true
            }
            None => {
                report(&format!(
                    "MISSING  [{}] no control named \"{label}\"",
                    self.tag
                ));
                false
            }
        }
    }

    fn esc(&mut self) {
        self.h.key_press(Key::Escape);
        self.h.run_steps(6);
    }

    fn away(&mut self) {
        self.h.hover_at(pos2(2.0, 2.0));
        self.h.run_steps(6);
    }

    /// A command from the palette, by (part of) its name.
    fn command(&mut self, name: &str) {
        self.h.key_press_modifiers(Modifiers::COMMAND, Key::K);
        self.h.run_steps(3);
        self.h.event(Event::Text(name.into()));
        self.h.run_steps(3);
        self.h.key_press(Key::Enter);
        self.h.run_steps(12);
    }

    fn chord(&mut self, modifiers: Modifiers, key: Key) {
        self.h.key_press_modifiers(modifiers, key);
        self.h.run_steps(8);
    }

    fn rect_of_track(&self, index: usize) -> Rect {
        let id = self.h.state().project().tracks[index].clips[0].id;
        self.h.state().clip_rect(&self.h.ctx, id).expect("in view")
    }
}

/// The looks the audit covers: the owner's common window in both themes, and
/// the smallest window at 200%.
const LOOKS: [(&str, f32, (f32, f32)); 3] = [
    ("dark", 1.0, (1280.0, 760.0)),
    ("light", 1.5, (1280.0, 760.0)),
    ("dark", 2.0, (960.0, 540.0)),
];

/// Runs `scene` for the looks the audit covers.
fn looks(name: &str, files: bool, scene: impl Fn(&mut Scene)) {
    for (appearance, scale, size) in LOOKS {
        let mut s = Scene::new(name, appearance, scale, size, files);
        scene(&mut s);
    }
}

/// [`looks`] with the fixture project `fixture` open.
fn looks_in(name: &str, fixture: &str, scene: impl Fn(&mut Scene)) {
    for (appearance, scale, size) in LOOKS {
        let mut s = Scene::with_project(name, appearance, scale, size, fixture);
        scene(&mut s);
    }
}

fn click(h: &mut H, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
}

fn right_click(h: &mut H, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
}

fn pos(r: Rect, fx: f32, fy: f32) -> Pos2 {
    Pos2::new(r.left() + r.width() * fx, r.top() + r.height() * fy)
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// A stereo 16-bit WAV of a soft tone, `seconds` long.
fn wav(path: &std::path::Path, seconds: f32) -> PathBuf {
    let rate = 48_000_u32;
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let v = ((i as f32 * 0.05).sin() * 9_000.0 * (1.0 - i as f32 / frames as f32)) as i16;
            let [a, b] = v.to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the WAV");
    path.to_path_buf()
}

#[path = "audit_shots/scenes.rs"]
mod scenes;
