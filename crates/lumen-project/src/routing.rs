//! Where audio goes (format v7, M4 slice 2): one rule for validating files,
//! refusing edits that would feed back, and building the engine's routing.

use std::collections::HashMap;

use crate::model::{Id, Output, Project, Track, TrackKind};

/// Where a track's output lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Another track's input: a group, a return or the master.
    Track(Id),
    /// A pair of device outputs: 0 is outputs 1 and 2.
    Device(u16),
}

/// Where `track`'s output goes in `project`. `Auto` is its group when it is
/// in one, otherwise the master; the master's `Auto` is the device's first
/// pair of outputs.
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

/// The tracks `track` feeds: where its output goes and the returns it
/// sends to.
pub fn feeds<'a>(project: &'a Project, track: &'a Track) -> impl Iterator<Item = Id> + 'a {
    let output = match destination(project, track) {
        Destination::Track(id) => Some(id),
        Destination::Device(_) => None,
    };
    output
        .into_iter()
        .chain(track.sends.iter().map(|send| send.to))
}

/// Every track, the master last.
pub fn all_tracks(project: &Project) -> impl Iterator<Item = &Track> {
    project
        .tracks
        .iter()
        .map(|t| &**t)
        .chain(std::iter::once(&*project.master))
}

/// Whether the routing feeds back into itself: through outputs, groups or
/// sends. Feeds to tracks that are not there are ignored (validation
/// reports them).
pub fn has_loop(project: &Project) -> bool {
    let tracks: Vec<&Track> = all_tracks(project).collect();
    let index: HashMap<Id, usize> = tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let edges: Vec<Vec<usize>> = tracks
        .iter()
        .map(|t| {
            feeds(project, t)
                .filter_map(|id| index.get(&id).copied())
                .collect()
        })
        .collect();
    // Depth-first, iterative: 0 unvisited, 1 on the path, 2 done.
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
        // A return sending into itself through a group it outputs to.
        track(&mut p, 1).output = Output::Track(Id(1));
        track(&mut p, 0).sends.push(Send {
            to: Id(2),
            level: Level(0.0),
            pre_fader: false,
        });
        assert!(has_loop(&p));
        // Groups inside each other.
        let mut p = project(&[TrackKind::Group, TrackKind::Group]);
        track(&mut p, 0).parent = Some(Id(2));
        track(&mut p, 1).parent = Some(Id(1));
        assert!(has_loop(&p));
        // A track sending to a return: fine.
        let mut p = project(&[TrackKind::Return, TrackKind::Audio]);
        track(&mut p, 1).sends.push(Send {
            to: Id(1),
            level: Level(-6.0),
            pre_fader: true,
        });
        assert!(!has_loop(&p));
    }
}
