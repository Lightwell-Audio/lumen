//! Vector icons (owner request, M2 slice 4). Each icon is geometry on a
//! 16 × 16 grid, scaled to the target rectangle when painted, so it is sharp
//! at every UI scale. They are drawn with egui's painter: at icon sizes that
//! is crisp and cheap, and Vello stays reserved for dense canvases (ADR-0018).

mod geometry;
mod tracks;
mod widgets;

pub use geometry::shapes;
pub use tracks::draw_type_icon;
pub use widgets::{
    CLOSE_SIZE, Look, beam, button, button_look, close_button, draw_headphones, toggle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ToStart,
    Play,
    Pause,
    Stop,
    Record,
    /// Counting in (M5 slice 6d): the record ring, hollow, that pulses.
    RecordRing,
    Loop,
    Metronome,
    /// Follow the playhead: a playhead line with an arrow.
    Follow,
    /// Snapping on: a horseshoe magnet.
    Magnet,
    /// Snapping off: the magnet, struck through.
    MagnetOff,
    /// A locked clip (M3 slice 6e): a padlock.
    Lock,
    /// The command palette (M3 slice 7): a magnifier.
    Search,
    /// A mouse, before a gesture in the info line (M4 slice 0).
    Mouse,
    /// The computer keyboard plays notes (M5 slice 2): a keycap, a rounded
    /// square with a smaller one set up inside it.
    Keycap,
    /// MIDI (M5 slice 2d): a five-pin DIN socket.
    Din,
    /// Monitor In (M5 slice 2 fix): a speaker with one wave.
    Speaker,
    /// Monitor Off: the speaker, struck through.
    SpeakerOff,
    /// Overdub (M5 slice 6d): layers, a stack of three.
    Layers,
    /// A favorite (M6 slice 1): a five-point star, outlined.
    Star,
    /// A favorite that is set: the star, filled.
    StarFilled,
    /// An instrument (M6 slice 1): a row of keys.
    Keys,
    /// An effect (M6 slice 1): a knob.
    Knob,
    /// A plugin Lumen will not load (M6 slice 1): a triangle with a mark.
    Warning,
    /// A power symbol: a device's on and off.
    Power,
    /// Three sliders: an effect.
    Sliders,
    /// A plug with a slash: a plugin that is not installed.
    PlugOff,
    /// Safe mode (M6 slice 5e): a shield with a check.
    Shield,
    /// The browser (the left panel): a window with its left column marked.
    PanelLeft,
    /// The detail panel: a window with its bottom strip marked.
    PanelBottom,
    /// A plugin's own window (M6 slice 5h): a window with its title bar.
    Window,
    /// The Pointer tool (the interface audit's M6): an arrow pointer.
    Pointer,
    /// The Draw tool: a pencil.
    Pencil,
    /// The Erase tool: an eraser on a line.
    Eraser,
    /// The Split tool: a cut between two clip edges.
    Split,
    /// The Mute tool: a circle, struck through.
    MuteCircle,
    /// The Zoom tool: a magnifier with a plus.
    ZoomIn,
    /// The tempo and meter lanes (H1): a stepped tempo line under a faint
    /// meter line.
    TempoLanes,
    /// The browser's Devices tab (M9 R7c): a rack with three slots.
    Rack,
    /// The Samples tab: a waveform line.
    Wave,
    /// The Projects tab: a page with its corner folded.
    Document,
    /// The Places tab and a folder row: a folder.
    Folder,
    /// The Project media tab: a disc.
    Disc,
    /// Reverse (M9.5 S3): an arrow running back over a short wave.
    Reverse,
    /// Polarity inverted (M9.5 S3): a ring with a slash that runs past it.
    Polarity,
    /// Normalize (M9.5 S3): a level raised to a ceiling.
    Normalize,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 46] = [
        Icon::ToStart,
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::Record,
        Icon::RecordRing,
        Icon::Loop,
        Icon::Metronome,
        Icon::Follow,
        Icon::Magnet,
        Icon::MagnetOff,
        Icon::Lock,
        Icon::Search,
        Icon::Mouse,
        Icon::Keycap,
        Icon::Din,
        Icon::Speaker,
        Icon::SpeakerOff,
        Icon::Layers,
        Icon::Star,
        Icon::StarFilled,
        Icon::Keys,
        Icon::Knob,
        Icon::Warning,
        Icon::Power,
        Icon::Sliders,
        Icon::PlugOff,
        Icon::Shield,
        Icon::PanelLeft,
        Icon::PanelBottom,
        Icon::Window,
        Icon::Pointer,
        Icon::Pencil,
        Icon::Eraser,
        Icon::Split,
        Icon::MuteCircle,
        Icon::ZoomIn,
        Icon::TempoLanes,
        Icon::Rack,
        Icon::Wave,
        Icon::Document,
        Icon::Folder,
        Icon::Disc,
        Icon::Reverse,
        Icon::Polarity,
        Icon::Normalize,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, Pos2, Rect, Shape, Vec2, pos2};

    #[test]
    fn every_icon_stays_inside_its_box_at_every_scale() {
        for scale in [1.0, 1.5, 2.0] {
            let rect = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(16.0 * scale));
            for icon in Icon::ALL {
                let shapes = shapes(icon, rect, Color32::WHITE);
                assert!(!shapes.is_empty(), "{icon:?}");
                for shape in shapes {
                    let bounds = shape.visual_bounding_rect();
                    assert!(
                        rect.expand(0.5).contains_rect(bounds),
                        "{icon:?} at {scale}: {bounds:?} outside {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn icons_scale_with_their_box() {
        let small = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        let large = Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0));
        for icon in Icon::ALL {
            let width = |r| {
                shapes(icon, r, Color32::WHITE)
                    .iter()
                    .map(Shape::visual_bounding_rect)
                    .fold(Rect::NOTHING, |a, b| a.union(b))
                    .width()
            };
            let ratio = width(large) / width(small);
            assert!((1.8..2.2).contains(&ratio), "{icon:?}: {ratio}");
        }
    }
}
