//! Zero crossings (M9.5 S2b, the owner's ZC A, D23): an edge placed by the
//! grid or the hand moves, within a few milliseconds, to the frame where the
//! louder channel is nearest zero, so a cut there jumps less. Stereo channels
//! rarely cross zero together, so this lowers the jump and cannot promise
//! none. Pure, so the search is tested; reading the frames is the editor's
//! (`app::audio_editor::crossing`).

/// How far either side of the candidate the search looks, in seconds.
pub(crate) const REACH_SECONDS: f64 = 0.005;

/// How loud frame `f` is for a cut: the larger of its two channels' sizes.
fn score(f: [i16; 2]) -> u16 {
    f[0].unsigned_abs().max(f[1].unsigned_abs())
}

/// The frame to cut at instead of `candidate`, from `frames` (the file's
/// samples from frame `first`), looking `reach` frames either side. The
/// lowest score wins, the nearest the candidate on a tie, and it must beat
/// the candidate's own score: `None` when nothing there is better (a silent
/// stretch included), or the candidate is not in `frames`.
pub(crate) fn best(frames: &[[i16; 2]], first: i64, candidate: i64, reach: i64) -> Option<i64> {
    let at = |frame: i64| {
        usize::try_from(frame - first)
            .ok()
            .and_then(|i| frames.get(i))
            .copied()
    };
    let own = score(at(candidate)?);
    let mut found: Option<(u16, i64, i64)> = None;
    for frame in candidate - reach..=candidate + reach {
        let Some(f) = at(frame) else {
            continue;
        };
        let s = score(f);
        let distance = (frame - candidate).abs();
        // Score, distance from the candidate, frame.
        let better = match found {
            None => true,
            Some((best, near, _)) => s < best || (s == best && distance < near),
        };
        if better {
            found = Some((s, distance, frame));
        }
    }
    found
        .filter(|(s, _, _)| *s < own)
        .map(|(_, _, frame)| frame)
}

/// The reach in frames at `rate`.
pub(crate) fn reach(rate: u32) -> i64 {
    (f64::from(rate) * REACH_SECONDS).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(values: &[(i16, i16)]) -> Vec<[i16; 2]> {
        values.iter().map(|(l, r)| [*l, *r]).collect()
    }

    #[test]
    fn opposite_polarity_stereo_is_scored_by_size() {
        // Left and right are mirror images: their sum is always zero, and
        // only their size says where the cut is quiet.
        let frames = wave(&[
            (900, -900),
            (400, -400),
            (-50, 50),
            (300, -300),
            (800, -800),
        ]);
        assert_eq!(best(&frames, 100, 104, 4), Some(102));
    }

    #[test]
    fn unequal_crossings_take_the_quieter_louder_channel() {
        // Frame 1 has left at zero and right loud; frame 3 has both quiet.
        let frames = wave(&[(500, 500), (0, 700), (300, 300), (40, -30), (600, 600)]);
        assert_eq!(best(&frames, 0, 0, 4), Some(3));
    }

    #[test]
    fn a_tie_goes_to_the_nearest_frame() {
        let frames = wave(&[(10, 10), (900, 900), (900, 900), (10, -10), (900, 900)]);
        // Frames 0 and 3 tie; 3 is one nearer to 2.
        assert_eq!(best(&frames, 0, 2, 2), Some(3));
    }

    #[test]
    fn silence_and_no_better_frame_leave_the_edge() {
        let silent = wave(&[(0, 0); 9]);
        assert_eq!(best(&silent, 0, 4, 4), None);
        let flat = wave(&[(200, 200); 9]);
        assert_eq!(best(&flat, 0, 4, 4), None);
        // The candidate is already the quietest.
        let dip = wave(&[(500, 500), (300, 300), (5, 5), (300, 300), (500, 500)]);
        assert_eq!(best(&dip, 0, 2, 2), None);
    }

    #[test]
    fn a_candidate_outside_the_frames_finds_nothing() {
        let frames = wave(&[(1, 1); 4]);
        assert_eq!(best(&frames, 10, 9, 2), None);
        assert_eq!(best(&frames, 10, 14, 2), None);
    }

    #[test]
    fn the_reach_is_five_milliseconds() {
        assert_eq!(reach(48_000), 240);
        assert_eq!(reach(44_100), 221);
    }
}
