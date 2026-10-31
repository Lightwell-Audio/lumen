//! The first-run window between frames, and the rules it shows (M9 slice
//! R7b, ADR-0115): which step is open, which steps were seen, what each
//! step's pill says, whether an ASIO driver is offered, and how many
//! plugins are ready. Drawn by `app/welcome.rs`.

use eframe::egui;
use lumen_plugin_catalog::catalog::{Catalog, Source};

/// The window's steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Sound,
    Plugins,
    Ready,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Sound, Step::Plugins, Step::Ready];

    pub fn name(self) -> &'static str {
        match self {
            Step::Sound => "Sound",
            Step::Plugins => "Plugins",
            Step::Ready => "Ready",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn next(self) -> Option<Step> {
        Self::ALL.get(self.index() + 1).copied()
    }

    pub fn back(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| Self::ALL[i])
    }
}

/// What a step's pill shows besides its name: ✓ only when the step's state
/// is good, a warning mark when it needs something, and nothing for a step
/// not yet seen or still under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    None,
    Good,
    Needs,
}

/// How a step stands, before whether it was seen is taken into account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    /// Still under way: a device starting, a scan running.
    Busy,
    Needs,
}

/// The mark for a step in `health`, `seen` or not.
pub fn mark(health: Health, seen: bool) -> Mark {
    match (seen, health) {
        (false, _) | (true, Health::Busy) => Mark::None,
        (true, Health::Good) => Mark::Good,
        (true, Health::Needs) => Mark::Needs,
    }
}

/// What the Sound step offers when an ASIO driver is installed and another
/// driver model is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsioOffer {
    /// One driver: "Use ASIO" uses it.
    One(String),
    /// Several: "Use ASIO" is a menu of their names.
    Several(Vec<String>),
}

/// The offer for the ASIO `drivers` installed, while `driver` is in use.
pub fn asio_offer(drivers: &[String], driver: &str) -> Option<AsioOffer> {
    if driver == "asio" {
        return None;
    }
    match drivers {
        [] => None,
        [one] => Some(AsioOffer::One(one.clone())),
        several => Some(AsioOffer::Several(several.to_vec())),
    }
}

/// Whether the window opens at start: when `asked` (`--first-run`), or on a
/// first start (setup not yet `done`) that is not `automated` (tests,
/// screenshots, benchmarks, the smoke test).
pub fn opens_at_start(asked: bool, done: bool, automated: bool) -> bool {
    asked || (!done && !automated)
}

/// The plugins ready to use: every plugin class the catalog lists, without
/// the built-in devices (a module with several classes counts each; a
/// blocked module counts none).
pub fn plugins_ready(catalog: &Catalog) -> usize {
    catalog
        .devices()
        .iter()
        .filter(|d| d.source == Source::Vst3)
        .count()
}

/// "1 plugin", "23 plugins".
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Lumi's poses, as textures, made the first time the window draws.
pub struct Lumi {
    pub waving: egui::TextureHandle,
    pub celebrating: egui::TextureHandle,
    pub curious: egui::TextureHandle,
}

const WAVING: &[u8] = include_bytes!("../assets/brand/lumi-waving.png");
const CELEBRATING: &[u8] = include_bytes!("../assets/brand/lumi-celebrating.png");
const CURIOUS: &[u8] = include_bytes!("../assets/brand/lumi-curious.png");

impl Lumi {
    /// Decodes the poses. `None` if one would not decode (it is built in,
    /// so a test fails first).
    fn load(ctx: &egui::Context) -> Option<Self> {
        let texture = |name: &str, bytes: &[u8]| {
            let image = crate::chain::pictures::decode(bytes)?;
            Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        };
        Some(Self {
            waving: texture("lumi waving", WAVING)?,
            celebrating: texture("lumi celebrating", CELEBRATING)?,
            curious: texture("lumi curious", CURIOUS)?,
        })
    }
}

/// The window between frames (not saved: closing it in any way sets
/// `Settings::setup_done`).
pub struct Welcome {
    pub open: bool,
    pub step: Step,
    /// The steps opened since the window opened.
    seen: [bool; 3],
    /// Next takes the keyboard when a step opens.
    pub focus_next: bool,
    /// At the end of the last frame something in or over the window would
    /// take Esc first (an open menu, a field being typed in, another
    /// window): then Esc does not skip setup.
    pub esc_for_child: bool,
    lumi: Option<Option<Lumi>>,
}

impl Default for Welcome {
    fn default() -> Self {
        Self {
            open: false,
            step: Step::Sound,
            seen: [false; 3],
            focus_next: false,
            esc_for_child: false,
            lumi: None,
        }
    }
}

impl Welcome {
    /// Opens on Sound, as on a first start.
    pub fn show(&mut self) {
        self.open = true;
        self.seen = [false; 3];
        self.go(Step::Sound);
    }

    /// Opens `step`.
    pub fn go(&mut self, step: Step) {
        self.step = step;
        self.seen[step.index()] = true;
        self.focus_next = true;
    }

    pub fn seen(&self, step: Step) -> bool {
        self.seen[step.index()]
    }

    /// Lumi's poses, decoded the first time they are asked for.
    pub fn lumi(&mut self, ctx: &egui::Context) -> Option<&Lumi> {
        self.lumi.get_or_insert_with(|| Lumi::load(ctx)).as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pill_is_marked_only_once_seen_and_settled() {
        assert_eq!(mark(Health::Good, false), Mark::None);
        assert_eq!(mark(Health::Needs, false), Mark::None);
        assert_eq!(mark(Health::Busy, true), Mark::None);
        assert_eq!(mark(Health::Good, true), Mark::Good);
        assert_eq!(mark(Health::Needs, true), Mark::Needs);
    }

    #[test]
    fn asio_is_offered_when_installed_and_not_in_use() {
        let one = ["Focusrite USB ASIO".to_owned()];
        assert_eq!(asio_offer(&[], "wasapi"), None);
        assert_eq!(
            asio_offer(&one, "wasapi"),
            Some(AsioOffer::One("Focusrite USB ASIO".into()))
        );
        assert_eq!(asio_offer(&one, "asio"), None, "already in use");
        let two = ["A ASIO".to_owned(), "B ASIO".to_owned()];
        assert_eq!(
            asio_offer(&two, "exclusive"),
            Some(AsioOffer::Several(two.to_vec()))
        );
    }

    #[test]
    fn steps_go_forward_and_back() {
        assert_eq!(Step::Sound.next(), Some(Step::Plugins));
        assert_eq!(Step::Ready.next(), None);
        assert_eq!(Step::Sound.back(), None);
        assert_eq!(Step::Ready.back(), Some(Step::Plugins));
        let mut w = Welcome::default();
        w.show();
        assert!(w.seen(Step::Sound) && !w.seen(Step::Plugins));
        w.go(Step::Ready);
        assert!(w.seen(Step::Ready) && !w.seen(Step::Plugins));
    }

    #[test]
    fn it_opens_on_a_first_start_or_when_asked_and_never_by_itself_when_automated() {
        assert!(opens_at_start(false, false, false), "a first start");
        assert!(!opens_at_start(false, true, false), "setup was done");
        assert!(
            !opens_at_start(false, false, true),
            "a test or a screenshot"
        );
        assert!(opens_at_start(true, true, true), "--first-run");
    }

    #[test]
    fn counts_read_as_words() {
        assert_eq!(count(1, "plugin", "plugins"), "1 plugin");
        assert_eq!(count(0, "plugin", "plugins"), "0 plugins");
        assert_eq!(count(23, "plugin", "plugins"), "23 plugins");
    }

    #[test]
    fn lumi_decodes() {
        for bytes in [WAVING, CELEBRATING, CURIOUS] {
            let image = crate::chain::pictures::decode(bytes).expect("decodes");
            assert_eq!(image.size, [256, 256]);
        }
    }
}
