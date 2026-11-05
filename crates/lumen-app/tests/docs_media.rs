//! Renders the user documentation's screenshots and clips from the real app
//! (docs/USER_DOCS.md §6). Not a test of behaviour: it is ignored, needs a
//! GPU (software Vulkan is enough) and `ffmpeg`, and runs one scene at a
//! time because the theme is process-wide:
//!
//! ```text
//! DOCS_MEDIA=site/docs/static/media cargo test -p lumen-app --test docs_media -- --ignored --test-threads=1
//! ```
//!
//! Each scene writes `<id>-dark` and `<id>-light` files: a WebP for a
//! screenshot, and an MP4 with a WebP poster (its last frame) for a clip.
//! The pointer and the keys pressed are drawn into clips here, so a reader
//! sees what was done as well as what happened.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use eframe::egui::{Event, Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use lumen_app::{App, Launch};
use lumen_project::testing::TempDir;

/// The scale every picture is rendered at: crisp on high-density screens.
const SCALE: f32 = 2.0;
/// Clips play at 30 frames a second; the harness steps at 60.
const STEPS_PER_FRAME: usize = 2;

/// Guides › Editing › Split a clip: the lead clip and the before and after
/// screenshots.
#[test]
#[ignore = "renders the docs' media; needs a GPU, ffmpeg and DOCS_MEDIA"]
fn split_a_clip() {
    for appearance in ["dark", "light"] {
        let dir = TempDir::new(&format!("docs-split-{appearance}"));
        let mut h = song(&dir, appearance);
        let drums = clip(&h, 0);
        let crop = arrangement(&h);
        let cut = pos2(drums.left() + drums.width() * 0.5, drums.bottom() - 8.0);

        // Before: the clip, the pointer resting where the cut will go.
        let mut take = Take::new(&format!("split-lead-{appearance}"), crop, SCALE);
        let rest = pos2(crop.right() - 60.0, crop.bottom() - 24.0);
        h.hover_at(rest);
        take.hold(&mut h, rest, 18);
        take.glide(&mut h, rest, cut, 22);
        take.click(&mut h, cut);
        take.hold(&mut h, cut, 10);
        still(
            &mut h,
            crop,
            &format!("split-before-{appearance}"),
            Some(cut),
        );
        take.keys(&mut h, cut, "Ctrl+E", |h| {
            h.key_press_modifiers(Modifiers::COMMAND, Key::E);
        });
        assert_eq!(
            h.state().project().tracks[0].clips.len(),
            2,
            "Ctrl+E split the clip"
        );
        take.hold(&mut h, cut, 40);
        let away = pos2(cut.x + 170.0, drums.bottom() - 10.0);
        take.glide(&mut h, cut, away, 18);
        take.hold(&mut h, away, 36);
        take.finish();
        h.hover_at(pos2(crop.left() + 8.0, crop.bottom() + 40.0));
        still(&mut h, crop, &format!("split-after-{appearance}"), None);
    }
}

/// A song to work on: drums and bass, four bars at 120 BPM, fitted to the
/// window with the browser and the detail panel out of the way.
fn song(dir: &TempDir, appearance: &str) -> Harness<'static, App> {
    let launch = Launch::for_test(dir.path()).appearing(appearance);
    let mut h = Harness::builder()
        .with_size(vec2(960.0, 420.0))
        .with_pixels_per_point(SCALE)
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
    let files = vec![
        drums(&dir.path().join("Drums.wav")),
        bass(&dir.path().join("Bass.wav")),
    ];
    h.input_mut().dropped_files = files
        .into_iter()
        .map(|p| std::sync::Arc::new(Dropped(p)) as eframe::egui::DroppedFileHandle)
        .collect();
    h.run_steps(60);
    if h.state().browser_shown() {
        command(&mut h, "Show or hide the browser");
    }
    if h.state().detail_shown() {
        h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        h.run_steps(20);
    }
    h.key_press(Key::W);
    h.run_steps(40);
    h
}

/// Runs a command from the palette by its name.
fn command(h: &mut Harness<'static, App>, name: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text(name.into()));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(30);
}

fn clip(h: &Harness<'static, App>, track: usize) -> Rect {
    let id = h.state().project().tracks[track].clips[0].id;
    h.state().clip_rect(&h.ctx, id).expect("in view")
}

/// The arrangement around the song's clips: the ruler, the track headers
/// and the lanes, without the transport bar (whose live CPU reading means
/// nothing in a picture).
fn arrangement(h: &Harness<'static, App>) -> Rect {
    let first = clip(h, 0);
    let last = clip(h, h.state().project().tracks.len() - 1);
    let width = h.ctx.content_rect().width();
    Rect::from_min_max(
        pos2(0.0, first.top() - 25.0),
        pos2(width, last.bottom() + 44.0),
    )
}

/// A frame, cut to `crop` (in points) and saved as `<name>.webp`.
fn still(h: &mut Harness<'static, App>, crop: Rect, name: &str, pointer: Option<Pos2>) {
    h.run_steps(6);
    let mut image = h.render().expect("render");
    if let Some(at) = pointer {
        draw_pointer(&mut image, at, SCALE, 0.0);
    }
    let (w, h_px) = (image.width(), image.height());
    let out = media_dir().join(format!("{name}.webp"));
    let px = crop_px(crop, SCALE, w, h_px);
    ffmpeg(
        &[
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{w}x{h_px}"),
            "-i",
            "-",
            "-vf",
            &format!("crop={}:{}:{}:{}", px[2], px[3], px[0], px[1]),
            "-frames:v",
            "1",
            "-c:v",
            "libwebp",
            "-quality",
            "88",
        ],
        &out,
        |stdin| stdin.write_all(image.as_raw()).expect("write the frame"),
    );
}

/// A clip being recorded: frames go to ffmpeg as they are rendered.
struct Take {
    name: String,
    crop: Rect,
    scale: f32,
    frames: usize,
    /// A click's ring: how many frames ago the button went down.
    since_click: Option<usize>,
    /// The keys shown, and the frames they show from and to.
    captions: Vec<(String, usize, usize)>,
    raw: PathBuf,
    size: (u32, u32),
}

impl Take {
    fn new(name: &str, crop: Rect, scale: f32) -> Self {
        let raw = std::env::temp_dir().join(format!("lumen-docs-{name}.rgba"));
        let _ = std::fs::remove_file(&raw);
        Self {
            name: name.into(),
            crop,
            scale,
            frames: 0,
            since_click: None,
            captions: Vec::new(),
            raw,
            size: (0, 0),
        }
    }

    /// One frame with the pointer at `at`.
    fn frame(&mut self, h: &mut Harness<'static, App>, at: Pos2) {
        h.run_steps(STEPS_PER_FRAME);
        let mut image = h.render().expect("render");
        let ring = self
            .since_click
            .map(|n| n as f32 / 12.0)
            .filter(|t| *t <= 1.0);
        draw_pointer(&mut image, at, self.scale, ring.unwrap_or(0.0));
        if let Some(t) = ring {
            draw_ring(&mut image, at, self.scale, t);
        }
        self.since_click = self.since_click.map(|n| n + 1);
        self.size = (image.width(), image.height());
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.raw)
            .expect("open the frames");
        file.write_all(image.as_raw()).expect("write a frame");
        self.frames += 1;
    }

    fn hold(&mut self, h: &mut Harness<'static, App>, at: Pos2, frames: usize) {
        for _ in 0..frames {
            h.hover_at(at);
            self.frame(h, at);
        }
    }

    /// Moves the pointer from `from` to `to` with an ease in and out.
    fn glide(&mut self, h: &mut Harness<'static, App>, from: Pos2, to: Pos2, frames: usize) {
        for i in 1..=frames {
            let t = i as f32 / frames as f32;
            let eased = t * t * (3.0 - 2.0 * t);
            let at = from + (to - from) * eased;
            h.hover_at(at);
            self.frame(h, at);
        }
    }

    fn click(&mut self, h: &mut Harness<'static, App>, at: Pos2) {
        h.hover_at(at);
        self.since_click = Some(0);
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: at,
                button: eframe::egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame(h, at);
        }
    }

    /// Shows `label` as keycaps for about a second and a half, pressing the
    /// keys (`press`) a third of a second in.
    fn keys(
        &mut self,
        h: &mut Harness<'static, App>,
        at: Pos2,
        label: &str,
        press: impl FnOnce(&mut Harness<'static, App>),
    ) {
        let from = self.frames;
        self.hold(h, at, 10);
        press(h);
        self.frame(h, at);
        self.captions.push((label.into(), from, from + 46));
    }

    /// Encodes the frames: an MP4 that loops, and its last frame as the
    /// poster.
    fn finish(self) {
        let (w, h) = self.size;
        let px = crop_px(self.crop, self.scale, w, h);
        let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/MartianMono.ttf");
        let mut filters = vec![format!("crop={}:{}:{}:{}", px[2], px[3], px[0], px[1])];
        let size = (15.0 * self.scale) as u32;
        // Keycaps, one box per key, at the bottom right. Martian Mono's
        // letters are 0.73 of its size wide.
        let pad = 9.0 * self.scale;
        let advance = 0.73 * size as f32;
        for (label, from, to) in &self.captions {
            let (a, b) = (*from as f32 / 30.0, *to as f32 / 30.0);
            let keys: Vec<&str> = label.split('+').collect();
            let widths: Vec<f32> = keys
                .iter()
                .map(|k| k.chars().count() as f32 * advance + 2.0 * pad)
                .collect();
            let gap = 8.0 * self.scale;
            let total = widths.iter().sum::<f32>() + gap * (keys.len() - 1) as f32;
            let mut left = px[2] as f32 - total - 24.0 * self.scale;
            for (key, width) in keys.iter().zip(&widths) {
                filters.push(format!(
                    "drawtext=fontfile='{}':text='{key}':fontsize={size}:fontcolor=0xE6E8EC:\
                     box=1:boxcolor=0x1F2228@0.95:boxborderw={}:x={}:y=h-th-{}:\
                     enable='between(t,{a:.2},{b:.2})'",
                    font.display(),
                    pad as u32,
                    (left + pad) as u32,
                    (22.0 * self.scale) as u32,
                ));
                left += width + gap;
            }
        }
        let raw = std::fs::read(&self.raw).expect("read the frames");
        let dir = media_dir();
        ffmpeg(
            &[
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{w}x{h}"),
                "-r",
                "30",
                "-i",
                "-",
                "-vf",
                &filters.join(","),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "slow",
                "-crf",
                "24",
                "-movflags",
                "+faststart",
                "-an",
            ],
            &dir.join(format!("{}.mp4", self.name)),
            |stdin| stdin.write_all(&raw).expect("write the frames"),
        );
        let last = &raw[raw.len() - (w * h * 4) as usize..];
        ffmpeg(
            &[
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{w}x{h}"),
                "-i",
                "-",
                "-vf",
                &filters[0],
                "-frames:v",
                "1",
                "-c:v",
                "libwebp",
                "-quality",
                "85",
            ],
            &dir.join(format!("{}.webp", self.name)),
            |stdin| stdin.write_all(last).expect("write the poster"),
        );
        let _ = std::fs::remove_file(&self.raw);
    }
}

/// `crop` in pixels, inside a `w` by `h` image: x, y, width and height,
/// with even sizes for the video encoder.
fn crop_px(crop: Rect, scale: f32, w: u32, h: u32) -> [u32; 4] {
    let x = ((crop.left() * scale).max(0.0) as u32).min(w - 2);
    let y = ((crop.top() * scale).max(0.0) as u32).min(h - 2);
    let cw = ((crop.width() * scale) as u32).min(w - x) & !1;
    let ch = ((crop.height() * scale) as u32).min(h - y) & !1;
    [x, y, cw, ch]
}

fn media_dir() -> PathBuf {
    // A relative folder is read from the repository's root, where the
    // command is typed (tests run in the crate's folder).
    let named = PathBuf::from(std::env::var("DOCS_MEDIA").expect("DOCS_MEDIA names a folder"));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(named);
    std::fs::create_dir_all(&dir).expect("make the media folder");
    dir
}

fn ffmpeg(args: &[&str], out: &Path, feed: impl FnOnce(&mut std::process::ChildStdin)) {
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(out)
        .stdin(Stdio::piped())
        .spawn()
        .expect("ffmpeg on the PATH");
    feed(child.stdin.as_mut().expect("ffmpeg's input"));
    drop(child.stdin.take());
    assert!(
        child.wait().expect("ffmpeg ran").success(),
        "ffmpeg wrote {}",
        out.display()
    );
}

/// The arrow pointer, drawn at `at` (points) with its tip there. `press`
/// from 0 to 1 shrinks it slightly while the button is down.
fn draw_pointer(image: &mut image::RgbaImage, at: Pos2, scale: f32, press: f32) {
    // The classic arrow, 1 unit = 1 point, tip at the origin.
    const ARROW: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.0, 17.0),
        (4.2, 13.2),
        (7.0, 19.5),
        (9.6, 18.4),
        (6.9, 12.2),
        (12.2, 12.2),
    ];
    let size = scale
        * (1.0 - 0.08 * (1.0 - (press * 2.0 - 1.0).abs()).min(1.0) * (press > 0.0) as u8 as f32);
    let points: Vec<(f32, f32)> = ARROW
        .iter()
        .map(|(x, y)| (at.x * scale + x * size, at.y * scale + y * size))
        .collect();
    let outline = 1.6 * scale;
    let (x0, y0) = ((at.x * scale - 4.0) as i32, (at.y * scale - 4.0) as i32);
    let (x1, y1) = (x0 + (16.0 * size) as i32 + 8, y0 + (24.0 * size) as i32 + 8);
    for y in y0.max(0)..y1.min(image.height() as i32) {
        for x in x0.max(0)..x1.min(image.width() as i32) {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let inside = contains(&points, p);
            let edge = distance_to_outline(&points, p);
            let colour = if inside && edge > outline * 0.5 {
                Some([255, 255, 255])
            } else if inside || edge <= outline * 0.5 {
                Some([16, 18, 22])
            } else {
                None
            };
            if let Some(c) = colour {
                let px = image.get_pixel_mut(x as u32, y as u32);
                px.0 = [c[0], c[1], c[2], 255];
            }
        }
    }
}

/// A click's ring around `at`, growing and fading as `t` goes from 0 to 1.
fn draw_ring(image: &mut image::RgbaImage, at: Pos2, scale: f32, t: f32) {
    let radius = (6.0 + 16.0 * t) * scale;
    let width = 2.0 * scale;
    let alpha = (1.0 - t) * 0.85;
    let (cx, cy) = (at.x * scale, at.y * scale);
    let reach = (radius + width) as i32 + 1;
    for y in (cy as i32 - reach).max(0)..(cy as i32 + reach).min(image.height() as i32) {
        for x in (cx as i32 - reach).max(0)..(cx as i32 + reach).min(image.width() as i32) {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (1.0 - ((d - radius).abs() - width * 0.5).max(0.0)).clamp(0.0, 1.0) * alpha;
            if cover > 0.0 {
                let px = image.get_pixel_mut(x as u32, y as u32);
                // The accent, #4FA3FF.
                for (channel, accent) in px.0.iter_mut().zip([79.0, 163.0, 255.0]) {
                    *channel = (*channel as f32 * (1.0 - cover) + accent * cover) as u8;
                }
            }
        }
    }
}

fn contains(points: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn distance_to_outline(points: &[(f32, f32)], p: (f32, f32)) -> f32 {
    let mut best = f32::MAX;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
        best = best.min((x * x + y * y).sqrt());
    }
    best
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

const RATE: u32 = 48_000;
/// Four bars at 120 BPM.
const SECONDS: f32 = 8.0;

/// A drum loop: a kick on every beat, a snare on two and four, and hats on
/// the eighths, so the waveform reads as rhythm.
fn drums(path: &Path) -> PathBuf {
    let beat = RATE as usize / 2;
    let mut noise = 0x2545_f491_u32;
    let samples = (0..(SECONDS * RATE as f32) as usize).map(|i| {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let white = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let in_beat = (i % beat) as f32 / RATE as f32;
        let in_eighth = (i % (beat / 2)) as f32 / RATE as f32;
        let beat_no = i / beat;
        let kick = (std::f32::consts::TAU * (48.0 + 90.0 * (-in_beat * 30.0).exp()) * in_beat)
            .sin()
            * (-in_beat * 9.0).exp();
        let snare = if beat_no % 2 == 1 {
            white * (-in_beat * 18.0).exp() * 0.7
        } else {
            0.0
        };
        let hat = white * (-in_eighth * 60.0).exp() * 0.25;
        (kick * 0.8 + snare + hat).clamp(-1.0, 1.0)
    });
    write_wav(path, samples)
}

/// A bass line: one note a bar, plucked and fading.
fn bass(path: &Path) -> PathBuf {
    let bar = RATE as usize * 2;
    let notes = [55.0_f32, 55.0, 65.4, 49.0];
    let samples = (0..(SECONDS * RATE as f32) as usize).map(|i| {
        let t = (i % bar) as f32 / RATE as f32;
        let f = notes[(i / bar) % notes.len()];
        let tone = (std::f32::consts::TAU * f * t).sin()
            + 0.3 * (std::f32::consts::TAU * 2.0 * f * t).sin();
        tone * (-t * 1.4).exp() * 0.55 * (t * 200.0).min(1.0)
    });
    write_wav(path, samples)
}

/// A stereo 16-bit WAV of `samples`.
fn write_wav(path: &Path, samples: impl Iterator<Item = f32>) -> PathBuf {
    let data: Vec<u8> = samples
        .flat_map(|s| {
            let [a, b] = ((s * 30_000.0) as i16).to_le_bytes();
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
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the WAV");
    path.to_path_buf()
}
