use lumen_core::time::Ticks;

use crate::model::Clip;

pub fn content_length(clip: &Clip) -> Ticks {
    clip.loop_length.filter(|l| l.0 > 0).unwrap_or(clip.length)
}

pub fn in_window(clip: &Clip, start: Ticks) -> bool {
    if let Some(period) = clip.loop_length.filter(|p| p.0 > 0) {
        let phase = clip.loop_phase.0.rem_euclid(period.0);
        start.0 >= 0 && start < period && (start.0 - phase).rem_euclid(period.0) < clip.length.0
    } else {
        start >= Ticks::ZERO && start < clip.length
    }
}

pub fn shown_notes(clip: &Clip) -> Vec<usize> {
    match &clip.content {
        crate::model::ClipContent::Midi { notes, .. } => (0..notes.len())
            .filter(|&i| in_window(clip, notes[i].start()))
            .collect(),
        crate::model::ClipContent::Audio { .. } => Vec::new(),
    }
}

pub fn to_clip_time(clip: &Clip, t: Ticks) -> Option<(Ticks, u32)> {
    let local = t - clip.start;
    if local < Ticks::ZERO || local >= clip.length {
        return None;
    }
    let content = content_length(clip);
    let phase = Ticks(clip.loop_phase.0.rem_euclid(content.0));
    let source = local + phase;
    Some((
        Ticks(source.0 % content.0),
        u32::try_from(source.0 / content.0).unwrap_or(u32::MAX),
    ))
}

pub fn to_arrangement(clip: &Clip, local: Ticks, repeat: u32) -> Ticks {
    let content = content_length(clip).0;
    let phase = clip.loop_phase.0.rem_euclid(content);
    let first = (local.0 - phase).rem_euclid(content);
    clip.start + Ticks(first + content * i64::from(repeat))
}

pub fn repeats(clip: &Clip) -> u32 {
    let content = content_length(clip).0.max(1);
    let phase = clip.loop_phase.0.rem_euclid(content);
    u32::try_from((clip.length.0 + phase + content - 1) / content)
        .unwrap_or(u32::MAX)
        .max(1)
}

pub fn shown(clip: &Clip, repeat: u32) -> Ticks {
    let content = content_length(clip);
    let phase = clip.loop_phase.0.rem_euclid(content.0.max(1));
    let used = if repeat == 0 {
        0
    } else {
        content.0 - phase + content.0 * (i64::from(repeat) - 1)
    };
    let visible = (clip.length.0 - used).clamp(0, content.0);
    Ticks(if repeat == 0 {
        visible.min(content.0 - phase)
    } else {
        visible
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClipContent;

    fn clip(start: i64, length: i64, looping: Option<i64>) -> Clip {
        Clip {
            id: crate::model::Id(1),
            name: String::new(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: looping.map(Ticks::quarters),
            loop_phase: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            warp: None,
            content: ClipContent::midi(),
        }
    }

    #[test]
    fn a_plain_clip_is_its_own_content() {
        let c = clip(8, 4, None);
        assert_eq!(content_length(&c), Ticks::quarters(4));
        assert_eq!(
            to_clip_time(&c, Ticks::quarters(9)),
            Some((Ticks::quarters(1), 0))
        );
        assert_eq!(to_clip_time(&c, Ticks::quarters(12)), None);
        assert_eq!(to_clip_time(&c, Ticks::quarters(7)), None);
        assert_eq!(repeats(&c), 1);
        assert_eq!(
            to_arrangement(&c, Ticks::quarters(1), 0),
            Ticks::quarters(9)
        );
    }

    #[test]
    fn a_short_phased_loop_exposes_the_source_notes_that_actually_start_in_its_window() {
        let mut c = clip(8, 1, Some(4));
        c.loop_phase = Ticks::quarters(2);
        assert!(in_window(&c, Ticks::quarters(2)));
        assert!(!in_window(&c, Ticks::ZERO));
        assert!(!in_window(&c, Ticks::quarters(3)));
        c.loop_phase = Ticks::quarters(3) + Ticks(Ticks::quarters(1).0 / 2);
        assert!(
            in_window(&c, Ticks::ZERO),
            "the visible window wraps into the next cycle"
        );
        assert!(!in_window(&c, Ticks::quarters(1)));
    }

    #[test]
    fn a_looping_clip_repeats_its_content() {
        let c = clip(8, 10, Some(4));
        assert_eq!(content_length(&c), Ticks::quarters(4));
        assert_eq!(
            to_clip_time(&c, Ticks::quarters(17)),
            Some((Ticks::quarters(1), 2))
        );
        assert_eq!(repeats(&c), 3);
        assert_eq!(shown(&c, 0), Ticks::quarters(4));
        assert_eq!(shown(&c, 2), Ticks::quarters(2), "the last repeat is cut");
        assert_eq!(
            to_arrangement(&c, Ticks::quarters(1), 2),
            Ticks::quarters(17)
        );
        let t = Ticks::quarters(13) + Ticks(5);
        let (local, repeat) = to_clip_time(&c, t).unwrap();
        assert_eq!(to_arrangement(&c, local, repeat), t);
    }

    #[test]
    fn a_phased_loop_starts_inside_the_source_repeat_and_round_trips() {
        let mut c = clip(8, 6, Some(4));
        c.loop_phase = Ticks::quarters(2);
        assert_eq!(
            to_clip_time(&c, Ticks::quarters(8)),
            Some((Ticks::quarters(2), 0))
        );
        assert_eq!(
            to_clip_time(&c, Ticks::quarters(10)),
            Some((Ticks::ZERO, 1))
        );
        assert_eq!(repeats(&c), 2);
        assert_eq!(shown(&c, 0), Ticks::quarters(2));
        assert_eq!(shown(&c, 1), Ticks::quarters(4));
        assert_eq!(
            to_arrangement(&c, Ticks::quarters(1), 0),
            Ticks::quarters(11),
            "source time before the phase appears after the first wrap"
        );
        let t = Ticks::quarters(13) + Ticks(5);
        let (local, repeat) = to_clip_time(&c, t).unwrap();
        assert_eq!(to_arrangement(&c, local, repeat), t);
    }

    #[test]
    fn a_zero_loop_length_is_no_loop() {
        let c = clip(0, 4, Some(0));
        assert_eq!(content_length(&c), Ticks::quarters(4));
        assert_eq!(repeats(&c), 1);
    }
}
