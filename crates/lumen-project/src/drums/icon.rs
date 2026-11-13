//! Which drawing a sound gets from its name (M8 D0): "kick" and "bd" are a
//! kick, "hat" with "open" an open hat, "tom" with "floor" or "low" a floor
//! tom, and so on. A name nothing matches is percussion. Every sound of the
//! built-in maps goes through here, so a kit's names and its icons cannot
//! drift apart (a test pins each preset's).

use crate::model::DrumIcon;

/// Whether `words` holds `word` as a word of its own.
fn has(words: &[&str], word: &str) -> bool {
    words.contains(&word)
}

/// Whether `words` holds a word starting with `stem` ("tom" finds "tom1").
fn starts(words: &[&str], stem: &str) -> bool {
    words.iter().any(|w| w.starts_with(stem))
}

/// The drawing `name` reads as.
pub fn icon_for_name(name: &str) -> DrumIcon {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    // A hat first: "open hi-hat" holds "hi" and "hat" too.
    let hat = has(&words, "hh")
        || has(&words, "hat")
        || has(&words, "hats")
        || lower.contains("hi-hat")
        || lower.contains("hihat");
    if hat {
        return if has(&words, "open") {
            DrumIcon::OpenHat
        } else if has(&words, "pedal") || has(&words, "foot") {
            DrumIcon::PedalHat
        } else {
            DrumIcon::ClosedHat
        };
    }
    // A ride before a crash: "ride cymbal" is a ride.
    if has(&words, "ride") {
        return DrumIcon::Ride;
    }
    if has(&words, "crash")
        || has(&words, "splash")
        || has(&words, "china")
        || has(&words, "chinese")
        || has(&words, "cymbal")
        || has(&words, "belltree")
    {
        return DrumIcon::Crash;
    }
    if has(&words, "tom") || starts(&words, "tom") {
        return if has(&words, "floor") || has(&words, "low") {
            DrumIcon::FloorTom
        } else {
            DrumIcon::Tom
        };
    }
    if has(&words, "kick")
        || has(&words, "bd")
        || has(&words, "bassdrum")
        || (has(&words, "bass") && has(&words, "drum"))
        || lower.contains("bass drum")
    {
        return DrumIcon::Kick;
    }
    if has(&words, "rim") || has(&words, "rimshot") || (has(&words, "side") && has(&words, "stick"))
    {
        return DrumIcon::Rim;
    }
    if has(&words, "snare") || has(&words, "sd") || starts(&words, "snare") {
        return DrumIcon::Snare;
    }
    if has(&words, "clap") || has(&words, "handclap") {
        return DrumIcon::Clap;
    }
    if has(&words, "fx")
        || has(&words, "effect")
        || has(&words, "whistle")
        || has(&words, "guiro")
        || has(&words, "cuica")
        || has(&words, "scratch")
        || has(&words, "vibraslap")
        || has(&words, "click")
        || has(&words, "metronome")
        || has(&words, "q")
    {
        return DrumIcon::Effect;
    }
    if has(&words, "cowbell") || has(&words, "bell") {
        return DrumIcon::Cowbell;
    }
    if has(&words, "shaker")
        || has(&words, "maracas")
        || has(&words, "maraca")
        || has(&words, "tambourine")
        || has(&words, "cabasa")
        || has(&words, "jingle")
    {
        return DrumIcon::Shaker;
    }
    DrumIcon::Percussion
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_reads_as_its_piece() {
        for (name, icon) in [
            ("Kick", DrumIcon::Kick),
            ("BD", DrumIcon::Kick),
            ("Acoustic Bass Drum", DrumIcon::Kick),
            ("Snare", DrumIcon::Snare),
            ("Acoustic Snare", DrumIcon::Snare),
            ("Snare 2", DrumIcon::Snare),
            ("Rim", DrumIcon::Rim),
            ("Side Stick", DrumIcon::Rim),
            ("Hand Clap", DrumIcon::Clap),
            ("Closed Hat", DrumIcon::ClosedHat),
            ("Closed Hi-Hat", DrumIcon::ClosedHat),
            ("HH", DrumIcon::ClosedHat),
            ("Pedal Hi-Hat", DrumIcon::PedalHat),
            ("Open Hat", DrumIcon::OpenHat),
            ("Open Hi-Hat", DrumIcon::OpenHat),
            ("Tom 1", DrumIcon::Tom),
            ("High Tom", DrumIcon::Tom),
            ("Floor Tom", DrumIcon::FloorTom),
            ("Low Floor Tom", DrumIcon::FloorTom),
            ("Low Tom", DrumIcon::FloorTom),
            ("Crash", DrumIcon::Crash),
            ("Crash Cymbal 1", DrumIcon::Crash),
            ("Splash Cymbal", DrumIcon::Crash),
            ("Ride", DrumIcon::Ride),
            ("Ride Cymbal 1", DrumIcon::Ride),
            ("Ride Bell", DrumIcon::Ride),
            ("Cowbell", DrumIcon::Cowbell),
            ("Shaker", DrumIcon::Shaker),
            ("Maracas", DrumIcon::Shaker),
            ("Open Triangle", DrumIcon::Percussion),
            ("Claves", DrumIcon::Percussion),
            ("Low Conga", DrumIcon::Percussion),
            ("Short Guiro", DrumIcon::Effect),
            ("High Q", DrumIcon::Effect),
            ("", DrumIcon::Percussion),
        ] {
            assert_eq!(icon_for_name(name), icon, "{name}");
        }
    }
}
