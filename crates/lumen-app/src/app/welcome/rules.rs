use eframe::egui;
use lumen_plugin_catalog::catalog::{Catalog, Source};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    None,
    Good,
    Needs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    Busy,
    Needs,
}

pub fn mark(health: Health, seen: bool) -> Mark {
    match (seen, health) {
        (false, _) | (true, Health::Busy) => Mark::None,
        (true, Health::Good) => Mark::Good,
        (true, Health::Needs) => Mark::Needs,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsioOffer {
    One(String),
    Several(Vec<String>),
}

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

pub fn opens_at_start(asked: bool, done: bool, automated: bool) -> bool {
    asked || (!done && !automated)
}

pub fn plugins_ready(catalog: &Catalog) -> usize {
    catalog
        .devices()
        .iter()
        .filter(|d| d.source == Source::Vst3)
        .count()
}

pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

pub struct Lumi {
    pub waving: egui::TextureHandle,
    pub celebrating: egui::TextureHandle,
    pub curious: egui::TextureHandle,
}

const WAVING: &[u8] = include_bytes!("../../../assets/brand/lumi-waving.png");
const CELEBRATING: &[u8] = include_bytes!("../../../assets/brand/lumi-celebrating.png");
const CURIOUS: &[u8] = include_bytes!("../../../assets/brand/lumi-curious.png");

impl Lumi {
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

pub struct Welcome {
    pub open: bool,
    pub step: Step,
    seen: [bool; 3],
    pub focus_next: bool,
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
    pub fn show(&mut self) {
        self.open = true;
        self.seen = [false; 3];
        self.go(Step::Sound);
    }

    pub fn go(&mut self, step: Step) {
        self.step = step;
        self.seen[step.index()] = true;
        self.focus_next = true;
    }

    pub fn seen(&self, step: Step) -> bool {
        self.seen[step.index()]
    }

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
