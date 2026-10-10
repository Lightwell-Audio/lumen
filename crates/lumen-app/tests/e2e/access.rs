//! Accessibility (M3 slice 8): every control a user can reach has a name a
//! screen reader can say, in the main window and in every window and menu
//! M3 added, and in M4's mixer, sends, returns and menus (M4 review). A control without one is a test failure, listed by role and
//! place so it can be found.

use eframe::egui::accesskit::{Action, Role};
use eframe::egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::{NodeT, Queryable, by};

use super::Lumen;
use super::properties::{select, with_two_clips};

/// Controls a user can act on that have no name, as "role at (x, y)".
fn unnamed(lumen: &Lumen) -> Vec<String> {
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
    lumen.click("Audio settings\u{2026}");
    check(&lumen, "audio settings", &mut problems);
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

/// What a screen reader says for the control with the keyboard focus:
/// its name, or its text.
fn focused(lumen: &Lumen) -> Option<String> {
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
    for _ in 0..40 {
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
