//! The bridge's tests.

use super::*;
use crate::document::{Document, Slot};

#[test]
fn plugins_are_promised_the_devices_block_at_most_the_engines() {
    assert_eq!(super::promised_frames(128), 128);
    assert_eq!(super::promised_frames(64), 64);
    assert_eq!(super::promised_frames(1024), 256, "the engine splits");
    assert_eq!(super::promised_frames(0), 256, "no stream yet");
}
use lumen_project::model::PluginRef;

/// A plugin with a stereo main output and fifteen stereo ones, and a track
/// playing each (the owner's orchestra with every output on).
fn orchestra() -> (Project, Vec<BusInfo>) {
    use lumen_project::model::{Color, PluginOutput};
    let bus = |n: usize| BusInfo {
        name: format!("St.{}", n + 1),
        channels: 2,
        // Every bus is flagged main, as a real orchestra plugin does.
        main: true,
        default_active: n == 0,
    };
    let outputs: Vec<BusInfo> = (0..16).map(bus).collect();
    let tracks = (1..16u32)
        .map(|b| {
            let mut t = Track::new(Id(100 + u64::from(b)), "Out", TrackKind::Audio, Color(0));
            t.plugin_output = Some(PluginOutput {
                device: Id(30),
                bus: b,
            });
            t
        })
        .collect();
    (
        lumen_project::testing::project_of("Outputs", tracks),
        outputs,
    )
}

#[test]
fn every_output_of_a_sixteen_output_plugin_is_turned_on_and_a_wide_main_leaves_less() {
    let (_, mut outputs) = orchestra();
    let mask = super::output_mask(&outputs);
    assert_eq!(
        mask, 0xFFFE,
        "fifteen pairs and the main pair are 32 channels"
    );
    // Past the block's room the lowest buses come first: a main bus of 40
    // channels leaves 24, twelve stereo pairs.
    outputs[0].channels = 40;
    let mask = super::output_mask(&outputs);
    assert_eq!(mask, 0b1_1111_1111_1110, "the lowest twelve");
}

fn known(values: &[(u32, f64)], base: &[(u32, f64)]) -> Parameters {
    Parameters {
        values: values.iter().copied().collect(),
        base: base.iter().copied().collect(),
        ..Parameters::default()
    }
}

fn held(values: &[(u32, f64)]) -> BTreeMap<u32, f64> {
    values.iter().copied().collect()
}

#[test]
fn a_value_the_project_holds_and_the_plugin_lacks_is_sent() {
    let p = known(&[(1, 0.5), (2, 0.5)], &[(1, 0.5), (2, 0.5)]);
    let mut send = p.differences(&held(&[(1, 0.9), (2, 0.5)]));
    send.sort_by_key(|&(id, _)| id);
    assert_eq!(send, [(1, 0.9)], "only what differs");
    assert!(p.differences(&held(&[(1, 0.5)])).is_empty());
}

#[test]
fn a_value_the_project_let_go_of_goes_back_to_what_it_was() {
    // The project held 0.9 for parameter 1, over a saved 0.5; an undo
    // takes the entry away: the plugin must go back to 0.5.
    let p = known(&[(1, 0.9)], &[(1, 0.5)]);
    assert_eq!(p.differences(&held(&[])), [(1, 0.5)]);
    // Already back: nothing to send.
    let p = known(&[(1, 0.5)], &[(1, 0.5)]);
    assert!(p.differences(&held(&[])).is_empty());
}

#[test]
fn a_value_set_inside_the_plugin_is_not_sent_back() {
    // The plugin reported 0.7 itself (known), then the document recorded
    // it: the project holds the same value, so nothing goes back.
    let p = known(&[(4, 0.7)], &[(4, 0.2)]);
    assert!(p.differences(&held(&[(4, 0.7)])).is_empty());
}

const CLASS: &str = "00112233445566778899AABBCCDDEEFF";

fn plugin(kind: DeviceKind) -> PluginRef {
    PluginRef::Vst3 {
        class_id: CLASS.into(),
        name: "Plug".into(),
        vendor: "Vendor".into(),
        kind,
    }
}

fn project_with(kind: DeviceKind, track: TrackKind) -> (Document, Id, Id) {
    let mut doc = Document::new(crate::empty_project());
    let t = doc.add_track(track, None);
    let device = doc.add_device(t, plugin(kind), Slot::End).unwrap();
    (doc, t, device)
}

fn mixer_handle() -> MixerHandle {
    let (_clips, players) = lumen_engine::clip_player();
    let (_midi, player) = lumen_engine::midi_player();
    lumen_engine::mixer(lumen_engine::Layered(players, player)).0
}

#[test]
fn a_plugin_that_cannot_be_found_is_missing_and_an_effect_keeps_its_saved_time() {
    let (mut doc, _, effect) = project_with(DeviceKind::Effect, TrackKind::Audio);
    // The plugin reported 512 frames at 48 kHz when the project was saved.
    doc.apply("report", None, |p| {
        let track = Arc::make_mut(&mut p.tracks[0]);
        track.devices[0].reported = Some(Reported {
            latency: 512,
            tail: 0,
            rate: 48_000,
        });
    });
    let mut hosting = Hosting::off();
    let mut mixer = mixer_handle();
    let mut latencies = Default::default();
    let catalog = Catalog::build(&Default::default(), Vec::new());
    hosting.update(
        doc.project(),
        &catalog,
        (96_000, 128),
        &mut mixer,
        &mut latencies,
    );
    assert_eq!(hosting.state(effect), Some(&State::Missing));
    assert_eq!(
        latencies.get(&effect),
        Some(&1_024),
        "scaled to the new rate"
    );
}

#[test]
fn only_one_instrument_on_a_midi_track_is_wanted() {
    let (doc, _, _) = project_with(DeviceKind::Instrument, TrackKind::Midi);
    assert_eq!(wanted(doc.project()).len(), 1);
    let (doc, _, _) = project_with(DeviceKind::Effect, TrackKind::Audio);
    let w = wanted(doc.project());
    assert_eq!(w.len(), 1);
    assert!(!w[0].instrument);
}

#[test]
fn a_device_that_leaves_the_project_is_forgotten() {
    let (mut doc, _, device) = project_with(DeviceKind::Effect, TrackKind::Audio);
    let mut hosting = Hosting::off();
    let mut mixer = mixer_handle();
    let mut latencies = Default::default();
    let catalog = Catalog::build(&Default::default(), Vec::new());
    hosting.update(
        doc.project(),
        &catalog,
        (48_000, 128),
        &mut mixer,
        &mut latencies,
    );
    assert!(hosting.state(device).is_some());
    doc.undo();
    hosting.update(
        doc.project(),
        &catalog,
        (48_000, 128),
        &mut mixer,
        &mut latencies,
    );
    assert!(hosting.state(device).is_none());
    assert!(latencies.is_empty());
}

#[test]
fn a_frozen_track_has_no_plugin_and_unfreezing_wants_it_again() {
    use lumen_project::edit::freeze::{Baked, plan};
    let (mut doc, track, device) = project_with(DeviceKind::Effect, TrackKind::Audio);
    // A clip to render, then freeze the track.
    doc.apply("clip", None, |p| {
        let mut clip = lumen_project::testing::small_project(1).tracks[2].clips[0].clone();
        Arc::make_mut(&mut clip).start = lumen_core::time::Ticks::ZERO;
        Arc::make_mut(&mut p.tracks[0]).clips.push(clip);
    });
    let media = doc.fresh_id();
    let baked: Vec<Baked> = plan(doc.project(), &[track])
        .parts
        .into_iter()
        .map(|part| Baked {
            end: part.end,
            part,
            media: lumen_project::model::Media {
                id: media,
                path: "frozen.wav".into(),
                original_path: None,
                frames: 48_000,
                channels: 2,
                sample_rate: 48_000,
                size_bytes: 0,
            },
            devices: Vec::new(),
        })
        .collect();
    assert_eq!(doc.freeze_tracks(&baked), vec![track]);
    assert!(
        wanted(doc.project()).is_empty(),
        "no host for a frozen track"
    );

    let mut hosting = Hosting::off();
    let mut mixer = mixer_handle();
    let mut latencies = Default::default();
    let catalog = Catalog::build(&Default::default(), Vec::new());
    hosting.update(
        doc.project(),
        &catalog,
        (48_000, 128),
        &mut mixer,
        &mut latencies,
    );
    assert!(hosting.state(device).is_none());
    doc.unfreeze_tracks(&[track]);
    hosting.update(
        doc.project(),
        &catalog,
        (48_000, 128),
        &mut mixer,
        &mut latencies,
    );
    assert!(hosting.state(device).is_some(), "it starts again");
}

/// One failure told twice (the engine gives up, then the host's end is seen)
/// counts once however far apart the two come; a failure after a reload
/// counts again. A slow macOS runner once heard the two more than a second
/// apart and showed "keeps stopping" a failure early.
#[test]
fn a_failure_told_twice_counts_once_and_one_after_a_reload_counts_again() {
    let mut failures = Vec::new();
    note_failure(&mut failures, &State::Ready);
    let failed = State::Failed("ended".into());
    note_failure(&mut failures, &failed);
    note_failure(&mut failures, &failed);
    assert_eq!(failures.len(), 1, "the same failure, told again later");
    note_failure(&mut failures, &State::Ready);
    note_failure(&mut failures, &State::Loading);
    assert_eq!(failures.len(), 3, "after reloads, each failure counts");
}

fn info(id: u32, title: &str, units: &str, steps: i32) -> ParamInfo {
    ParamInfo {
        id,
        title: title.to_owned(),
        units: units.to_owned(),
        steps,
        default: 0.0,
        flags: 1,
        unit: 0,
        value: 0.0,
    }
}

/// Parameters that know the words for a value and the steps of a chooser.
fn worded() -> Parameters {
    let mut p = Parameters {
        list: vec![info(1, "Cutoff", "Hz", 0), info(2, "Mode", "", 2)],
        ..Parameters::default()
    };
    p.remember_text(1, 0.5, "1.2 kHz".to_owned());
    p.choices
        .insert(2, vec!["Low".into(), "Band".into(), "High".into()]);
    p.choices_asked.insert(2);
    p
}

#[test]
fn a_list_read_again_with_new_names_units_or_steps_forgets_the_plugins_words() {
    // A patch that names its controls anew may also say their values
    // otherwise, at the same value (review of ADR-0125).
    for now in [
        vec![info(1, "Tone", "Hz", 0), info(2, "Mode", "", 2)],
        vec![info(1, "Cutoff", "%", 0), info(2, "Mode", "", 2)],
        vec![info(1, "Cutoff", "Hz", 0), info(2, "Mode", "", 3)],
    ] {
        let mut p = worded();
        assert!(p.forget_texts_if_renamed(&now));
        assert!(p.texts.is_empty() && p.choices.is_empty());
        assert!(p.choices_asked.is_empty(), "the steps are asked for again");
    }
}

#[test]
fn a_list_read_again_unchanged_keeps_the_words_unless_the_plugin_said_it_renamed() {
    // A checkpoint reads every value again: nothing renamed, nothing lost.
    let same = vec![info(1, "Cutoff", "Hz", 0), info(2, "Mode", "", 2)];
    let mut p = worded();
    assert!(!p.forget_texts_if_renamed(&same));
    assert_eq!(p.texts[&1], vec![(0.5, "1.2 kHz".to_owned())]);
    assert_eq!(p.choices[&2].len(), 3);
    // The plugin said its titles changed, though the list looks the same:
    // its words may have, and are asked for again.
    p.retitled = true;
    assert!(p.forget_texts_if_renamed(&same));
    assert!(p.texts.is_empty() && p.choices.is_empty());
    assert!(!p.retitled, "once");
}
