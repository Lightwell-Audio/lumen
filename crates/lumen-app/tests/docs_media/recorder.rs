use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hasher};
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use eframe::egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, Rect, ViewportId};
use egui_kittest::Harness;
use lumen_app::App;
use lumen_app::testing::InputDeviceTurn;
use lumen_audio::fake::Pace;

use crate::pointer;

pub const SCALE: f32 = 2.0;
const STEPS_PER_FRAME: usize = 2;
const RATE: usize = 48_000;
const BLOCK: u64 = 128;
const RECORDER_VERSION: u64 = 1;

thread_local! {
    static PHASES: RefCell<BTreeMap<&'static str, (Duration, u64)>> = RefCell::default();
}

pub fn timed<T>(phase: &'static str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    PHASES.with(|p| {
        let mut p = p.borrow_mut();
        let phase = p.entry(phase).or_default();
        phase.0 += started.elapsed();
        phase.1 += 1;
    });
    out
}

pub fn phases() -> String {
    PHASES.with(|p| {
        let mut all: Vec<_> = std::mem::take(&mut *p.borrow_mut()).into_iter().collect();
        all.sort_by_key(|(_, (d, _))| std::cmp::Reverse(*d));
        all.iter()
            .map(|(n, (d, c))| format!("{n} {:.1}s/{c}", d.as_secs_f32()))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

pub mod pace {
    pub const FPS: usize = 30;
    pub const SETTLE: usize = 12;
    pub const RESULT: usize = 36;
    pub const SPEED: f32 = 520.0;
    pub const GLIDE: (usize, usize) = (10, 28);
    pub const RING: usize = 12;
    pub const KEYS: usize = 46;
    pub const KEYS_LEAD: usize = 10;
    pub const READ: usize = 45;
}

fn at_rest(h: &Harness<'static, App>) -> bool {
    h.output()
        .viewport_output
        .get(&ViewportId::ROOT)
        .is_some_and(|v| !v.repaint_delay.is_zero())
}

fn verifying() -> bool {
    std::env::var_os("DOCS_VERIFY_REUSE").is_some()
}

pub fn rendering() -> bool {
    std::env::var_os("DOCS_MEDIA").is_some()
}

pub fn themes() -> &'static [&'static str] {
    if !rendering() {
        return &["dark"];
    }
    match std::env::var("DOCS_THEME").as_deref() {
        Ok("dark") => &["dark"],
        Ok("light") => &["light"],
        Ok(other) => panic!("DOCS_THEME is dark or light, not {other:?}"),
        Err(_) => crate::scenes::THEMES,
    }
}

thread_local! {
    static WRITTEN: RefCell<BTreeSet<String>> = RefCell::default();
    static MARKED: RefCell<BTreeMap<String, Vec<String>>> = RefCell::default();
}

pub fn written() -> BTreeSet<String> {
    WRITTEN.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub fn marked() -> BTreeMap<String, Vec<String>> {
    MARKED.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

fn wrote(path: &Path) {
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    would_write(name);
}

fn would_write(name: String) {
    WRITTEN.with(|w| w.borrow_mut().insert(name));
}

pub fn still(
    h: &mut Harness<'static, App>,
    crop: Rect,
    id: &str,
    theme: &str,
    pointer: Option<Pos2>,
    callouts: &[(&str, Rect)],
) {
    h.run_steps(6);
    MARKED.with(|m| {
        let names = callouts.iter().map(|(name, _)| (*name).to_owned());
        m.borrow_mut().insert(id.to_owned(), names.collect())
    });
    if !rendering() {
        would_write(format!("{id}-{theme}.webp"));
        if !callouts.is_empty() {
            would_write(format!("{id}.callouts.json"));
        }
        return;
    }
    let mut image = h.render().expect("render");
    if let Some(at) = pointer {
        let icon = h.output().platform_output.cursor_icon;
        pointer::draw(&mut image, icon, at, SCALE, 0.0);
    }
    let image = cut(&image, crop);
    let out = media_dir().join(format!("{id}-{theme}.webp"));
    ffmpeg(
        &raw_input(image.width(), image.height(), None)
            .into_iter()
            .chain(["-frames:v", "1", "-c:v", "libwebp", "-lossless", "1"].map(String::from))
            .collect::<Vec<_>>(),
        &out,
        |stdin| stdin.write_all(image.as_raw()).expect("write the frame"),
    );
    if !callouts.is_empty() {
        let at: Vec<String> = callouts
            .iter()
            .map(|(name, r)| {
                let f = |v: f32, from: f32, span: f32| (v - from) / span;
                format!(
                    "{name:?}:[{:.4},{:.4},{:.4},{:.4}]",
                    f(r.left(), crop.left(), crop.width()),
                    f(r.top(), crop.top(), crop.height()),
                    r.width() / crop.width(),
                    r.height() / crop.height(),
                )
            })
            .collect();
        let path = media_dir().join(format!("{id}.callouts.json"));
        std::fs::write(&path, format!("{{\"at\":{{{}}}}}\n", at.join(",")))
            .expect("write callouts");
        wrote(&path);
    }
}

pub struct Take {
    name: String,
    crop: Rect,
    focus: f32,
    at: Pos2,
    icon: CursorIcon,
    frames: usize,
    since_click: Option<usize>,
    keycaps: Vec<(String, usize, usize)>,
    raw_path: PathBuf,
    raw: Option<BufWriter<File>>,
    hash: DefaultHasher,
    size: (u32, u32),
    last: Vec<u8>,
    clock: Clock,
    sound: Option<Sound>,
    base: Option<image::RgbaImage>,
    idle: bool,
}

struct Clock {
    device: InputDeviceTurn,
    was: (Option<Pace>, u32),
    granted: u64,
    heard: u64,
}

impl Clock {
    fn start(h: &mut Harness<'static, App>, device: InputDeviceTurn) -> Self {
        let mut was = (None, 1);
        device.set(|d| {
            was = (d.pace, d.deliver_every);
            d.pace = Some(Pace::Every(Duration::from_micros(1)));
            d.allowance = Some(0);
            d.deliver_every = 1;
        });
        h.run_steps(2);
        let started = Instant::now();
        let mut quiet_since = Instant::now();
        while quiet_since.elapsed() < Duration::from_millis(50) {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the test device kept playing after it was held"
            );
            std::thread::sleep(Duration::from_millis(5));
            if !h.state_mut().heard().is_empty() {
                quiet_since = Instant::now();
            }
        }
        Self {
            device,
            was,
            granted: 0,
            heard: 0,
        }
    }

    fn tick(&mut self, h: &mut Harness<'static, App>, frame: usize) -> Vec<(f32, f32)> {
        let want = ((frame as u64 + 1) * (RATE / pace::FPS) as u64).div_ceil(BLOCK);
        let more = want - self.granted;
        self.device
            .set(|d| *d.allowance.as_mut().expect("a clocked device") += more);
        self.granted = want;
        let started = Instant::now();
        let mut played = Vec::new();
        while self.heard < self.granted * BLOCK {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the test device stopped playing"
            );
            std::thread::sleep(Duration::from_millis(1));
            let heard = h.state_mut().heard();
            self.heard += heard.len() as u64;
            played.extend(heard);
        }
        assert_eq!(
            self.heard,
            self.granted * BLOCK,
            "the test device plays blocks of {BLOCK} frames"
        );
        played
    }

    fn stop(&self) {
        let (pace, deliver_every) = self.was;
        self.device.set(|d| {
            d.pace = pace;
            d.deliver_every = deliver_every;
            d.allowance = None;
        });
    }
}

struct Sound {
    track: Vec<(f32, f32)>,
    captions: Vec<(usize, Option<String>, String)>,
    chapters: Vec<(usize, Option<String>, String)>,
    playhead: Vec<Option<f64>>,
}

impl Take {
    pub fn clip(
        h: &mut Harness<'static, App>,
        device: InputDeviceTurn,
        id: &str,
        theme: &str,
        crop: Rect,
        focus: f32,
        at: Pos2,
    ) -> Self {
        let clock = Clock::start(h, device);
        Self::new(format!("{id}-{theme}"), crop, focus, at, clock, None)
    }

    pub fn walk(
        h: &mut Harness<'static, App>,
        device: InputDeviceTurn,
        id: &str,
        crop: Rect,
        at: Pos2,
    ) -> Self {
        let clock = Clock::start(h, device);
        let sound = Sound {
            track: Vec::new(),
            captions: Vec::new(),
            chapters: Vec::new(),
            playhead: Vec::new(),
        };
        Self::new(id.to_owned(), crop, 0.5, at, clock, Some(sound))
    }

    fn new(
        name: String,
        crop: Rect,
        focus: f32,
        at: Pos2,
        clock: Clock,
        sound: Option<Sound>,
    ) -> Self {
        let raw_path = std::env::temp_dir().join(format!("lumen-docs-{name}.rgba"));
        let raw = rendering()
            .then(|| BufWriter::new(File::create(&raw_path).expect("make the frames file")));
        let mut hash = DefaultHasher::new();
        hash.write_u64(RECORDER_VERSION);
        Self {
            name,
            crop,
            focus,
            at,
            icon: CursorIcon::Default,
            frames: 0,
            since_click: None,
            keycaps: Vec::new(),
            raw_path,
            raw,
            hash,
            size: (0, 0),
            last: Vec::new(),
            clock,
            sound,
            base: None,
            idle: false,
        }
    }

    fn frame(&mut self, h: &mut Harness<'static, App>, quiet: bool) {
        let played = timed("device", || self.clock.tick(h, self.frames));
        if let Some(sound) = &mut self.sound {
            sound.track.extend(played);
        }
        timed("steps", || h.run_steps(STEPS_PER_FRAME));
        self.icon = h.output().platform_output.cursor_icon;
        if self.raw.is_some() {
            let image = self.picture(h, quiet);
            self.size = (image.width(), image.height());
            timed("hash", || self.hash.write(image.as_raw()));
            if let Some(raw) = &mut self.raw {
                timed("write", || {
                    raw.write_all(image.as_raw()).expect("write a frame")
                });
            }
            timed("keep", || self.last = image.into_raw());
        }
        self.idle = at_rest(h);
        self.since_click = self.since_click.map(|n| n + 1);
        if let Some(sound) = &mut self.sound {
            let app = h.state();
            sound
                .playhead
                .push(app.is_playing().then(|| app.playhead().as_quarters()));
        }
        self.frames += 1;
    }

    fn picture(&mut self, h: &mut Harness<'static, App>, quiet: bool) -> image::RgbaImage {
        let rest = quiet && self.idle && at_rest(h) && self.base.is_some();
        let mut image = match (&self.base, rest && !verifying()) {
            (Some(base), true) => {
                timed("reuse", || ());
                base.clone()
            }
            _ => {
                let drawn = timed("render", || h.render().expect("render"));
                if rest {
                    let same = self.base.as_ref() == Some(&drawn);
                    timed(if same { "verified" } else { "MISMATCH" }, || ());
                }
                self.base = Some(drawn.clone());
                drawn
            }
        };
        let ring = self
            .since_click
            .map(|n| n as f32 / pace::RING as f32)
            .filter(|t| *t <= 1.0);
        timed("pointer", || {
            pointer::draw(&mut image, self.icon, self.at, SCALE, ring.unwrap_or(0.0));
            if let Some(t) = ring {
                pointer::ring(&mut image, self.at, SCALE, t);
            }
        });
        timed("cut", || cut(&image, self.crop))
    }

    pub fn hold(&mut self, h: &mut Harness<'static, App>, frames: usize) {
        for i in 0..frames {
            if i == 0 {
                h.hover_at(self.at);
            }
            self.frame(h, i > 0);
        }
    }

    pub fn settle(&mut self, h: &mut Harness<'static, App>) {
        self.hold(h, pace::SETTLE);
    }

    pub fn result(&mut self, h: &mut Harness<'static, App>) {
        self.hold(h, pace::RESULT);
    }

    pub fn move_to(&mut self, h: &mut Harness<'static, App>, to: Pos2) {
        let from = self.at;
        let seconds = from.distance(to) / pace::SPEED;
        let frames = ((seconds * pace::FPS as f32) as usize).clamp(pace::GLIDE.0, pace::GLIDE.1);
        for i in 1..=frames {
            let t = i as f32 / frames as f32;
            self.at = from + (to - from) * (t * t * (3.0 - 2.0 * t));
            h.hover_at(self.at);
            self.frame(h, false);
        }
    }

    fn button(&mut self, h: &mut Harness<'static, App>, pressed: bool, modifiers: Modifiers) {
        h.event(Event::PointerButton {
            pos: self.at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
        self.frame(h, false);
    }

    pub fn press(&mut self, h: &mut Harness<'static, App>) {
        self.since_click = Some(0);
        self.button(h, true, Modifiers::NONE);
    }

    pub fn release(&mut self, h: &mut Harness<'static, App>) {
        self.button(h, false, Modifiers::NONE);
    }

    pub fn click(&mut self, h: &mut Harness<'static, App>) {
        self.press(h);
        self.release(h);
    }

    pub fn keys(
        &mut self,
        h: &mut Harness<'static, App>,
        label: &str,
        press: impl FnOnce(&mut Harness<'static, App>),
    ) {
        let from = self.frames;
        self.hold(h, pace::KEYS_LEAD);
        press(h);
        self.frame(h, false);
        self.keycaps.push((label.into(), from, from + pace::KEYS));
    }

    pub fn caption(&mut self, text: &str) {
        let at = self.frames;
        self.sound_mut().captions.push((at, None, text.into()));
    }

    pub fn chapter(&mut self, id: &str, title: &str) {
        let at = self.frames;
        self.sound_mut()
            .chapters
            .push((at, Some(id.into()), title.into()));
    }

    fn sound_mut(&mut self) -> &mut Sound {
        self.sound
            .as_mut()
            .expect("captions and chapters are for walkthroughs")
    }

    pub fn finish(mut self) -> InputDeviceTurn {
        self.encode();
        self.clock.device
    }

    fn encode(&mut self) {
        self.clock.stop();
        if let Some(sound) = &self.sound {
            sound.check_sync();
            sound.check_captions(self.frames);
        }
        let Some(mut raw) = self.raw.take() else {
            would_write(format!("{}.mp4", self.name));
            would_write(format!("{}.webp", self.name));
            if self.sound.is_some() {
                would_write(format!("{}.captions.vtt", self.name));
                would_write(format!("{}.chapters.vtt", self.name));
            }
            return;
        };
        raw.flush().expect("write the frames");
        drop(raw);
        let (w, h) = self.size;
        let filters = self.keycap_filters();
        self.hash.write(filters.as_bytes());
        for setting in VIDEO.iter().chain(&AUDIO).chain(&POSTER) {
            self.hash.write(setting.as_bytes());
        }
        self.hash.write(ffmpeg_version().as_bytes());
        let dir = media_dir();
        let outputs = [format!("{}.mp4", self.name), format!("{}.webp", self.name)];
        let mut pcm = None;
        if let Some(sound) = &self.sound {
            let bytes: Vec<u8> = sound
                .track
                .iter()
                .flat_map(|(l, r)| l.to_le_bytes().into_iter().chain(r.to_le_bytes()))
                .collect();
            self.hash.write(&bytes);
            let path = self.raw_path.with_extension("pcm");
            std::fs::write(&path, bytes).expect("write the sound");
            pcm = Some(path);
            for (kind, cues) in [("captions", &sound.captions), ("chapters", &sound.chapters)] {
                let path = dir.join(format!("{}.{kind}.vtt", self.name));
                std::fs::write(&path, vtt(cues, self.frames)).expect("write the cues");
                wrote(&path);
            }
        }
        let cache = cache_dir().join(format!("{:016x}", self.hash.finish()));
        if outputs.iter().all(|o| cache.join(o).is_file()) {
            eprintln!("  {}: unchanged, from the cache", self.name);
            for o in &outputs {
                std::fs::copy(cache.join(o), dir.join(o)).expect("copy from the cache");
                wrote(&dir.join(o));
            }
        } else {
            let mut args = raw_input(w, h, Some(pace::FPS));
            if let Some(pcm) = &pcm {
                let pcm = pcm.display().to_string();
                args.extend(
                    ["-f", "f32le", "-ar", "48000", "-ac", "2", "-i", &pcm].map(String::from),
                );
            }
            if !filters.is_empty() {
                args.extend(["-vf".into(), filters.clone()]);
            }
            args.extend(VIDEO.map(String::from));
            if pcm.is_some() {
                args.extend(AUDIO.map(String::from));
            } else {
                args.push("-an".into());
            }
            args.extend(["-movflags", "+faststart"].map(String::from));
            let raw_path = self.raw_path.clone();
            ffmpeg(&args, &dir.join(&outputs[0]), |stdin| {
                let mut frames = File::open(&raw_path).expect("read the frames");
                std::io::copy(&mut frames, stdin).expect("write the frames");
            });
            let mut args = raw_input(w, h, None);
            args.extend(POSTER.map(String::from));
            ffmpeg(&args, &dir.join(&outputs[1]), |stdin| {
                stdin.write_all(&self.last).expect("write the poster");
            });
            std::fs::create_dir_all(&cache).expect("make the cache");
            for o in &outputs {
                std::fs::copy(dir.join(o), cache.join(o)).expect("fill the cache");
            }
        }
        std::fs::write(cache.join("used"), "").expect("mark the cache entry");
        let _ = std::fs::remove_file(&self.raw_path);
        if let Some(pcm) = pcm {
            let _ = std::fs::remove_file(pcm);
        }
    }

    fn keycap_filters(&self) -> String {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let shown = w.min(h * 4.0 / 3.0);
        let right = (w - shown) * self.focus + shown;
        let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/MartianMono.ttf");
        let size = 15.0 * SCALE;
        let pad = 9.0 * SCALE;
        let rim = 1.5 * SCALE;
        let cap = size + 2.0 * pad;
        let top = h - 22.0 * SCALE - cap;
        let gap = 8.0 * SCALE;
        let mut filters = Vec::new();
        for (label, from, to) in &self.keycaps {
            let shows = format!(
                "enable='between(t,{:.2},{:.2})'",
                *from as f32 / pace::FPS as f32,
                *to as f32 / pace::FPS as f32
            );
            let keys: Vec<&str> = label.split('+').collect();
            let widths: Vec<f32> = keys
                .iter()
                .map(|k| (k.chars().count() as f32 * 0.73 * size).max(size) + 2.0 * pad)
                .collect();
            let total = widths.iter().sum::<f32>() + gap * (keys.len() - 1) as f32;
            let mut left = right - total - 24.0 * SCALE;
            for (key, width) in keys.iter().zip(&widths) {
                let [x, y, bw, bh] = [left, top, *width, cap].map(|v| v.round() as i32);
                let r = rim.round() as i32;
                filters.push(format!(
                    "drawbox=x={x}:y={y}:w={bw}:h={bh}:color=0x4A5060:t=fill:{shows}"
                ));
                filters.push(format!(
                    "drawbox=x={}:y={}:w={}:h={}:color=0x22262D:t=fill:{shows}",
                    x + r,
                    y + r,
                    bw - 2 * r,
                    bh - 3 * r
                ));
                filters.push(format!(
                    "drawtext=fontfile='{}':text='{key}':fontsize={}:fontcolor=0xEEF0F3:\
                     x={x}+({bw}-tw)/2:y={y}+({}-th)/2:{shows}",
                    font.display(),
                    size as u32,
                    bh - 2 * r,
                ));
                left += width + gap;
            }
        }
        filters.join(",")
    }
}

impl Sound {
    fn check_captions(&self, end: usize) {
        for (i, (from, _, text)) in self.captions.iter().enumerate() {
            let to = self.captions.get(i + 1).map_or(end, |c| c.0);
            assert!(
                to - from >= pace::READ,
                "\"{text}\" shows for {:.2} s, too short to read",
                (to - from) as f64 / pace::FPS as f64
            );
        }
    }

    fn check_sync(&self) {
        let frame = 1.0 / pace::FPS as f64;
        let Some(first) = self.playhead.iter().position(Option::is_some) else {
            return;
        };
        let onset = self
            .track
            .iter()
            .position(|(l, r)| l.abs().max(r.abs()) > 0.02)
            .expect("the walkthrough is heard");
        let heard_at = onset as f64 / RATE as f64;
        let seen_at = first as f64 * frame;
        assert!(
            (heard_at - seen_at).abs() <= 2.0 * frame,
            "the sound starts at {heard_at:.3} s, the playhead at {seen_at:.3} s"
        );
        let playing: Vec<(usize, f64)> = self
            .playhead
            .iter()
            .enumerate()
            .skip(first)
            .map_while(|(i, q)| q.map(|q| (i, q)))
            .collect();
        let ((i0, q0), (i1, q1)) = (playing[0], playing[playing.len() - 1]);
        let moved = (q1 - q0) * 0.5;
        let took = (i1 - i0) as f64 * frame;
        assert!(
            (moved - took).abs() <= 2.0 * frame,
            "the playhead moved {moved:.3} s in {took:.3} s of video"
        );
    }
}

fn vtt(cues: &[(usize, Option<String>, String)], end: usize) -> String {
    let stamp = |frame: usize| {
        let ms = frame * 1000 / pace::FPS;
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        )
    };
    let mut out = String::from("WEBVTT\n");
    for (i, (from, id, text)) in cues.iter().enumerate() {
        let to = cues.get(i + 1).map_or(end, |c| c.0);
        out += "\n";
        if let Some(id) = id {
            out += &format!("{id}\n");
        }
        out += &format!("{} --> {}\n{text}\n", stamp(*from), stamp(to));
    }
    out
}

fn cut(image: &image::RgbaImage, crop: Rect) -> image::RgbaImage {
    let (w, h) = (image.width(), image.height());
    let x = ((crop.left() * SCALE).max(0.0) as u32).min(w - 2);
    let y = ((crop.top() * SCALE).max(0.0) as u32).min(h - 2);
    let cw = ((crop.width() * SCALE) as u32).min(w - x) & !1;
    let ch = ((crop.height() * SCALE) as u32).min(h - y) & !1;
    image::imageops::crop_imm(image, x, y, cw, ch).to_image()
}

fn raw_input(w: u32, h: u32, rate: Option<usize>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{w}x{h}"),
    ]
    .map(String::from)
    .to_vec();
    if let Some(rate) = rate {
        args.extend(["-r".into(), rate.to_string()]);
    }
    args.extend(["-i", "-"].map(String::from));
    args
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn media_dir() -> PathBuf {
    let named = PathBuf::from(std::env::var("DOCS_MEDIA").expect("DOCS_MEDIA names a folder"));
    let dir = repo().join(named);
    std::fs::create_dir_all(&dir).expect("make the media folder");
    dir
}

fn cache_dir() -> PathBuf {
    std::env::var_os("DOCS_MEDIA_CACHE")
        .map_or_else(|| repo().join("target/docs-media-cache"), PathBuf::from)
}

const VIDEO: [&str; 8] = [
    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "slow", "-crf", "24",
];
const AUDIO: [&str; 5] = ["-c:a", "aac", "-b:a", "128k", "-shortest"];
const POSTER: [&str; 6] = ["-frames:v", "1", "-c:v", "libwebp", "-quality", "85"];

fn ffmpeg_version() -> String {
    let out = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .expect("ffmpeg on the PATH");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn ffmpeg(args: &[String], out: &Path, feed: impl FnOnce(&mut std::process::ChildStdin)) {
    timed("ffmpeg", || ffmpeg_run(args, out, feed));
}

fn ffmpeg_run(args: &[String], out: &Path, feed: impl FnOnce(&mut std::process::ChildStdin)) {
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
    wrote(out);
}
