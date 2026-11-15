pub fn db_text(db: f32) -> String {
    if db <= -59.9 {
        "\u{2212}\u{221E} dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

pub fn readout_db(db: f32) -> String {
    if db.abs() >= 9.95 {
        format!("{db:+.0}").replace('-', "\u{2212}")
    } else {
        db_text(db).replace(" dB", "")
    }
}

pub fn ms_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{:.0} ms", ms.max(0.0))
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

pub fn pan_text(pan: f32) -> String {
    let n = (pan * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}
