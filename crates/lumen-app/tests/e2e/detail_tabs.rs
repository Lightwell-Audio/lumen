use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_project::model::Id;

use super::{Lumen, midi_play, wav};

fn palette(lumen: &mut Lumen, query: &str) {
    midi_play::palette(lumen, query);
    lumen.frames(5);
}

fn with_two_clips(name: &str) -> (Lumen, [Id; 2], [Id; 2]) {
    let mut lumen = Lumen::start(name);
    let kick = wav(&lumen.dir().join("kick.wav"), 1.0);
    let snare = wav(&lumen.dir().join("snare.wav"), 1.0);
    lumen.drop_files(&[kick, snare]);
    lumen.frames(40);
    assert_eq!(lumen.track_names(), ["kick", "snare"]);
    let tracks = &lumen.app().project().tracks;
    let ids = [tracks[0].id, tracks[1].id];
    let clips = [tracks[0].clips[0].id, tracks[1].clips[0].id];
    (lumen, ids, clips)
}

fn click_at(lumen: &mut Lumen, at: Pos2) {
    lumen.harness.hover_at(at);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
}

fn click_clip(lumen: &mut Lumen, clip: Id) {
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view");
    click_at(lumen, Pos2::new(r.left() + 12.0, r.top() + 6.0));
}

fn click_header(lumen: &mut Lumen, name: &str) {
    let at = lumen
        .harness
        .get_by_label(&format!("{name}: double-click to rename"))
        .rect()
        .center();
    click_at(lumen, at);
    lumen.frames(30);
}

fn on_clip_tab(lumen: &Lumen) -> bool {
    !lumen.app().devices_tab_shown() && !lumen.app().mixer_tab_shown()
}

#[test]
fn the_subject_is_the_track_of_the_clip_or_header_clicked_last() {
    let (mut lumen, [kick, snare], [kick_clip, snare_clip]) = with_two_clips("detail-subject");
    click_clip(&mut lumen, kick_clip);
    assert_eq!(lumen.app().detail_subject(), Some(kick));
    click_header(&mut lumen, "snare");
    assert_eq!(lumen.app().detail_subject(), Some(snare));
    click_clip(&mut lumen, kick_clip);
    assert_eq!(lumen.app().detail_subject(), Some(kick));
    click_clip(&mut lumen, snare_clip);
    assert_eq!(lumen.app().detail_subject(), Some(snare));
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert_eq!(lumen.app().detail_subject(), None);
    assert!(lumen.shows("No track selected"));
}

#[test]
fn enter_opens_an_audio_clip_in_the_clip_tab_and_a_header_click_goes_to_devices() {
    let (mut lumen, [kick, snare], [kick_clip, _]) = with_two_clips("detail-enter");
    assert!(!lumen.shows("Clip tab"), "no clip is open yet");
    click_clip(&mut lumen, kick_clip);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert_eq!(lumen.app().opened_clip_in_detail(), Some((kick_clip, true)));
    assert!(on_clip_tab(&lumen));
    assert!(lumen.shows("Clip tab"));
    assert_eq!(lumen.app().detail_subject(), Some(kick));

    click_header(&mut lumen, "snare");
    assert!(lumen.app().devices_tab_shown());
    assert_eq!(lumen.app().detail_subject(), Some(snare));
    assert_eq!(lumen.app().opened_clip_in_detail(), Some((kick_clip, true)));
}

#[test]
fn a_midi_clip_opens_in_the_note_editor_in_the_same_tab() {
    let mut lumen = Lumen::start("detail-midi");
    palette(&mut lumen, "Add MIDI track");
    lumen.frames(30);
    palette(&mut lumen, "Create MIDI clip");
    lumen.frames(10);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    assert_eq!(lumen.app().opened_clip_in_detail(), Some((clip, false)));
    assert!(on_clip_tab(&lumen));
    palette(&mut lumen, "Show devices");
    assert!(lumen.app().devices_tab_shown());
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    assert_eq!(lumen.app().opened_clip_in_detail(), Some((clip, false)));
    assert_eq!(lumen.app().editor_open_clip(), Some(clip));
    assert!(on_clip_tab(&lumen));
}

#[test]
fn deleting_the_opened_clip_goes_back_to_devices() {
    let (mut lumen, _, [kick_clip, _]) = with_two_clips("detail-delete");
    click_clip(&mut lumen, kick_clip);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(on_clip_tab(&lumen));
    click_clip(&mut lumen, kick_clip);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(5);
    assert!(lumen.app().devices_tab_shown());
    assert_eq!(lumen.app().opened_clip_in_detail(), None);
    assert!(!lumen.shows("Clip tab"));
}

#[test]
fn show_devices_clip_and_mixer_are_commands() {
    let (mut lumen, _, [kick_clip, _]) = with_two_clips("detail-commands");
    palette(&mut lumen, "Show mixer");
    assert!(lumen.app().mixer_tab_shown());
    palette(&mut lumen, "Show devices");
    assert!(lumen.app().devices_tab_shown());

    let before = lumen.app().devices_tab_shown();
    palette(&mut lumen, "Show clip");
    assert_eq!(lumen.app().devices_tab_shown(), before);

    click_clip(&mut lumen, kick_clip);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    palette(&mut lumen, "Show mixer");
    assert!(lumen.app().mixer_tab_shown());
    palette(&mut lumen, "Show clip");
    assert!(on_clip_tab(&lumen));
}

#[test]
fn clicking_the_already_selected_header_leaves_the_clip_tab() {
    let mut lumen = Lumen::start("detail-reclick");
    palette(&mut lumen, "Add MIDI track");
    lumen.frames(30);
    let name = lumen.track_names()[0].clone();
    click_header(&mut lumen, &name);
    palette(&mut lumen, "Create MIDI clip");
    lumen.frames(10);
    assert!(on_clip_tab(&lumen));
    click_header(&mut lumen, &name);
    assert!(lumen.app().devices_tab_shown());
}

#[test]
fn enter_belongs_to_an_open_window_not_to_the_selected_clip() {
    let (mut lumen, _, [kick_clip, _]) = with_two_clips("detail-enter-window");
    click_clip(&mut lumen, kick_clip);
    palette(&mut lumen, "Project settings");
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert_eq!(
        lumen.app().opened_clip_in_detail(),
        None,
        "nothing opened behind the window"
    );
}
