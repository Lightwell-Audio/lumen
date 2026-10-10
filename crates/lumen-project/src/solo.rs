//! Who is heard: mute and solo across groups, returns and the master (M4
//! slice 4). The owner agreed these rules as a table of cases before they
//! were built (`docs/milestones/M4.md` §5); the tests below are that table.
//!
//! A track's *path* is the track, the tracks that play into it, and the
//! tracks it plays out through (its groups, a return it outputs to, the
//! master).
//! - Nothing soloed: everything not muted is heard.
//! - While something is soloed, only the soloed tracks' paths are heard.
//!   Returns are solo-safe (they stay open while tracks are soloed) unless
//!   a return is soloed itself; the master is never silenced by a solo.
//! - A soloed return plays wet only: the sends into it stay on while the
//!   sending tracks' own outputs go silent.
//! - Mute beats solo along the whole path: a muted track silences its
//!   output and every send, and so does a muted group or return it plays
//!   through.

use std::collections::{HashMap, HashSet};

use crate::model::{Id, Project, Track, TrackKind};
use crate::routing::{Destination, all_tracks, destination};

/// What one track lets out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hearing {
    /// Its output is heard.
    pub output: bool,
    /// Its sends are heard (they can be while its output is not: a soloed
    /// return plays wet only).
    pub sends: bool,
    /// Its output is silent although the track is not muted itself:
    /// another track's solo, or a muted group it plays through (shown as a
    /// hollow M).
    pub silenced: bool,
    /// Not soloed itself, but heard because something it plays into is
    /// (a track in a soloed group; shown as a hollow S).
    pub soloed_through: bool,
}

impl Hearing {
    const OPEN: Self = Self {
        output: true,
        sends: true,
        silenced: false,
        soloed_through: false,
    };
}

/// What every track in a project lets out.
#[derive(Debug, Clone, Default)]
pub struct Heard(HashMap<Id, Hearing>);

impl Heard {
    /// The track's hearing; a track the project does not have is open.
    pub fn of(&self, id: Id) -> Hearing {
        self.0.get(&id).copied().unwrap_or(Hearing::OPEN)
    }
}

/// Whether any track (never the master) is soloed.
pub fn any_soloed(project: &Project) -> bool {
    project.tracks.iter().any(|t| t.solo)
}

/// Applies the rules to every track and the master.
pub fn hearing(project: &Project) -> Heard {
    let tracks: Vec<&Track> = all_tracks(project).collect();
    let by_id: HashMap<Id, &Track> = tracks.iter().map(|t| (t.id, *t)).collect();
    // The tracks each track plays out through, in order. Validation refuses
    // loops; the walk stops after every track anyway.
    let chain = |track: &Track| {
        let mut out = Vec::new();
        let mut at = destination(project, track);
        while let Destination::Track(id) = at {
            let Some(next) = by_id.get(&id) else { break };
            if out.len() > tracks.len() {
                break;
            }
            out.push(id);
            at = destination(project, next);
        }
        out
    };
    let chains: HashMap<Id, Vec<Id>> = tracks.iter().map(|t| (t.id, chain(t))).collect();
    let soloed: HashSet<Id> = tracks
        .iter()
        .filter(|t| t.solo && t.kind != TrackKind::Master)
        .map(|t| t.id)
        .collect();
    let muted = |id: Id| by_id.get(&id).is_some_and(|t| t.mute);
    let is_return = |id: Id| by_id.get(&id).is_some_and(|t| t.kind == TrackKind::Return);

    // Downstream of a soloed track: open so the solo reaches the device.
    let downstream: HashSet<Id> = soloed
        .iter()
        .flat_map(|id| chains[id].iter().copied())
        .collect();
    let a_return_soloed = soloed.iter().any(|&id| is_return(id));
    // Tracks sending into a soloed return: their sends stay on.
    let wet: HashSet<Id> = tracks
        .iter()
        .filter(|t| t.sends.iter().any(|s| soloed.contains(&s.to)))
        .map(|t| t.id)
        .collect();

    let mut heard = HashMap::with_capacity(tracks.len());
    for track in &tracks {
        let id = track.id;
        let through = &chains[&id];
        let muted_path = track.mute || through.iter().any(|&t| muted(t));
        let into_solo = through.iter().any(|t| soloed.contains(t));
        let (output, sends) = if soloed.is_empty() {
            (true, true)
        } else {
            let safe = track.kind == TrackKind::Master
                || (track.kind == TrackKind::Return && !a_return_soloed);
            let on_path = soloed.contains(&id) || into_solo || downstream.contains(&id);
            // Its sound reaches a track whose sends feed a soloed return.
            let feeds_wet = through.iter().any(|t| wet.contains(t));
            let output = on_path || safe || feeds_wet;
            (output, output || wet.contains(&id))
        };
        heard.insert(
            id,
            Hearing {
                output: output && !muted_path,
                sends: sends && !muted_path,
                silenced: !track.mute && !(output && !muted_path),
                soloed_through: !track.solo && into_solo,
            },
        );
    }
    Heard(heard)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{Color, Level, Output, Send};
    use crate::testing::small_project;

    /// Drums (group: Kick, Snare), Bass, Vocal, returns A and B.
    struct Setup {
        p: Project,
    }

    const DRUMS: Id = Id(1);
    const KICK: Id = Id(2);
    const SNARE: Id = Id(3);
    const BASS: Id = Id(4);
    const VOCAL: Id = Id(5);
    const A: Id = Id(6);
    const B: Id = Id(7);

    impl Setup {
        fn new() -> Self {
            let mut p = small_project(1);
            let kinds = [
                TrackKind::Group,
                TrackKind::Audio,
                TrackKind::Audio,
                TrackKind::Midi,
                TrackKind::Audio,
                TrackKind::Return,
                TrackKind::Return,
            ];
            p.tracks = kinds
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
            let mut s = Self { p };
            s.track(KICK).parent = Some(DRUMS);
            s.track(SNARE).parent = Some(DRUMS);
            s.send(SNARE, A);
            s.send(VOCAL, A);
            s.send(BASS, B);
            s
        }

        fn track(&mut self, id: Id) -> &mut Track {
            if id == self.p.master.id {
                return Arc::make_mut(&mut self.p.master);
            }
            let t = self.p.tracks.iter_mut().find(|t| t.id == id).unwrap();
            Arc::make_mut(t)
        }

        fn send(&mut self, from: Id, to: Id) {
            self.track(from).sends.push(Send {
                to,
                level: Level(-6.0),
                pre_fader: false,
            });
        }

        fn solo(mut self, ids: &[Id]) -> Self {
            for &id in ids {
                self.track(id).solo = true;
            }
            self
        }

        fn mute(mut self, ids: &[Id]) -> Self {
            for &id in ids {
                self.track(id).mute = true;
            }
            self
        }

        /// The tracks whose output is heard, in id order.
        fn out(&self) -> Vec<Id> {
            let heard = hearing(&self.p);
            let mut ids: Vec<Id> = all_tracks(&self.p)
                .map(|t| t.id)
                .filter(|&id| heard.of(id).output)
                .collect();
            ids.sort();
            ids
        }

        fn of(&self, id: Id) -> Hearing {
            hearing(&self.p).of(id)
        }
    }

    fn with_master(p: &Project, ids: &[Id]) -> Vec<Id> {
        let mut v = ids.to_vec();
        v.push(p.master.id);
        v.sort();
        v
    }

    #[test]
    fn case_01_nothing_soloed_everything_unmuted_is_heard() {
        let s = Setup::new().mute(&[BASS]);
        let all = [DRUMS, KICK, SNARE, VOCAL, A, B];
        assert_eq!(s.out(), with_master(&s.p, &all));
        let bass = s.of(BASS);
        assert!(!bass.output && !bass.sends, "mute silences every send");
        assert!(!bass.silenced, "muted by itself, not silenced");
    }

    #[test]
    fn case_02_a_solo_keeps_its_path_and_the_returns() {
        let s = Setup::new().solo(&[VOCAL]);
        assert_eq!(s.out(), with_master(&s.p, &[VOCAL, A, B]));
        assert!(s.of(VOCAL).sends, "its send to A is heard");
        let snare = s.of(SNARE);
        assert!(
            !snare.output && !snare.sends,
            "another track's send is silent"
        );
        assert!(snare.silenced && s.of(BASS).silenced);
    }

    #[test]
    fn case_03_ctrl_click_adds_a_second_path() {
        let s = Setup::new().solo(&[VOCAL, BASS]);
        assert_eq!(s.out(), with_master(&s.p, &[BASS, VOCAL, A, B]));
    }

    #[test]
    fn case_04_a_track_in_a_group_keeps_its_group_open() {
        let s = Setup::new().solo(&[KICK]);
        assert_eq!(s.out(), with_master(&s.p, &[DRUMS, KICK, A, B]));
        assert!(s.of(SNARE).silenced, "its sibling is silent");
        assert!(!s.of(DRUMS).silenced);
    }

    #[test]
    fn case_05_a_soloed_group_plays_everything_inside_it() {
        let s = Setup::new().solo(&[DRUMS]);
        assert_eq!(s.out(), with_master(&s.p, &[DRUMS, KICK, SNARE, A, B]));
        assert!(s.of(KICK).soloed_through && s.of(SNARE).soloed_through);
        assert!(!s.of(DRUMS).soloed_through, "soloed itself");
        assert!(s.of(SNARE).sends, "the snare's send to A is heard");
    }

    #[test]
    fn case_06_mute_beats_a_group_solo() {
        let s = Setup::new().solo(&[DRUMS]).mute(&[KICK]);
        assert_eq!(s.out(), with_master(&s.p, &[DRUMS, SNARE, A, B]));
        assert!(
            !s.of(KICK).silenced,
            "muted itself: a lit M, not a hollow one"
        );
    }

    #[test]
    fn case_07_a_soloed_track_in_a_muted_group_is_silent() {
        let s = Setup::new().solo(&[SNARE]).mute(&[DRUMS]);
        let snare = s.of(SNARE);
        assert!(!snare.output && !snare.sends, "its send to A goes too");
        assert!(snare.silenced, "silent through its group's mute");
    }

    #[test]
    fn case_08_a_soloed_return_plays_wet_only() {
        let s = Setup::new().solo(&[A]);
        assert_eq!(s.out(), with_master(&s.p, &[A]), "only the return's output");
        let vocal = s.of(VOCAL);
        assert!(
            !vocal.output && vocal.sends,
            "the vocal's send feeds A, its output is silent"
        );
        let snare = s.of(SNARE);
        assert!(snare.sends, "the snare sends to A");
        assert!(!s.of(B).output, "the other return is not solo-safe now");
        assert!(!s.of(BASS).sends, "the bass sends to B only");
    }

    #[test]
    fn case_08b_a_group_sending_to_a_soloed_return_keeps_its_tracks_playing() {
        let mut s = Setup::new();
        s.send(DRUMS, B);
        let s = s.solo(&[B]);
        assert!(s.of(DRUMS).sends && !s.of(DRUMS).output);
        assert!(
            s.of(KICK).output,
            "the kick plays into the drums, which feed B"
        );
    }

    #[test]
    fn case_09_another_tracks_send_to_the_same_return_is_silent() {
        let s = Setup::new().solo(&[VOCAL]);
        assert!(s.of(A).output);
        assert!(!s.of(SNARE).sends);
    }

    #[test]
    fn case_10_a_muted_return_while_a_track_is_soloed() {
        let s = Setup::new().solo(&[VOCAL]).mute(&[A]);
        assert_eq!(s.out(), with_master(&s.p, &[VOCAL, B]));
        assert!(s.of(VOCAL).output, "the vocal is heard dry");
    }

    #[test]
    fn case_11_returns_are_solo_safe_for_tracks() {
        let s = Setup::new().solo(&[BASS]);
        assert!(s.of(A).output && s.of(B).output);
        assert!(!s.of(A).silenced);
    }

    #[test]
    fn case_12_a_soloed_track_out_to_a_device_pair() {
        let mut s = Setup::new();
        s.track(BASS).output = Output::Device(1);
        let s = s.solo(&[BASS]);
        assert!(s.of(BASS).output);
    }

    #[test]
    fn case_13_the_master_is_never_silenced_by_solo_but_its_mute_silences_all() {
        let s = Setup::new().solo(&[VOCAL]);
        assert!(s.of(s.p.master.id).output);
        let master = s.p.master.id;
        let mut s = Setup::new();
        s.track(master).solo = true; // ignored: the master cannot solo
        s.track(BASS).output = Output::Device(1);
        let s = s.mute(&[master]);
        assert_eq!(s.out(), vec![BASS], "only what bypasses the master");
        assert!(s.of(VOCAL).silenced);
    }

    #[test]
    fn soloed_through_marks_only_what_plays_into_a_solo() {
        let s = Setup::new().solo(&[KICK]);
        assert!(!s.of(DRUMS).soloed_through, "downstream is not hollow-S");
        assert!(!s.of(KICK).soloed_through);
        assert!(!any_soloed(&Setup::new().p));
        assert!(any_soloed(&s.p));
    }
}
