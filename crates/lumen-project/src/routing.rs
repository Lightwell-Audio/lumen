use std::collections::HashMap;

use crate::model::{Id, Output, Project, Track, TrackKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Track(Id),
    Device(u16),
}

pub fn destination(project: &Project, track: &Track) -> Destination {
    if track.kind == TrackKind::Master {
        return match track.output {
            Output::Device(pair) => Destination::Device(pair),
            _ => Destination::Device(0),
        };
    }
    let master = Destination::Track(project.master.id);
    match track.output {
        Output::Auto => track.parent.map_or(master, Destination::Track),
        Output::Master => master,
        Output::Track(id) => Destination::Track(id),
        Output::Device(pair) => Destination::Device(pair),
    }
}

pub fn feeds<'a>(project: &'a Project, track: &'a Track) -> impl Iterator<Item = Id> + 'a {
    let output = match destination(project, track) {
        Destination::Track(id) => Some(id),
        Destination::Device(_) => None,
    };
    output
        .into_iter()
        .chain(track.sends.iter().map(|send| send.to))
}

pub fn all_tracks(project: &Project) -> impl Iterator<Item = &Track> {
    project
        .tracks
        .iter()
        .map(|t| &**t)
        .chain(std::iter::once(&*project.master))
}

pub fn has_loop(project: &Project) -> bool {
    let tracks: Vec<&Track> = all_tracks(project).collect();
    let index: HashMap<Id, usize> = tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let mut edges: Vec<Vec<usize>> = tracks
        .iter()
        .map(|t| {
            feeds(project, t)
                .filter_map(|id| index.get(&id).copied())
                .collect()
        })
        .collect();
    for (to, track) in tracks.iter().enumerate() {
        for sidechain in track.devices.iter().filter_map(|d| d.sidechain) {
            if let Some(&from) = index.get(&sidechain.from) {
                edges[from].push(to);
            }
        }
    }
    let mut state = vec![0u8; tracks.len()];
    for start in 0..tracks.len() {
        if state[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        state[start] = 1;
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            if let Some(&to) = edges[node].get(*next) {
                *next += 1;
                match state[to] {
                    1 => return true,
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    _ => {}
                }
            } else {
                state[node] = 2;
                stack.pop();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{Color, Level, Send};
    use crate::testing::small_project;

    fn project(kinds: &[TrackKind]) -> Project {
        let mut project = small_project(1);
        project.tracks = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                Arc::new(Track::new(
                    Id(i as u64 + 1),
                    format!("T{i}"),
                    kind,
                    Color(0),
                ))
            })
            .collect();
        project
    }

    fn track(project: &mut Project, i: usize) -> &mut Track {
        Arc::make_mut(&mut project.tracks[i])
    }

    #[test]
    fn auto_goes_to_the_group_or_else_the_master() {
        let mut p = project(&[TrackKind::Group, TrackKind::Audio, TrackKind::Audio]);
        track(&mut p, 1).parent = Some(Id(1));
        let master = p.master.id;
        assert_eq!(destination(&p, &p.tracks[1]), Destination::Track(Id(1)));
        assert_eq!(destination(&p, &p.tracks[2]), Destination::Track(master));
        track(&mut p, 1).output = Output::Master;
        assert_eq!(destination(&p, &p.tracks[1]), Destination::Track(master));
        track(&mut p, 2).output = Output::Device(2);
        assert_eq!(destination(&p, &p.tracks[2]), Destination::Device(2));
        assert_eq!(destination(&p, &p.master), Destination::Device(0));
    }

    #[test]
    fn loops_through_outputs_groups_and_sends_are_found() {
        let mut p = project(&[TrackKind::Group, TrackKind::Return, TrackKind::Audio]);
        assert!(!has_loop(&p));
        track(&mut p, 1).output = Output::Track(Id(1));
        track(&mut p, 0).sends.push(Send {
            to: Id(2),
            level: Level(0.0),
            pre_fader: false,
        });
        assert!(has_loop(&p));
        let mut p = project(&[TrackKind::Group, TrackKind::Group]);
        track(&mut p, 0).parent = Some(Id(2));
        track(&mut p, 1).parent = Some(Id(1));
        assert!(has_loop(&p));
        let mut p = project(&[TrackKind::Return, TrackKind::Audio]);
        track(&mut p, 1).sends.push(Send {
            to: Id(1),
            level: Level(-6.0),
            pre_fader: true,
        });
        assert!(!has_loop(&p));
    }
}
