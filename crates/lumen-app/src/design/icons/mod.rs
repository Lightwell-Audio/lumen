mod drums;
mod geometry;
mod tracks;
mod widgets;

pub use drums::{DrumGlyph, paint_drum_glyph};
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
    RecordRing,
    Loop,
    Metronome,
    Follow,
    Magnet,
    MagnetOff,
    Lock,
    Search,
    Mouse,
    Keycap,
    Din,
    Speaker,
    SpeakerOff,
    Layers,
    Star,
    StarFilled,
    Keys,
    Knob,
    Warning,
    Power,
    Sliders,
    PlugOff,
    Shield,
    PanelLeft,
    PanelBottom,
    Window,
    Pointer,
    Pencil,
    Eraser,
    Split,
    MuteCircle,
    ZoomIn,
    TempoLanes,
    Rack,
    Wave,
    Document,
    Folder,
    Disc,
    Reverse,
    Polarity,
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
