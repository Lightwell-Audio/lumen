use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_media::detect::{Analysis, CONFIDENT, Tempo};
use lumen_project::edit::warp::Change;
use lumen_project::model::{Clip, ClipContent, Id, WarpMode};

use crate::App;
use crate::app::track_freeze::FROZEN_TIMING;
use crate::audio_editor::detect::file_key;

const SNAP: f64 = 0.05;

pub(crate) type Found = Option<(u64, Arc<Analysis>, u8)>;

pub(crate) fn start_bpm(stated: Option<f64>, detected: Option<Tempo>, song: f64) -> f64 {
    stated
        .or_else(|| {
            detected
                .filter(|t| t.confidence >= CONFIDENT)
                .map(|t| t.bpm)
        })
        .unwrap_or(song)
        .clamp(20.0, 999.0)
}

impl App {
    fn warp_targets(&self) -> Vec<Arc<Clip>> {
        let opened = self.opened_audio();
        let project = self.document.project();
        let picked: Vec<_> = project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| {
                matches!(c.content, ClipContent::Audio { .. })
                    && self.arrangement.clips.contains(&c.id)
            })
            .cloned()
            .collect();
        if !picked.is_empty() || self.arrangement.clips.iter().next().is_some() {
            return picked;
        }
        project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| Some(c.id) == opened)
            .cloned()
            .collect()
    }

    pub(crate) fn warp_unavailable(&self) -> Result<(), &'static str> {
        if self.warp_targets().is_empty() {
            return Err("Select an audio clip first");
        }
        Ok(())
    }

    pub(crate) fn warped_unavailable(&self) -> Result<(), &'static str> {
        let targets = self.warp_targets();
        if targets.is_empty() {
            return Err("Select an audio clip first");
        }
        if targets.iter().all(|c| c.warp.is_none()) {
            return Err("Warp the clip first");
        }
        Ok(())
    }

    pub(crate) fn found_for(&self, clip: &Clip) -> Found {
        self.audio_editor.detect.found(clip)
    }

    fn stated_bpm(&self, clip: &Clip) -> Option<f64> {
        let ClipContent::Audio { media, .. } = &clip.content else {
            return None;
        };
        self.lanes.library.get(media.0)?.info.tempo.map(|t| t.bpm)
    }

    pub(crate) fn warp_start_bpm(&self, clip: &Clip, found: &Found) -> f64 {
        let Some(key) = file_key(clip) else {
            return 120.0;
        };
        let detected = found
            .as_ref()
            .filter(|(k, ..)| *k == key)
            .and_then(|(_, a, _)| a.tempo);
        let song = lumen_core::time::bpm_at(&self.document.project().tempo, clip.start);
        start_bpm(self.stated_bpm(clip), detected, song)
    }

    pub(crate) fn toggle_clip_warp(&mut self) {
        let found = self.found_for_targets(&self.warp_targets());
        self.toggle_clip_warp_with(&found);
    }

    pub(crate) fn toggle_clip_warp_with(&mut self, found: &Found) {
        let targets = self.warp_targets();
        let ids: Vec<Id> = targets.iter().map(|c| c.id).collect();
        if !targets.is_empty() && targets.iter().all(|c| c.warp.is_some()) {
            self.document.unwarp_clips(&ids);
            return;
        }
        let tempos: Vec<(Id, f64)> = targets
            .iter()
            .map(|c| (c.id, self.warp_start_bpm(c, found)))
            .collect();
        let start = |clip: &Clip| {
            tempos
                .iter()
                .find_map(|(id, bpm)| (*id == clip.id).then_some(*bpm))
                .unwrap_or(120.0)
        };
        let done = self.document.warp_clips_by(&ids, &start);
        if done.changed == 0 && done.refused > 0 {
            self.say("That clip's file is not in the project, so it cannot be warped".to_owned());
        }
    }

    fn found_for_targets(&self, targets: &[Arc<Clip>]) -> Found {
        let clip = targets.iter().find(|c| file_key(c).is_some())?;
        self.found_for(clip)
    }

    pub(crate) fn scale_clip_tempo(&mut self, factor: f64) {
        let ids: Vec<Id> = self.warp_targets().iter().map(|c| c.id).collect();
        let done = self.document.scale_warp_tempo(&ids, factor);
        if done.changed == 0 && done.refused > 0 {
            self.say(
                "That tempo is out of range, or would put the start of the clip outside its file"
                    .to_owned(),
            );
        }
    }

    pub(crate) fn set_clip_tempo(&mut self, clip: Id, bpm: f64) {
        let done = self
            .document
            .set_warp(&[clip], &Change::OriginalBpm(bpm), None);
        if done.changed == 0 && done.refused > 0 {
            self.say(
                "That tempo is out of range, or would put the start of the clip outside its file"
                    .to_owned(),
            );
        }
    }

    pub(crate) fn set_clip_transpose(&mut self, clip: Id, cents: i32) {
        let done =
            self.document
                .set_warp(&[clip], &Change::Transpose(cents), Some("warp transpose"));
        if done.changed == 0 && done.refused > 0 {
            self.say("That transpose is out of range".to_owned());
        }
    }

    pub(crate) fn set_clip_warp_mode(&mut self, clip: Id, mode: WarpMode) {
        if self.clip(clip).is_some_and(|c| c.locked) {
            self.say("The clip is locked, so its warp mode was not changed".to_owned());
            return;
        }
        self.document.set_warp(&[clip], &Change::Mode(mode), None);
    }

    pub(crate) fn transpose_unavailable(&self) -> Result<(), &'static str> {
        let targets = self.warp_targets();
        if targets.is_empty() {
            return Err("Select an audio clip first");
        }
        if targets.iter().all(|c| c.warp.is_none()) {
            return Err("Warp the clip first");
        }
        if !targets
            .iter()
            .filter_map(|c| c.warp.as_ref())
            .any(|w| w.mode.transposes())
        {
            return Err("Repitch plays at the tempo's pitch: pick Melodic, Full mix or Rhythmic");
        }
        Ok(())
    }

    pub(crate) fn transpose_clips_command(&mut self, semitones: i32) {
        let ids: Vec<Id> = self
            .warp_targets()
            .iter()
            .filter(|c| c.warp.as_ref().is_some_and(|w| w.mode.transposes()))
            .map(|c| c.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let done = self.document.transpose_warp(&ids, semitones * 100);
        if done.changed == 0 {
            self.say("That is as far as a clip can be transposed".to_owned());
        }
    }

    pub(crate) fn set_tempo_from_clip(&mut self) {
        let Some(clip) = self.warp_targets().into_iter().find(|c| c.warp.is_some()) else {
            return;
        };
        let Some(bpm) = clip.warp.as_ref().map(|w| w.original_bpm) else {
            return;
        };
        if self
            .document
            .project()
            .tracks
            .iter()
            .any(|t| t.frozen.is_some())
        {
            self.say(FROZEN_TIMING.to_owned());
            return;
        }
        self.document.set_tempo_at(clip.start, bpm, None);
        self.say(format!("The song's tempo is now {bpm:.1} BPM"));
    }

    pub(crate) fn warp_from_here_command(&mut self) {
        let Some(clip) = self.warp_targets().into_iter().next() else {
            return;
        };
        let found = self.found_for(&clip);
        let cursor = (self.audio_editor.framed_for == Some(clip.id))
            .then_some(self.audio_editor.cursor)
            .flatten();
        self.warp_from_here(&clip, cursor, &found);
    }

    pub(crate) fn warp_from_here(&mut self, clip: &Clip, cursor: Option<Ticks>, found: &Found) {
        let here = cursor.unwrap_or_else(|| self.transport.playhead());
        let bar = self.transport.timeline.meter.nearest_bar_start(here);
        let bpm = self.warp_start_bpm(clip, found);
        let Some(key) = file_key(clip) else {
            return;
        };
        let snap = |seconds: f64| {
            let (_, analysis, sensitivity) = found.as_ref().filter(|(k, ..)| *k == key)?;
            analysis
                .nearest(seconds, *sensitivity, SNAP)
                .map(|t| t.seconds)
        };
        let done = self
            .document
            .warp_from_here(clip.id, bpm, (here, bar), &snap);
        if done.changed == 0 && done.refused > 0 {
            self.say(
                "There is not enough of the file to move it that far: the clip would run past its start or end"
                    .to_owned(),
            );
        } else if done.changed == 0 && !clip.locked {
            self.say("That sound is already on the bar line".to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo(bpm: f64, confidence: f32) -> Tempo {
        Tempo {
            bpm,
            confidence,
            beats: None,
        }
    }

    #[test]
    fn warp_starts_from_the_stated_tempo_then_a_confident_detection_then_the_song() {
        let strong = Some(tempo(93.0, 0.9));
        assert_eq!(start_bpm(Some(88.0), strong, 120.0), 88.0);
        assert_eq!(start_bpm(None, strong, 120.0), 93.0);
        assert_eq!(start_bpm(None, Some(tempo(93.0, 0.2)), 120.0), 120.0);
        assert_eq!(start_bpm(None, None, 120.0), 120.0);
        assert_eq!(start_bpm(Some(5.0), None, 120.0), 20.0, "kept in range");
    }
}
