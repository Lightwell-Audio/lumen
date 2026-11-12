//! The file cards a clip's hover shows, and its properties card repeats (M3
//! slice 6e): what the file is (format, rate, channels, length, size), any
//! conversion or failure, and its path, shortened in the middle so the name
//! always shows.

use eframe::egui::{self, RichText};

use crate::media::{Item, State};
use crate::theme;

/// The hover card for a clip's file; `marks` says what was done to the clip
/// (reversed, polarity inverted), and `properties` is the properties card's
/// shortcut, if it has one.
pub fn file_card(ui: &mut egui::Ui, item: &Item, marks: Option<&str>, properties: Option<&str>) {
    ui.label(RichText::new(&item.name).strong());
    file_facts(ui, item);
    if let Some(marks) = marks {
        ui.label(RichText::new(marks).color(theme::p().text_secondary));
    }
    // Every gesture on a clip, so none stays hidden (owner, after 6c: slip
    // was not discoverable).
    ui.separator();
    for line in [
        "Header: drag to move · Ctrl copies · Ctrl+Alt slips the audio",
        "Below the header: drag to select time · Ctrl+Shift selects clips",
        "Sides: drag to trim · Shift loops",
        "Top corners: fades · their middle dot bends them · the line sets gain",
        "Right-click: edits, color, mute, lock and fade shapes",
    ] {
        ui.label(RichText::new(line).small().weak());
    }
    // The way into the properties card (slice 6e, the owner's P1), with
    // its shortcut as the keymap has it.
    if let Some(keys) = properties {
        ui.label(
            RichText::new(format!("{keys}: edit its properties"))
                .small()
                .color(theme::p().accent),
        );
    }
}

/// What a clip's file is: format, rate, channels, length, size, any
/// conversion or failure, and its path. The hover card and the clip's
/// properties card show it.
pub fn file_facts(ui: &mut egui::Ui, item: &Item) {
    let info = &item.info;
    let mut facts = vec![info.format.clone()];
    if let Some(rate) = info.sample_rate {
        facts.push(format!("{:.1} kHz", f64::from(rate) / 1000.0));
    }
    if let Some(channels) = info.channels {
        facts.push(match channels {
            1 => "mono".into(),
            2 => "stereo".into(),
            n => format!("{n} channels"),
        });
    }
    if let Some(seconds) = info.seconds {
        facts.push(crate::position::format_time(seconds));
    }
    // The size arrives with the load.
    if info.size_bytes > 0 {
        facts.push(format!("{:.1} MB", info.size_bytes as f64 / 1_048_576.0));
    }
    ui.label(facts.join(" · "));
    match &item.state {
        State::Ready(audio) if Some(audio.sample_rate()) != info.sample_rate => {
            ui.label(format!(
                "Playing at {:.1} kHz (converted)",
                f64::from(audio.sample_rate()) / 1000.0
            ));
        }
        State::Failed(why) => {
            ui.label(RichText::new(why).color(theme::p().warning));
        }
        _ => {}
    }
    // Shortened in the middle, so the file's name always shows (the
    // interface audit's S6: cut at its end, the name was what went).
    let font = egui::FontId::proportional(theme::size::CAPTION);
    let room = ui.available_width();
    let shown = shorten_middle(&item.path, |text| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), theme::p().text_hint)
            .size()
            .x
            <= room
    });
    ui.add(egui::Label::new(RichText::new(shown).small().weak()).truncate())
        .on_hover_text(item.path.display().to_string());
}

/// `path` as it `fits`: whole if it can be, otherwise with the folders
/// nearest the file left out as "…", keeping the start and the file's name.
fn shorten_middle(path: &std::path::Path, fits: impl Fn(&str) -> bool) -> String {
    let whole = path.display().to_string();
    if fits(&whole) {
        return whole;
    }
    use std::path::Component;
    let sep = std::path::MAIN_SEPARATOR_STR;
    // The drive and root ("C:\", "/") lead the first folder; the rest are
    // names.
    let mut root = String::new();
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => root.push_str(&p.as_os_str().to_string_lossy()),
            Component::RootDir => root.push_str(sep),
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    let Some((name, folders)) = parts.split_last() else {
        return whole;
    };
    for keep in (1..folders.len()).rev() {
        let text = format!(
            "{root}{}{sep}\u{2026}{sep}{name}",
            folders[..keep].join(sep)
        );
        if fits(&text) {
            return text;
        }
    }
    format!("\u{2026}{sep}{name}")
}

#[cfg(test)]
mod path_tests {
    use super::shorten_middle;
    use std::path::PathBuf;

    #[test]
    fn a_long_path_keeps_its_start_and_its_file_name() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let path: PathBuf = ["base", "Users", "someone", "Music", "Samples", "kick.wav"]
            .iter()
            .collect();
        assert_eq!(shorten_middle(&path, |_| true), path.display().to_string());
        let short = shorten_middle(&path, |t| t.chars().count() <= 26);
        assert!(
            short.ends_with(&format!("\u{2026}{sep}kick.wav")),
            "{short}"
        );
        assert!(short.starts_with("base"), "{short}");
        assert!(short.chars().count() <= 26, "{short}");
        assert_eq!(
            shorten_middle(&path, |_| false),
            format!("\u{2026}{sep}kick.wav")
        );
        // From the root: one separator after it, never two.
        let rooted = PathBuf::from(sep).join(&path);
        let short = shorten_middle(&rooted, |t| t.chars().count() <= 27);
        assert!(short.starts_with(&format!("{sep}base")), "{short}");
        assert!(!short.contains(&format!("{sep}{sep}")), "{short}");
    }
}
