//! Where a MIDI clip's notes sit on the arrangement (M5 slice 3a).
//!
//! A note's start is in **clip time**: ticks from the clip's start. MIDI
//! clips keep no hidden content offset (trimming the start or slipping moves
//! the notes themselves, `edit::trim` and `edit::slip`), so the one thing
//! between clip time and the arrangement is looping: with a `loop_length`,
//! the clip's content is that long and repeats from the clip's start for as
//! long as the clip lasts; without one, the content is the clip.
//!
//! The piano roll edits one content, and shows its repeats beside it.

use lumen_core::time::Ticks;

use crate::model::Clip;

/// How long the clip's content is: its loop length when it loops, otherwise
/// the clip. Notes live in `[0, content_length)`.
pub fn content_length(clip: &Clip) -> Ticks {
    clip.loop_length.filter(|l| l.0 > 0).unwrap_or(clip.length)
}

/// Where the arrangement time `t` falls in the clip's content, with the
/// repeat it is in (0 for the first), or `None` outside the clip.
pub fn to_clip_time(clip: &Clip, t: Ticks) -> Option<(Ticks, u32)> {
    let local = t - clip.start;
    if local < Ticks::ZERO || local >= clip.length {
        return None;
    }
    let content = content_length(clip);
    Some((
        Ticks(local.0 % content.0),
        u32::try_from(local.0 / content.0).unwrap_or(u32::MAX),
    ))
}

/// The arrangement time of clip time `local` in repeat `repeat` (0 is the
/// clip's own content).
pub fn to_arrangement(clip: &Clip, local: Ticks, repeat: u32) -> Ticks {
    clip.start + Ticks(content_length(clip).0 * i64::from(repeat)) + local
}

/// How many repeats show in the clip: 1 for one that does not loop, counting
/// a partial last repeat.
pub fn repeats(clip: &Clip) -> u32 {
    let content = content_length(clip).0.max(1);
    u32::try_from((clip.length.0 + content - 1) / content)
        .unwrap_or(u32::MAX)
        .max(1)
}

/// How much of repeat `repeat`'s content shows before the clip ends (all of
/// it but in the last repeat).
pub fn shown(clip: &Clip, repeat: u32) -> Ticks {
    let content = content_length(clip);
    let used = Ticks(content.0 * i64::from(repeat));
    (clip.length - used).clamp(Ticks::ZERO, content)
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
            fade_out: Default::default(),
            loop_length: looping.map(Ticks::quarters),
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
        // The round trip.
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
