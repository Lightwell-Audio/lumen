use lumen_core::time::{TempoMap, Ticks};

use crate::model::{Clip, ClipContent, Project};

use super::{in_window, shift_events, shift_notes};

/// The part of `clip` inside `[a, b)`, or `None` if they do not meet. It
/// keeps `clip`'s id; give it a new one if both parts stay.
pub fn slice(
    _project: &Project,
    tempo: &TempoMap,
    clip: &Clip,
    a: Ticks,
    b: Ticks,
) -> Option<Clip> {
    let start = clip.start.max(a);
    let end = (clip.start + clip.length).min(b);
    if start >= end {
        return None;
    }

    let cut = start - clip.start;
    let tail = clip.start + clip.length - end;
    let mut part = clip.clone();
    part.start = start;
    part.length = end - start;

    let looping = clip.loop_length.is_some_and(|period| period.0 > 0);
    if cut > Ticks::ZERO && !looping {
        // The other passes of a loop recording move with the one that plays.
        let seconds = tempo.seconds_at(start) - tempo.seconds_at(clip.start);
        super::takes::advance(&mut part, cut, seconds, None, &|at| in_window(clip, at));
    }
    if looping {
        part.loop_phase = Ticks(clip.loop_phase.0.saturating_add(cut.0));
    }

    match &mut part.content {
        ClipContent::Audio { .. } => {
            if cut > Ticks::ZERO && !looping {
                part.audio_phase_seconds += tempo.seconds_at(start) - tempo.seconds_at(clip.start);
                // Keep the integer media anchor unchanged. The app converts
                // this source-time phase at the active device rate.
            }
        }
        ClipContent::Midi { notes, events } => {
            if !looping {
                for note in notes.iter_mut() {
                    if in_window(clip, note.start()) {
                        *note = note.with_carries_on(true);
                    }
                }
                shift_notes(notes, cut);
                shift_events(events, cut);
            }
        }
    }

    // Loop envelopes remain in canonical loop-local coordinates. The engine
    // uses loop_phase when reading them; shifting them here would apply the
    // phase twice.
    if !looping {
        part.envelopes.shift(Ticks::ZERO - cut);
    }

    if cut > Ticks::ZERO {
        part.fade_in = crop_fade_start(part.fade_in, cut);
    }
    if tail > Ticks::ZERO {
        part.fade_out = crop_fade_end(part.fade_out, tail);
    }
    part.fade_in = crop_fade_visible(part.fade_in, part.length);
    part.fade_out = crop_fade_visible(part.fade_out, part.length - part.fade_in.length);
    Some(part)
}

pub(super) fn crop_fade_start(
    mut fade: lumen_core::fade::Fade,
    removed: Ticks,
) -> lumen_core::fade::Fade {
    if removed <= Ticks::ZERO || fade.length <= Ticks::ZERO {
        return fade;
    }
    let visible = (fade.length - removed).max(Ticks::ZERO);
    if visible < fade.length {
        if visible == Ticks::ZERO {
            return lumen_core::fade::Fade::default();
        }
        fade.span = if fade.span == Ticks::ZERO {
            fade.length
        } else {
            fade.span
        };
        fade.offset = fade.offset + removed.min(fade.length);
        fade.length = visible;
    }
    fade
}

pub(super) fn crop_fade_end(
    mut fade: lumen_core::fade::Fade,
    removed: Ticks,
) -> lumen_core::fade::Fade {
    if removed <= Ticks::ZERO || fade.length <= Ticks::ZERO {
        return fade;
    }
    let visible = (fade.length - removed).max(Ticks::ZERO);
    if visible < fade.length {
        if visible == Ticks::ZERO {
            return lumen_core::fade::Fade::default();
        }
        fade.span = if fade.span == Ticks::ZERO {
            fade.length
        } else {
            fade.span
        };
        fade.offset = fade.offset + removed.min(fade.length);
        fade.length = visible;
    }
    fade
}

fn crop_fade_visible(mut fade: lumen_core::fade::Fade, visible: Ticks) -> lumen_core::fade::Fade {
    if fade.length > visible {
        if visible == Ticks::ZERO {
            return lumen_core::fade::Fade::default();
        }
        fade.span = if fade.span == Ticks::ZERO {
            fade.length
        } else {
            fade.span
        };
        fade.length = visible;
    }
    fade
}
