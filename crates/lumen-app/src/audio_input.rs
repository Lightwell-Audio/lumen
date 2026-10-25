//! Audio input as the engine plays it (M9 slice R1, ADR-0100): which of the
//! device's open inputs each audio track listens to, and when it hears them.
//! Kept pure, so the translation is tested, as `routing::routing` is.

use lumen_engine::{InputRoute, Monitoring};
use lumen_project::model::{AudioInput, Monitor, Project, Track, TrackKind};

/// Where `input` sits among the device channels open as inputs (`open`, in
/// the order the engine sees them): its first channel's place, when the
/// channel after it is open too for a pair. `None` when the device does not
/// have it open, so the track is silent and says why.
pub fn place(input: AudioInput, open: &[usize]) -> Option<usize> {
    let first = open.iter().position(|&c| c == usize::from(input.channel))?;
    (!input.stereo || open.get(first + 1) == Some(&(usize::from(input.channel) + 1)))
        .then_some(first)
}

/// When a track's input is heard (the owner's R2): monitor In always, Auto
/// while the track is armed, Off never.
pub fn monitoring(track: &Track) -> Monitoring {
    match track.monitor {
        Monitor::In => Monitoring::Always,
        Monitor::Auto if track.armed => Monitoring::Armed,
        Monitor::Auto | Monitor::Off => Monitoring::Never,
    }
}

/// The routes of every audio track that has an input the device has open. A
/// frozen track plays its file and listens to nothing.
pub fn routes(project: &Project, open: &[usize]) -> Vec<InputRoute> {
    project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio && t.frozen.is_none())
        .filter_map(|t| {
            let input = t.input?;
            Some(InputRoute {
                track: t.id.0,
                channel: place(input, open)?,
                stereo: input.stereo,
                monitoring: monitoring(t),
            })
        })
        .collect()
}

/// Why a track's input cannot be heard or recorded, in words for the track
/// (`None` when it can, or it has none).
pub fn unavailable(track: &Track, open: &[usize]) -> Option<&'static str> {
    let input = track.input?;
    if open.is_empty() {
        return Some("The audio device has no inputs open");
    }
    place(input, open)
        .is_none()
        .then_some("This input is not on the audio device in use")
}

/// The first channels of the stereo pairs the open inputs offer: two open
/// channels side by side, the first even (1+2, 3+4, ...).
pub fn pairs(open: &[usize]) -> Vec<u16> {
    open.iter()
        .filter(|&&c| c % 2 == 0 && open.contains(&(c + 1)))
        .map(|&c| c as u16)
        .collect()
}

/// The input after `current` in the order the input menu lists them: none,
/// each stereo pair, each mono input, and round to none again. An input the
/// device does not have is followed by the first one it does.
pub fn next_input(current: Option<AudioInput>, open: &[usize]) -> Option<AudioInput> {
    let choices: Vec<Option<AudioInput>> = std::iter::once(None)
        .chain(pairs(open).into_iter().map(|c| Some(AudioInput::pair(c))))
        .chain(open.iter().map(|&c| Some(AudioInput::mono(c as u16))))
        .collect();
    match choices.iter().position(|&c| c == current) {
        Some(at) => choices[(at + 1) % choices.len()],
        None => choices.get(1).copied().flatten(),
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{Color, Id};
    use lumen_project::testing::project_of;

    use super::*;

    fn audio(id: u64, input: Option<AudioInput>, armed: bool, monitor: Monitor) -> Track {
        let mut track = Track::new(Id(id), format!("T{id}"), TrackKind::Audio, Color(0));
        track.input = input;
        track.armed = armed;
        track.monitor = monitor;
        track
    }

    #[test]
    fn an_input_is_found_among_the_open_ones_by_its_device_channel() {
        let open = [2, 3, 5];
        assert_eq!(place(AudioInput::mono(3), &open), Some(1));
        assert_eq!(place(AudioInput::pair(2), &open), Some(0));
        assert_eq!(place(AudioInput::pair(3), &open), None, "4 is not open");
        assert_eq!(place(AudioInput::mono(4), &open), None);
        assert_eq!(place(AudioInput::mono(0), &[]), None);
    }

    #[test]
    fn monitoring_follows_the_monitor_mode_and_the_arm() {
        let of = |armed, monitor| monitoring(&audio(1, None, armed, monitor));
        assert_eq!(of(true, Monitor::Auto), Monitoring::Armed);
        assert_eq!(of(false, Monitor::Auto), Monitoring::Never);
        assert_eq!(of(false, Monitor::In), Monitoring::Always);
        assert_eq!(of(true, Monitor::Off), Monitoring::Never);
    }

    #[test]
    fn only_audio_tracks_with_an_available_input_get_a_route() {
        let mut midi = Track::new(Id(9), "Keys", TrackKind::Midi, Color(0));
        midi.armed = true;
        let mut frozen = audio(5, Some(AudioInput::mono(0)), true, Monitor::Auto);
        frozen.frozen = Some(lumen_project::model::Frozen {
            id: Id(50),
            media: Id(51),
            start: Default::default(),
            length: Default::default(),
            mono: false,
        });
        let project = project_of(
            "p",
            vec![
                audio(1, Some(AudioInput::mono(1)), true, Monitor::Auto),
                audio(2, Some(AudioInput::pair(0)), false, Monitor::In),
                audio(3, None, true, Monitor::Auto),
                audio(4, Some(AudioInput::mono(7)), true, Monitor::Auto),
                frozen,
                midi,
            ],
        );
        let routes = routes(&project, &[0, 1]);
        assert_eq!(
            routes,
            vec![
                InputRoute {
                    track: 1,
                    channel: 1,
                    stereo: false,
                    monitoring: Monitoring::Armed
                },
                InputRoute {
                    track: 2,
                    channel: 0,
                    stereo: true,
                    monitoring: Monitoring::Always
                },
            ]
        );
    }

    #[test]
    fn a_missing_input_is_explained() {
        let track = audio(1, Some(AudioInput::mono(5)), true, Monitor::Auto);
        assert!(unavailable(&track, &[0, 1]).unwrap().contains("not on"));
        assert!(unavailable(&track, &[]).unwrap().contains("no inputs"));
        assert!(unavailable(&track, &[5]).is_none());
        let none = audio(2, None, true, Monitor::Auto);
        assert!(unavailable(&none, &[]).is_none());
    }
    #[test]
    fn next_input_walks_none_pairs_then_mono_inputs_and_round() {
        let open = [0, 1, 2];
        let mut at = None;
        let mut seen = Vec::new();
        for _ in 0..6 {
            at = next_input(at, &open);
            seen.push(at);
        }
        assert_eq!(
            seen,
            vec![
                Some(AudioInput::pair(0)),
                Some(AudioInput::mono(0)),
                Some(AudioInput::mono(1)),
                Some(AudioInput::mono(2)),
                None,
                Some(AudioInput::pair(0)),
            ]
        );
        // One the device lacks is followed by the first it has; with no
        // inputs open there is nothing to choose.
        assert_eq!(
            next_input(Some(AudioInput::mono(9)), &open),
            Some(AudioInput::pair(0))
        );
        assert_eq!(next_input(Some(AudioInput::mono(9)), &[]), None);
        assert_eq!(next_input(None, &[]), None);
    }
}
