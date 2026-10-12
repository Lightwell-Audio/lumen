//! Clip properties and drops (M3 slice 6e): 0 mutes, the menu locks,
//! Ctrl+R renames in the title bar, the properties card (the owner's P1)
//! sets name, colour, gain and fades from the keyboard, and files dropped
//! on a track land on it, or between tracks make a track there.

use eframe::egui::{Color32, Event, FontFamily, FontId, Key, Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, Color, Id};

use super::{Lumen, wav};

/// Lumen with two two-second files dropped in: two tracks, a clip each.
pub(super) fn with_two_clips(name: &str) -> (Lumen, Id, Id) {
    let mut lumen = Lumen::start(name);
    let kick = wav(&lumen.dir().join("kick.wav"), 2.0);
    let snare = wav(&lumen.dir().join("snare.wav"), 2.0);
    lumen.drop_files(&[kick, snare]);
    // New tracks glide to their height: let them settle.
    lumen.frames(40);
    // In the order they were dropped, whichever loads first.
    assert_eq!(lumen.track_names(), ["kick", "snare"]);
    let tracks = &lumen.app().project().tracks;
    let (a, b) = (tracks[0].clips[0].id, tracks[1].clips[0].id);
    (lumen, a, b)
}

pub(super) fn rect(lumen: &Lumen, clip: Id) -> Rect {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view")
}

pub(super) fn clip(lumen: &Lumen, id: Id) -> Clip {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .map(|c| (**c).clone())
        .expect("the clip exists")
}

/// Clicks `clip`'s header, selecting it.
pub(super) fn select(lumen: &mut Lumen, id: Id) {
    let r = rect(lumen, id);
    let p = Pos2::new(r.left() + 12.0, r.top() + 6.0);
    lumen.harness.hover_at(p);
    lumen.harness.drag_at(p);
    lumen.frames(1);
    lumen.harness.drop_at(p);
    lumen.frames(2);
    assert_eq!(lumen.app().selected_clips(), [id]);
}

/// Right-clicks `clip` for the lanes' menu.
fn menu(lumen: &mut Lumen, id: Id) {
    let r = rect(lumen, id);
    let p = Pos2::new(r.left() + 12.0, r.top() + 6.0);
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
}

/// Types `text` into the focused field, replacing what is there, and
/// presses Enter.
fn type_in(lumen: &mut Lumen, text: &str) {
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(text.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn zero_mutes_and_unmutes_the_selected_clips_in_one_step() {
    let (mut lumen, kick, snare) = with_two_clips("clip-mute");
    select(&mut lumen, kick);
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(clip(&lumen, kick).muted);
    assert!(!clip(&lumen, snare).muted, "only the selection");
    assert_eq!(lumen.app().undo_label(), Some("Mute clip"));
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(!clip(&lumen, kick).muted, "0 again unmutes");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(clip(&lumen, kick).muted, "undo takes back one step");
}

#[test]
fn a_clip_locked_from_the_menu_stays_put_but_can_be_muted() {
    let (mut lumen, kick, _) = with_two_clips("clip-lock");
    menu(&mut lumen, kick);
    lumen.click("Lock clip");
    assert!(clip(&lumen, kick).locked);
    let start = clip(&lumen, kick).start;

    // A drag does nothing, nor does Delete; muting still works.
    let r = rect(&lumen, kick);
    let from = Pos2::new(r.left() + 12.0, r.top() + 6.0);
    lumen.drag(from, Pos2::new(from.x + 200.0, from.y), Modifiers::NONE);
    assert_eq!(clip(&lumen, kick).start, start);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    assert!(
        lumen.info().starts_with("Locked clip “kick”"),
        "{}",
        lumen.info()
    );
    select(&mut lumen, kick);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(clip(&lumen, kick).start, start, "still there");
    select(&mut lumen, kick);
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(clip(&lumen, kick).muted);

    // The menu unlocks.
    menu(&mut lumen, kick);
    lumen.click("Unlock clip");
    assert!(!clip(&lumen, kick).locked);
}

#[test]
fn ctrl_r_renames_a_clip_in_its_title_bar_and_esc_cancels() {
    let (mut lumen, kick, _) = with_two_clips("clip-rename");
    select(&mut lumen, kick);
    lumen.key(Modifiers::COMMAND, Key::R);
    assert!(lumen.shows("Clip name"), "a field in the title bar");
    type_in(&mut lumen, "Big kick");
    assert_eq!(clip(&lumen, kick).name, "Big kick");
    assert_eq!(lumen.app().undo_label(), Some("Rename clip"));
    assert!(!lumen.shows("Clip name"), "closed");

    // F2 too; Esc keeps the old name.
    lumen.key(Modifiers::NONE, Key::F2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Nope".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(clip(&lumen, kick).name, "Big kick");
}

#[test]
fn the_properties_card_edits_a_clip_from_the_keyboard() {
    let (mut lumen, kick, snare) = with_two_clips("clip-card");
    select(&mut lumen, kick);
    lumen.key(Modifiers::ALT, Key::Enter);
    assert!(
        lumen.shows("Clip name"),
        "the card is open, its name focused"
    );

    // The name, typed into the focused field.
    type_in(&mut lumen, "Kick in");
    assert_eq!(clip(&lumen, kick).name, "Kick in");

    // A colour, and the track's colour again.
    lumen.click("Colour 3");
    let third = lumen_app::testing::clip_colour(2);
    assert_eq!(clip(&lumen, kick).color, Some(third));
    lumen.click("Track colour");
    assert_eq!(clip(&lumen, kick).color, None::<Color>);

    // Gain and a fade, typed into their sliders.
    lumen.harness.get_by_label_contains("Clip gain").click();
    lumen.frames(2);
    lumen.harness.event(Event::Text("-6".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    let gain = match clip(&lumen, kick).content {
        ClipContent::Audio { gain_db, .. } => gain_db,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    };
    assert!((gain + 6.0).abs() < 1e-3, "gain {gain}");
    // Esc while typing cancels, and leaves the card open.
    lumen.harness.get_by_label_contains("Clip gain").click();
    lumen.frames(2);
    lumen.harness.event(Event::Text("-20".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    let after = match clip(&lumen, kick).content {
        ClipContent::Audio { gain_db, .. } => gain_db,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    };
    assert!((after - gain).abs() < 1e-3, "Esc kept {gain}, not {after}");
    assert!(lumen.shows("Clip name"), "the card stays open");
    lumen.harness.get_by_label_contains("Fade in: ").click();
    lumen.frames(2);
    lumen.harness.event(Event::Text("250".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    // At 120 BPM, 250 ms is an eighth note.
    assert_eq!(
        clip(&lumen, kick).fade_in.length,
        Ticks(Ticks::quarters(1).0 / 2)
    );

    // Mute and Lock; a locked clip's gain and fades wait.
    lumen.click("Mute clip");
    lumen.click("Lock clip");
    let c = clip(&lumen, kick);
    assert!(c.muted && c.locked);
    assert!(lumen.shows("Locked: unlock it to change gain and fades"));

    // Selecting another clip moves the card there; none closes it.
    select(&mut lumen, snare);
    lumen.frames(2);
    assert!(lumen.shows("Clip name"));
    assert!(!lumen.is_on("Lock clip"), "the snare's card");
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.shows("Clip name"), "closed");
}

#[test]
fn files_dropped_on_a_track_land_on_it_and_between_tracks_make_one_there() {
    let (mut lumen, kick, _) = with_two_clips("clip-drop");
    let tracks = lumen.app().project().tracks.len();
    let r = rect(&lumen, kick);

    // Onto the kick's lane, right of its clip: on that track.
    let hat = wav(&lumen.dir().join("hat.wav"), 0.5);
    lumen.drop_files_at(Pos2::new(r.right() + 150.0, r.center().y), &[hat]);
    let project = lumen.app().project();
    assert_eq!(project.tracks.len(), tracks, "no new track");
    let names: Vec<&str> = project.tracks[0]
        .clips
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["kick", "hat"]);
    assert_eq!(lumen.app().undo_label(), Some("Import hat"));

    // On the line between the two tracks: a new track there.
    let before = lumen.track_names();
    let line = rect(&lumen, lumen.app().project().tracks[0].clips[0].id).bottom() + 3.0;
    let bass = wav(&lumen.dir().join("bass.wav"), 0.5);
    lumen.drop_files_at(Pos2::new(r.right() + 150.0, line), &[bass]);
    assert_eq!(lumen.track_names(), [&before[0], "bass", &before[1]]);
}

/// The card's close button sits level with the middle of its title's
/// letters, not of the title's line, which leaves room below for
/// descenders (owner, MT-APP-14 run 3: it sat 6.5 points high, as it did
/// in every dialog).
#[test]
fn the_cards_close_button_is_level_with_its_title() {
    let (mut lumen, kick, _) = with_two_clips("card-title");
    select(&mut lumen, kick);
    lumen.key(Modifiers::ALT, Key::Enter);
    let close = lumen.harness.get_by_label("Close").rect();
    // The card's title: the "Clip" left of the close button, on its row
    // (not the detail panel's Clip tab).
    let title = lumen
        .harness
        .get_all_by_label("Clip")
        .map(|node| node.rect())
        .find(|r| r.right() < close.left() && (r.center().y - close.center().y).abs() < 12.0)
        .expect("the card's title");
    // Where the C is drawn in the display face, from the top of its line.
    let display = FontId::new(15.0, FontFamily::Name("display".into()));
    let galley = lumen
        .harness
        .ctx
        .fonts_mut(|f| f.layout_no_wrap("C".into(), display, Color32::WHITE));
    let placed = &galley.rows[0];
    let c = &placed.row.glyphs[0];
    let middle = placed.pos.y + c.pos.y + c.uv_rect.offset.y + c.uv_rect.size.y / 2.0;
    let off = close.center().y - (title.top() + middle);
    assert!(
        off.abs() <= 0.5,
        "the close button is {off} points off the title's letters"
    );
}
