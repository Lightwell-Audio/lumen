//! Where a device the browser chose goes (the owner's D3 and D5). Pure, so
//! every case is tested; `App::add_device` applies the answer.

use lumen_project::model::{DeviceKind, Id, Project, TrackKind};
use lumen_project::routing::all_tracks;

use crate::document::Slot;

/// What choosing a device does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Add it to this track at `slot`.
    Track { track: Id, slot: Slot },
    /// Make a track of the right kind after `after` and put it there.
    NewTrack { after: Option<Id> },
}

/// The track a choice lands on. The tracks selected count, the last chosen
/// first. An instrument needs a MIDI track; an effect takes any track but the
/// master. With no track that fits, a new one is made (D3). An effect goes
/// after the chosen card when it is on that track, else at the end (D5).
pub(crate) fn place(
    project: &Project,
    selected: &[Id],
    card: Option<Id>,
    kind: DeviceKind,
) -> Place {
    let fits = |id: &Id| {
        all_tracks(project)
            .find(|t| t.id == *id)
            .is_some_and(|t| match kind {
                DeviceKind::Instrument => t.kind == TrackKind::Midi,
                DeviceKind::Effect => true,
            })
    };
    match selected.iter().rev().find(|id| fits(id)) {
        Some(&track) => {
            let slot = match (kind, card) {
                (DeviceKind::Effect, Some(card))
                    if all_tracks(project)
                        .any(|t| t.id == track && t.devices.iter().any(|d| d.id == card)) =>
                {
                    Slot::After(card)
                }
                _ => Slot::End,
            };
            Place::Track { track, slot }
        }
        None => Place::NewTrack {
            after: selected
                .iter()
                .rev()
                .find(|id| {
                    all_tracks(project).any(|t| {
                        t.id == **id && !matches!(t.kind, TrackKind::Return | TrackKind::Master)
                    })
                })
                .copied(),
        },
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{Device, PluginRef, Track};

    use super::*;

    fn effect(id: u64) -> Device {
        Device::new(
            Id(id),
            PluginRef::Builtin {
                name: "utility".into(),
            },
            false,
            Vec::new(),
        )
    }

    fn project() -> Project {
        let mut project = crate::empty_project();
        let color = project.master.color;
        let mut audio = Track::new(Id(1), "Audio 1".to_owned(), TrackKind::Audio, color);
        audio.devices = vec![effect(10), effect(11)];
        let midi = Track::new(Id(2), "MIDI 1".to_owned(), TrackKind::Midi, color);
        project.tracks = vec![audio.into(), midi.into()];
        project
    }

    #[test]
    fn an_effect_goes_after_the_chosen_card_or_at_the_end() {
        let p = project();
        assert_eq!(
            place(&p, &[Id(1)], Some(Id(10)), DeviceKind::Effect),
            Place::Track {
                track: Id(1),
                slot: Slot::After(Id(10))
            }
        );
        assert_eq!(
            place(&p, &[Id(1)], None, DeviceKind::Effect),
            Place::Track {
                track: Id(1),
                slot: Slot::End
            }
        );
        // A chosen card on another track is not a place on this one.
        assert_eq!(
            place(&p, &[Id(2)], Some(Id(10)), DeviceKind::Effect),
            Place::Track {
                track: Id(2),
                slot: Slot::End
            }
        );
    }

    #[test]
    fn an_instrument_needs_a_midi_track_and_the_selection_can_hold_several() {
        let p = project();
        assert_eq!(
            place(&p, &[Id(1), Id(2)], None, DeviceKind::Instrument),
            Place::Track {
                track: Id(2),
                slot: Slot::End
            }
        );
        // The last chosen track that fits wins.
        assert_eq!(
            place(&p, &[Id(2), Id(1)], None, DeviceKind::Effect),
            Place::Track {
                track: Id(1),
                slot: Slot::End
            }
        );
    }

    #[test]
    fn nothing_that_fits_makes_a_track_after_the_selection() {
        let p = project();
        assert_eq!(
            place(&p, &[Id(1)], None, DeviceKind::Instrument),
            Place::NewTrack { after: Some(Id(1)) }
        );
        assert_eq!(
            place(&p, &[], None, DeviceKind::Effect),
            Place::NewTrack { after: None }
        );
        // The master takes an effect, but no instrument (and no track is
        // made after it).
        let master = p.master.id;
        assert_eq!(
            place(&p, &[master], None, DeviceKind::Effect),
            Place::Track {
                track: master,
                slot: Slot::End
            }
        );
        assert_eq!(
            place(&p, &[master], None, DeviceKind::Instrument),
            Place::NewTrack { after: None }
        );
    }
}
