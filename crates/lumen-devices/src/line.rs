//! Reading a delay line between two frames (M8 slice B3, the parent's review).

/// The frame `behind` frames before `write` in `line`, read linearly between
/// the two frames it falls between.
///
/// The whole frames and the fraction are split before any arithmetic with
/// `write`, so the answer depends only on the line's contents and `behind`,
/// never on how far the write point has travelled. Subtracting `behind` from
/// `write as f32` instead rounds the fraction to the write point's float grid:
/// a sixteenth of a frame at 4 s of 192 kHz, and a different fraction for a
/// render that started with a lead-in than for playback that did not.
pub(crate) fn read_behind(line: &[f32], write: usize, behind: f32) -> f32 {
    let len = line.len();
    let behind = behind.max(0.0);
    let whole = behind.floor();
    let frac = behind - whole;
    let whole = (whole as usize) % len;
    let newer = (write % len + len - whole) % len;
    let older = (newer + len - 1) % len;
    let a = line[newer];
    a + (line[older] - a) * frac
}

#[cfg(test)]
mod tests {
    use super::read_behind;

    /// The same contents read the same at any write point: the fraction never
    /// meets the write index's float grid.
    #[test]
    fn a_read_does_not_depend_on_where_the_write_point_is() {
        const LEN: usize = 800_000;
        let pattern = |k: usize| ((k as f32) * 0.37).sin();
        for write in [10, 5_000, 700_000, LEN - 1] {
            // The frame written `k` frames ago holds pattern(k).
            let mut line = vec![0.0f32; LEN];
            for k in 0..64 {
                line[(write + LEN - k) % LEN] = pattern(k);
            }
            for behind in [0.0f32, 0.25, 1.5, 7.0625, 31.999] {
                let whole = behind.floor() as usize;
                let frac = behind - behind.floor();
                let want = pattern(whole) + (pattern(whole + 1) - pattern(whole)) * frac;
                assert_eq!(
                    read_behind(&line, write, behind).to_bits(),
                    want.to_bits(),
                    "write {write}, behind {behind}"
                );
            }
        }
    }
}
