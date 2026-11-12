//! The audio thread's clip player: takes new lists, crossfades the clips
//! that moved, and plays each clip into its track's bus.

use crate::envelope::EnvelopeGains;
use crate::mixer::{BUS_CHANNELS, Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::stream::{NoLoader, PageLoad, PagePool};
use crate::timeline::Timeline;

use super::crossfades::{Crossfade, Ramp, SWAP_PER_SECOND, Swap};
use super::fades::Fades;
use super::mix::{Placement, SideMix};
use super::pages::for_each_page;
use super::seams::{Seam, resolve as set_seams};
use super::set::ClipSet;
use super::{Clip, ClipAudio};

/// Clip edge fades: 2 ms.
const FADE_PER_SECOND: u32 = 500;
/// How fast a level glides: full scale in 20 ms (owner: pan and volume
/// drags crackled while the level stepped once per screen frame).
const GLIDE_PER_SECOND: f32 = 50.0;
/// A streamed clip needs its pages 40 ms ahead (twice a full glide).
const LOOKAHEAD_PER_SECOND: u32 = 25;

/// Audio side: plays the clips. A [`Source`] for the engine.
///
/// `L` decides whether a streamed page it misses may be loaded: the audio
/// thread's player is `ClipPlayer<NoLoader>` (what [`clip_player`] makes) and
/// has no way to reach a loader at all; only an offline render's player may
/// load ([`PageLoad`], sweep M16b).
///
/// [`Source`]: crate::source::Source
/// [`clip_player`]: super::handle::clip_player
pub struct ClipPlayer<L = NoLoader> {
    incoming: rtrb::Consumer<Box<ClipSet>>,
    retired: rtrb::Producer<Box<ClipSet>>,
    /// A replaced list that did not fit in the retired queue yet.
    stash: Option<Box<ClipSet>>,
    current: Box<ClipSet>,
    /// The list `current` replaced, while its moved clips fade out.
    swap: Option<Swap>,
    /// The envelope gains of the clip being played.
    gains: EnvelopeGains,
    /// What this player may load: nothing, for the audio thread's.
    policy: std::marker::PhantomData<L>,
}

fn fade_length(rate: u32) -> u32 {
    (rate / FADE_PER_SECOND).max(1)
}

impl<L: PageLoad> ClipPlayer<L> {
    /// A player on `incoming`, handing replaced lists back through
    /// `retired`, starting with no clips. Allocates, so call it off the
    /// audio thread.
    pub(super) fn new(
        incoming: rtrb::Consumer<Box<ClipSet>>,
        retired: rtrb::Producer<Box<ClipSet>>,
    ) -> Self {
        Self {
            incoming,
            retired,
            stash: None,
            current: Box::new(ClipSet::new(
                Vec::new(),
                &mut std::collections::HashMap::new(),
                false,
            )),
            swap: None,
            gains: EnvelopeGains::new(),
            policy: std::marker::PhantomData,
        }
    }
}

/// The block being rendered.
struct Block {
    /// Timeline samples of the first frame and one past the last.
    from: i64,
    to: i64,
    /// Edge fade length, in frames.
    edge: i64,
    /// How far a level moves in one frame.
    glide: f32,
    /// How far past the block a streamed clip's pages must be in memory: more
    /// than a glide, so a clip that will run out fades while it still plays.
    ahead: i64,
}

/// The block being rendered and the room to work out its envelope gains.
struct Work<'a> {
    block: Block,
    gains: &'a mut EnvelopeGains,
}

#[derive(Clone, Copy)]
struct ClipRender<'a> {
    fades: &'a Fades,
    seam: Seam,
}

/// Plays `clip` at its kept level into its track's bus in `buses`, faded
/// by `ramp`. A tail (rendered ahead of a jump) reads the level but does
/// not move it. `L` says whether a missing streamed page may be loaded: the
/// audio thread's player passes [`NoLoader`], which cannot (sweep M16b).
fn play<L: PageLoad>(
    clip: &Clip,
    kept: Kept<'_>,
    cx: &RenderContext<'_>,
    work: &mut Work<'_>,
    ramp: Ramp,
    render: ClipRender<'_>,
    buses: &mut Buses<'_>,
) {
    let ClipRender { fades, seam } = render;
    let Work { block, gains } = work;
    let rate = cx.sample_rate;
    if clip.audio.sample_rate() != rate || clip.length <= 0 {
        return;
    }
    let mut scratch = *kept.level;
    let level = if cx.tail { &mut scratch } else { kept.level };
    let Some(mut bus) = buses.track(clip.track) else {
        // Its track is not in the routing: not heard, so it fades in if
        // the track joins while it plays.
        *level = f32::NAN;
        return;
    };
    let mut goal = clip.gain;
    let start = cx.timeline.sample_at(clip.start, rate);
    let end = start + clip.length;
    let (a, mut b) = (block.from.max(start), block.to.min(end));
    if let Ramp::Out(x) = ramp {
        b = b.min(x.end);
    }
    if a >= b {
        // Silent in this block: nothing to glide from.
        *level = goal;
        return;
    }
    if let ClipAudio::Stream(stream) = &*clip.audio {
        // Read from disk: while any page it reads in this block or a little
        // after is missing, it glides out (on audio it still has) and back in
        // once its pages are there. Each time it has to is one underrun.
        let ready = for_each_page(clip, start, a..b + block.ahead, stream.frames(), &mut |p| {
            L::ensure(stream, p)
        });
        if !ready {
            goal = 0.0;
        }
        if !cx.tail {
            if !ready && !*kept.starved {
                stream.pool().underrun();
            }
            *kept.starved = !ready;
        }
    }
    if level.is_nan() {
        // First heard. At its start the edge fade covers it; partway
        // through, it fades in.
        *level = if a == start { goal } else { 0.0 };
    }
    if goal == 0.0 && *level == 0.0 {
        return;
    }
    let sources = clip.audio.channels();
    if sources == 0 {
        // A clip with no channels has nothing to play (and no side to pick).
        return;
    }
    let enveloped = !clip.envelope.is_empty();
    if enveloped {
        gains.fill(
            &clip.envelope,
            (
                start - clip.shape.phase,
                clip.shape.period.filter(|p| *p > 0),
            ),
            (block.from, a, b),
            rate,
        );
    }
    let gains: &EnvelopeGains = gains;
    let plain = clip.shape.plain() && !enveloped;
    let mut gain = *level;
    // A crossfade half needs no 2 ms edge fade at its end of the clip.
    let blend = [clip.shape.fade_in.blend, clip.shape.fade_out.blend];
    let seamless = [seam.edges[0] || blend[0], seam.edges[1] || blend[1]];
    let seam_lead = if blend[0] {
        seam.lead.max(block.edge)
    } else {
        seam.lead
    };
    for side in 0..BUS_CHANNELS {
        let at = Placement {
            start,
            from: block.from,
            edge: block.edge,
            fades,
            env: enveloped.then(|| gains.side(side)),
            seamless,
            seam_lead,
            seam_total: seam.total,
        };
        let target = bus.channel(side);
        let side_mix = SideMix {
            clip,
            at,
            span: a..b,
            from: *level,
            goal,
            glide: block.glide,
            ramp,
            plain,
        };
        // Mono plays on both sides; extra channels are ignored.
        gain = match &*clip.audio {
            ClipAudio::Memory { channels, .. } => {
                side_mix.run(target, channels[side.min(sources - 1)].as_slice())
            }
            ClipAudio::Stream(stream) => side_mix.run(target, &stream.side_with::<L>(side)),
        };
    }
    *level = gain;
}

/// A clip's kept state across blocks.
struct Kept<'a> {
    level: &'a mut f32,
    /// A streamed clip faded out because its audio was not in memory.
    starved: &'a mut bool,
}

impl<L: PageLoad> ClipPlayer<L> {
    /// Takes a new clip list if one arrived and no crossfade is running.
    /// With `fade`, clips that moved crossfade into it. Real-time safe.
    fn take_new(&mut self, fade: bool) {
        if let Some(old) = self.stash.take() {
            self.retire(old);
            if self.stash.is_some() {
                return;
            }
        }
        if self.swap.is_some() {
            return;
        }
        let Ok(mut new) = self.incoming.pop() else {
            return;
        };
        // Only the newest list matters: lists that queued up (while
        // stopped, or during a crossfade) go straight back unplayed, so a
        // drag's old positions never play late.
        // Skipping a list needs room for it and for the one it replaces
        // in the end, or the stash would be overwritten and a list freed
        // here; newer lists then wait a block.
        while self.retired.slots() >= 2
            && let Ok(newer) = self.incoming.pop()
        {
            self.retire(std::mem::replace(&mut new, newer));
        }
        let moved = new.carry_levels(&mut self.current);
        let old = std::mem::replace(&mut self.current, new);
        if fade && moved {
            self.swap = Some(Swap {
                old,
                done: 0,
                length: 0,
            });
        } else {
            self.retire(old);
        }
    }

    /// Hands `old` back to be freed, keeping it if the queue is full. Only
    /// called while the stash is empty.
    fn retire(&mut self, old: Box<ClipSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    /// The page pools the lists playing read from: at most one each, and the
    /// old list's only when it is another pool.
    fn pools(&self) -> [Option<&PagePool>; 2] {
        let current = self.current.pool.as_deref();
        let old = self
            .swap
            .as_ref()
            .and_then(|s| s.old.pool.as_deref())
            .filter(|old| current.is_none_or(|c| !std::ptr::eq(*old, c)));
        [current, old]
    }

    fn enter_pools(&self) {
        self.pools().into_iter().flatten().for_each(PagePool::enter);
    }

    fn leave_pools(&self) {
        self.pools().into_iter().flatten().for_each(PagePool::leave);
    }

    /// Ends the crossfade once it is done, or at once with `now`.
    fn finish_swap(&mut self, now: bool) {
        if self.stash.is_some() {
            return;
        }
        if let Some(swap) = self.swap.take_if(|s| now || s.done >= s.length) {
            self.retire(swap.old);
        }
    }
}

impl<L: PageLoad> TrackSource for ClipPlayer<L> {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Buses<'_>) {
        if cx.mode == Mode::Ring {
            // Stopped, and only instrument and effect tails ring. The engine
            // renders instead of idling while a note rings, so lists sent
            // meanwhile are taken here, or a held live note would fill the
            // queue and the interface would hold its next edits back.
            self.idle();
            return;
        }
        if !cx.tail {
            self.take_new(true);
        }
        let rate = cx.sample_rate;
        let frames = out.frames() as i64;
        let block = Block {
            from: cx.start,
            to: cx.start + frames,
            edge: i64::from(fade_length(rate)),
            glide: GLIDE_PER_SECOND / rate as f32,
            ahead: i64::from(rate / LOOKAHEAD_PER_SECOND),
        };
        // Pages read in this block stay in memory until it ends.
        self.enter_pools();
        set_seams(&mut self.current, cx);
        let crossfade = self.swap.as_mut().map(|swap| {
            if swap.length == 0 {
                swap.length = i64::from((rate / SWAP_PER_SECOND).max(1));
            }
            Crossfade {
                end: block.from + (swap.length - swap.done),
                length: swap.length,
            }
        });
        let mut work = Work {
            block,
            gains: &mut self.gains,
        };
        let set = &mut *self.current;
        for i in 0..set.clips.len() {
            let ramp = match crossfade {
                Some(x) if set.swapped[i] => Ramp::In(x),
                _ => Ramp::Steady,
            };
            play::<L>(
                &set.clips[i],
                Kept {
                    level: &mut set.levels[i],
                    starved: &mut set.starved[i],
                },
                cx,
                &mut work,
                ramp,
                ClipRender {
                    fades: &set.fades[i],
                    seam: Seam {
                        edges: set.seams[i],
                        lead: set.seam_lead[i],
                        total: set.seam_total[i],
                    },
                },
                out,
            );
        }
        if let (Some(swap), Some(x)) = (self.swap.as_mut(), crossfade) {
            set_seams(&mut swap.old, cx);
            let old = &mut *swap.old;
            for i in (0..old.clips.len()).filter(|&i| old.swapped[i]) {
                play::<L>(
                    &old.clips[i],
                    Kept {
                        level: &mut old.levels[i],
                        starved: &mut old.starved[i],
                    },
                    cx,
                    &mut work,
                    Ramp::Out(x),
                    ClipRender {
                        fades: &old.fades[i],
                        seam: Seam {
                            edges: old.seams[i],
                            lead: old.seam_lead[i],
                            total: old.seam_total[i],
                        },
                    },
                    out,
                );
            }
            if !cx.tail {
                swap.done += frames;
            }
        }
        self.leave_pools();
        self.finish_swap(false);
    }

    fn idle(&mut self) {
        self.finish_swap(true);
        self.take_new(false);
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        // A jump is crossfaded by the engine: a crossfade between lists ends
        // here, and levels start at their targets rather than gliding.
        self.finish_swap(true);
        self.take_new(false);
        let set = &mut *self.current;
        for (clip, level) in set.clips.iter().zip(set.levels.iter_mut()) {
            *level = clip.gain;
        }
    }
}
