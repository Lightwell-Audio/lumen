use lumen_core::midi::{Event, Kind, Msg};

use crate::source::{Mode, RenderContext};

use super::super::Target;
use super::super::capture::{self, Captured, Heard};
use super::super::hold::{self, Clock};
use super::super::live::{AUDITION, LiveIn};
use super::super::recording::{live_position, record_window};
use super::MidiPlayer;

impl MidiPlayer {
    pub(super) fn take_feed(&mut self) {
        if let Some(old) = self.old_feed.take()
            && let Err(rtrb::PushError::Full(old)) = self.old_feeds.push(old)
        {
            self.old_feed = Some(old);
            return;
        }
        if let Ok(feed) = self.feeds.pop()
            && let Some(old) = self.live.replace(feed)
            && let Err(rtrb::PushError::Full(old)) = self.old_feeds.push(old)
        {
            self.old_feed = Some(old);
        }
    }

    pub(super) fn play_live(&mut self, cx: &RenderContext<'_>, frames: usize) {
        self.recording = cx.recording;
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let start = cx.device;
        let pass = self.pass;
        let tracks = &mut self.current.tracks;
        let (capture, lost) = (&mut self.capture, &mut self.lost);
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        for track in tracks.iter_mut() {
            let on = cx.recording && track.record.is_on();
            if on && !track.state.recording {
                track.state.record_pass = pass;
            }
            track.state.window = record_window(track, pass);
            if on && !track.state.recording {
                let at = capture::heard(cx, start, self.wrapped_since_locate).0;
                let (position, position_pass) = live_position(track, clock, at, pass, false);
                let window = record_window(track, position_pass);
                let wins = on && (window.0..window.1).contains(&position);
                for down in &mut track.state.down {
                    down.wins = wins;
                    let msg = Msg::note_on(down.channel, down.key, down.velocity);
                    let kept = capture.push(Captured {
                        track: track.id,
                        source: down.source,
                        at,
                        pass,
                        frame: start,
                        state: Heard::Recording,
                        msg,
                    });
                    if kept.is_err() {
                        *lost = lost.saturating_add(1);
                    }
                }
                for i in 0..track.state.down.len() {
                    let played = track.state.down[i].played;
                    if wins {
                        track.state.give_way(played, 0);
                    }
                }
            }
            track.state.recording = on;
        }
        let wrapped = self.wrapped_since_locate;
        let last = frames.saturating_sub(1) as u64;
        let last32 = last as u32;
        let mut floor = 0_u32;
        live.pull(start, start + frames as u64, &mut |m: LiveIn| {
            let offset = (m.frame.saturating_sub(start).min(last) as u32).max(floor);
            for track in tracks.iter_mut() {
                let kit = if m.raw { None } else { track.kit.as_deref() };
                if m.source != AUDITION && track.record.takes(m.source, m.msg) {
                    let recording = cx.recording && track.record.is_on();
                    let (at, back) = capture::heard(cx, m.played, wrapped);
                    let (position, position_pass) = live_position(track, clock, at, pass, back);
                    let window = record_window(track, position_pass);
                    let wins = recording && (window.0..window.1).contains(&position);
                    track.state.press(m.source, m.msg, wins, kit);
                    if let Kind::NoteOn { key, .. } = m.msg.kind()
                        && wins
                    {
                        let played = kit.map_or(key, |kit| kit.key(key).key);
                        track.state.give_way(played, offset);
                    }
                    if cx.recording {
                        let kept = capture.push(Captured {
                            track: track.id,
                            source: m.source,
                            at,
                            pass: pass.wrapping_sub(u32::from(back)),
                            frame: m.played,
                            state: Heard::Recording,
                            msg: m.msg,
                        });
                        if kept.is_err() {
                            *lost = lost.saturating_add(1);
                        }
                    }
                }
                if !cx.recording
                    && m.source != AUDITION
                    && (track.record.takes(m.source, m.msg) || track.listen.takes(m.source, m.msg))
                {
                    let (at, _) = capture::heard(cx, m.played, wrapped);
                    let kept = capture.push(Captured {
                        track: track.id,
                        source: m.source,
                        at,
                        pass,
                        frame: m.played,
                        state: if cx.mode == Mode::Play {
                            Heard::Playing
                        } else {
                            Heard::Stopped
                        },
                        msg: m.msg,
                    });
                    if kept.is_err() {
                        *lost = lost.saturating_add(1);
                    }
                }
                if !track.sounds() {
                    continue;
                }
                let heard = if m.source == AUDITION {
                    m.track == Some(track.id) && m.msg.0[0] & 0xF0 != 0xF0
                } else {
                    track.listen.takes(m.source, m.msg)
                };
                if !heard {
                    continue;
                }
                let out = match track.target {
                    Target::Port { channel, .. } => Some(channel),
                    Target::Instrument => None,
                };
                if let Some(msg) = track.state.live.take(m.source, m.msg, out, kit) {
                    if let Kind::NoteOn { key, .. } = msg.kind() {
                        track.state.choke(kit, key, offset);
                    }
                    let offset = apart_from_its_on(&mut track.state.events, msg, offset, last32);
                    floor = floor.max(offset);
                    let event = Event { offset, msg };
                    if hold::ends_a_held_note(msg) {
                        track.state.push_release(event);
                    } else if !track.state.push(event) {
                        track.state.live.refused(msg);
                    }
                }
            }
        });
    }
}

fn apart_from_its_on(events: &mut [Event], msg: Msg, offset: u32, last: u32) -> u32 {
    let lumen_core::midi::Kind::NoteOff { key, .. } = msg.kind() else {
        return offset;
    };
    let on = events.iter_mut().rev().find(|e| {
        e.offset >= offset
            && e.msg.channel() == msg.channel()
            && matches!(e.msg.kind(), lumen_core::midi::Kind::NoteOn { key: k, .. } if k == key)
    });
    let Some(on) = on else {
        return offset;
    };
    if on.offset < last {
        on.offset + 1
    } else {
        on.offset = last.saturating_sub(1);
        last
    }
}
