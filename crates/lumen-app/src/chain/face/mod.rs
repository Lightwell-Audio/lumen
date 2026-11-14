//! What a built-in device draws on its card (M8, the owner's round 2,
//! `docs/specimens/m8/devices.html`: built-ins are set pieces, 2026-11-12;
//! DESIGN.md §7k).
//!
//! Every built-in is one **card** drawn by `chain/card/screen/`: a header
//! tinted with the device's hue (the name in the display face, what the device
//! is, a Preset pill), a deep **screen** with the device's picture and its live
//! readouts, and a row of lit dials and pills under idea names. What differs
//! from device to device is declared here, as data, in a [`Drawing`]:
//!
//! * `card` and `screen`: the whole card's size and its screen's height.
//! * `draw`: the picture, drawn into the rectangle it is given. It is called
//!   at every step (below), so it fits the rectangle rather than crops it.
//! * `controls`: the dials and pills, in the order they shrink away.
//! * `handles`: on-screen handles, where the picture is the idea (drag Prism's
//!   beams apart for Width): each moves one parameter, as its dial does.
//! * [`Parts`], for a drawing made of repeated pickable parts (Iris's eight
//!   bands): a point on the screen and a numbered pill for each, with the
//!   card's dials aimed at the picked one.
//!
//! # Steps
//!
//! The card steps down with the Devices tab's height and stays a set piece at
//! every step ([`Step`], `geometry.rs`): **whole** (the specimen's card),
//! **short** (the chain's 232 x 272: a smaller screen, the picture redrawn for
//! it, the dials on the screen's lower edge), **compact** (232 x 116: the
//! header and four lit dials over a thin band of the picture), and a folded
//! rail whose hue bar glows with the device's level. The dials are the same
//! `controls`, laid out by the card for each step: a device never positions
//! anything for the smaller steps.
//!
//! # Alive
//!
//! A [`Scene`] carries the device's `level` (0 to 1, read from the track's
//! meter, which the interface already keeps), the `power` of its screen (it
//! dims to a cool grey on bypass and lights again with a short fade) and
//! whether motion is reduced. A drawing is a pure function of the scene: calm
//! at rest, brighter with level, and still when `still` is set. Nothing here
//! asks for a repaint: the meters and the fades do.
//!
//! Every dial and pill is Lumen's own control (`widgets::param::param_dial`,
//! `behave`): drag, Shift fine, the wheel, double-click reset, type-in, the
//! right-click menu (automation, MIDI learn), keyboard focus, accessible names
//! and the override state. A handle is the same edit as its dial, as one
//! undo step; Shift is fine, double-click is the default, the arrow keys move
//! it, and it is a Tab stop with an accessible name.
//!
//! # Plugging in a device
//!
//! 1. Write `chain/face/<device>.rs` with a `pub(crate) static DRAWING:
//!    Drawing` and the `fn draw(&Scene)` it names. Sizes and colours of the
//!    card are in `theme::personality`; a drawing's own geometry stays in its
//!    file as named constants.
//! 2. Add one line to [`DRAWINGS`], keyed by the device's stable saved name.
//!
//! ```ignore
//! pub(crate) static DRAWING: Drawing = Drawing {
//!     card: vec2(380.0, 330.0),
//!     screen: 176.0,
//!     deep: personality::SCREEN,
//!     reads: &[dynamics::GAIN_REDUCTION],
//!     draw,
//!     controls: &[
//!         Control::knob(dynamics::THRESHOLD, "Pull", |c| dial_at(c, 0.20)),
//!         Control::pill(dynamics::TRUE_PEAK, "True", |c| pill_at(c, 0.88)),
//!     ],
//!     handles: &[Handle { name: "Threshold", param: dynamics::THRESHOLD, .. }],
//! };
//! ```
//!
//! A `param` is the device table's id, so a value read, a drag, an automation
//! lane and a preset are the parameter's own. A readout that needs the audio's
//! live state beyond the level (a gain reduction) needs a channel from the
//! engine and is that device's own slice to add.

use eframe::egui::{Color32, CursorIcon, Painter, Pos2, Rect, Vec2};
use lumen_devices::params::ParamInfo;

use super::geometry::Step;
use super::strip::Slot;

pub(crate) mod afterglow;
pub(crate) mod eq;
pub(crate) mod parallax;
pub(crate) mod prism;

/// Every device with a drawing, by its stable saved name: add a line here.
const DRAWINGS: &[(&str, &Drawing)] = &[
    (lumen_devices::utility::NAME, &prism::DRAWING),
    (lumen_devices::eq::NAME, &eq::DRAWING),
    (lumen_devices::delay::NAME, &parallax::DRAWING),
    (lumen_devices::reverb::NAME, &afterglow::DRAWING),
];

/// The devices whose drawings are made of picked parts (Iris's bands), beside
/// their line in [`DRAWINGS`].
const PARTED: &[(&str, &Parts)] = &[(lumen_devices::eq::NAME, &eq::PARTS)];

/// The drawing of the device saved as `name`, if it has one.
pub(crate) fn drawing_of(name: &str) -> Option<&'static Drawing> {
    DRAWINGS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, drawing)| *drawing)
}

/// The parts of the drawing of the device saved as `name`, if it has some.
pub(crate) fn parts_of(name: &str) -> Option<&'static Parts> {
    PARTED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, parts)| *parts)
}

/// What a device declares about its card.
pub(crate) struct Drawing {
    /// The whole card's size, and its screen's height.
    pub card: Vec2,
    pub screen: f32,
    /// The deep colour the screen fades to from the device's hue.
    pub deep: Color32,
    /// The parameters the drawing reads besides its controls' own.
    pub reads: &'static [u32],
    /// Draws the picture and its live readouts, clipped to the rectangle it is
    /// given.
    pub draw: fn(&Scene<'_>),
    /// The dials and pills, in the order they give way to a smaller step.
    pub controls: &'static [Control],
    /// What can be dragged on the screen itself.
    pub handles: &'static [Handle],
}

impl Drawing {
    /// Every parameter the drawing reads, has a control or a handle for,
    /// controls first: the slots a row builds, in this order.
    pub(crate) fn params(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.controls.iter().map(|c| c.param).collect();
        let more = self
            .handles
            .iter()
            .flat_map(|h| [Some(h.param), h.also.as_ref().map(|a| a.param)])
            .flatten()
            .chain(self.reads.iter().copied());
        for id in more {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// The dials, which are what the short and the compact card keep.
    pub(crate) fn dials(&self) -> impl Iterator<Item = &Control> {
        self.controls.iter().filter(|c| c.kind == Kind::Knob)
    }
}

/// A dial or pill on a card.
pub(crate) struct Control {
    /// The device table's id.
    pub param: u32,
    /// The idea name drawn under it ("Pull"); the parameter's own name is in
    /// its tooltip and accessible name.
    pub name: &'static str,
    pub kind: Kind,
    /// Where it sits on the whole card, from the card's rectangle: a dial's
    /// cell is centred on its centre, a pill's is the pill. The smaller steps
    /// lay the controls out themselves.
    pub place: fn(Rect) -> Rect,
    /// Lit in its own colour rather than the device's hue.
    pub lit: Option<Color32>,
    /// How its value is worded under a dial: the parameter's own words, or
    /// less of them.
    pub shown: fn(&str) -> String,
    /// A shorter idea name for the short and compact cards, where a column
    /// is about 50 pt wide; the tooltip and accessible name keep `name`.
    pub short: Option<&'static str>,
    /// A dial that stands for another parameter while the values say so
    /// (Parallax's Time is the note while the delay follows the tempo, and the
    /// milliseconds while it does not): the parameter and the name it has now.
    /// The tooltip and the accessible name say both.
    pub follows: Option<fn(&Values<'_>) -> Aim>,
    /// A pill over a parameter that is not simply off and on (Parallax's Sync is
    /// a note other than Free).
    pub switch: Option<Switch>,
}

/// A pill over a parameter whose value is not 0 or 1: what reads as on, and
/// the value turning it on writes (turning it off writes 0).
#[derive(Clone, Copy)]
pub(crate) struct Switch {
    pub on: fn(f64) -> bool,
    pub turn_on: fn(&Values<'_>) -> f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A lit dial in the device's hue.
    Knob,
    /// A pill: a switch, or a chooser's current word.
    Pill,
}

impl Control {
    pub(crate) const fn knob(param: u32, name: &'static str, place: fn(Rect) -> Rect) -> Self {
        Self {
            param,
            name,
            kind: Kind::Knob,
            place,
            lit: None,
            shown: whole,
            short: None,
            follows: None,
            switch: None,
        }
    }

    pub(crate) const fn pill(param: u32, name: &'static str, place: fn(Rect) -> Rect) -> Self {
        Self {
            param,
            name,
            kind: Kind::Pill,
            place,
            lit: None,
            shown: whole,
            short: None,
            follows: None,
            switch: None,
        }
    }

    /// Names it `short` under a dial on the short and the compact card.
    pub(crate) const fn short(mut self, short: &'static str) -> Self {
        self.short = Some(short);
        self
    }

    /// Lit in `colour` instead of the device's hue.
    pub(crate) const fn lit(mut self, colour: Color32) -> Self {
        self.lit = Some(colour);
        self
    }

    /// Words its value with `shown` under a dial.
    pub(crate) const fn showing(mut self, shown: fn(&str) -> String) -> Self {
        self.shown = shown;
        self
    }

    /// Makes it stand for the parameter `follows` names from the values.
    pub(crate) const fn follows(mut self, follows: fn(&Values<'_>) -> Aim) -> Self {
        self.follows = Some(follows);
        self
    }

    /// Makes a pill read and write its parameter as `switch` says.
    pub(crate) const fn switch(mut self, switch: Switch) -> Self {
        self.switch = Some(switch);
        self
    }
}

/// A value as its parameter words it.
pub(crate) fn whole(text: &str) -> String {
    text.to_owned()
}

/// What can be dragged on a screen: one parameter, moved by where the pointer
/// is on the picture.
pub(crate) struct Handle {
    /// Its accessible name ("Beam spread"); the parameter's own is kept
    /// beside it ("Beam spread (Width)").
    pub name: &'static str,
    /// The device table's id it moves.
    pub param: u32,
    /// Where it can be grabbed, from the values and the picture's rectangle.
    pub grab: fn(&Values<'_>, Rect) -> Rect,
    /// The value (0 to 1) the pointer's place stands for.
    pub value_at: fn(&Values<'_>, Pos2, Rect) -> f64,
    /// The words of the live chip while it is dragged.
    pub chip: fn(&Values<'_>, f64) -> String,
    /// The cursor over it.
    pub cursor: CursorIcon,
    /// The parameter it moves while the values say so, instead of `param`
    /// (the echo trail's Time is the note while the delay is synced).
    pub follows: Option<fn(&Values<'_>) -> u32>,
    /// A second parameter the same grab moves by the other axis of the pointer
    /// (the echo trail's depth is Feedback while its spacing is Time): one
    /// gesture, one undo step. The arrow keys then move `param` across and this
    /// one up and down.
    pub also: Option<Also>,
}

/// What a two-way [`Handle`] moves besides its own parameter.
pub(crate) struct Also {
    /// The second parameter's id.
    pub param: u32,
    /// The value (0 to 1) the pointer's place stands for.
    pub value_at: fn(&Values<'_>, Pos2, Rect) -> f64,
}

/// A drawing made of repeated parts, one of them picked: Iris's eight bands.
/// Each part has a point on the screen (dragged, wheeled, double-clicked, kept
/// by the keys, a Tab stop), a numbered pill on the card, and the card's dials
/// aim at the picked part: the same three dials are every band's Freq, Gain and
/// Q, in the picked band's colour (M8 slice B1, the owner's round 2).
///
/// A device with parts adds one line to [`PARTED`], beside its [`DRAWINGS`]
/// line; its [`Control`]s name a part's field through a parameter of the first
/// part, and [`Parts::aim`] says which parameter that is on the picked one. A
/// part's edits are the same `Output`s a dial's are: a drag is one undo step.
pub(crate) struct Parts {
    pub count: usize,
    /// Every parameter the parts read or move beyond the controls' own: the
    /// slots a row builds for them.
    pub reads: &'static [u32],
    /// The part the card starts on, from the device's values.
    pub first: fn(&Values<'_>) -> usize,
    /// The part's own colour: its point, its pill, the dials while it is picked.
    pub colour: fn(usize) -> Color32,
    /// Whether the part is on, for how its point and pill read.
    pub is_on: fn(&Values<'_>, usize) -> bool,
    /// The parameter and idea name control `control` (an index into
    /// `controls`) stands for on `part`, or `None` when the part has nothing
    /// for it (a notch has no gain).
    pub aim: fn(&Values<'_>, usize, usize) -> Option<Aim>,
    /// Where the part's point is on the picture, and the room it is pressed
    /// in, or `None` when it has no point.
    pub at: fn(&Values<'_>, usize, Rect) -> Option<Pos2>,
    /// What the part's parameters become when its point is dragged to a place
    /// (a part switched off is switched on by the drag).
    pub drag: fn(&Values<'_>, usize, Pos2, Rect) -> Writes,
    /// What an arrow key does: steps across and up, finer when `fine`.
    pub nudge: fn(&Values<'_>, usize, Vec2, bool) -> Writes,
    /// What the wheel (or Page Up and Down) does over the point, in notches.
    pub wheel: fn(&Values<'_>, usize, f32, bool) -> Option<Write>,
    /// What a double-click on the point puts back, and what the menu's reset
    /// puts back (the whole part).
    pub reset_point: fn(&Values<'_>, usize) -> Writes,
    pub reset_all: fn(&Values<'_>, usize) -> Writes,
    /// The part's on/off switch.
    pub switch: fn(usize) -> u32,
    /// What a screen reader says of the point ("Band 3, bell, 1.20 kHz,
    /// +3.0 dB"), and of the pill ("Band 3 (bell)").
    pub words: fn(&Values<'_>, usize) -> String,
    pub pill_words: fn(&Values<'_>, usize) -> String,
    /// The pill's number on its face, and where the pill is on the whole card.
    pub number: fn(usize) -> String,
    pub pill: fn(Rect, usize) -> Rect,
    /// The control (an index into `controls`) Enter on a point types into.
    pub typed: usize,
}

/// A parameter and the value (0 to 1) a part's edit writes to it, and the
/// writes of one edit: they are one undo step.
pub(crate) type Write = (u32, f64);
pub(crate) type Writes = Vec<Write>;

/// What a dial or a pill stands for on the picked part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Aim {
    pub param: u32,
    pub name: &'static str,
}

/// The most parts a drawing can have.
pub(crate) const MAX_PARTS: usize = 8;

/// The device's current values, for the drawing.
#[derive(Clone, Copy)]
pub(crate) struct Values<'a> {
    pub slots: &'a [Slot],
    pub table: &'static [ParamInfo],
}

impl Values<'_> {
    fn slot(&self, id: u32) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// The value, 0 to 1 (0 for a parameter the drawing did not declare).
    pub(crate) fn norm(&self, id: u32) -> f32 {
        self.slot(id).map_or(0.0, |s| s.value as f32)
    }

    /// The value in the parameter's own units (130.0 for a width of 130 %).
    pub(crate) fn plain(&self, id: u32) -> f32 {
        self.plain_at(id, self.norm(id))
    }

    /// What `norm` (0 to 1) is in the parameter's own units.
    pub(crate) fn plain_at(&self, id: u32, norm: f32) -> f32 {
        lumen_devices::params::find(self.table, id).map_or(0.0, |info| info.value(norm))
    }

    /// A parameter's default, 0 to 1.
    pub(crate) fn default_norm(&self, id: u32) -> f64 {
        lumen_devices::params::find(self.table, id)
            .map_or(0.0, |info| f64::from(info.normalised(info.default)))
    }

    /// What a plain value is, 0 to 1.
    pub(crate) fn norm_of(&self, id: u32, plain: f32) -> f64 {
        lumen_devices::params::find(self.table, id)
            .map_or(0.0, |info| f64::from(info.normalised(plain)))
    }
}

/// What a drawing is given.
pub(crate) struct Scene<'a> {
    /// Clipped to the picture's rectangle.
    pub painter: &'a Painter,
    /// The picture's rectangle: the whole screen on a whole card, what the
    /// dials leave of it on a short one, a thin band on a compact one.
    pub rect: Rect,
    pub step: Step,
    /// The device's hue (greyed as the screen's power fades).
    pub hue: Color32,
    pub values: Values<'a>,
    /// How lit the screen is: 1 running, 0 bypassed, between while it fades.
    pub power: f32,
    /// What the device is hearing, 0 to 1; 0 while motion is reduced.
    pub level: f32,
    /// Motion is reduced: nothing but the values moves.
    pub still: bool,
    /// How lit each handle is (0 to 1): the pointer is on it, or it is held or
    /// focused. Indexed as `Drawing::handles`.
    pub handle_lit: [f32; MAX_HANDLES],
    /// How far a changed preset's readouts have come back (0 to 1): a drawing
    /// brings its readouts in with it.
    pub settle: f32,
    /// The picked part of a drawing with [`Parts`] (0 for one without), and
    /// how lit each part's point is (hovered, held or focused), indexed as the
    /// parts.
    pub picked: usize,
    pub parts_lit: [f32; MAX_PARTS],
    /// The device's live analysis, the points across the picture from its low
    /// end to its high, in decibels, and how present it is (0 to 1: it fades in
    /// and out as audio starts and stops, and is 0 while motion is reduced).
    pub spectrum: Option<&'a [f32]>,
    pub mist: f32,
    /// The rate the project runs at, for a drawing that works a response out.
    pub rate: u32,
    /// The tempo at the playhead, for a drawing that keeps time with the song
    /// (a delay whose time is a note).
    pub bpm: f32,
    /// The card's own id, for a drawing that keeps a little state between
    /// frames (a reverb's swell fading with its decay).
    pub id: eframe::egui::Id,
}

impl Scene<'_> {
    /// `colour` as the screen shows it: drained toward a cool grey as its
    /// power fades.
    pub(crate) fn ink(&self, colour: Color32) -> Color32 {
        if self.power >= 1.0 {
            colour
        } else {
            crate::theme::grey_of(colour)
                .gamma_multiply(MUTED + (1.0 - MUTED) * self.power)
                .lerp_to_gamma(colour, self.power)
        }
    }
}

/// How strong a drained colour stays.
const MUTED: f32 = 0.5;

/// The most handles a drawing can have.
pub(crate) const MAX_HANDLES: usize = 4;

#[cfg(test)]
mod tests;
