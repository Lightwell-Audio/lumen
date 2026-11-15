use std::collections::BTreeMap;

use eframe::egui::{Event, Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_devices::Preset as SynthPreset;
use lumen_project::model::PluginRef;
use lumen_project::testing::TempDir;

use super::lane_view::run_command;
use super::{Lumen, access};

const EFFECTS: [(&str, &str); 6] = [
    ("Prism", "utility"),
    ("Iris", "eq"),
    ("Parallax", "delay"),
    ("Afterglow", "reverb"),
    ("Gravity", "compressor"),
    ("Horizon", "limiter"),
];

fn row(lumen: &Lumen, name: &str) -> Pos2 {
    lumen
        .harness
        .get_by_label_contains(&format!("{name}, Lumen"))
        .rect()
        .center()
}

fn shown(lumen: &Lumen, label: &str) -> bool {
    lumen
        .harness
        .query_all_by_label_contains(label)
        .next()
        .is_some()
}

fn devices(lumen: &Lumen, track: usize) -> Vec<String> {
    lumen
        .app()
        .project()
        .tracks
        .get(track)
        .map(|track| {
            track
                .devices
                .iter()
                .filter_map(|d| match &d.plugin {
                    PluginRef::Builtin { name } => Some(name.clone()),
                    PluginRef::Vst3 { .. } => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn with_a_track(name: &str) -> (Lumen, Pos2) {
    let mut lumen = Lumen::start(name);
    lumen.key(Modifiers::COMMAND, Key::T);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(10);
    let header = lumen
        .harness
        .get_by_label_contains("Track Audio 1")
        .rect()
        .center();
    (lumen, header)
}
#[test]
fn each_built_in_effect_is_added_by_a_double_click_in_one_undo_step() {
    let (mut lumen, _) = with_a_track("browser-builtin-add");
    for (name, device) in EFFECTS {
        lumen.double_click(row(&lumen, name));
        assert_eq!(devices(&lumen, 0), [device.to_owned()], "{name} is added");
        assert_eq!(lumen.app().undo_label(), Some(&*format!("Add {name}")));
        lumen.key(Modifiers::COMMAND, Key::Z);
        assert!(
            devices(&lumen, 0).is_empty(),
            "{name} is undone in one step"
        );
    }
}

#[test]
fn each_built_in_effect_is_added_by_dragging_its_row_to_a_track_or_its_chain() {
    let (mut lumen, _) = with_a_track("browser-builtin-drag");
    for (name, device) in EFFECTS {
        let header = lumen
            .harness
            .get_by_label_contains("Track Audio 1")
            .rect()
            .center();
        lumen.drag(row(&lumen, name), header, Modifiers::NONE);
        assert_eq!(devices(&lumen, 0), [device.to_owned()], "{name} is dropped");
        assert_eq!(lumen.app().undo_label(), Some(&*format!("Add {name}")));
        lumen.key(Modifiers::COMMAND, Key::Z);
        assert!(devices(&lumen, 0).is_empty());
    }

    lumen.show_whole_cards();
    let chain = lumen
        .harness
        .get_by_label("Add a device to the chain")
        .rect()
        .center();
    lumen.drag(row(&lumen, "Gravity"), chain, Modifiers::NONE);
    assert_eq!(devices(&lumen, 0), ["compressor"], "dropped in the chain");
}

#[test]
fn the_lumen_filter_shows_only_the_built_ins_and_combines_with_a_category() {
    let mut lumen = Lumen::start("browser-lumen-only");
    lumen.app_mut().pretend_plugins(
        &[(
            "Vendor A",
            &[
                ("Bus Compressor", "Fx|Dynamics"),
                ("Room Reverb", "Fx|Reverb"),
            ],
        )],
        &[],
    );
    lumen.frames(5);
    assert!(shown(&lumen, "Bus Compressor, Vendor A"));
    assert!(shown(&lumen, "Gravity, Lumen"));

    run_command(&mut lumen, "Show only Lumen's devices");
    assert!(!shown(&lumen, "Bus Compressor"));
    assert!(!shown(&lumen, "Room Reverb"));
    assert!(shown(&lumen, "Flare, Lumen"));
    assert!(shown(&lumen, "Gravity, Lumen"));

    lumen.click("Dynamics filter");
    assert!(lumen.is_on("Dynamics filter"));
    assert!(shown(&lumen, "Gravity, Lumen"));
    assert!(shown(&lumen, "Horizon, Lumen"));
    assert!(!shown(&lumen, "Flare, Lumen"));
    assert!(!shown(&lumen, "Iris, Lumen"));
    assert!(!shown(&lumen, "Bus Compressor"));

    run_command(&mut lumen, "Show only Lumen's devices");
    assert!(
        shown(&lumen, "Bus Compressor"),
        "the filter turned off and the category still narrows"
    );
}

#[test]
fn a_search_for_a_kind_tag_finds_the_built_in_it_describes() {
    let mut lumen = Lumen::start("browser-kind-search");
    lumen.key(Modifiers::COMMAND, Key::F);
    lumen.harness.event(Event::Text("comp".into()));
    lumen.frames(3);
    assert!(shown(&lumen, "Gravity, Lumen"));
    assert!(shown(&lumen, "Dynamics, 1 device"));
    assert!(!shown(&lumen, "Horizon, Lumen"));
    assert!(!shown(&lumen, "Flare, Lumen"));

    lumen.click("Clear search");
    lumen.harness.event(Event::Text("reverb".into()));
    lumen.frames(3);
    assert!(shown(&lumen, "Afterglow, Lumen"));
    assert!(!shown(&lumen, "Gravity, Lumen"));

    lumen.click("Clear search");
    lumen.harness.event(Event::Text("eq".into()));
    lumen.frames(3);
    assert!(shown(&lumen, "Iris, Lumen"));
}

#[test]
fn add_device_opens_the_browser_on_devices_with_its_search_ready() {
    let mut lumen = Lumen::start("browser-add-device");
    lumen.click("Samples tab");
    assert_eq!(lumen.app().browser_tab_name(), "Samples");

    run_command(&mut lumen, "Add device\u{2026}");
    assert_eq!(lumen.app().browser_tab_name(), "Devices");
    assert!(lumen.shows("Search devices"));
    lumen.run_until("the search field to have the keyboard", |lumen| {
        access::focused(lumen).as_deref() == Some("Search devices")
    });
}

#[test]
fn flares_factory_presets_are_in_its_preset_menu_and_load_in_one_undo_step() {
    let dir = TempDir::new("e2e-browser-factory-presets");
    let presets = dir.path().join("presets");
    let with_presets = presets.clone();
    let mut lumen = Lumen::start_with(dir, |launch| {
        launch.with_preset_folders(with_presets, Vec::new())
    });
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    lumen.frames(10);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(5);
    lumen.show_whole_cards();
    let device = lumen.app().project().tracks[0]
        .instrument()
        .expect("a new MIDI track plays Flare")
        .id;

    lumen.click("Preset of Flare");
    assert!(lumen.shows("Search presets"));
    lumen.click("Pad");

    let track = &lumen.app().project().tracks[0];
    assert_eq!(
        track.instrument().and_then(|d| d.preset()),
        Some("pad"),
        "the card's preset is Flare's Pad"
    );
    let held: BTreeMap<u32, f64> = lumen.app().device_params(device).into_iter().collect();
    let wanted: BTreeMap<u32, f64> = SynthPreset::Pad
        .values()
        .into_iter()
        .map(|(id, value)| (id, f64::from(value)))
        .collect();
    assert_eq!(held, wanted, "the values are the factory preset's");
    assert_eq!(lumen.app().undo_label(), Some("Instrument: Pad"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        lumen.app().project().tracks[0]
            .instrument()
            .and_then(|d| d.preset()),
        Some("poly"),
        "one step back is the preset the track had"
    );
}
