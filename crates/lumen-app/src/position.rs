//! The transport position as the user reads and types it: bars, beats and
//! sixteenths ("3.2.1", one-based), or minutes and seconds
//! ("1:05.250").

use lumen_core::time::{PPQ, Ticks};
use lumen_engine::Timeline;

const SIXTEENTH: i64 = PPQ / 4;

/// "bar.beat.sixteenth", one-based.
pub fn format_bbt(timeline: &Timeline, t: Ticks) -> String {
    let bbt = timeline.meter.bbt_at(t);
    format!(
        "{}.{}.{}",
        bbt.bar + 1,
        bbt.beat + 1,
        bbt.ticks.0 / SIXTEENTH + 1
    )
}

/// "m:ss.mmm".
pub fn format_time(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// Parses "3", "3.2", "3.2.4" (bars, beats, sixteenths, one-based) or
/// "1:05" / "1:05.25" (minutes and seconds).
pub fn parse(input: &str, timeline: &Timeline) -> Result<Ticks, String> {
    let input = input.trim();
    if let Some((minutes, seconds)) = input.split_once(':') {
        let minutes: u32 = minutes
            .trim()
            .parse()
            .map_err(|_| format!("{minutes:?} is not a number of minutes"))?;
        let seconds: f64 = seconds
            .trim()
            .parse()
            .ok()
            .filter(|s: &f64| (0.0..60.0).contains(s))
            .ok_or_else(|| format!("{seconds:?} is not 0–59.999 seconds"))?;
        return Ok(timeline
            .tempo
            .ticks_at_seconds(f64::from(minutes) * 60.0 + seconds));
    }
    let parts: Vec<&str> = input.split('.').map(str::trim).collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return Err("type bars.beats.sixteenths, like 3.2.1, or a time like 1:05".into());
    }
    let numbers = parts
        .iter()
        .map(|p| {
            p.parse::<i64>()
                .map_err(|_| format!("{p:?} is not a number"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bar = numbers[0];
    if bar < 1 {
        return Err("bars start at 1".into());
    }
    let start = timeline.meter.bar_start(bar - 1);
    let meter = timeline.meter.meter_at(start);
    let beat = numbers.get(1).copied().unwrap_or(1);
    if !(1..=i64::from(meter.numerator)).contains(&beat) {
        return Err(format!(
            "bar {bar} has beats 1–{} ({meter})",
            meter.numerator
        ));
    }
    let per_beat = (meter.beat().0 / SIXTEENTH).max(1);
    let sixteenth = numbers.get(2).copied().unwrap_or(1);
    if !(1..=per_beat).contains(&sixteenth) {
        return Err(format!("a beat in {meter} has sixteenths 1–{per_beat}"));
    }
    Ok(start + Ticks((beat - 1) * meter.beat().0 + (sixteenth - 1) * SIXTEENTH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::meter::Meter;

    fn t44() -> Timeline {
        Timeline::constant(120.0, Meter::FOUR_FOUR)
    }

    #[test]
    fn positions_format_one_based() {
        let tl = t44();
        assert_eq!(format_bbt(&tl, Ticks::ZERO), "1.1.1");
        assert_eq!(
            format_bbt(&tl, Ticks::quarters(5) + Ticks(SIXTEENTH * 3)),
            "2.2.4"
        );
        assert_eq!(format_time(0.0), "0:00.000");
        assert_eq!(format_time(65.25), "1:05.250");
    }

    #[test]
    fn typed_positions_round_trip() {
        let tl = Timeline::constant(
            97.0,
            Meter {
                numerator: 7,
                denominator: 8,
            },
        );
        for text in ["1.1.1", "3.7.2", "12.4.1"] {
            let t = parse(text, &tl).unwrap();
            assert_eq!(format_bbt(&tl, t), text);
        }
        assert_eq!(parse("3", &t44()), Ok(Ticks::quarters(8)));
        assert_eq!(parse(" 3.2 ", &t44()), Ok(Ticks::quarters(9)));
        assert_eq!(parse("0:02", &t44()), Ok(Ticks::quarters(4)));
        assert_eq!(parse("1:00.5", &t44()), Ok(Ticks::quarters(121)));
    }

    #[test]
    fn mistakes_explain_themselves() {
        let tl = t44();
        assert!(parse("", &tl).is_err());
        assert!(parse("0", &tl).unwrap_err().contains("start at 1"));
        assert!(parse("2.5", &tl).unwrap_err().contains("1–4"));
        assert!(parse("2.1.5", &tl).unwrap_err().contains("1–4"));
        assert!(parse("1:75", &tl).is_err());
        assert!(parse("a.b", &tl).unwrap_err().contains("not a number"));
        assert!(parse("1.2.3.4", &tl).is_err());
    }
}
