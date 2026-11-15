mod capture;
pub mod chime;
mod clips;
mod declick;
mod engine;
mod envelope;
mod input;
pub mod loopback;
mod metronome;
mod midi;
mod mixer;
mod playhead;
pub mod preview;
mod render;
mod scrub;
mod snapshot;
mod source;
mod stream;
pub mod testing;
mod timeline;

pub use capture::{
    CaptureReader, CaptureSink, Chunk as CaptureChunk, RING_CHUNKS, RING_SAMPLES, capture,
};
pub use clips::{
    Clip, ClipAudio, ClipPlayer, ClipShape, ClipsHandle, FadeFrames, clip_player,
    clip_player_for_render, for_each_page,
};
pub use engine::{
    Command, Engine, EngineHandle, HOST_SHARE, OFFLINE_WAIT, Region, TransportReader, engine,
};
pub use envelope::ClipEnvelope;
pub use input::{
    CLIP_LEVEL as INPUT_CLIP_LEVEL, InputHandle, InputLevel, InputPlayer, InputRoute, MAX_INPUTS,
    MAX_ROUTES as MAX_INPUT_ROUTES, Monitoring, input_player,
};
pub use metronome::{ClickSound, MetronomeSettings};
pub use midi::{
    AUDITION, CLOCK_TICKS, Captured, DrumKit, Heard, INPUT_SLOTS, InstrumentSpec, KEYBOARD, KitKey,
    Listen, LiveIn, LiveInput, MidiClip, MidiControl, MidiHandle, MidiNote, MidiOut, MidiPlayer,
    MidiTrack, RecordTiming, SOURCES as LIVE_SOURCES, Target, WRAP, control_list, midi_player,
    note_list, record_position,
};
pub use mixer::{
    AutoLane, AutoPoint, BUS_CHANNELS, Bus, Buses, Compensation, CueOut, Destination, Device, KNOT,
    Layered, Level, MAX_BUILTINS, MAX_DEVICE_LATENCY, MAX_HOSTS, Mixer, MixerHandle,
    NodeAutomation, NodeSpec, ParamLane, Plan, RMS_SECONDS, Routing, RoutingError, SendSpec,
    SidechainSpec, Slot, Tap, TrackSource, compile as compile_routing, db_gain, mixer,
};
pub use render::{
    ClipRender, Ended, Finished, MOST_BLOCK, SourceRender, render_clips, render_source,
};
pub use scrub::{MAX_SPEED as MAX_SCRUB_SPEED, ScrubStyle};
pub use snapshot::TransportSnapshot;
pub use source::{Inputs, Jump, Mode, Output, RenderContext, Source};
pub use stream::plan as stream_plan;
pub use stream::{
    LoadPages, NoLoader, OfflineLoader, PAGE, PageLoad, PageLoader, PagePool, PoolWriter, ReadPage,
    StreamAudio,
};
pub use timeline::Timeline;
