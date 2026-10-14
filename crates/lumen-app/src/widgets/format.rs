//! How values read as text: decibels, milliseconds and pan.

/// "−3.0 dB", "−∞ dB".
pub fn db_text(db: f32) -> String {
    if db <= -59.9 {
        "\u{2212}\u{221E} dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// A peak readout in a few characters, without " dB": "−3.0", "−12", "+6.0".
/// From ten dB up the decimal goes, so the number fits a strip's readout
/// box (the owner's report on overflowing meter numbers). Only for a level
/// above the floor.
pub fn readout_db(db: f32) -> String {
    if db.abs() >= 9.95 {
        format!("{db:+.0}").replace('-', "\u{2212}")
    } else {
        db_text(db).replace(" dB", "")
    }
}

/// "0 ms", "125 ms", "1.25 s".
pub fn ms_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{:.0} ms", ms.max(0.0))
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// "C", "L 30", "R 100".
pub fn pan_text(pan: f32) -> String {
    let n = (pan * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}
