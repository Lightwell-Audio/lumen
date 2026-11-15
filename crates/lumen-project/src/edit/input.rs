use crate::model::{AudioInput, Id, Project, TrackKind};

pub fn first_free(project: &Project, open: u16, track: Id) -> Option<AudioInput> {
    if open == 0 {
        return None;
    }
    let taken = |channel: u16| {
        project
            .tracks
            .iter()
            .filter(|t| t.id != track && t.kind == TrackKind::Audio)
            .filter_map(|t| t.input)
            .any(|input| input.channels().contains(&channel))
    };
    let channel = (0..open).find(|&c| !taken(c)).unwrap_or(0);
    Some(AudioInput::mono(channel))
}

pub fn route_armed(project: &mut Project, ids: &[Id], open: u16) -> bool {
    let mut changed = false;
    for &id in ids {
        let Some(index) = project
            .tracks
            .iter()
            .position(|t| t.id == id && t.kind == TrackKind::Audio && t.armed && t.input.is_none())
        else {
            continue;
        };
        if let Some(input) = first_free(project, open, id) {
            std::sync::Arc::make_mut(&mut project.tracks[index]).input = Some(input);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{Color, Track};
    use crate::testing::project_of;

    fn audio(id: u64, input: Option<AudioInput>) -> Track {
        let mut track = Track::new(Id(id), format!("T{id}"), TrackKind::Audio, Color(0));
        track.input = input;
        track
    }

    #[test]
    fn the_first_input_nobody_uses_is_chosen() {
        let project = project_of(
            "p",
            vec![
                audio(1, Some(AudioInput::mono(0))),
                audio(2, Some(AudioInput::pair(1))),
                audio(3, None),
            ],
        );
        assert_eq!(first_free(&project, 8, Id(3)), Some(AudioInput::mono(3)));
        assert_eq!(first_free(&project, 8, Id(1)), Some(AudioInput::mono(0)));
    }

    #[test]
    fn every_input_taken_shares_the_first_and_no_inputs_choose_nothing() {
        let project = project_of(
            "p",
            vec![
                audio(1, Some(AudioInput::mono(0))),
                audio(2, Some(AudioInput::mono(1))),
                audio(3, None),
            ],
        );
        assert_eq!(first_free(&project, 2, Id(3)), Some(AudioInput::mono(0)));
        assert_eq!(first_free(&project, 0, Id(3)), None);
    }

    #[test]
    fn tracks_armed_together_get_different_inputs_and_routed_ones_keep_theirs() {
        let mut project = project_of(
            "p",
            vec![
                audio(1, Some(AudioInput::mono(0))),
                audio(2, None),
                audio(3, None),
                audio(4, None),
            ],
        );
        for index in 0..4 {
            Arc::make_mut(&mut project.tracks[index]).armed = true;
        }
        Arc::make_mut(&mut project.tracks[3]).armed = false;
        assert!(route_armed(&mut project, &[Id(1), Id(2), Id(3), Id(4)], 4));
        let inputs: Vec<_> = project.tracks.iter().map(|t| t.input).collect();
        assert_eq!(
            inputs,
            vec![
                Some(AudioInput::mono(0)),
                Some(AudioInput::mono(1)),
                Some(AudioInput::mono(2)),
                None
            ]
        );
        assert!(
            !route_armed(&mut project, &[Id(1), Id(2), Id(3)], 4),
            "settled"
        );
    }
}
