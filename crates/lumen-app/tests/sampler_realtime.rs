use std::sync::Arc;

use lumen_app::testing::{SampleSource, SamplerLibrary};
use lumen_core::midi::Msg;
use lumen_devices::Instrument;
use lumen_devices::ParamBlock;
use lumen_devices::sampler::{Face, Pad, Region, Sample, Sampler, Sound};
use lumen_project::model::{Face as ModelFace, Id, Pad as ModelPad, Sampler as ModelSampler};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;
const OTHER_RATE: u32 = 96_000;
const MOST: usize = 256;

fn big_sample(rate: u32) -> Arc<Sample> {
    let frames = 5 * rate as usize;
    let data = (0..frames * 2)
        .map(|k| (k as f32 * 0.001).sin() * 0.3)
        .collect();
    Arc::new(Sample::new(rate, 2, frames, data).expect("a stereo sample"))
}

fn sound_model(media: Id) -> ModelSampler {
    let mut model = ModelSampler {
        face: ModelFace::Pads,
        ..ModelSampler::default()
    };
    model.keep_pads(&ModelSampler::pads_for(None));
    let mut pad = ModelPad::playing(36, media);
    pad.output = 0;
    model.pads[1] = pad;
    model
}

fn block() -> ParamBlock {
    let mut events = ParamBlock::new();
    events.start(MOST);
    events
}

fn note(key: u8) -> lumen_core::midi::Event {
    lumen_core::midi::Event {
        offset: 0,
        msg: Msg::note_on(0, key, 100),
    }
}

#[test]
fn a_rate_change_while_a_voice_rings_never_frees_its_sample_on_the_audio_thread() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    let media = Id(1);
    let sample = big_sample(RATE);

    let mut library = SamplerLibrary::from_snapshot(
        RATE,
        vec![(
            media,
            SampleSource {
                sample: Arc::clone(&sample),
                file_rate: RATE,
                frames: sample.frames() as u64,
            },
        )],
    );
    let model = sound_model(media);

    let mut device = Sampler::new(Arc::new(library.build(&model, None)));
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    device.automate(&block());
    device.render(RATE, &[note(36)], &mut left, &mut right);
    assert!(device.active(), "the note rings");

    library.update(OTHER_RATE);
    assert!(library.has_stale(), "the old buffers wait for the engine");

    assert_eq!(library.release_stale(), 0, "held by the device");

    let events = block();
    let struck = [note(36)];
    let (_, violations) = lumen_rt::count_rt_violations(|| {
        device.automate(&events);
        device.render(RATE, &[], &mut left, &mut right);
        device.silence();
        device.automate(&events);
        device.render(RATE, &struck, &mut left, &mut right);
        device.silence();
    });
    assert_eq!(violations, 0, "the audio thread touched the heap");

    drop(device);
    drop(sample);
    let freed = library.release_stale();
    assert!(freed > 0, "the old buffers were freed off the audio thread");
    assert!(!library.has_stale());
}

#[test]
fn a_sound_built_from_the_library_plays_its_pad() {
    let media = Id(7);
    let sample = big_sample(RATE);
    let library = SamplerLibrary::from_snapshot(
        RATE,
        vec![(
            media,
            SampleSource {
                sample: Arc::clone(&sample),
                file_rate: RATE,
                frames: sample.frames() as u64,
            },
        )],
    );
    let model = sound_model(media);
    let sound: Sound = library.build(&model, None);
    assert_eq!(sound.face, Face::Pads);
    let pad: &Pad = sound.pad(36).expect("the pad");
    let region: &Region = pad.region.as_ref().expect("its sample");
    assert_eq!(region.sample.frames(), sample.frames());
}
