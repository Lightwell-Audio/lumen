use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    #[default]
    MinSec,
    Samples,
    Timecode24,
    Timecode25,
    Timecode2997Drop,
    Timecode30,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 6] = [
        TimeFormat::MinSec,
        TimeFormat::Samples,
        TimeFormat::Timecode24,
        TimeFormat::Timecode25,
        TimeFormat::Timecode2997Drop,
        TimeFormat::Timecode30,
    ];

    pub fn next(self) -> TimeFormat {
        let at = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            TimeFormat::MinSec => "Minutes and seconds",
            TimeFormat::Samples => "Samples",
            TimeFormat::Timecode24 => "Timecode, 24 fps",
            TimeFormat::Timecode25 => "Timecode, 25 fps",
            TimeFormat::Timecode2997Drop => "Timecode, 29.97 fps drop-frame",
            TimeFormat::Timecode30 => "Timecode, 30 fps",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            TimeFormat::MinSec => "Time",
            TimeFormat::Samples => "Samples",
            TimeFormat::Timecode24 => "TC 24",
            TimeFormat::Timecode25 => "TC 25",
            TimeFormat::Timecode2997Drop => "TC 29.97",
            TimeFormat::Timecode30 => "TC 30",
        }
    }

    fn rate(self) -> Option<(u64, u64, bool)> {
        match self {
            TimeFormat::Timecode24 => Some((24, 1, false)),
            TimeFormat::Timecode25 => Some((25, 1, false)),
            TimeFormat::Timecode2997Drop => Some((30_000, 1_001, true)),
            TimeFormat::Timecode30 => Some((30, 1, false)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub seconds: f64,
    pub text: String,
}

pub fn format(format: TimeFormat, seconds: f64, rate: u32) -> String {
    let seconds = seconds.max(0.0);
    match format {
        TimeFormat::MinSec => min_sec(seconds, 3),
        TimeFormat::Samples => grouped((seconds * f64::from(rate)).round() as u64),
        _ => {
            let (num, den, drop) = format.rate().expect("timecode has a rate");
            timecode(
                (seconds * num as f64 / den as f64 + 1e-6).floor() as u64,
                format,
                drop,
            )
        }
    }
}

pub fn marks(
    format: TimeFormat,
    from: f64,
    to: f64,
    points_per_second: f64,
    rate: u32,
    gap: f64,
) -> Vec<Mark> {
    if to <= from || points_per_second <= 0.0 {
        return Vec::new();
    }
    let (units_per_second, ladder): (f64, Vec<u64>) = match format {
        TimeFormat::MinSec => (1e5, nice_ladder(1, 360_000_000)),
        TimeFormat::Samples => (f64::from(rate), nice_ladder(1, 10_000_000_000)),
        _ => {
            let (num, den, _) = format.rate().expect("timecode has a rate");
            let nominal = num.div_ceil(den);
            let mut ladder = vec![1, 2, 5, 10];
            for s in [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1_800, 3_600] {
                ladder.push(s * nominal);
            }
            (num as f64 / den as f64, ladder)
        }
    };
    let Some(&step) = ladder
        .iter()
        .find(|&&s| s as f64 / units_per_second * points_per_second >= gap)
    else {
        return Vec::new();
    };
    let first = (from * units_per_second / step as f64).ceil().max(0.0) as u64;
    let last = (to * units_per_second / step as f64).floor() as u64;
    let decimals = if format == TimeFormat::MinSec {
        (5 - (step as f64).log10().floor() as i32).clamp(0, 5) as usize
    } else {
        0
    };
    (first..=last.min(first + 1_000))
        .map(|k| {
            let unit = k * step;
            let seconds = unit as f64 / units_per_second;
            let text = match format {
                TimeFormat::MinSec => min_sec(seconds, decimals),
                TimeFormat::Samples => grouped(unit),
                _ => {
                    let (_, _, drop) = format.rate().expect("timecode has a rate");
                    timecode(unit, format, drop)
                }
            };
            Mark { seconds, text }
        })
        .collect()
}

fn nice_ladder(low: u64, high: u64) -> Vec<u64> {
    let mut ladder = Vec::new();
    let mut decade = low;
    while decade <= high {
        for m in [1, 2, 5] {
            ladder.push(decade * m);
        }
        decade *= 10;
    }
    ladder
}

fn min_sec(seconds: f64, decimals: usize) -> String {
    let scale = 10_u64.pow(decimals as u32);
    let total = (seconds * scale as f64).round() as u64;
    let (whole, frac) = (total / scale, total % scale);
    let (h, m, s) = (whole / 3_600, whole / 60 % 60, whole % 60);
    let mut text = if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    };
    if decimals > 0 {
        text.push_str(&format!(".{frac:0decimals$}"));
    }
    text
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn timecode(n: u64, format: TimeFormat, drop: bool) -> String {
    let (num, den, _) = format.rate().expect("timecode has a rate");
    let nominal = num.div_ceil(den);
    let mut number = n;
    if drop {
        let (tens, rest) = (n / 17_982, n % 17_982);
        number += 18 * tens
            + if rest >= 2 {
                2 * ((rest - 2) / 1_798)
            } else {
                0
            };
    }
    let frames = number % nominal;
    let seconds = number / nominal;
    format!(
        "{:02}:{:02}:{:02}{}{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        if drop { ';' } else { ':' },
        frames
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_steps_through_every_format_and_wraps() {
        let mut format = TimeFormat::MinSec;
        for expected in TimeFormat::ALL.iter().skip(1).chain(&TimeFormat::ALL[..1]) {
            format = format.next();
            assert_eq!(format, *expected);
        }
    }

    #[test]
    fn the_position_reads_in_every_format() {
        assert_eq!(format(TimeFormat::MinSec, 65.25, 48_000), "1:05.250");
        assert_eq!(format(TimeFormat::MinSec, 3_725.0, 48_000), "1:02:05.000");
        assert_eq!(format(TimeFormat::Samples, 65.0, 48_000), "3,120,000");
        assert_eq!(format(TimeFormat::Timecode25, 65.25, 48_000), "00:01:05:06");
        assert_eq!(
            format(TimeFormat::Timecode24, 1.0 / 24.0, 48_000),
            "00:00:00:01"
        );
        assert_eq!(
            format(TimeFormat::Timecode30, 3_600.0, 48_000),
            "01:00:00:00"
        );
    }

    #[test]
    fn drop_frame_skips_two_numbers_a_minute_but_not_every_tenth() {
        let tc = |n| timecode(n, TimeFormat::Timecode2997Drop, true);
        assert_eq!(tc(0), "00:00:00;00");
        assert_eq!(tc(1_799), "00:00:59;29");
        assert_eq!(tc(1_800), "00:01:00;02", "frames 00 and 01 are skipped");
        assert_eq!(tc(17_981), "00:09:59;29");
        assert_eq!(tc(17_982), "00:10:00;00", "not at the tenth minute");
        assert_eq!(tc(107_892), "01:00:00;00");
        assert_eq!(
            format(TimeFormat::Timecode2997Drop, 3_600.0, 48_000),
            "01:00:00;00"
        );
    }

    #[test]
    fn marks_take_round_steps_far_enough_apart() {
        let marks = marks(TimeFormat::MinSec, 0.0, 60.0, 10.0, 48_000, 60.0);
        let texts: Vec<&str> = marks.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            ["0:00", "0:10", "0:20", "0:30", "0:40", "0:50", "1:00"]
        );
        let deep = super::marks(TimeFormat::MinSec, 1.0, 1.001, 384_000.0, 48_000, 60.0);
        assert_eq!(deep[0].text, "0:01.0000");
        assert_eq!(deep[1].text, "0:01.0002");
        let samples = super::marks(TimeFormat::Samples, 1.0, 1.001, 384_000.0, 48_000, 60.0);
        assert_eq!(samples[0].text, "48,000");
        assert_eq!(samples[1].text, "48,010");
        let tc = super::marks(TimeFormat::Timecode25, 0.0, 100.0, 5.0, 48_000, 120.0);
        assert_eq!(tc[1].text, "00:00:30:00");
        assert!((tc[1].seconds - 30.0).abs() < 1e-9);
    }

    #[test]
    fn the_time_format_saves_as_a_word() {
        assert_eq!(
            serde_json::to_string(&TimeFormat::Timecode2997Drop).unwrap(),
            "\"timecode2997_drop\""
        );
    }
}
