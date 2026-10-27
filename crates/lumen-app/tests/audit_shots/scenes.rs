//! Visual capture scenarios; common app harness lives in the parent.
use super::*;

/// Owner's four visual corrections: compact tempo, menu organization,
/// equal letter caps and shortcut-search padding.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn owner_polish() {
    for appearance in ["dark", "light"] {
        for scale in [1.0, 1.5, 2.0] {
            let mut s = Scene::new("owner-polish", appearance, scale, (960.0, 540.0), false);
            s.snap("tempo-120");
            assert!(s.click("Tempo"));
            s.chord(Modifiers::COMMAND, Key::A);
            s.h.event(Event::Text("999.00".to_owned()));
            s.chord(Modifiers::NONE, Key::Enter);
            s.steps(10);
            s.snap("tempo-999");
            assert!(s.click("Lumen"));
            s.snap("app-menu");
            s.esc();
            s.command("Keyboard shortcuts");
            s.snap("shortcut-search");
            s.esc();
            s.drop_files(1.5);
            let clip = s.rect_of_track(0);
            right_click(&mut s.h, pos(clip, 0.2, 0.1));
            s.steps(12);
            let cut = s.find_row("Cut").expect("clip context menu");
            s.h.hover_at(cut.center());
            for _ in 0..3 {
                s.h.event(Event::MouseWheel {
                    unit: eframe::egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -640.0),
                    modifiers: Modifiers::NONE,
                    phase: eframe::egui::TouchPhase::Move,
                });
                s.steps(10);
            }
            let last = s.find_row("Properties").expect("the menu's last row");
            assert!(last.top() >= 0.0 && last.bottom() <= 540.0);
            s.snap("arrangement-menu");
        }
    }
}

/// Finds the names to drive the other scenes with.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn labels() {
    let mut s = Scene::new("labels", "dark", 1.0, (1280.0, 760.0), true);
    s.command("Add MIDI track");
    s.steps(20);
    s.dump("main");
    s.snap("labels");
}

// ---- Scenes ----

/// The empty window, the transport, every chip and the palette.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn chrome() {
    looks("chrome", false, |s| {
        s.snap("empty");
        s.click("Lumen");
        s.snap("app-menu");
        s.esc();
        s.click("Command palette");
        s.snap("palette-open");
        s.h.event(Event::Text("zzzz".into()));
        s.steps(4);
        s.snap("palette-none");
        s.esc();
        s.click("Audio:");
        s.snap("audio-chip");
        s.esc();
        s.click("MIDI:");
        s.snap("midi-chip");
        s.esc();
        for (n, label) in [
            "Go to start",
            "Stop",
            "Play from the start marker",
            "Record",
            "Overdub",
            "Loop",
            "Metronome",
            "Follow playhead",
            "Computer keyboard plays notes",
            "Clear all solos",
            "Tempo",
            "Time signature",
            "Position",
            "Command palette",
            "Browser",
            "Detail",
        ]
        .iter()
        .enumerate()
        {
            if s.hover(label) {
                s.snap(&format!("tip{n:02}"));
            }
            s.away();
        }
        for label in [
            "Loop",
            "Metronome",
            "Follow playhead",
            "Position",
            "Tempo",
            "Record",
        ] {
            if s.right(label) {
                s.snap(&format!("rc-{}", label.replace(' ', "-")));
                s.esc();
            }
        }
        for label in ["Tempo", "Time signature", "Position"] {
            if s.click(label) {
                s.snap(&format!("click-{}", label.replace(' ', "-")));
                s.esc();
            }
        }
        s.click("Browser");
        s.snap("browser-hidden");
        s.click("Detail");
        s.snap("both-hidden");
        s.click("Browser");
        s.click("Detail");
        s.command("Keyboard shortcuts");
        s.snap("shortcuts");
        s.esc();
    });
}

/// Every menu on the arrangement: clips, lanes, rulers, tracks, the add menu
/// and the toolbar.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn arrangement() {
    looks("arrangement", true, |s| {
        s.snap("rest");
        let kick = s.rect_of_track(0);
        // A clip's menu, a clip selection and the range's menu.
        click(&mut s.h, pos(kick, 0.5, 0.4));
        s.steps(4);
        s.snap("clip-selected");
        right_click(&mut s.h, pos(kick, 0.5, 0.4));
        s.steps(6);
        s.snap("clip-menu");
        s.esc();
        s.chord(Modifiers::COMMAND, Key::A);
        right_click(&mut s.h, pos(kick, 0.5, 0.4));
        s.steps(6);
        s.snap("clips-menu");
        s.esc();
        s.chord(Modifiers::NONE, Key::Escape);
        // The info line over a clip's header, then with Ctrl held: what Ctrl
        // does lights and the rest dims (the interface audit's I1).
        s.h.hover_at(pos(kick, 0.5, 0.15));
        s.steps(4);
        s.snap("info-clip");
        s.h.event(eframe::egui::Event::ModifiersChanged(Modifiers::COMMAND));
        s.steps(4);
        s.snap("info-clip-ctrl");
        s.h.event(eframe::egui::Event::ModifiersChanged(Modifiers::NONE));
        // Empty lanes.
        right_click(&mut s.h, pos(kick, 6.0, 0.5));
        s.steps(6);
        s.snap("lanes-menu");
        s.esc();
        // A time range, and its menu.
        s.h.drag_at(pos(kick, 1.4, 0.7));
        s.h.run_steps(2);
        s.h.hover_at(pos(kick, 3.4, 0.7));
        s.h.run_steps(2);
        s.h.drop_at(pos(kick, 3.4, 0.7));
        s.steps(6);
        s.snap("range");
        right_click(&mut s.h, pos(kick, 2.4, 0.7));
        s.steps(6);
        s.snap("range-menu");
        s.esc();
        s.chord(Modifiers::NONE, Key::Escape);
        // Rulers.
        for (n, label) in [
            "Ruler: click to move",
            "Time ruler: drag",
            "Scrub strip",
            "Loop brace",
        ]
        .iter()
        .enumerate()
        {
            if s.right(label) {
                s.snap(&format!("ruler-menu{n}"));
                s.esc();
            }
        }
        // The toolbar.
        for (n, label) in [
            "Grid: ",
            "Arrangement tool",
            "Tempo and meter lanes",
            "Time format",
            "Set locator",
        ]
        .iter()
        .enumerate()
        {
            if s.click(label) {
                s.snap(&format!("toolbar{n}"));
                s.esc();
            }
        }
        // Tracks.
        for (n, label) in [
            "Track kick",
            "Track Master",
            "Mute kick",
            "Arm kick",
            "kick: double-click to rename",
        ]
        .iter()
        .enumerate()
        {
            if s.right(label) {
                s.snap(&format!("track-menu{n}"));
                s.esc();
            }
        }
        if s.click("Add track") {
            s.snap("add-track-menu");
            s.esc();
        }
        s.command("Add MIDI track");
        s.steps(10);
        s.snap("midi-track-added");
        if s.right("Track MIDI 1") {
            s.snap("midi-track-menu");
            s.esc();
        }
        s.command("Clip properties");
        s.snap("clip-properties");
        s.esc();
        // A group of two tracks.
        let ids: Vec<_> =
            s.h.state()
                .project()
                .tracks
                .iter()
                .take(2)
                .map(|t| t.id)
                .collect();
        s.h.state_mut().select_tracks(&ids);
        s.chord(Modifiers::COMMAND, Key::G);
        s.steps(20);
        s.snap("group");
        s.chord(Modifiers::NONE, Key::ArrowLeft);
        s.steps(20);
        s.snap("group-folded");
        // Zoomed far in and far out.
        for _ in 0..8 {
            s.chord(Modifiers::NONE, Key::Equals);
        }
        s.snap("zoomed-in");
        for _ in 0..16 {
            s.chord(Modifiers::NONE, Key::Minus);
        }
        s.snap("zoomed-out");
        s.command("Show info line");
        s.snap("info-line");
        s.command("Tempo and meter lanes");
        s.snap("song-lanes");
    });
}

fn double_click(h: &mut H, p: Pos2) {
    click(h, p);
    click(h, p);
}

/// The mixer, the add menu, the device chain with a synth, and the clip tab
/// before and after a MIDI clip exists.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mixer_and_devices() {
    looks("mixer", true, |s| {
        s.command("Add MIDI track");
        s.steps(10);
        s.click("Mixer tab");
        s.snap("mixer");
        for (n, label) in [
            "kick fader in the mixer",
            "kick pan in the mixer",
            "Mixer strip: kick",
            "kick in the mixer: double-click",
            "Mixer strip: Master",
            "Master fader in the mixer",
        ]
        .iter()
        .enumerate()
        {
            if s.right(label) {
                s.snap(&format!("mixer-menu{n}"));
                s.esc();
            }
        }
        if s.click("Add track in the mixer") {
            s.snap("mixer-add");
            s.esc();
        }
        if s.click("Cue") {
            s.snap("cue");
        }
        s.click_last("Devices tab");
        s.snap("devices-empty-audio");
        s.h.state_mut().select_tracks(&[]);
        let midi =
            s.h.state()
                .project()
                .tracks
                .iter()
                .find(|t| t.name == "MIDI 1")
                .map(|t| t.id);
        if let Some(id) = midi {
            s.h.state_mut().select_tracks(&[id]);
        }
        s.steps(10);
        s.snap("devices-midi-empty");
        if let Some(r) = s.find("Synth, Lumen, instrument") {
            double_click(&mut s.h, r.center());
            s.steps(30);
        }
        s.snap("devices-synth");
        for (n, label) in ["Synth, Lumen", "Bypass", "Preset", "Parameters"]
            .iter()
            .enumerate()
        {
            if s.right(label) {
                s.snap(&format!("device-menu{n}"));
                s.esc();
            }
        }
        s.click("Clip tab");
        s.snap("clip-tab-empty");
        s.command("Create MIDI clip");
        s.steps(20);
        s.snap("clip-tab-new-midi");
    });
}

/// The MIDI clip editor and its menus, from the demo project.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn midi_editor() {
    looks_in("midi", "midi-demo.lumen", |s| {
        s.h.state_mut().set_detail_size(430.0);
        s.steps(10);
        s.snap("project");
        let id = s.h.state().project().tracks[0].clips[0].id;
        let r = s.h.state().clip_rect(&s.h.ctx, id).expect("in view");
        click(&mut s.h, pos(r, 0.1, 0.6));
        click(&mut s.h, pos(r, 0.1, 0.6));
        s.steps(30);
        s.snap("editor");
        right_click(&mut s.h, pos(r, 0.3, 0.6));
        s.steps(6);
        s.snap("clip-menu-midi");
        s.esc();
        for name in [
            "Quantize options",
            "Velocity and controller lane",
            "Mod wheel lane",
            "Pitch bend lane",
            "Pressure lane",
            "Humanize notes",
            "Constrain to scale",
            "Pop out clip editor",
        ] {
            s.command(name);
            s.steps(10);
            s.snap(&name.replace(' ', "-").to_lowercase());
            s.esc();
        }
        if let Some(p) =
            s.h.state()
                .editor_point(lumen_core::time::Ticks::quarters(4), 60)
        {
            right_click(&mut s.h, p + vec2(10.0, 0.0));
            s.steps(6);
            s.snap("editor-menu");
            s.esc();
        }
    });
}

/// Automation lanes and the song lanes, from the demo project.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation() {
    looks_in("auto", "automation-demo.lumen", |s| {
        s.snap("project");
        s.command("Show or hide automation lanes");
        s.steps(20);
        s.snap("lanes");
        s.command("Choose the automation parameter");
        s.steps(10);
        s.snap("chooser");
        s.esc();
        s.command("Show or hide the tempo and meter lanes");
        s.steps(20);
        s.snap("song-lanes");
        s.command("Controller mappings");
        s.steps(10);
        s.snap("mappings");
        s.esc();
        s.command("Show or hide the clip's envelope");
        s.steps(10);
        s.snap("envelope");
        for name in [
            "Automation mode: Touch",
            "Automation tool: sine",
            "Add a tempo change at the playhead",
        ] {
            s.command(name);
            s.steps(10);
            s.snap(&name.replace([' ', ':'], "-").to_lowercase());
        }
    });
}

/// Windows: history, audio settings, preferences, project dialogs.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn windows() {
    looks("windows", true, |s| {
        s.chord(Modifiers::COMMAND, Key::D);
        s.click("Lumen");
        s.click("History");
        s.steps(10);
        s.snap("history");
        s.esc();
        s.click("Lumen");
        s.click("Audio and MIDI settings");
        s.steps(20);
        s.snap("audio-settings");
        s.esc();
        s.click("Lumen");
        s.click("Open recent");
        s.snap("open-recent");
        s.esc();
        s.esc();
        s.command("New project");
        s.steps(20);
        s.snap("new-project-unsaved");
        s.esc();
    });
}

/// Submenus of the clip and track menus, opened by hovering their rows.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn submenus() {
    looks("submenus", true, |s| {
        let kick = s.rect_of_track(0);
        click(&mut s.h, pos(kick, 0.5, 0.4));
        for (n, row) in ["Edit", "Colour", "Fades"].iter().enumerate() {
            right_click(&mut s.h, pos(kick, 0.5, 0.4));
            s.steps(6);
            if let Some(r) = s.find_row(row) {
                click(&mut s.h, r.center());
                s.steps(12);
                s.snap(&format!("clip-sub{n}"));
            } else {
                report(&format!("MISSING  [{}] no row \"{row}\"", s.tag));
            }
            s.esc();
            s.esc();
        }
        s.h.state_mut().select_tracks(&[]);
        for (n, row) in ["Colour", "Bounce"].iter().enumerate() {
            if !s.right("Track kick") {
                continue;
            }
            if let Some(r) = s.find_row(row) {
                click(&mut s.h, r.center());
                s.steps(12);
                s.snap(&format!("track-sub{n}"));
            }
            s.esc();
            s.esc();
        }
    });
}

/// Panels hidden by the Browser and Detail buttons: are their controls still
/// in the accessibility tree (reachable by Tab and a screen reader)?
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn hidden_panels() {
    let mut s = Scene::new("hidden", "dark", 1.0, (1280.0, 760.0), true);
    s.click("Browser");
    s.click("Detail");
    s.steps(90);
    let screen = s.h.ctx.content_rect();
    let outside: Vec<String> =
        s.h.query_all(by().predicate(|n| n.label().is_some_and(|l| !l.trim().is_empty())))
            .filter(|n| !n.rect().intersects(screen))
            .map(|n| {
                format!(
                    "{:?} \"{}\"",
                    n.accesskit_node().role(),
                    n.accesskit_node().label().unwrap_or_default()
                )
            })
            .collect();
    report(&format!(
        "HIDDEN-PANELS {} labelled controls outside the window after 90 frames",
        outside.len()
    ));
    for o in outside.iter().take(12) {
        report(&format!("    {o}"));
    }
}

/// The corrected menus in both themes at all required scales. The lower
/// grid options must actually be visible after scrolling, not just present
/// in the accessibility tree.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn corrected_menus() {
    use eframe::egui::{MouseWheelUnit, TouchPhase};
    for appearance in ["dark", "light"] {
        for scale in [1.0, 1.5, 2.0] {
            let mut s = Scene::new("corrected", appearance, scale, (960.0, 540.0), false);
            assert!(s.click("Lumen"));
            s.snap("project-menu");
            s.esc();
            assert!(s.click("Grid: "));
            s.snap("grid-top");
            let top = s.find("Narrower").expect("grid row");
            s.h.hover_at(top.center());
            for _ in 0..3 {
                s.h.event(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -640.0),
                    modifiers: Modifiers::NONE,
                    phase: TouchPhase::Move,
                });
                s.steps(10);
            }
            let dotted = s.find("Dotted").expect("bottom grid option");
            assert!(
                dotted.top() >= 0.0 && dotted.bottom() <= 540.0,
                "{dotted:?}"
            );
            s.h.get_by_label("Dotted").focus();
            s.steps(2);
            s.snap("grid-bottom-focused");
            s.esc();
            assert!(s.click("Audio:"));
            s.snap("audio-popover");
            let tone = s.find("Test tone").expect("audio panel");
            s.h.hover_at(tone.center());
            for _ in 0..3 {
                s.h.event(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -640.0),
                    modifiers: Modifiers::NONE,
                    phase: TouchPhase::Move,
                });
                s.steps(10);
            }
            let reset = s.find("Reset overload").expect("last audio action");
            assert!(reset.top() >= 0.0 && reset.bottom() <= 540.0, "{reset:?}");
            s.snap("audio-bottom");
        }
    }
}

/// The plugins the cleanup audit's browser lists: three vendors besides
/// Lumen's own, a dozen categories and a blocked module (anonymous names).
const PRETEND: [(&str, &[(&str, &str)]); 3] = [
    (
        "Studio A",
        &[
            ("Bus compressor", "Fx|Dynamics"),
            ("Plate reverb", "Fx|Reverb"),
            ("Tape delay", "Fx|Delay"),
            ("Channel EQ", "Fx|EQ"),
        ],
    ),
    (
        "Studio B",
        &[
            ("Analog poly", "Instrument|Synth"),
            ("Drum machine", "Instrument|Drum"),
            ("Electric piano", "Instrument|Piano"),
            ("Saturator", "Fx|Distortion"),
        ],
    ),
    (
        "Studio C with a long vendor name",
        &[
            ("Spectrum analyzer with a long name", "Fx|Analyzer"),
            ("Chorus", "Fx|Modulation"),
            ("Limiter", "Fx|Dynamics"),
        ],
    ),
];

/// The browser (the cleanup audit, 2026-10-27): a filled list, its search,
/// filters, chips, folds, a selection, a device's menu, a blocked module and
/// the empty states.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn browser() {
    looks("browser", true, |s| {
        s.h.state_mut()
            .pretend_plugins(&PRETEND, &["Hangs on load"]);
        s.steps(10);
        s.snap("filled");
        s.hover("Plate reverb, Studio A");
        s.steps(4);
        s.snap("row-hover");
        s.away();
        s.click("Plate reverb, Studio A");
        s.steps(4);
        s.snap("row-selected");
        if s.right("Plate reverb, Studio A") {
            s.snap("device-menu");
            s.esc();
        }
        s.click("Hangs on load, blocked");
        s.steps(6);
        s.snap("blocked-note");
        if s.right("Hangs on load, blocked") {
            s.snap("blocked-menu");
            s.esc();
        }
        s.click("Search devices");
        s.h.event(Event::Text("verb".into()));
        s.steps(6);
        s.snap("search");
        s.h.event(Event::Text("zzz".into()));
        s.steps(6);
        s.snap("search-none");
        s.click("Clear search");
        s.steps(4);
        s.click("Instruments");
        s.steps(4);
        s.snap("instruments");
        s.click("All");
        s.steps(4);
        s.click("Dynamics");
        s.steps(4);
        s.snap("chip");
        s.away();
    });
}

/// The mixer with many kinds of strip: audio, MIDI, a group, two returns,
/// the master; a selected strip, menus and a short panel (the cleanup audit).
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mixer_full() {
    looks("mixer-full", true, |s| {
        s.command("Add MIDI track");
        for _ in 0..2 {
            s.click("Add track");
            s.click("Add return track");
            s.steps(6);
        }
        let ids: Vec<_> =
            s.h.state()
                .project()
                .tracks
                .iter()
                .take(2)
                .map(|t| t.id)
                .collect();
        s.h.state_mut().select_tracks(&ids);
        s.chord(Modifiers::COMMAND, Key::G);
        s.steps(20);
        s.h.state_mut().set_detail_size(420.0);
        s.click_last("Mixer tab");
        s.steps(20);
        s.snap("mixer");
        s.h.state_mut().select_tracks(&ids[..1]);
        s.steps(6);
        s.snap("selected");
        for (n, label) in [
            "Output of kick in the mixer",
            "kick send A in the mixer",
            "+",
        ]
        .iter()
        .enumerate()
        {
            if s.click(label) {
                s.snap(&format!("pop{n}"));
                s.esc();
            }
        }
        s.hover("kick fader in the mixer");
        s.steps(4);
        s.snap("fader-hover");
        s.away();
        s.h.state_mut().set_detail_size(240.0);
        s.steps(20);
        s.snap("short");
    });
}
