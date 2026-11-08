//! The audio editor's properties strip (M9.5 S3, ADR-0128): Gain, the two
//! fades, Mute and Lock, Reverse, Polarity and Normalize sit in a strip
//! under the header and edit the opened clip, each as the card and the menus
//! do; the Clip chip renames and recolours; the File chip says where the
//! file is and relinks or reveals it; and every control stays reachable at
//! the smallest windows and from the keyboard alone.

use eframe::egui::{Event, Key, Modifiers, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent};

use super::Lumen;
use super::audio_editor_handles::{open, opened, shown};
use super::midi_play;
use super::properties::{clip, select, with_two_clips};

fn gain_of(c: &Clip) -> f32 {
    match &c.content {
        ClipContent::Audio { gain_db, .. } => *gain_db,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    }
}

/// Types `text` into the field named `label` and presses Enter.
fn type_into(lumen: &mut Lumen, label: &str, text: &str) {
    lumen.harness.get_by_label_contains(label).click();
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(text.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
}

#[test]
fn the_strip_replaces_the_column_and_edits_the_opened_clip_in_one_step_each() {
    let (mut lumen, id) = opened("strip-edit");
    for name in [
        "Clip gain",
        "Mute clip",
        "Lock clip",
        "Reverse",
        "Polarity",
        "Normalize",
    ] {
        assert!(
            lumen
                .harness
                .query_all_by_label_contains(name)
                .next()
                .is_some(),
            "{name} is in the strip"
        );
    }
    for fade in ["Fade in", "Fade out"] {
        assert!(
            lumen
                .harness
                .query_all_by_label_contains(fade)
                .next()
                .is_some(),
            "{fade} is in the strip"
        );
    }
    assert!(!shown(&lumen).strip.is_empty());

    // Gain typed in: one step, and the same clip the card makes.
    let undo_before = lumen.app().undo_label().map(str::to_owned);
    type_into(&mut lumen, "Clip gain: ", "-6");
    assert!((gain_of(&clip(&lumen, id)) + 6.0).abs() < 1e-3);
    let strip_clip = clip(&lumen, id);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(gain_of(&clip(&lumen, id)), 0.0, "one undo restores it");
    assert_eq!(lumen.app().undo_label().map(str::to_owned), undo_before);
    lumen.app_mut().set_clip_gain_like_the_card(id, -6.0);
    assert_eq!(clip(&lumen, id), strip_clip, "the same edit as the card's");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);

    // A fade length typed in: 250 ms is an eighth note at 120 BPM.
    type_into(&mut lumen, "Fade in: ", "250");
    assert_eq!(
        clip(&lumen, id).fade_in.length,
        Ticks(Ticks::quarters(1).0 / 2)
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(clip(&lumen, id).fade_in.length, Ticks(0));

    // Mute and Lock are toggles on the clip.
    lumen.click("Mute clip");
    assert!(clip(&lumen, id).muted);
    lumen.click("Lock clip");
    assert!(clip(&lumen, id).locked);
}

#[test]
fn processing_acts_on_the_opened_clip_even_when_another_is_selected() {
    let (mut lumen, kick, snare) = with_two_clips("strip-processing");
    open(&mut lumen, kick);
    // The selection moves to the other clip while the editor keeps the kick
    // (the editor follows a selection only when it is made there).
    lumen.app_mut().select_clip(snare);
    lumen.frames(3);
    assert_eq!(
        lumen.app().opened_audio_clip(),
        Some(kick),
        "still the kick"
    );

    lumen.click("Reverse");
    assert!(clip(&lumen, kick).reversed, "the opened clip reversed");
    assert!(!clip(&lumen, snare).reversed, "the selected one did not");
    assert_eq!(
        lumen.app().selected_clips(),
        [kick],
        "it became the selection"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!clip(&lumen, kick).reversed, "one undo step");

    lumen.app_mut().select_clip(snare);
    lumen.frames(3);
    lumen.click("Polarity");
    assert!(clip(&lumen, kick).inverted && !clip(&lumen, snare).inverted);
    assert_eq!(lumen.app().undo_label(), Some("Invert polarity"));
}

#[test]
fn a_locked_clip_disables_gain_fades_and_processing_but_not_mute_and_lock() {
    let (mut lumen, id) = opened("strip-locked");
    lumen.click("Lock clip");
    assert!(clip(&lumen, id).locked);
    for name in ["Reverse", "Polarity", "Normalize"] {
        assert!(
            lumen
                .harness
                .get_by_label(name)
                .accesskit_node()
                .is_disabled(),
            "{name} waits while the clip is locked"
        );
    }
    let before = clip(&lumen, id);
    lumen.harness.get_by_label("Reverse").click();
    lumen.frames(3);
    assert_eq!(
        clip(&lumen, id),
        before,
        "a disabled control changes nothing"
    );
    // Mute and Lock stay live.
    lumen.click("Mute clip");
    assert!(clip(&lumen, id).muted);
    lumen.click("Lock clip");
    assert!(!clip(&lumen, id).locked);
    assert!(
        !lumen
            .harness
            .get_by_label("Reverse")
            .accesskit_node()
            .is_disabled(),
        "unlocked again"
    );
}

#[test]
fn the_clip_chip_renames_and_recolours_the_opened_clip() {
    let (mut lumen, id) = opened("strip-chip");
    lumen
        .harness
        .get_by_label_contains("Clip kick: name and color")
        .click();
    lumen.frames(3);
    assert!(lumen.shows("Clip name"), "the popover opens on the name");
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Big kick".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert_eq!(clip(&lumen, id).name, "Big kick");
    lumen.click("Color 3");
    assert_eq!(
        clip(&lumen, id).color,
        Some(lumen_app::testing::clip_colour(2))
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(!lumen.shows("Clip name"), "Esc closes it");
}

#[test]
fn the_file_chip_gives_the_files_facts_and_reveal_notes_its_path() {
    let (mut lumen, id) = opened("strip-file");
    let media = match &clip(&lumen, id).content {
        ClipContent::Audio { media, .. } => *media,
        ClipContent::Midi { .. } => panic!("audio"),
    };
    let chip = lumen.harness.get_by_label_contains("File: kick").rect();
    assert!(chip.width() > 0.0);
    lumen.harness.get_by_label_contains("File: kick").click();
    lumen.frames(3);
    assert!(lumen.shows("Show in file manager"));
    assert!(
        !lumen.shows("Relink\u{2026}"),
        "a file that is there needs no relink"
    );
    lumen.click("Show in file manager");
    let revealed = lumen.app().revealed_files();
    assert_eq!(revealed.len(), 1, "{revealed:?}");
    assert_eq!(
        Some(revealed[0].clone()),
        lumen.app().media_file(media),
        "the path of the clip's file"
    );
}

/// The popover's Relink button, the last one drawn (the banner has its own).
fn relink_button(lumen: &mut Lumen) {
    lumen
        .harness
        .get_all_by_label("Relink\u{2026}")
        .last()
        .expect("a Relink button")
        .click();
    lumen.frames(3);
}

#[test]
fn a_missing_file_says_so_and_relinks_from_the_editor() {
    let (mut lumen, kick) = super::copies::with_outside_audio("strip-missing");
    lumen.app_mut().set_copy_imported(false);
    let project = lumen_app::testing::project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(project.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let moved = lumen.dir().join("Moved").join("kick.wav");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&kick, &moved).unwrap();
    let dir = lumen.crash();
    let mut lumen = Lumen::start_with(dir, |l| l.opening(project));
    lumen.wait_for("the project", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    lumen.wait_for("the missing file to be noticed", |app| {
        app.missing_media_count() == Some(1)
    });
    let id = lumen.app().project().tracks[0].clips[0].id;
    // A missing file has no wave to hold handles: the strip is what shows.
    lumen.app_mut().select_clip(id);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the audio editor", |app| app.opened_audio_clip().is_some());
    lumen.frames(5);
    assert!(
        lumen
            .harness
            .query_by_label_contains("File missing")
            .is_some(),
        "the chip says the file is missing"
    );
    lumen.harness.get_by_label_contains("File missing").click();
    lumen.frames(3);
    assert!(lumen.harness.query_all_by_label("Relink\u{2026}").count() >= 1);
    assert!(
        lumen
            .harness
            .get_by_label("Show in file manager")
            .accesskit_node()
            .is_disabled(),
        "there is nothing to show"
    );

    // Cancelling says nothing and changes nothing.
    lumen.app_mut().answer_next_dialog(vec![]);
    relink_button(&mut lumen);
    lumen.frames(10);
    assert!(
        lumen.app().media_tab_shown(),
        "the answer is on the media tab"
    );
    assert!(lumen.app().media_rows()[0].1, "still missing");

    // The file where it went is taken, and a toast says so.
    lumen.harness.get_by_label_contains("File missing").click();
    lumen.frames(3);
    lumen.app_mut().answer_next_dialog(vec![moved]);
    relink_button(&mut lumen);
    lumen.wait_for("the relink", |app| !app.media_rows()[0].1);
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.starts_with("Relinked")),
        "{:?}",
        lumen.app().note()
    );
}

/// Every control of the strip that is drawn at the current size, by name,
/// with its rectangle, and the strip's layout.
fn strip_controls(lumen: &Lumen) -> (&'static str, Vec<(&'static str, Rect)>) {
    let layout = shown(lumen).strip;
    let body = shown(lumen).body;
    let wanted = [
        ("Clip chip", "name and color"),
        ("Gain", "Clip gain: "),
        ("Mute", "Mute clip"),
        ("Lock", "Lock clip"),
        ("Fade in", "Fade in: "),
        ("Fade out", "Fade out: "),
        ("Reverse", "Reverse"),
        ("Polarity", "Polarity"),
        ("Normalize", "Normalize"),
        ("More", "More clip properties"),
        ("File", "File"),
    ];
    let mut found = Vec::new();
    for (name, label) in wanted {
        for node in lumen.harness.query_all_by_label_contains(label) {
            let r = node.rect();
            // The strip sits above the wave and its ruler.
            if r.width() > 0.0 && r.bottom() < body.top() && r.top() > body.top() - 130.0 {
                // The header's own buttons share some words: keep the strip's.
                if name == "File"
                    && !node
                        .accesskit_node()
                        .label()
                        .is_some_and(|l| l.starts_with("File"))
                {
                    continue;
                }
                found.push((name, r));
            }
        }
    }
    (layout, found)
}

#[test]
fn every_control_is_there_and_clear_of_the_others_in_each_layout() {
    let (mut lumen, _) = opened("strip-fit");
    let mut seen = std::collections::BTreeSet::new();
    for width in [1400.0, 1100.0, 960.0, 800.0, 700.0, 640.0, 560.0, 480.0] {
        lumen.harness.set_size(vec2(width, 540.0));
        lumen.app_mut().set_browser_size(220.0);
        lumen.frames(20);
        let body = shown(&lumen).body;
        let (layout, found) = strip_controls(&lumen);
        seen.insert(layout);
        let has = |name: &str| found.iter().any(|(n, _)| *n == name);

        // What each layout draws, and what it folds away.
        let (fades, processing, menu) = match layout {
            "full" | "icons" | "dot" => (true, true, false),
            "fold processing" => (true, false, true),
            "fold fades" => (false, false, true),
            other => panic!("a layout the test does not know: {other}"),
        };
        for always in ["Gain", "Mute", "Lock", "File", "Clip chip"] {
            assert!(has(always), "{always} at {width} ({layout}): {found:?}");
        }
        for (name, drawn) in [
            ("Fade in", fades),
            ("Fade out", fades),
            ("Reverse", processing),
            ("Polarity", processing),
            ("Normalize", processing),
            ("More", menu),
        ] {
            assert_eq!(has(name), drawn, "{name} at {width} ({layout}): {found:?}");
        }

        for (i, (a, ra)) in found.iter().enumerate() {
            assert!(
                ra.left() >= body.left() - 1.0 && ra.right() <= body.right() + 0.5,
                "{a} is outside the editor at {width} ({layout}): {ra:?}, body {body:?}"
            );
            for (b, rb) in &found[i + 1..] {
                assert!(
                    !ra.shrink(0.5).intersects(rb.shrink(0.5)),
                    "{a} and {b} overlap at {width} ({layout}): {ra:?} {rb:?}"
                );
            }
        }
        assert!(
            body.height() >= 40.0,
            "the wave keeps its room at {width}: {body:?}"
        );
    }
    for layout in ["full", "fold processing", "fold fades"] {
        assert!(
            seen.contains(layout),
            "the widths reached {layout}, not only {seen:?}"
        );
    }
}

#[test]
fn alt_enter_takes_the_keyboard_to_gain_in_the_editor_and_opens_the_card_elsewhere() {
    let (mut lumen, id) = opened("strip-alt-enter");
    // The pointer on the editor: it is where the work is.
    let body = shown(&lumen).body;
    lumen.harness.hover_at(body.center());
    lumen.frames(3);
    lumen.key(Modifiers::ALT, Key::Enter);
    lumen.frames(4);
    assert!(
        !lumen.shows("Clip name"),
        "no floating card when the strip is where the work is"
    );
    assert!(
        super::access::focused(&lumen).is_some_and(|f| f.starts_with("Clip gain")),
        "Gain has the keyboard: {:?}",
        super::access::focused(&lumen)
    );
    // Typing starts a new gain on the focused slider.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("-3".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!((gain_of(&clip(&lumen, id)) + 3.0).abs() < 1e-3);

    // A clip elsewhere: the arrangement's selection is not the opened clip.
    let (mut lumen, kick, snare) = with_two_clips("strip-alt-enter-card");
    open(&mut lumen, kick);
    select(&mut lumen, snare);
    lumen.key(Modifiers::ALT, Key::Enter);
    lumen.frames(4);
    assert!(
        lumen.shows("Clip name"),
        "the card opens for the selected clip"
    );
}

#[test]
fn alt_enter_from_a_strip_control_stays_in_the_strip_wherever_the_pointer_is() {
    let (mut lumen, _) = opened("strip-alt-enter-strip");
    // Alt+Enter and a Tab put the keyboard on Mute; the pointer then leaves
    // the editor for the arrangement.
    let body = shown(&lumen).body;
    lumen.harness.hover_at(body.center());
    lumen.frames(3);
    lumen.key(Modifiers::ALT, Key::Enter);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Tab);
    lumen.frames(3);
    lumen.harness.hover_at(vec2(700.0, 120.0).to_pos2());
    lumen.frames(4);
    assert!(
        super::access::focused(&lumen).is_some_and(|f| f.starts_with("Mute clip")),
        "the keyboard is on Mute: {:?}",
        super::access::focused(&lumen)
    );
    lumen.key(Modifiers::ALT, Key::Enter);
    lumen.frames(4);
    assert!(
        !lumen.shows("Clip name"),
        "no card: the keyboard is in the strip"
    );
    assert!(
        super::access::focused(&lumen).is_some_and(|f| f.starts_with("Clip gain")),
        "Gain has the keyboard: {:?}",
        super::access::focused(&lumen)
    );
}

#[test]
fn lock_is_a_command_for_one_or_several_clips_and_undoes_in_one_step() {
    let (mut lumen, kick, snare) = with_two_clips("strip-lock-command");
    lumen.app_mut().select_clips(&[kick, snare]);
    lumen.frames(3);
    midi_play::palette(&mut lumen, "Lock clip");
    lumen.frames(3);
    assert!(clip(&lumen, kick).locked && clip(&lumen, snare).locked);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!clip(&lumen, kick).locked && !clip(&lumen, snare).locked);
}

#[test]
fn the_full_strip_is_walked_by_tab_in_order_and_changes_nothing() {
    let (mut lumen, id) = opened("strip-keys");
    let body = shown(&lumen).body;
    lumen.harness.hover_at(body.center());
    lumen.frames(3);
    lumen.key(Modifiers::ALT, Key::Enter);
    lumen.frames(3);
    assert_eq!(shown(&lumen).strip, "full", "a wide window");
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..14 {
        lumen.key(Modifiers::NONE, Key::Tab);
        lumen.frames(1);
        if let Some(label) = super::access::focused(&lumen) {
            seen.push(label);
        }
    }
    // The strip's controls, in the order D36 gives them (the Clip chip comes
    // first, before Gain, which Alt+Enter started on).
    let order = [
        "Mute clip",
        "Lock clip",
        "Fade in",
        "Fade in shape",
        "Fade out",
        "Fade out shape",
        "Reverse",
        "Polarity",
        "Normalize",
        "File",
    ];
    let mut at = 0;
    for want in order {
        let found = seen[at..].iter().position(|l| l.starts_with(want));
        assert!(
            found.is_some(),
            "{want} comes after {:?} in the tab order: {seen:?}",
            &seen[..at]
        );
        at += found.unwrap() + 1;
    }
    let c = clip(&lumen, id);
    assert!(!c.reversed && !c.inverted && !c.muted && !c.locked);
}

#[test]
fn a_folded_strip_is_used_from_the_more_menu_and_escape_gives_the_keyboard_back() {
    let (mut lumen, id) = opened("strip-folded");
    lumen.harness.set_size(vec2(480.0, 540.0));
    lumen.app_mut().set_browser_size(220.0);
    lumen.frames(20);
    assert_eq!(shown(&lumen).strip, "fold fades", "the narrowest window");
    assert!(!lumen.shows("Reverse"), "Reverse is in the menu");

    // Reverse from the menu: the opened clip, one step.
    lumen.click("More clip properties");
    lumen.click("Reverse");
    assert!(clip(&lumen, id).reversed);
    assert_eq!(lumen.app().selected_clips(), [id]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!clip(&lumen, id).reversed, "one undo step");

    // Fades… opens the fades page, whose slider edits the clip.
    lumen.click("More clip properties");
    lumen.click("Fades…");
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Fade in: ")
            .next()
            .is_some(),
        "the fades page"
    );
    type_into(&mut lumen, "Fade in: ", "250");
    assert_eq!(
        clip(&lumen, id).fade_in.length,
        Ticks(Ticks::quarters(1).0 / 2)
    );

    // Esc closes the menu and the keyboard is on the button that opened it.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Fade in: ")
            .next()
            .is_none(),
        "the menu is closed"
    );
    assert!(
        super::access::focused(&lumen).is_some_and(|f| f.starts_with("More clip properties")),
        "the keyboard is back on the ⋯ button: {:?}",
        super::access::focused(&lumen)
    );
}

#[test]
fn the_strip_stays_when_the_editor_pops_out() {
    let (mut lumen, id) = opened("strip-popout");
    midi_play::palette(&mut lumen, "Pop out");
    lumen.frames(10);
    assert!(
        lumen.app().audio_editor_popped(),
        "the editor has its window"
    );
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Clip gain: ")
            .next()
            .is_some(),
        "the popped-out editor keeps its strip"
    );
    type_into(&mut lumen, "Clip gain: ", "-4");
    assert!((gain_of(&clip(&lumen, id)) + 4.0).abs() < 1e-3);
}
