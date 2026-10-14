//! Overdub at clip boundaries (ADR-0058), end to end: the owner's report on
//! MIDI Arrangement Overdub.
//!
//! 1. Recording Overdub over an empty lane shows its clip from the moment
//!    recording begins, before anything is played.
//! 2. Overdub begun inside a clip, over a note that is sounding: the new
//!    clip keeps the note whole, so it carries on across the boundary
//!    instead of looking cut off there.
//! 3. That note sustains while recording and when played back: it is
//!    struck once and let go once, not cut at the boundary or struck again
//!    at each commit (about once a second).
//! 4. Played from the new clip's start, the note is not struck again: it
//!    belongs to the clip it began in (ADR-0059).

use lumen_core::time::Ticks;
use lumen_project::edit::seams;
use lumen_project::model::{Id, Note};
use lumen_project::testing::project_of;

use super::midi_play::{palette, wait_open};
use super::record::{
    beats, clips_of, heard_project, keyboard, launch, play_to, reach, record, stop, track, track_at,
};

/// The note-ons and note-offs the fake output was sent for `pitch`.
fn ons_and_offs(fake: &lumen_midi::fake::FakeControl, pitch: u8) -> (usize, usize) {
    let sent: Vec<Vec<u8>> = fake
        .sent()
        .into_iter()
        .map(|(_, bytes)| bytes)
        .filter(|b| b.len() == 3 && b[1] == pitch)
        .collect();
    let on = |b: &Vec<u8>| b[0] & 0xF0 == 0x90 && b[2] > 0;
    let off = |b: &Vec<u8>| b[0] & 0xF0 == 0x80 || (b[0] & 0xF0 == 0x90 && b[2] == 0);
    (
        sent.iter().filter(|b| on(b)).count(),
        sent.iter().filter(|b| off(b)).count(),
    )
}

#[test]
fn an_overdub_over_an_empty_lane_shows_its_clip_before_any_note() {
    let fake = keyboard();
    let project = project_of("Record", vec![track(None, true)]);
    let mut lumen = launch("overdub-empty", &project, &fake);
    palette(&mut lumen, "Overdub");
    assert!(lumen.app().overdub_on(), "Overdub is on");
    record(&mut lumen);
    reach(&mut lumen, beats(1.0));
    lumen.frames(1);
    let drawn = lumen.app().drawn_clips(0);
    assert_eq!(drawn.len(), 1, "the clip shows at once: {drawn:?}");
    assert_eq!(drawn[0].start, Ticks::ZERO, "from where recording began");
    // Past a commit (about once a second at 120 BPM) the clip is in the
    // project too, and it stays when nothing was played.
    reach(&mut lumen, beats(3.0));
    let stopped = stop(&mut lumen);
    let clips = clips_of(&lumen);
    assert_eq!(clips.len(), 1, "{clips:?}");
    assert_eq!(
        (clips[0].start, clips[0].start + clips[0].length),
        (Ticks::ZERO, stopped)
    );
}

#[test]
fn a_note_held_where_overdub_begins_sustains_across_the_new_clip() {
    // At 480 BPM a beat lasts 125 ms: beats 3 to 24 hold two commits.
    let (mut project, fake) = heard_project(vec![]);
    let mut keys = track_at((0, 32), vec![Note::new(beats(1.0), beats(30.0), 50, 100)]);
    keys.midi_to = project.tracks[0].midi_to.clone();
    project.tracks = vec![std::sync::Arc::new(keys)];
    let mut lumen = launch("overdub-held", &project, &fake);
    wait_open(&mut lumen, &fake);
    palette(&mut lumen, "Overdub");
    play_to(&mut lumen, beats(3.0));
    assert_eq!(ons_and_offs(&fake, 50), (1, 0), "the note sounds");
    record(&mut lumen);
    reach(&mut lumen, beats(24.0));
    assert_eq!(
        ons_and_offs(&fake, 50),
        (1, 0),
        "neither cut where recording began nor struck again at a commit"
    );
    stop(&mut lumen);

    // The new clip keeps the note whole, so it carries on through it.
    let clips = clips_of(&lumen);
    assert_eq!(clips.len(), 3, "before, the take, after: {clips:?}");
    assert_eq!(clips[0].id, Id(3));
    let carried = seams::carried(&lumen.app().project().tracks[0]);
    assert_eq!(carried[0].len(), 0);
    assert_eq!(carried[1].len(), 1, "the take carries the note on");
    assert_eq!(carried[2].len(), 1, "and so does the rest of the clip");

    // Played back from the start, it sounds once from beat 1 to beat 31.
    // The stop lets the held note go, and the sender thread delivers that
    // off a moment later: count from when it has reached the port, or a slow
    // machine counts it against this pass.
    lumen.wait_for("the stop's note-off at the port", |_| {
        ons_and_offs(&fake, 50).1 >= 1
    });
    let before = ons_and_offs(&fake, 50);
    lumen.click("Go to start");
    play_to(&mut lumen, beats(28.0));
    let (on, off) = ons_and_offs(&fake, 50);
    assert_eq!(
        (on - before.0, off - before.1),
        (1, 0),
        "one note through both boundaries"
    );
    stop(&mut lumen);

    // Played from the take's start (a click on its header puts the playhead
    // there), the note is not struck: it began in the clip before.
    let take = clips[1].clone();
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, take.id)
        .expect("the take is in view");
    let header = eframe::egui::pos2(r.left() + 12.0, r.top() + 6.0);
    lumen.drag(header, header, eframe::egui::Modifiers::NONE);
    lumen.wait_for("the playhead at the take", move |app| {
        app.playhead() == take.start
    });
    let before = ons_and_offs(&fake, 50);
    play_to(&mut lumen, beats(28.0));
    assert_eq!(
        ons_and_offs(&fake, 50).0,
        before.0,
        "not struck again where the take begins"
    );
    stop(&mut lumen);
}
