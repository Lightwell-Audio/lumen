//! Accessibility (M3 slice 8): every control a user can reach has a name a
//! screen reader can say, in the main window and in every window and menu
//! M3 added, and in M4's mixer, sends, returns and menus (M4 review). A control without one is a test failure, listed by role and
//! place so it can be found.

use eframe::egui::accesskit::{Action, Role};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::{NodeT, Queryable, by};

use super::Lumen;
use super::properties::{select, with_two_clips};

/// Controls a user can act on that have no name, as "role at (x, y)".
pub(super) fn unnamed(lumen: &Lumen) -> Vec<String> {
    lumen
        .harness
        .query_all(by().predicate(|node| {
            let data = node.data();
            let acts = data.supports_action(Action::Click)
                || data.supports_action(Action::Focus)
                || matches!(
                    node.role(),
                    Role::TextInput | Role::Slider | Role::SpinButton | Role::CheckBox
                );
            let named = node.label().is_some_and(|l| !l.trim().is_empty())
                || node.value().is_some_and(|v| !v.trim().is_empty());
            // Containers (a menu's frame) are acted on through their
            // named children; scroll bars are known by their role.
            let container = node.children().next().is_some()
                && !matches!(node.role(), Role::Button | Role::Label);
            acts && !named && !container && !matches!(node.role(), Role::Window | Role::ScrollBar)
        }))
        .map(|node| {
            let r = node.rect();
            let kids: Vec<String> = node
                .accesskit_node()
                .children()
                .take(4)
                .map(|c| format!("{:?} {:?}", c.role(), c.label()))
                .collect();
            format!(
                "{:?} at ({:.0}, {:.0}) size {:.0}x{:.0} actions click {} focus {} kids {kids:?}",
                node.accesskit_node().role(),
                r.left(),
                r.top(),
                r.width(),
                r.height(),
                node.accesskit_node().data().supports_action(Action::Click),
                node.accesskit_node().data().supports_action(Action::Focus),
            )
        })
        .collect()
}

fn check(lumen: &Lumen, view: &str, problems: &mut Vec<String>) {
    for p in unnamed(lumen) {
        problems.push(format!("{view}: {p}"));
    }
}

#[test]
fn track_colour_swatches_have_names_and_recolour_in_one_undo_step() {
    let mut lumen = Lumen::start("track-colour-access");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(30);
    let before = lumen.app().project().tracks[0].color;
    let header = lumen.harness.get_by_label_contains("Track Audio 1").rect();
    lumen.right_click(header.center());
    let colour = lumen.harness.get_by_label("Color").rect();
    lumen.harness.hover_at(colour.center());
    lumen.frames(30);
    assert!(
        unnamed(&lumen).is_empty(),
        "color submenu controls are named"
    );
    lumen.click("Track color 2");
    assert_ne!(lumen.app().project().tracks[0].color, before);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].color, before);
}

#[test]
fn every_control_in_every_m3_view_has_a_name() {
    let (mut lumen, kick, _) = with_two_clips("access");
    let mut problems = Vec::new();
    select(&mut lumen, kick);
    check(&lumen, "main window", &mut problems);

    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("dup".into()));
    lumen.frames(2);
    check(&lumen, "palette", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(Event::Text("keyboard shortcuts".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    check(&lumen, "shortcuts window", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    select(&mut lumen, kick);
    lumen.key(Modifiers::ALT, Key::Enter);
    check(&lumen, "clip card", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.key(Modifiers::NONE, Key::Escape);

    // A clip's right-click menu.
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, kick).expect("in view");
    let p = r.left_top() + eframe::egui::vec2(12.0, 6.0);
    lumen.harness.hover_at(p);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
    check(&lumen, "clip menu", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    check(&lumen, "audio settings", &mut problems);
    lumen.click("MIDI tab");
    check(&lumen, "MIDI settings", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    lumen.click("Lumen");
    lumen.click("History\u{2026}");
    check(&lumen, "history", &mut problems);

    assert!(
        problems.is_empty(),
        "unnamed controls:\n{}",
        problems.join("\n")
    );
}

/// M4's views (review finding M4): the Mixer tab with a group, a return
/// and the master; tracks unfolded to their sends and a return to its
/// senders; every track selected (controls step up); a strip's output
/// menu and the mixer's pan-law menu.
#[test]
fn every_control_in_every_m4_view_has_a_name() {
    let mut lumen = Lumen::start("access-m4");
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::SHIFT, Key::ArrowUp);
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.click("Add track");
    lumen.click("Add return track");
    lumen.app_mut().set_detail_size(480.0);
    lumen.frames(3);
    let mut problems = Vec::new();
    let strips = lumen
        .harness
        .query_all(by().predicate(|n| n.label().is_some_and(|l| l.starts_with("Mixer strip"))))
        .count();
    assert!(strips >= 5, "the mixer shows its strips: {strips}");
    check(&lumen, "mixer", &mut problems);

    lumen.click("Fold Audio 3");
    lumen.click("Fold Return 1");
    check(&lumen, "sends and senders unfolded", &mut problems);

    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.frames(2);
    check(&lumen, "every track selected", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    lumen.click("Audio 3 output in the mixer: Master");
    check(&lumen, "output menu", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    let master = lumen
        .harness
        .get_by_label_contains("Mixer strip: Master,")
        .rect();
    lumen.right_click(eframe::egui::pos2(master.left() - 60.0, master.center().y));
    assert!(
        lumen.shows("\u{2212}6 dB, linear"),
        "the pan law menu is open"
    );
    check(&lumen, "pan law menu", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    assert!(
        problems.is_empty(),
        "unnamed controls:
{}",
        problems.join(
            "
"
        )
    );
}

#[test]
fn every_control_in_every_m7_view_has_a_name() {
    use lumen_project::automation::Target;

    use super::lane_view::{click_at, run_command, spot, tone};

    let (mut lumen, track) = tone("access-m7");
    let mut problems = Vec::new();

    // The lane over the clips, with its chip, tool strip and a point.
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    let at = spot(&lumen, track, 4.0, -12.0);
    click_at(&mut lumen, at);
    check(&lumen, "lane, tool strip and a point", &mut problems);

    // The chip's menu: parameters, the modes, the plugin list.
    lumen.click("Automation lane of tone: Volume");
    check(&lumen, "lane chip menu", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    // A control the lane drives, moved: the override state and its button.
    lumen.app_mut().set_track_volume(track, -3.0);
    lumen.frames(10);
    assert!(lumen.app().shows_lane_overridden(track, Target::Volume));
    check(&lumen, "lane taken over, with Re-enable", &mut problems);
    lumen.app_mut().reenable_lane(track, Target::Volume);
    lumen.frames(10);

    // The right-click menu every automatable control ends with.
    lumen.click("Fold tone");
    lumen.frames(40);
    let slider = lumen.harness.get_by_label_contains("Volume tone:").rect();
    lumen.right_click(slider.center());
    assert!(
        lumen.shows("Learn a controller\u{2026}"),
        "the control's menu is open"
    );
    check(&lumen, "control menu", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    // The mapping list, empty and with one mapping.
    run_command(&mut lumen, "Controller mappings");
    assert!(lumen.app().mappings_window_open());
    check(&lumen, "mappings window, empty", &mut problems);
    lumen
        .app_mut()
        .give_mapping(track, Target::Volume, 0, 7)
        .unwrap();
    lumen.frames(5);
    check(&lumen, "mappings window, one mapping", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);

    // The tempo and meter lanes, with a change in each.
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);
    let tempo = lumen.app().song_tempo_spot(8.0, 100.0).unwrap();
    click_at(&mut lumen, tempo);
    let meter = lumen.app().song_meter_spot(8.0).unwrap();
    click_at(&mut lumen, meter);
    check(&lumen, "tempo and meter lanes", &mut problems);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.key(Modifiers::NONE, Key::T);
    lumen.frames(40);

    // A clip's envelope, with its chip.
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::E);
    lumen.frames(40);
    assert!(
        lumen
            .app()
            .automation_shown(track)
            .is_some_and(|t| t.clip().is_some())
    );
    check(&lumen, "clip envelope", &mut problems);

    assert!(
        problems.is_empty(),
        "unnamed controls:\n{}",
        problems.join("\n")
    );
}

/// What a screen reader says for the control with the keyboard focus:
/// its name, or its text.
pub(super) fn focused(lumen: &Lumen) -> Option<String> {
    lumen
        .harness
        .query_all(by().predicate(|node| node.is_focused()))
        .next()
        .map(|node| {
            let n = node.accesskit_node();
            n.label().or_else(|| n.value()).unwrap_or_default()
        })
}

/// Where `name` comes in `order`.
fn place(order: &[String], name: &str) -> usize {
    order
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("{name} is never focused: {order:#?}"))
}

/// Tab goes through the window in reading order: the transport bar from
/// left to right (its panel toggles last, slice 8: they came first), then
/// the panes, each pane's name before its controls. Every stop is named.
#[test]
fn tab_walks_the_window_in_reading_order() {
    let (mut lumen, _, _) = with_two_clips("access-tab");
    let mut order = Vec::new();
    // Far enough to pass the browser's controls (M6 slice 1: its list is one
    // stop, the pane's, so rows cost no Tabs) and reach the arrangement.
    for _ in 0..60 {
        lumen.key(Modifiers::NONE, Key::Tab);
        order.push(focused(&lumen).unwrap_or_default());
    }
    assert!(
        order.iter().all(|n| !n.trim().is_empty()),
        "a nameless stop: {order:#?}"
    );
    let transport = [
        "Lumen",
        "Go to start",
        "Stop",
        "Play from the start marker",
        "Record",
        "Overdub",
        "Loop",
        "Metronome",
        "Follow playhead",
        "Tempo",
        "Command palette",
        "Browser",
        "Detail",
    ];
    let at: Vec<usize> = transport.iter().map(|n| place(&order, n)).collect();
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "left to right: {order:#?}"
    );
    let arrangement = place(&order, "Arrangement");
    assert!(at.iter().all(|&t| t < arrangement), "the transport first");
    assert!(place(&order, "kick: double-click to rename") > arrangement);
}

#[test]
fn switches_and_segmented_choices_work_by_name() {
    // M4.5 slice 2: the app menu's on/off settings are switches (named as
    // checkboxes) and its small choices segmented controls (radio buttons).
    // The owner's C: the menu stays open through changes, fades once the
    // pointer leaves it, and a command closes it at once.
    let mut lumen = Lumen::start("switches");
    lumen.click("Lumen");
    lumen.click("Mixing");
    assert!(!lumen.is_on("Meters in track colors"));
    lumen.click("Meters in track colors");
    assert!(
        lumen.is_on("Meters in track colors"),
        "a click switches it on"
    );
    assert!(
        lumen.shows("Project settings\u{2026}"),
        "and the menu stays open"
    );
    lumen.click("Appearance");
    // Not the theme or Reduce motion: they are process-wide, and would reach
    // the tests running beside this one.
    assert!(lumen.is_on("100%") && !lumen.is_on("125%"));
    lumen.click("125%");
    assert!(
        lumen.is_on("125%") && !lumen.is_on("100%"),
        "one choice at a time"
    );
    // The pointer leaves: after a moment, it fades away.
    lumen.harness.hover_at(egui::pos2(900.0, 500.0));
    lumen.frames(10);
    assert!(lumen.shows("Project settings\u{2026}"), "not at once");
    lumen.frames(60);
    assert!(
        !lumen.shows("Project settings\u{2026}"),
        "gone after the pause and fade"
    );
    // A command closes it straight away: its fading picture has no controls.
    lumen.click("Lumen");
    lumen.click("History\u{2026}");
    assert!(!lumen.shows("Project settings\u{2026}"));
}

#[test]
fn a_menus_segmented_controls_line_up_with_its_labels() {
    // M4.5 slice 6, the design sweep: switches, segmented controls and
    // captions sat at a menu's very edge, a little left of its rows'
    // labels. They now come in as far as the labels do.
    let mut lumen = Lumen::start("menu-inset");
    lumen.click("Lumen");
    lumen.click("Appearance");
    lumen.frames(30);
    // A segment sits 2 pt inside its control (`space::XXS`); in a menu the
    // control itself comes in as well, by the rows' padding.
    let row = lumen.harness.get_by_label("Reduce motion").rect();
    let dark = lumen.harness.get_by_label("Dark").rect();
    let inset = dark.left() - row.left();
    assert!(
        inset > 2.0 + 0.5,
        "the theme's choices start {inset} in from the row: only their own inset"
    );
}

#[test]
fn a_closing_menu_asks_for_the_frames_its_fade_needs() {
    // Owner, MT-POL-2: menus vanished instead of fading. The frame that
    // closed one asked for no next frame, so an idle window drew the next
    // one only after the fade was over.
    let mut lumen = Lumen::start("menu-fade");
    lumen.click("Lumen");
    lumen.frames(30);
    lumen.harness.get_by_label("History\u{2026}").click();
    // The frame that closes it is the first without its controls (from
    // then on only its picture fades): that frame must ask for the next.
    let mut closing_frame_asked = None;
    for _ in 0..5 {
        lumen.harness.step();
        if !lumen.shows("Project settings\u{2026}") {
            let causes = lumen.harness.ctx.repaint_causes();
            closing_frame_asked = Some(causes.iter().any(|c| c.file.ends_with("menu.rs")));
            break;
        }
    }
    assert_eq!(
        closing_frame_asked,
        Some(true),
        "the fade's next frame is asked for"
    );
}

/// The record controls are named (M5 slice 6d): Record and Overdub in the
/// bar, Overdub a toggle that is on after a click; the metronome's menu has
/// the count-in choices and the record-only switch, which stay where they
/// are as they are changed (the menu stays open).
#[test]
fn the_record_controls_are_named_and_the_count_in_is_in_the_metronome_menu() {
    let mut lumen = Lumen::start("access-record");
    assert!(lumen.shows("Record"), "the Record button is named");
    assert!(!lumen.is_on("Overdub"), "Overdub starts off");
    lumen.click("Overdub");
    assert!(lumen.is_on("Overdub"), "a click switches Overdub on");
    lumen.click("Overdub");
    assert!(!lumen.is_on("Overdub"), "and off again");

    let metronome = lumen.harness.get_by_label("Metronome").rect().center();
    lumen.right_click(metronome);
    assert!(lumen.shows("Count-in"), "the menu names its count-in");
    lumen.click("Count-in: 1 bar");
    assert!(lumen.shows("Only while recording"), "the menu stays open");
    assert!(!lumen.is_on("Only while recording"));
    lumen.click("Only while recording");
    assert!(lumen.is_on("Only while recording"), "the switch is on");
}
