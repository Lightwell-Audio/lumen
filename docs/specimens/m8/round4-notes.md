# Faces, round 4: research notes and the shared motion vocabulary

Pitch for the M8 owner review (OR-2 to OR-9, 2026-11-14). Nothing here is built
and no Rust file was changed. The pictures are in `round4.html` (live, at real
card size; Canvas 2D standing in for egui's painter) with two stills,
`round4-dark.png` and `round4-light.png` (100 %, drum loop, signal on).

The page's controls: Theme, Scale (100/150/200 %), Signal (silence, drum loop,
sustained pad), Reduce motion, Pause. Dials drag, wheel and double-click reset;
Flare's pages and tiles click; Flare's filter and Parallax's trail drag.

Contents: 1 What the owner found · 2 Research notes · 3 The motion vocabulary
(tokens for `design/motion.rs`) · 4 Rest and Reduce motion · 5 Cost · 6 Per
device: directions, recommendations, data needs · 7 Flare's 34 parameters ·
8 Parallax: how the picture stays in step · 9 Not done, risks

---

## 1. What the owner found, and the cause

| Finding | Cause (from reading `chain/face/*.rs`, `design/motion.rs`) |
|---|---|
| All faces feel weak and lifeless | One easing (`ease_out`, cubic) and one spring (`spring()`, critically damped: it can never overshoot). Meter pictures follow the level with one symmetric time constant, so a hit and its decay look the same. Faces repaint at `METER_FRAME` (33 ms, 30 fps) while audio flows. |
| Horizon shows a graph with no signal | The history is drawn whatever the level; there is no rest picture. |
| Iris: rays disconnected, curve jagged | Geometry drawn from a different place than the node; curve sampled coarsely. (A separate change fixes the data; the specimens assume it.) |
| Flare: generic, labels cut | 34 parameters in one strip; one width for every label. |
| Parallax: how does it stay in step | The pulse today runs on wall time and the delay time, not on when hits happened. |

The thread through all of it: the faces show *state*, but not *events*. A hit,
a note, a landing echo have no visible beat, so nothing is felt.

## 2. Research notes

Neutral, from general graphics, audio-metering and animation practice.

**Response curves.** A filter's response is a closed form (the biquad's
magnitude at a frequency). Sampling it every 1.5 pt (about 235 samples across
352 pt) and drawing through the points with midpoint quadratics is smooth by
construction; a jagged curve means too few samples, a curve joined from
analyser bins, or lines between nodes. The analyser is a different thing:
smooth it in space (a 1-2-1 kernel, twice) and in time (fast up, slow down)
and draw it as a curve, never as columns, and keep it visibly quieter than
the response so the response stays the subject. Nodes sit on the curve at
their own centre frequency, and every ray, bracket and fill is anchored on the
node's position from the same function, so nothing can drift from it.

**Glow.** A glow reads as energy when it is additive and radial: stops at 0,
.35, .7 and 1 of the radius with alphas a, .42a, .1a, 0 (a quick approximation
of a Gaussian). On egui this is a mesh of a fan of triangles with vertex
colours, so one glow costs one small mesh, not a shader. Additive blend is
available as a vertex colour with zero alpha and premultiplied colour, which is
how the specimens' "lighter" layers should be built. Three levels carry the
meaning: rest (barely there), live (follows the signal), hit (full). See the
glow ladder in section 3. Never put colour alone on state: rest also changes
shape (the picture is simpler), and on-state keeps a label.

**Easing and overshoot.** Overshoot makes a thing feel like it has mass. A
second-order spring with natural frequency ω and damping ratio ζ overshoots by
`exp(-π ζ / sqrt(1 - ζ²))`: ζ .55 gives 12.6 %, ζ .38 gives 28 %, ζ 1 gives
none (today). Use a small overshoot for what the user moves (a handle, a node,
a dial head) and a large one for one beat only (a ball landing). Integrate a
spring in sub-steps of at most 4 ms so a slow frame cannot change its shape,
or evaluate its closed form. A bounded set of curves is enough: `out` (today),
`out_back` for a beat that lands, `out_expo` for a thing thrown outward and
settling long, `in_out` (a smootherstep) for a travelling ball.

**Fast-attack, eased-release meters.** A meter that moves with one time
constant looks lifeless because it hides the difference between a hit and a
tail. Ballistics: attack in a few milliseconds (so a transient is seen at all
and is not smeared), release over 150 to 300 ms so the eye can read it, a peak
marker held for about 0.7 s and then falling at about 1 unit/s. Gain reduction
is the mirror: fast down, eased back up, with the release time following the
device's own release so the picture and the sound recover together.

**Reacting to audio energy.** Three reads are enough, in this order of
cheapness: the level (already in `Scene`), an onset (a rise in level over a
short window, the usual spectral-flux idea reduced to the level), and the
band energies (the spectrum, already read for the EQ). Use energy to drive
*scale and brightness*, not position: size and glow can move at the rate of the
meter without looking like noise; position jitter at 30 fps looks like
dropped frames. Use onsets for *events* (a ring, a pop, a ball launch). With
today's 33 ms meter frame, an onset detected from the level is accurate to
about a frame; an exact onset needs the engine to timestamp it (section 8).

**Idle and rest.** Silence must look intentional. A rest picture is a still
composition that is complete without motion: a dim extent (the room's outline,
the ceiling line, the filter's response), no history, no particles, no loop.
Enter it after the signal is below about -66 dB for 0.45 s (a hysteresis so a
pause between notes does not flicker), settle over about 0.3 s, and wake in
about 50 ms. A resting face asks for no repaint at all: that is the "near-zero
CPU when idle" requirement.

**Frame-rate independence.** Three rules: (1) a follower is
`cur += (target - cur) * (1 - exp(-dt / tau))`, never `cur += k`; (2) a spring
is sub-stepped or closed form; (3) event pictures are closed-form functions of
*age* (`now - event time`), for example a flash `exp(-age / tau)`, a squash
`1 + a * exp(-age / t) * cos(w * age)`, a ring `ease_out_expo(age / life)`.
Rule 3 means an event has no per-frame state to lose, and a dropped frame
cannot desynchronise it. Clamp `dt` to 1/20 s so a stall does not jump.

**Reduce motion.** `motion::time()` already returns 0 for durations. For the
faces: every follower and spring snaps to its target; no ring, ball, mote, star
or pulse is created; no loop; the picture still shows the live level and
values as a static frame (so the device is still usable) and the rest picture
is used when there is no signal. In the specimen, the Reduce motion switch does
exactly this.

**Under about 0.3 ms a frame.** See section 5: a budget by primitive count,
not a measurement.

## 3. The motion vocabulary (proposed tokens)

Fits `design/motion.rs`: constants in seconds, `f32`, next to `FAST`, `BASE`
and `GLIDE`, and small pure functions, so it can be unit-tested like `ease_out`
and `spring`. All new, none edited yet.

| Token | Value | Use |
|---|---|---|
| `FAST` / `BASE` / `GLIDE` | .10 / .18 / .22 s | unchanged (state changes) |
| `METER_ATTACK` | .004 s (tau) | follower up for level, gain reduction, energy |
| `METER_RELEASE` | .16 s (tau) | follower down; a face may use its own (Gravity: the device's release) |
| `PEAK_HOLD` / `PEAK_FALL` | .70 s / 1.1 per s | peak marker |
| `FLASH_RISE` / `FLASH_TAU` | .010 s / .12 s | an impulse: up in a frame, falls like a decay |
| `SNAP` | ω 46, ζ .55 (~12 %) | a handle, node, dial head, page tile |
| `POP` | ω 34, ζ .38 (~28 %) | one hero beat: a ball landing, a tile arriving |
| `SOFT` | ω 32, ζ 1 | today's `spring()` (critically damped) |
| `ease_out_back` | c1 1.70158 | a beat that lands |
| `ease_out_expo` | `1 - 2^(-10 t)` | a ring thrown outward |
| `ease_in_out` | smootherstep | a ball travelling one delay |
| `GLOW_REST` / `LIVE` / `ENERGY` / `HIT` | .10 / .30 / +.50 x energy / 1.0 | a glow's share of full strength |
| `SILENCE_DB` | -66 | below this the face is quiet |
| `SLEEP_HOLD` / `SLEEP_TAU` / `WAKE_TAU` | .45 s / .30 s / .05 s | to rest, settle, wake |
| `DT_MAX` | 1/20 s | clamp a frame's step |
| `FACE_FRAME` | 16 ms while awake (today `METER_FRAME` 33 ms) | faces repaint at 60 fps only while there is signal; 0 at rest |

Functions to add (names are a proposal): `follow(cur, target, tau_up, tau_down,
dt) -> f32`, `Spring { omega, zeta }` with `step(&mut self, target, dt)`,
`ease_out_back`, `ease_out_expo`, `ease_in_out`, `flash(age)`, `awake(level_db,
&mut RestState, dt) -> f32` for the rest/wake blend. The specimen's panel at the
top of the page runs each piece on the live signal.

**How audio energy drives motion, as a rule.**

1. *Level* `lv` (0..1) through `follow(METER_ATTACK, METER_RELEASE)` gives
   **energy**. Energy drives glow (`GLOW_LIVE + GLOW_ENERGY x energy`), size
   (a few percent), brightness. Never position.
2. *Onsets* (a rise of the level over a threshold, with a short refractory
   time) create **events**: a ring, a pop, a ball. Each event stores its time
   and is drawn as a function of age.
3. *Controls* (a dial turned, a node dragged) move a value; the picture follows
   it with `SNAP` so it has a little life but never lags the hand (during a
   drag the value is used directly).
4. *Rest* gates everything: below `SILENCE_DB` for `SLEEP_HOLD`, `awake` goes
   to 0 over `SLEEP_TAU`; every moving piece is multiplied by `awake` or not
   created, and the face stops asking for repaints.

## 4. Rest and Reduce motion, device by device

| Device | Resting picture | Reduce motion |
|---|---|---|
| Horizon | A still horizon line with a dim sun; no history graph; "ceiling -x dB" | Same, plus the live level as a still bar; no pulse |
| Flare | The route (A), the filter response (B) or one dim star (C); no pulses; the page and dials are fully live | No pulses, no pops; the newest note's position is a static dot |
| Gravity | A flat membrane, ball at rest, "dB at rest" | No rings; the press is a static depth |
| Prism | The prism and a dim white beam, no wedges | Wedges drawn at the current width, no shimmer |
| Iris | The response, nodes and grid; the analyser absent | The analyser drawn as a single frame |
| Afterglow | The room's outline (4 dim rings), a dim glow | The glow follows the level; no rings or motes |
| Parallax | The floor, the ghost slots dim by feedback; no ball | Slots light from the level; no ball, no ripple |

## 5. Cost: under about 0.3 ms a frame

Budget by primitive count; **not measured in egui** (it was not built). Rule of
thumb used: egui tessellates on the CPU, so cost follows vertex count; keep a
face under about 3,000 vertices, a handful of meshes, and no per-frame heap
allocation. A glow is a mesh of 24 sides = 25 vertices.

| Direction | Main primitives | Estimate |
|---|---|---|
| Horizon A | area mesh (~200 columns), ceiling, glow | ~700 vertices |
| Flare A | 6 rounded tiles, 3 waveforms of 48 points, 1 filter curve (45 points x2), ADSR, 6 pulses with glows | ~1,800 |
| Flare B | 96 bars (one mesh of 96 quads), filter curve 120 points, 96 small glows (replace by one mesh) | ~1,500 with one mesh, 4,000 with 96 discs |
| Flare C | up to 8 stars x (halo + 8 rays), dust ≤ 96 quads | ~1,200 |
| Gravity A | membrane mesh 14 x 9, rings | ~1,200 |
| Prism A | 5 wedge meshes with additive blend, prism, beam | ~600 |
| Iris A | curve 235 points, analyser 235, 8 band fills, 8 nodes | ~2,500 |
| Afterglow A1 | one glow (56 sides), ≤ 6 ring strokes | ~700 |
| Afterglow A2 | glow, ≤ 130 motes as one mesh of quads | ~900 |
| Parallax A | floor, 12 slots, ≤ 6 balls x 6 trail discs | ~1,500 |

Two things matter more than the shapes: (1) a face that is at rest draws once
and asks for nothing; (2) the repaint rate while awake. Today's 33 ms
`METER_FRAME` caps every face at 30 fps, which is itself part of why they feel
lifeless. The proposal is a 16 ms `FACE_FRAME` while awake, which doubles the
face's frame count; with the budget above that is about 0.5 ms of face painting
per second of audio at most, but it **needs to be measured** with the
`bench` harness before it is adopted (the 200-track project target is the
worry only if many faces are visible together; the chain shows one at a time).

## 6. Per device: directions, recommendations, data needs

"New read" means a new value from the engine to the UI, beyond what `Scene`
holds today (level, spectrum, mist, gain reduction, bpm, rate, values, still).

**Horizon (limiter).** A Skyline (recommended) · B Dawn · C Bars. All three have
a rest picture and a pressed-band squeeze. *Why A:* the history is readable at a
glance and the ceiling line is the horizon itself. No new read.

**Flare (polysynth).** A Path (recommended) · B Harmonics · C Constellation.
All three: seven pages, dials that follow the page, every label fits (section
7). *Why A:* it draws the synth as a route, so choosing the page is the picture
and every note sends a visible packet of light from oscillator to filter to amp;
it is the most welcoming and the clearest teacher. *Hybrid idea:* B's harmonics
as the Filter tile's expanded view (click the filter tile and the screen swaps
to B); worth doing only if A tests well. *New read:* the sounding voices (note,
velocity, stage), a small snapshot per meter frame. Without it Flare can still
draw A's tiles from the parameters but not the notes.

**Gravity (compressor).** A Well (recommended) · B Slope · C Press. *Why A:* a
membrane that a hit presses down makes the job physical, and rings crossing it
give each hit a beat even in a steady groove. No new read.

**Prism (utility).** A Fan (recommended) · B Field · C Ribbons. *Why A:* a real
prism: the white beam goes in, five wedges come out, and they fold back to one
white beam as Width goes to 0 %. B needs a stereo correlation read to be
truthful (one new value). A and C need only the level.

**Iris (EQ).** A Glass (recommended) · B Aurora · C Blue. *Why A:* the clearest
and most precise; a crisp curve on glass, each band's influence a soft fill,
the selected node carrying its value. The curve is the exact response, sampled
every 1.5 pt; the analyser smoothed and drawn as a curve; rays start at the node.
B is the more expressive; C the most technical. No new read.

**Afterglow (reverb).** A1 Bloom (recommended) · A2 Motes. *Why A1:* it keeps
today's glow and rings, but each hit throws a ring outward (fast start, long
ease), so the tail you hear is a ring you watch leaving; it falls at the real
decay time (the hero number is the decay). Cheapest to build. Needs onsets (from
the level stream; no new engine data).

**Parallax (delay).** A Court (recommended) · B Zigzag. Both add the ping-pong
ball; see section 8. *Why A:* it keeps today's floor, lanes and light and adds
one thing, the ball. Needs input onsets and, for exactness, the audio clock.

## 7. Flare's 34 parameters, in seven pages

Source: `crates/lumen-devices/src/synth/params/table.rs`. Each page has at most
six controls and is chosen by a tile (A), a pill (B, C) or by clicking the
screen; the dial row below shows only that page, so every label has at least
60 pt on the 380 pt card ("Release", "Sustain", "Bass mono" all fit at 10.5 pt).

| Page | Controls | Count |
|---|---|---|
| Osc 1 | Shape, Octave, Detune, Level, Pan | 5 |
| Osc 2 | Shape, Octave, Detune, Level, Pan | 5 |
| Out | Noise, Glide, Voices, Velocity, Level | 5 |
| Filter | Cutoff, Reso, Env, Key, Vel, + a Type pill (LP / BP / HP) | 6 |
| Filter env | Attack, Decay, Sustain, Release | 4 |
| Amp env | Attack, Decay, Sustain, Release | 4 |
| LFO | Rate, Depth, Div, + Sync and Target pills | 5 |
| **Total** | | **34** |

Pages are keyboard-reachable in the real build (a Tab stop for each pill; the
dials already are). The page the user last opened is remembered with the
device. The oscillator tiles draw their real shape, octave and detune; the
filter tile draws the live response (including the envelope's sweep and the LFO
if aimed at the cutoff); the amp tile draws the real envelope with the newest
voice riding it. These are all functions of the parameter values and the voice
snapshot, so A also works as a *teaching* picture while the transport is
stopped (the parameters alone draw it).

## 8. Parallax: how the picture stays in step with the repeats

The picture is computed from the **song clock**, not from the frame.

1. Each input hit (an onset) is stored with its time `hit.T` and a strength.
2. Echo *i* (0-based) is due at `hit.T + (i + 1) x delay`, with strength
   `hit x feedback^i`.
3. The ball for echo *i* flies for exactly one delay interval: it leaves slot
   *i - 1* at `hit.T + i x delay` and lands on slot *i* at the due instant.
   Its position is `lerp(slot[i-1], slot[i], ease_in_out(u))` with
   `u = (now - (hit.T + i x delay)) / delay`, plus an arc `sin(pi u)`.
4. The landing is a function of `age = now - due`: a flash `exp(-age/.12)`, a
   squash `1 + .55 exp(-age/.09) cos(24 age)`, a ripple `ease_out(age/.5)`.
   There is no per-frame state, so it cannot drift and a dropped frame moves
   the picture to where the audio is.
5. The delay in seconds is `note x 60 / bpm` when Sync is on (a note change or
   a tempo change moves the next landings; a ball already in flight keeps its
   schedule, as the audio's does) and the milliseconds when it is off.
6. Ping on: the sides alternate (echo 0 on L, echo 1 on R) and the lanes slide
   apart with a short spring; off: the ball bounces down the middle.

**What the engine must give for this to be exact.** `Scene` has `level` and
`bpm` only; the card's pulse today uses the wall clock (`input.time`). Needed:
(a) the *audio frame count* delivered with each meter read, so "now" is the
audio's clock (minus the output latency, which the engine knows), and (b) the
*input onsets with their frame*, or the input level as a short history from
which the UI finds them. Without (b), onsets come from the level stream and are
accurate to one meter frame (33 ms today, 16 ms at `FACE_FRAME`); that is
visibly in step for a 1/8 delay and not exact. Checked in `face/mod.rs`: `Scene` has
no frame count or clock (its fields are power, level, still, handle_lit,
settle, picked, spectrum, mist, rate, bpm, reading and id), so this is a new
read, not something to reuse; whether the engine's meter path can carry one was
not checked. The tail after
Stop: the picture keeps running until the last echo has landed, which needs the
read to continue for `(count + 1) x delay` after the transport stops.

## 9. Not done, and risks

- Everything is Canvas 2D in a browser, not egui. The primitives map one to one
  (lines, polygons, arcs, discs, gradients, additive blends as vertex-coloured
  meshes), and **nothing needs a custom shader**, but no direction was built or
  timed in egui. Gradient-filled ellipses (Afterglow's glow) are a mesh fan;
  the specimen's `ctx.ellipse` rings are a few dozen line segments.
- The simulated signal stands for the meter, the analyser and the note stream.
  It is a deterministic drum loop at 100 bpm, a pad chord every 8 beats, and
  silence. Real music will have more transients and more sustained energy; the
  onset threshold in particular needs tuning on real material.
- Reduce motion and the rest picture are shown (switch and Silence). Scale 150
  and 200 % were looked at on the page; the 100/150/200 % checks of the
  checklist were not done per card in both themes beyond the stills.
- Not tested: real hardware, any frame rate other than the browser's, the 16 ms
  `FACE_FRAME`'s cost, voice stealing with real polyphony counts, drag
  behaviour with touch, accessibility names (the real build needs one for each
  page pill and tile).
- Flare C is the weakest at a glance (sparse in a drum loop) and is kept as a
  contrast; Prism B needs a new read; Zigzag loses the depth. None is
  recommended.
