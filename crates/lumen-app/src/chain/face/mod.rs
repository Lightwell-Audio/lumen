use eframe::egui::{Color32, CursorIcon, Painter, Pos2, Rect, Vec2};
use lumen_devices::params::ParamInfo;

use super::geometry::Step;
use super::strip::Slot;

pub(crate) mod afterglow;
#[allow(dead_code)]
pub(crate) mod ballistics;
pub(crate) mod cadence;
pub(crate) mod eq;
pub(crate) mod gravity;
pub(crate) mod horizon;
mod meter;
mod paint;
pub(crate) mod parallax;
pub(crate) mod prism;

const DRAWINGS: &[(&str, &Drawing)] = &[
    (lumen_devices::utility::NAME, &prism::DRAWING),
    (lumen_devices::eq::NAME, &eq::DRAWING),
    (lumen_devices::delay::NAME, &parallax::DRAWING),
    (lumen_devices::reverb::NAME, &afterglow::DRAWING),
    (lumen_devices::dynamics::compressor::NAME, &gravity::DRAWING),
    (lumen_devices::dynamics::limiter::NAME, &horizon::DRAWING),
];

const PARTED: &[(&str, &Parts)] = &[(lumen_devices::eq::NAME, &eq::PARTS)];

pub(crate) fn drawing_of(name: &str) -> Option<&'static Drawing> {
    DRAWINGS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, drawing)| *drawing)
}

pub(crate) fn parts_of(name: &str) -> Option<&'static Parts> {
    PARTED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, parts)| *parts)
}

pub(crate) struct Drawing {
    pub card: Vec2,
    pub screen: f32,
    pub deep: Color32,
    pub reads: &'static [u32],
    pub draw: fn(&Scene<'_>),
    pub controls: &'static [Control],
    pub handles: &'static [Handle],
}

impl Drawing {
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

    pub(crate) fn dials(&self) -> impl Iterator<Item = &Control> {
        self.controls.iter().filter(|c| c.kind == Kind::Knob)
    }
}

pub(crate) struct Control {
    pub param: u32,
    pub name: &'static str,
    pub kind: Kind,
    pub place: fn(Rect) -> Rect,
    pub lit: Option<Color32>,
    pub shown: fn(&str) -> String,
    pub short: Option<&'static str>,
    pub follows: Option<fn(&Values<'_>) -> Aim>,
    pub switch: Option<Switch>,
}

#[derive(Clone, Copy)]
pub(crate) struct Switch {
    pub on: fn(f64) -> bool,
    pub turn_on: fn(&Values<'_>) -> f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Knob,
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

    pub(crate) const fn short(mut self, short: &'static str) -> Self {
        self.short = Some(short);
        self
    }

    pub(crate) const fn lit(mut self, colour: Color32) -> Self {
        self.lit = Some(colour);
        self
    }

    pub(crate) const fn showing(mut self, shown: fn(&str) -> String) -> Self {
        self.shown = shown;
        self
    }

    pub(crate) const fn follows(mut self, follows: fn(&Values<'_>) -> Aim) -> Self {
        self.follows = Some(follows);
        self
    }

    pub(crate) const fn switch(mut self, switch: Switch) -> Self {
        self.switch = Some(switch);
        self
    }
}

pub(crate) fn whole(text: &str) -> String {
    text.to_owned()
}

pub(crate) struct Handle {
    pub name: &'static str,
    pub param: u32,
    pub grab: fn(&Values<'_>, Rect) -> Rect,
    pub value_at: fn(&Values<'_>, Pos2, Rect) -> f64,
    pub chip: fn(&Values<'_>, f64) -> String,
    pub cursor: CursorIcon,
    pub follows: Option<fn(&Values<'_>) -> u32>,
    pub also: Option<Also>,
}

pub(crate) struct Also {
    pub param: u32,
    pub value_at: fn(&Values<'_>, Pos2, Rect) -> f64,
}

pub(crate) struct Parts {
    pub count: usize,
    pub reads: &'static [u32],
    pub first: fn(&Values<'_>) -> usize,
    pub colour: fn(usize) -> Color32,
    pub is_on: fn(&Values<'_>, usize) -> bool,
    pub aim: fn(&Values<'_>, usize, usize) -> Option<Aim>,
    pub at: fn(&Values<'_>, usize, Rect) -> Option<Pos2>,
    pub drag: fn(&Values<'_>, usize, Pos2, Rect) -> Writes,
    pub nudge: fn(&Values<'_>, usize, Vec2, bool) -> Writes,
    pub wheel: fn(&Values<'_>, usize, f32, bool) -> Option<Write>,
    pub reset_point: fn(&Values<'_>, usize) -> Writes,
    pub reset_all: fn(&Values<'_>, usize) -> Writes,
    pub switch: fn(usize) -> u32,
    pub words: fn(&Values<'_>, usize) -> String,
    pub pill_words: fn(&Values<'_>, usize) -> String,
    pub number: fn(usize) -> String,
    pub pill: fn(Rect, usize) -> Rect,
    pub typed: usize,
}

pub(crate) type Write = (u32, f64);
pub(crate) type Writes = Vec<Write>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Aim {
    pub param: u32,
    pub name: &'static str,
}

pub(crate) const MAX_PARTS: usize = 8;

#[derive(Clone, Copy)]
pub(crate) struct Values<'a> {
    pub slots: &'a [Slot],
    pub table: &'static [ParamInfo],
}

impl Values<'_> {
    fn slot(&self, id: u32) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    pub(crate) fn norm(&self, id: u32) -> f32 {
        self.slot(id).map_or(0.0, |s| s.value as f32)
    }

    pub(crate) fn plain(&self, id: u32) -> f32 {
        self.plain_at(id, self.norm(id))
    }

    pub(crate) fn plain_at(&self, id: u32, norm: f32) -> f32 {
        lumen_devices::params::find(self.table, id).map_or(0.0, |info| info.value(norm))
    }

    pub(crate) fn default_norm(&self, id: u32) -> f64 {
        lumen_devices::params::find(self.table, id)
            .map_or(0.0, |info| f64::from(info.normalised(info.default)))
    }

    pub(crate) fn norm_of(&self, id: u32, plain: f32) -> f64 {
        lumen_devices::params::find(self.table, id)
            .map_or(0.0, |info| f64::from(info.normalised(plain)))
    }
}

pub(crate) struct Scene<'a> {
    pub painter: &'a Painter,
    pub rect: Rect,
    pub step: Step,
    pub hue: Color32,
    pub values: Values<'a>,
    pub power: f32,
    pub level: f32,
    pub still: bool,
    pub handle_lit: [f32; MAX_HANDLES],
    pub settle: f32,
    pub picked: usize,
    pub parts_lit: [f32; MAX_PARTS],
    pub spectrum: Option<&'a [f32]>,
    pub mist: f32,
    pub rate: u32,
    pub bpm: f32,
    pub reading: Option<lumen_devices::MeterReading>,
    pub id: eframe::egui::Id,
}

impl Scene<'_> {
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

const MUTED: f32 = 0.5;

pub(crate) const MAX_HANDLES: usize = 4;

#[cfg(test)]
mod tests;
