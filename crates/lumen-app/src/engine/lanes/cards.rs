use eframe::egui::{self, RichText};

use crate::media::{Item, State};
use crate::theme;

pub fn file_card(ui: &mut egui::Ui, item: &Item, marks: Option<&str>, properties: Option<&str>) {
    ui.label(RichText::new(&item.name).strong());
    file_facts(ui, item);
    if let Some(marks) = marks {
        ui.label(RichText::new(marks).color(theme::p().text_secondary));
    }
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
    if let Some(keys) = properties {
        ui.label(
            RichText::new(format!("{keys}: edit its properties"))
                .small()
                .color(theme::p().accent),
        );
    }
}

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

fn shorten_middle(path: &std::path::Path, fits: impl Fn(&str) -> bool) -> String {
    let whole = path.display().to_string();
    if fits(&whole) {
        return whole;
    }
    use std::path::Component;
    let sep = std::path::MAIN_SEPARATOR_STR;
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
        let rooted = PathBuf::from(sep).join(&path);
        let short = shorten_middle(&rooted, |t| t.chars().count() <= 27);
        assert!(short.starts_with(&format!("{sep}base")), "{short}");
        assert!(!short.contains(&format!("{sep}{sep}")), "{short}");
    }
}
