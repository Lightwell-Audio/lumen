use lumen_engine::{InputRoute, Monitoring};
use lumen_project::model::{AudioInput, Monitor, Project, Track, TrackKind};

pub fn place(input: AudioInput, open: &[usize]) -> Option<usize> {
    let first = open.iter().position(|&c| c == usize::from(input.channel))?;
    (!input.stereo || open.get(first + 1) == Some(&(usize::from(input.channel) + 1)))
        .then_some(first)
}

pub fn monitoring(track: &Track) -> Monitoring {
    match track.monitor {
        Monitor::In => Monitoring::Always,
        Monitor::Auto if track.armed => Monitoring::Armed,
        Monitor::Auto | Monitor::Off => Monitoring::Never,
    }
}

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
                record: t.armed,
            })
        })
        .collect()
}

pub fn unavailable(track: &Track, open: &[usize]) -> Option<&'static str> {
    let input = track.input?;
    if open.is_empty() {
        return Some("The audio device has no inputs open");
    }
    place(input, open)
        .is_none()
        .then_some("This input is not on the audio device in use")
}

pub fn pairs(open: &[usize]) -> Vec<u16> {
    open.iter()
        .filter(|&&c| c % 2 == 0 && open.contains(&(c + 1)))
        .map(|&c| c as u16)
        .collect()
}

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
                    monitoring: Monitoring::Armed,
                    record: true,
                },
                InputRoute {
                    track: 2,
                    channel: 0,
                    stereo: true,
                    monitoring: Monitoring::Always,
                    record: false,
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
        assert_eq!(
            next_input(Some(AudioInput::mono(9)), &open),
            Some(AudioInput::pair(0))
        );
        assert_eq!(next_input(Some(AudioInput::mono(9)), &[]), None);
        assert_eq!(next_input(None, &[]), None);
    }
}
