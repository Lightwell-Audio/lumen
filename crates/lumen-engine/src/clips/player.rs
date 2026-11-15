use crate::envelope::EnvelopeGains;
use crate::mixer::{BUS_CHANNELS, Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::stream::{LoadPages, NoLoader, PageLoad, PagePool};
use crate::timeline::Timeline;

use super::crossfades::{Crossfade, Ramp, SWAP_PER_SECOND, Swap};
use super::fades::Fades;
use super::mix::{Placement, SideMix};
use super::pages::for_each_page;
use super::seams::{Seam, resolve as set_seams};
use super::set::ClipSet;
use super::{Clip, ClipAudio};

const FADE_PER_SECOND: u32 = 500;
const GLIDE_PER_SECOND: f32 = 50.0;
const LOOKAHEAD_PER_SECOND: u32 = 25;

pub struct ClipPlayer<L = NoLoader> {
    incoming: rtrb::Consumer<Box<ClipSet>>,
    retired: rtrb::Producer<Box<ClipSet>>,
    stash: Option<Box<ClipSet>>,
    current: Box<ClipSet>,
    swap: Option<Swap>,
    gains: EnvelopeGains,
    policy: std::marker::PhantomData<L>,
}

fn fade_length(rate: u32) -> u32 {
    (rate / FADE_PER_SECOND).max(1)
}

impl<L: PageLoad> ClipPlayer<L> {
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

struct Block {
    from: i64,
    to: i64,
    edge: i64,
    glide: f32,
    ahead: i64,
}

struct Work<'a> {
    block: Block,
    gains: &'a mut EnvelopeGains,
}

#[derive(Clone, Copy)]
struct ClipRender<'a> {
    fades: &'a Fades,
    seam: Seam,
}

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
        *level = goal;
        return;
    }
    if let ClipAudio::Stream(stream) = &*clip.audio {
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
        *level = if a == start { goal } else { 0.0 };
    }
    if goal == 0.0 && *level == 0.0 {
        return;
    }
    let sources = clip.audio.channels();
    if sources == 0 {
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
        gain = match &*clip.audio {
            ClipAudio::Memory { channels, .. } => {
                side_mix.run(target, channels[side.min(sources - 1)].as_slice())
            }
            ClipAudio::Stream(stream) => side_mix.run(target, &stream.side_with::<L>(side)),
        };
    }
    *level = gain;
}

struct Kept<'a> {
    level: &'a mut f32,
    starved: &'a mut bool,
}

impl<L: PageLoad> ClipPlayer<L> {
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

    fn retire(&mut self, old: Box<ClipSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

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

    fn finish_swap(&mut self, now: bool) {
        if self.stash.is_some() {
            return;
        }
        if let Some(swap) = self.swap.take_if(|s| now || s.done >= s.length) {
            self.retire(swap.old);
        }
    }
}

impl<L: PageLoad> ClipPlayer<L> {
    #[inline(always)]
    fn render_block(&mut self, cx: &RenderContext<'_>, out: &mut Buses<'_>) {
        if cx.mode == Mode::Ring {
            self.idle_now();
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

    fn idle_now(&mut self) {
        self.finish_swap(true);
        self.take_new(false);
    }

    fn locate_now(&mut self) {
        self.finish_swap(true);
        self.take_new(false);
        let set = &mut *self.current;
        for (clip, level) in set.clips.iter().zip(set.levels.iter_mut()) {
            *level = clip.gain;
        }
    }
}

macro_rules! track_source {
    ($side:ty) => {
        impl TrackSource for ClipPlayer<$side> {
            fn render(&mut self, cx: &RenderContext<'_>, out: &mut Buses<'_>) {
                self.render_block(cx, out);
            }

            fn idle(&mut self) {
                self.idle_now();
            }

            fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
                self.locate_now();
            }
        }
    };
}

track_source!(NoLoader);
track_source!(LoadPages);
