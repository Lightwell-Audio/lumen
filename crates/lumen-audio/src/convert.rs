//! Conversion between Lumen's channel-major `f32` blocks and the interleaved
//! buffers that WASAPI and cpal use. Real-time safe: no allocation.

/// Interleaved sample formats a device buffer can hold (little-endian).
#[cfg_attr(not(windows), allow(dead_code))] // used by the WASAPI backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    /// 24-bit samples packed in 3 bytes.
    I24,
    /// 32-bit integers; also 24 valid bits in a 32-bit container, which
    /// WASAPI stores left-justified.
    I32,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl SampleFormat {
    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
            SampleFormat::F32 | SampleFormat::I32 => 4,
        }
    }
}

/// Writes `frames` frames from channel-major `source` (`channels × frames`
/// samples) into the interleaved byte buffer `out`, which holds
/// `out_channels` channels per frame. `map[i]` is the device channel that
/// source channel `i` goes to; device channels nobody writes are silent.
/// Integer formats are clipped, never wrapped.
#[cfg_attr(not(windows), allow(dead_code))] // used by the WASAPI backend
pub fn interleave_bytes(
    source: &[f32],
    frames: usize,
    map: &[usize],
    out_channels: usize,
    format: SampleFormat,
    out: &mut [u8],
) {
    let width = format.bytes();
    let used = frames * out_channels * width;
    out[..used].fill(0);
    for (channel, &target) in map.iter().enumerate() {
        let samples = &source[channel * frames..(channel + 1) * frames];
        for (frame, &sample) in samples.iter().enumerate() {
            let at = (frame * out_channels + target) * width;
            let slot = &mut out[at..at + width];
            let clipped = sample.clamp(-1.0, 1.0);
            match format {
                SampleFormat::F32 => slot.copy_from_slice(&sample.to_le_bytes()),
                SampleFormat::I16 => {
                    slot.copy_from_slice(&((clipped * 32_767.0).round() as i16).to_le_bytes())
                }
                SampleFormat::I24 => {
                    let value = (clipped * 8_388_607.0).round() as i32;
                    slot.copy_from_slice(&value.to_le_bytes()[..3]);
                }
                SampleFormat::I32 => {
                    let value = (f64::from(clipped) * 2_147_483_647.0).round() as i32;
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
}

/// [`interleave_bytes`] for interleaved `f32` buffers (cpal).
#[cfg_attr(windows, allow(dead_code))] // used by the cpal backend
pub fn interleave_f32(
    source: &[f32],
    frames: usize,
    map: &[usize],
    out_channels: usize,
    out: &mut [f32],
) {
    out[..frames * out_channels].fill(0.0);
    for (channel, &target) in map.iter().enumerate() {
        let samples = &source[channel * frames..(channel + 1) * frames];
        for (frame, &sample) in samples.iter().enumerate() {
            out[frame * out_channels + target] = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(format: SampleFormat, bytes: &[u8]) -> f64 {
        match format {
            SampleFormat::F32 => f64::from(f32::from_le_bytes(bytes.try_into().unwrap())),
            SampleFormat::I16 => {
                f64::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32_767.0
            }
            SampleFormat::I24 => {
                let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]) << 8 >> 8;
                f64::from(value) / 8_388_607.0
            }
            SampleFormat::I32 => {
                f64::from(i32::from_le_bytes(bytes.try_into().unwrap())) / 2_147_483_647.0
            }
        }
    }

    #[test]
    fn channels_land_in_their_device_slots_and_others_are_silent() {
        // Two source channels into device channels 3 and 0 of four.
        let source = [0.5, -0.5, 0.25, -0.25];
        let mut out = [9.0_f32; 8];
        interleave_f32(&source, 2, &[3, 0], 4, &mut out);
        assert_eq!(out, [0.25, 0.0, 0.0, 0.5, -0.25, 0.0, 0.0, -0.5]);
    }

    #[test]
    fn every_format_round_trips_within_its_resolution() {
        let source: Vec<f32> = (0..64).map(|i| (i as f32 / 32.0) - 1.0).collect();
        for (format, tolerance) in [
            (SampleFormat::F32, 0.0),
            (SampleFormat::I16, 1.0 / 32_767.0),
            (SampleFormat::I24, 1.0 / 8_388_607.0),
            (SampleFormat::I32, 1e-7),
        ] {
            let width = format.bytes();
            let mut out = vec![0xAA_u8; source.len() * width];
            interleave_bytes(&source, source.len(), &[0], 1, format, &mut out);
            for (i, &expected) in source.iter().enumerate() {
                let got = decode(format, &out[i * width..(i + 1) * width]);
                assert!(
                    (got - f64::from(expected)).abs() <= tolerance,
                    "{format:?} sample {i}: {got} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn integer_output_is_clipped_not_wrapped() {
        for format in [SampleFormat::I16, SampleFormat::I24, SampleFormat::I32] {
            let width = format.bytes();
            let mut out = vec![0_u8; 2 * width];
            interleave_bytes(&[2.0, -2.0], 2, &[0], 1, format, &mut out);
            assert!((decode(format, &out[..width]) - 1.0).abs() < 1e-6);
            assert!((decode(format, &out[width..]) + 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn unused_bytes_beyond_the_block_are_untouched() {
        let mut out = [7_u8; 12];
        interleave_bytes(&[0.0], 1, &[0], 2, SampleFormat::I16, &mut out);
        assert_eq!(out[..4], [0, 0, 0, 0]);
        assert!(out[4..].iter().all(|&b| b == 7));
    }
}
