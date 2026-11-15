use std::sync::Arc;

use lumen_core::pan::PanLaw;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Level, Output, Project, Send, Track, TrackKind};

use super::Document;

pub const MAX_RETURNS: usize = 12;

pub const SEND_MAX_DB: f32 = 6.0;
pub const TRIM_RANGE_DB: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Invalid,
    Loop,
    TooDeep,
    TooManyReturns,
}

impl Refused {
    pub fn reason(self) -> &'static str {
        match self {
            Refused::Invalid => "That cannot go there",
            Refused::Loop => "That would feed the audio back into itself",
            Refused::TooDeep => "Groups go three deep at most",
            Refused::TooManyReturns => "A project holds 12 return tracks at most",
        }
    }
}

impl Document {
    pub fn set_output(&mut self, id: Id, output: Output) -> Result<(), Refused> {
        let project = self.project();
        let track = find(project, id).ok_or(Refused::Invalid)?;
        let master = track.kind == TrackKind::Master;
        let fits = match output {
            Output::Auto | Output::Device(_) => true,
            Output::Master => !master,
            Output::Track(to) => {
                !master
                    && to != id
                    && project.tracks.iter().any(|t| {
                        t.id == to && matches!(t.kind, TrackKind::Group | TrackKind::Return)
                    })
            }
        };
        if !fits {
            return Err(Refused::Invalid);
        }
        self.routed("Change output", None, id, |t| t.output = output)
    }

    pub fn set_send(&mut self, id: Id, to: Id, level: Level) -> Result<(), Refused> {
        self.check_send(id, to)?;
        let level = if level.is_silent() || level.0.is_nan() {
            Level::SILENT
        } else {
            Level(level.0.min(SEND_MAX_DB))
        };
        let merge = format!("send {:x} {:x}", id.0, to.0);
        self.moved(id, Target::Send { to });
        self.routed("Change send", Some(&merge), id, |t| {
            match t.sends.iter_mut().find(|s| s.to == to) {
                Some(send) => send.level = level,
                None => t.sends.push(Send {
                    to,
                    level,
                    pre_fader: false,
                }),
            }
        })
    }

    pub fn set_send_pre_fader(&mut self, id: Id, to: Id, pre_fader: bool) -> Result<(), Refused> {
        self.check_send(id, to)?;
        let label = if pre_fader {
            "Send before the fader"
        } else {
            "Send after the fader"
        };
        self.routed(label, None, id, |t| {
            match t.sends.iter_mut().find(|s| s.to == to) {
                Some(send) => send.pre_fader = pre_fader,
                None => t.sends.push(Send {
                    to,
                    level: Level::SILENT,
                    pre_fader,
                }),
            }
        })
    }

    pub fn set_invert(&mut self, ids: &[Id], invert: bool) {
        let label = if invert {
            "Invert polarity"
        } else {
            "Restore polarity"
        };
        self.history.apply(label, None, |project| {
            for track in tracks_mut(project) {
                if ids.contains(&track.id) && track.invert != invert {
                    Arc::make_mut(track).invert = invert;
                }
            }
        });
    }

    pub fn set_trim(&mut self, id: Id, db: f32) {
        if !db.is_finite() {
            return;
        }
        let db = db.clamp(-TRIM_RANGE_DB, TRIM_RANGE_DB);
        self.moved(id, Target::Trim);
        let merge = format!("trim {:x}", id.0);
        let _ = self.routed("Change input trim", Some(&merge), id, |t| t.trim_db = db);
    }

    pub fn set_pan_law(&mut self, law: PanLaw) {
        if self.project().pan_law == law {
            return;
        }
        self.history
            .apply("Change pan law", None, |project| project.pan_law = law);
    }

    fn check_send(&self, id: Id, to: Id) -> Result<(), Refused> {
        let project = self.project();
        let from = find(project, id).ok_or(Refused::Invalid)?;
        let to_return = project
            .tracks
            .iter()
            .any(|t| t.id == to && t.kind == TrackKind::Return);
        if from.kind == TrackKind::Master || to == id || !to_return {
            return Err(Refused::Invalid);
        }
        Ok(())
    }

    fn routed(
        &mut self,
        label: &str,
        merge: Option<&str>,
        id: Id,
        change: impl Fn(&mut Track),
    ) -> Result<(), Refused> {
        let mut trial = self.project().clone();
        let track = tracks_mut(&mut trial)
            .find(|t| t.id == id)
            .ok_or(Refused::Invalid)?;
        change(Arc::make_mut(track));
        if lumen_project::routing::has_loop(&trial) {
            return Err(Refused::Loop);
        }
        self.history.apply(label, merge, |project| {
            if let Some(track) = tracks_mut(project).find(|t| t.id == id) {
                change(Arc::make_mut(track));
            }
        });
        Ok(())
    }
}

pub(super) fn find(project: &Project, id: Id) -> Option<&Track> {
    lumen_project::routing::all_tracks(project).find(|t| t.id == id)
}

pub(super) fn tracks_mut(project: &mut Project) -> impl Iterator<Item = &mut Arc<Track>> {
    project
        .tracks
        .iter_mut()
        .chain(std::iter::once(&mut project.master))
}

pub(super) fn forget_routes_to(project: &mut Project, gone: &[Id]) {
    for track in tracks_mut(project) {
        let output = matches!(track.output, Output::Track(to) if gone.contains(&to));
        let sends = track.sends.iter().any(|s| gone.contains(&s.to));
        if output || sends {
            let track = Arc::make_mut(track);
            if output {
                track.output = Output::Auto;
            }
            track.sends.retain(|s| !gone.contains(&s.to));
            track.automation.retain(
                |l| !matches!(l.target, lumen_project::automation::Target::Send { to } if gone.contains(&to)),
            );
        }
    }
    for map in &mut project.drum_maps {
        for sound in &mut map.sounds {
            if matches!(sound.route, lumen_project::model::Route::Track(to) if gone.contains(&to)) {
                sound.route = lumen_project::model::Route::Main;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(kinds: &[TrackKind]) -> (Document, Vec<Id>) {
        let mut doc = Document::new(crate::empty_project());
        for &kind in kinds {
            doc.add_track(kind, None);
        }
        let ids = doc.project().tracks.iter().map(|t| t.id).collect();
        (doc, ids)
    }

    fn track(doc: &Document, id: Id) -> &Track {
        find(doc.project(), id).unwrap()
    }

    #[test]
    fn outputs_go_to_groups_returns_the_master_or_a_device_pair_and_undo() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Return, TrackKind::Audio]);
        let audio = ids[2];
        for output in [
            Output::Track(ids[0]),
            Output::Track(ids[1]),
            Output::Master,
            Output::Device(3),
        ] {
            assert_eq!(doc.set_output(audio, output), Ok(()));
            assert_eq!(track(&doc, audio).output, output);
        }
        doc.undo();
        assert_eq!(track(&doc, audio).output, Output::Master);
        assert_eq!(doc.history().undo_label(), Some("Change output"));
        assert_eq!(
            doc.set_output(audio, Output::Track(audio)),
            Err(Refused::Invalid)
        );
        assert_eq!(
            doc.set_output(ids[0], Output::Track(audio)),
            Err(Refused::Invalid)
        );
        assert_eq!(
            doc.set_output(Id(999), Output::Master),
            Err(Refused::Invalid)
        );
        let master = doc.project().master.id;
        assert_eq!(doc.set_output(master, Output::Device(1)), Ok(()));
        assert_eq!(
            doc.set_output(master, Output::Master),
            Err(Refused::Invalid)
        );
    }

    #[test]
    fn edits_that_would_feed_back_are_refused_and_change_nothing() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Return]);
        let (group, ret) = (ids[0], ids[1]);
        let before = doc.state();
        assert_eq!(doc.set_output(ret, Output::Track(group)), Ok(()));
        assert_eq!(doc.set_send(group, ret, Level(0.0)), Err(Refused::Loop));
        assert_eq!(doc.set_send_pre_fader(group, ret, true), Err(Refused::Loop));
        assert!(track(&doc, group).sends.is_empty());
        doc.undo();
        assert_eq!(doc.state(), before);
    }

    #[test]
    fn sends_start_post_fader_and_a_drag_is_one_step() {
        let (mut doc, ids) = doc_with(&[TrackKind::Return, TrackKind::Audio]);
        let (ret, audio) = (ids[0], ids[1]);
        for db in [-30.0, -12.0, -6.0, 9.0] {
            doc.set_send(audio, ret, Level(db)).unwrap();
        }
        let send = track(&doc, audio).send_to(ret).copied().unwrap();
        assert_eq!((send.level, send.pre_fader), (Level(SEND_MAX_DB), false));
        doc.seal();
        doc.set_send_pre_fader(audio, ret, true).unwrap();
        assert!(track(&doc, audio).send_to(ret).unwrap().pre_fader);
        doc.undo();
        doc.undo();
        assert!(track(&doc, audio).sends.is_empty(), "the whole drag undone");
        assert_eq!(doc.set_send(ret, audio, Level(0.0)), Err(Refused::Invalid));
        let master = doc.project().master.id;
        assert_eq!(doc.set_send(master, ret, Level(0.0)), Err(Refused::Invalid));
    }

    #[test]
    fn deleting_a_return_takes_its_sends_and_undo_brings_them_back() {
        let (mut doc, ids) = doc_with(&[TrackKind::Return, TrackKind::Audio, TrackKind::Audio]);
        let ret = ids[0];
        doc.set_send(ids[1], ret, Level(-6.0)).unwrap();
        doc.set_output(ids[2], Output::Track(ret)).unwrap();
        doc.delete_tracks(&[ret]);
        assert!(track(&doc, ids[1]).sends.is_empty());
        assert_eq!(track(&doc, ids[2]).output, Output::Auto);
        lumen_project::validate(doc.project()).unwrap();
        doc.undo();
        assert_eq!(track(&doc, ids[1]).send_to(ret).unwrap().level, Level(-6.0));
        assert_eq!(track(&doc, ids[2]).output, Output::Track(ret));
    }

    #[test]
    fn polarity_trim_and_the_pan_law_are_steps_of_their_own() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let master = doc.project().master.id;
        doc.set_invert(&[ids[0], master], true);
        assert!(track(&doc, ids[0]).invert && track(&doc, master).invert);
        for db in [3.0, 12.0, 40.0] {
            doc.set_trim(ids[0], db);
        }
        assert_eq!(track(&doc, ids[0]).trim_db, TRIM_RANGE_DB);
        doc.seal();
        doc.set_pan_law(PanLaw::Linear);
        assert_eq!(doc.project().pan_law, PanLaw::Linear);
        doc.undo();
        doc.undo();
        assert_eq!(track(&doc, ids[0]).trim_db, 0.0);
        assert_eq!(doc.project().pan_law, PanLaw::ConstantPower);
    }

    #[test]
    fn a_project_holds_twelve_returns_at_most() {
        let mut doc = Document::new(crate::empty_project());
        let first = doc.add_track(TrackKind::Return, None);
        for _ in 1..MAX_RETURNS {
            doc.add_track(TrackKind::Return, None);
        }
        assert!(doc.returns_full());
        let tracks = doc.project().tracks.len();
        doc.add_track(TrackKind::Return, None);
        assert_eq!(
            doc.project().tracks.len(),
            tracks,
            "a thirteenth is refused"
        );
        assert!(doc.duplicate_tracks(&[first]).is_empty(), "nor duplicated");
        assert_eq!(doc.project().tracks.len(), tracks);
        let audio = doc.add_track(TrackKind::Audio, None);
        assert_eq!(
            doc.duplicate_tracks(&[first, audio]).len(),
            1,
            "the audio track is"
        );
    }
}
