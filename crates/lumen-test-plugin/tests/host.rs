//! The test plugins, driven through their VST3 interfaces by a small host.

mod support;

use std::ffi::c_char;

use lumen_test_plugin::faults::Faults;
use lumen_test_plugin::params::{Kind, LATENCY, LEVEL, MAX_LATENCY, WAVE};
use support::{Plugin, Told, note_off, note_on};
use vst3::Steinberg::Vst::IAudioProcessorTrait;
use vst3::Steinberg::Vst::IEditController;
use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, IComponentHandler, IComponentTrait, IEditControllerTrait, MediaTypes_,
    ParameterInfo, RestartFlags_, String128,
};
use vst3::Steinberg::{
    IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2, kInvalidArgument,
    kResultFalse, kResultOk,
};
use vst3::{ComWrapper, Interface};

fn text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn wide(buf: &[u16]) -> String {
    let units: Vec<u16> = buf.iter().copied().take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&units)
}

/// Eight frames of silence, as the blocks below ask for.
const SILENCE: [&[f32]; 2] = [&[0.0; 8], &[0.0; 8]];

#[test]
fn the_factory_lists_four_classes_with_categories() {
    let factory = lumen_test_plugin::factory();
    // SAFETY: plain factory queries with valid out pointers.
    unsafe {
        assert_eq!(factory.countClasses(), 4);
        let mut names = Vec::new();
        for i in 0..4 {
            let mut info: PClassInfo2 = std::mem::zeroed();
            let f2 = factory
                .cast::<vst3::Steinberg::IPluginFactory2>()
                .expect("factory 2");
            assert_eq!(f2.getClassInfo2(i, &mut info), kResultOk);
            names.push((
                text(&info.category),
                text(&info.subCategories),
                text(&info.vendor),
            ));
        }
        assert_eq!(
            names[0],
            (
                "Audio Module Class".into(),
                "Instrument|Synth".into(),
                "Test Vendor".into()
            )
        );
        assert_eq!(names[2].1, "Fx");
        assert_eq!(names[1].0, "Component Controller Class");
        let mut plain: PClassInfo = std::mem::zeroed();
        assert_eq!(factory.getClassInfo(4, &mut plain), kInvalidArgument);
        assert_eq!(
            text(&{
                assert_eq!(factory.getClassInfo(2, &mut plain), kResultOk);
                plain.name
            }),
            "Deterministic Test Effect"
        );
    }
}

#[test]
fn the_factory_creates_the_components_it_lists() {
    let factory = lumen_test_plugin::factory();
    for cid in [lumen_test_plugin::INSTRUMENT, lumen_test_plugin::EFFECT] {
        let mut object = std::ptr::null_mut();
        // SAFETY: valid class and interface ids; `object` receives a reference.
        let result = unsafe {
            factory.createInstance(
                cid.as_ptr(),
                vst3::Steinberg::Vst::IComponent::IID.as_ptr().cast(),
                &mut object,
            )
        };
        assert_eq!(result, kResultOk);
        assert!(!object.is_null());
        // SAFETY: an owned reference from createInstance.
        drop(unsafe { vst3::ComPtr::<vst3::Steinberg::Vst::IComponent>::from_raw(object.cast()) });
    }
    let mut object = std::ptr::null_mut();
    let unknown = [7_i8; 16];
    // SAFETY: valid ids.
    let result = unsafe {
        factory.createInstance(
            unknown.as_ptr(),
            vst3::Steinberg::Vst::IComponent::IID.as_ptr().cast(),
            &mut object,
        )
    };
    assert_ne!(result, kResultOk);
}

#[test]
fn buses_match_the_plugin_kind() {
    let inst = Plugin::new(Kind::Instrument, "");
    let fx = Plugin::new(Kind::Effect, "");
    let audio = MediaTypes_::kAudio as i32;
    let events = MediaTypes_::kEvent as i32;
    let (input, output) = (
        BusDirections_::kInput as i32,
        BusDirections_::kOutput as i32,
    );
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(inst.component.getBusCount(audio, input), 0);
        assert_eq!(inst.component.getBusCount(audio, output), 1);
        assert_eq!(inst.component.getBusCount(events, input), 1);
        assert_eq!(fx.component.getBusCount(audio, input), 1);
        assert_eq!(fx.component.getBusCount(events, input), 0);
        let mut info: BusInfo = std::mem::zeroed();
        assert_eq!(
            fx.component.getBusInfo(audio, output, 0, &mut info),
            kResultOk
        );
        assert_eq!(info.channelCount, 2);
        assert_eq!(
            fx.component.getBusInfo(audio, output, 1, &mut info),
            kInvalidArgument
        );
    }
}

#[test]
fn a_note_sounds_from_its_sample_offset() {
    let p = Plugin::new(Kind::Instrument, "");
    p.start();
    let (result, out) = p.process(
        64,
        None,
        vec![note_on(10, 69, 1.0), note_off(40, 69)],
        &[(LEVEL, &[(0, 1.0)])],
    );
    assert_eq!(result, kResultOk);
    assert!(out[0][..10].iter().all(|&s| s == 0.0));
    assert!(out[0][10..40].iter().all(|&s| s == 1.0 || s == -1.0));
    assert!(out[0][40..].iter().all(|&s| s == 0.0));
    assert_eq!(out[0], out[1]);
}

#[test]
fn a_parameter_change_lands_on_its_sample() {
    let p = Plugin::new(Kind::Instrument, "");
    p.start();
    let (_, out) = p.process(
        32,
        None,
        vec![note_on(0, 69, 1.0)],
        &[(LEVEL, &[(0, 1.0), (16, 0.5)])],
    );
    assert_eq!(out[0][15], 1.0);
    assert_eq!(out[0][16], 0.5);
}

#[test]
fn the_effect_delays_by_its_latency_and_reports_it() {
    let p = Plugin::new(Kind::Effect, "");
    p.start();
    let delay = 24.0 / f64::from(MAX_LATENCY);
    let mut impulse = vec![0.0_f32; 64];
    impulse[0] = 1.0;
    let (result, out) = p.process(
        64,
        Some([&impulse, &impulse]),
        vec![],
        &[(LATENCY, &[(0, delay)])],
    );
    assert_eq!(result, kResultOk);
    let at: Vec<usize> = (0..64).filter(|&i| out[0][i] != 0.0).collect();
    assert_eq!(at, [24]);
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(p.processor.getLatencySamples(), 24);
        assert_eq!(p.processor.getTailSamples(), 24);
    }
}

#[test]
fn changing_latency_in_the_controller_asks_the_host_to_restart() {
    let p = Plugin::new(Kind::Effect, "");
    p.start();
    let told = ComWrapper::new(Told::default());
    let handler = told.to_com_ptr::<IComponentHandler>().expect("listed");
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(
            p.controller.setComponentHandler(handler.as_ptr()),
            kResultOk
        );
        assert_eq!(p.controller.setParamNormalized(LEVEL, 0.5), kResultOk);
        assert!(told.restarts.lock().unwrap().is_empty());
        assert_eq!(p.controller.setParamNormalized(LATENCY, 0.25), kResultOk);
        assert_eq!(p.controller.setParamNormalized(LATENCY, 0.25), kResultOk);
    }
    assert_eq!(
        *told.restarts.lock().unwrap(),
        [RestartFlags_::kLatencyChanged]
    );
}

#[test]
fn state_round_trips_between_instances() {
    let a = Plugin::new(Kind::Effect, "");
    a.start();
    a.process(
        8,
        Some(SILENCE),
        vec![],
        &[(LEVEL, &[(0, 0.3)]), (LATENCY, &[(0, 0.5)])],
    );
    let (result, bytes) = a.save();
    assert_eq!(result, kResultOk);
    let b = Plugin::new(Kind::Effect, "");
    b.start();
    assert_eq!(b.load(bytes.clone()), kResultOk);
    assert_eq!(b.save().1, bytes);
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(
            b.processor.getLatencySamples(),
            u32::from(MAX_LATENCY as u16 / 2)
        )
    };
    // The controller follows through setComponentState.
    let (stream, _) = support::Stream::open(bytes);
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(b.controller.setComponentState(stream.as_ptr()), kResultOk);
        assert_eq!(b.controller.getParamNormalized(LEVEL), 0.3);
        assert_eq!(b.controller.getParamNormalized(LATENCY), 0.5);
    }
}

#[test]
fn state_for_another_plugin_or_damaged_is_refused() {
    let inst = Plugin::new(Kind::Instrument, "");
    let fx = Plugin::new(Kind::Effect, "");
    let (_, inst_state) = inst.save();
    assert_eq!(fx.load(inst_state), kResultFalse);
    assert_eq!(fx.load(vec![1, 2, 3]), kResultFalse);
    assert_eq!(fx.load(vec![]), kResultFalse);
}

#[test]
fn state_faults_fail_as_asked() {
    let failing = Plugin::new(Kind::Instrument, "state_fail");
    assert_ne!(failing.save().0, kResultOk);
    let garbage = Plugin::new(Kind::Instrument, "state_garbage");
    let (result, bytes) = garbage.save();
    assert_eq!(result, kResultOk);
    let clean = Plugin::new(Kind::Instrument, "");
    assert_eq!(clean.load(bytes), kResultFalse);
}

#[test]
fn process_faults_that_do_not_kill_the_process() {
    let nan = Plugin::new(Kind::Instrument, "process_nan@1");
    nan.start();
    let (_, first) = nan.process(8, None, vec![note_on(0, 69, 1.0)], &[]);
    assert!(first[0].iter().all(|s| s.is_finite()));
    let (result, second) = nan.process(8, None, vec![], &[]);
    assert_eq!(result, kResultOk);
    assert!(second[0].iter().all(|s| s.is_nan()));

    let error = Plugin::new(Kind::Effect, "process_error@2");
    error.start();
    assert_eq!(error.process(8, Some(SILENCE), vec![], &[]).0, kResultOk);
    assert_eq!(error.process(8, Some(SILENCE), vec![], &[]).0, kResultOk);
    assert_ne!(error.process(8, Some(SILENCE), vec![], &[]).0, kResultOk);

    let slow = Plugin::new(Kind::Effect, "process_slow=3000");
    slow.start();
    let start = std::time::Instant::now();
    slow.process(8, Some(SILENCE), vec![], &[]);
    assert!(start.elapsed() >= std::time::Duration::from_micros(3000));
}

#[test]
fn activating_again_restarts_the_fault_clock_and_silences() {
    let p = Plugin::new(Kind::Instrument, "process_error@1");
    p.start();
    p.process(8, None, vec![note_on(0, 69, 1.0)], &[]);
    assert_ne!(p.process(8, None, vec![], &[]).0, kResultOk);
    // SAFETY: plain calls on live objects.
    unsafe {
        p.component.setActive(0);
        p.component.setActive(1);
    }
    let (result, out) = p.process(8, None, vec![], &[]);
    assert_eq!(result, kResultOk);
    assert!(out[0].iter().all(|&s| s == 0.0), "the old note is gone");
}

#[test]
fn bad_metadata_is_reported_as_asked() {
    let bad = Plugin::new(Kind::Instrument, "bad_buses,bad_params");
    // SAFETY: plain calls on live objects.
    unsafe {
        let mut info: BusInfo = std::mem::zeroed();
        bad.component.getBusInfo(
            MediaTypes_::kAudio as i32,
            BusDirections_::kOutput as i32,
            0,
            &mut info,
        );
        assert_eq!(info.channelCount, 9_999);
        assert_eq!(bad.controller.getParameterCount(), 4_000_000);
        let mut p: ParameterInfo = std::mem::zeroed();
        assert_eq!(
            bad.controller.getParameterInfo(3_999_999, &mut p),
            kResultOk
        );
        assert_eq!(p.id, 0);
        assert_eq!(
            bad.controller.getParameterInfo(4_000_000, &mut p),
            kInvalidArgument
        );
    }
}

#[test]
fn the_controller_describes_and_formats_parameters() {
    let p = Plugin::new(Kind::Instrument, "");
    // SAFETY: plain calls on live objects.
    unsafe {
        assert_eq!(p.controller.getParameterCount(), 2);
        let mut info: ParameterInfo = std::mem::zeroed();
        assert_eq!(
            p.controller.getParameterInfo(WAVE as i32, &mut info),
            kResultOk
        );
        assert_eq!(wide(&info.title), "Wave");
        assert_eq!(info.stepCount, 1);
        let mut shown: String128 = [0; 128];
        assert_eq!(
            p.controller.getParamStringByValue(WAVE, 1.0, &mut shown),
            kResultOk
        );
        assert_eq!(wide(&shown), "Saw");
        let mut typed: Vec<u16> = "Saw".encode_utf16().chain([0]).collect();
        let mut value = 0.0;
        assert_eq!(
            p.controller
                .getParamValueByString(WAVE, typed.as_mut_ptr(), &mut value),
            kResultOk
        );
        assert_eq!(value, 1.0);
        assert_eq!(p.controller.setParamNormalized(99, 0.5), kInvalidArgument);
        assert_eq!(
            p.controller.setParamNormalized(LEVEL, f64::NAN),
            kInvalidArgument
        );
        assert!(p.controller.createView(c"editor".as_ptr()).is_null());
    }
}

#[test]
fn faults_parse_from_text_only() {
    assert!(Faults::parse("process_crash@5").is_ok());
    let _ = IEditController::IID;
}
