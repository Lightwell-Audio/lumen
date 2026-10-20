//! The test plugin loaded with the operating system's loader, in process:
//! the module, its classes and an instance, with the plugin's output
//! checked sample by sample.

mod support;

use lumen_plugin_host::instance::Format;
use lumen_plugin_host::protocol::ParamQueue;
use lumen_plugin_host::{Instance, Module, ModuleError};
use lumen_test_plugin::params::{LATENCY, LEVEL, MAX_LATENCY};
use support::{effect_class, note_off, note_on, plugin};

const FORMAT: Format = Format {
    sample_rate: 48_000.0,
    max_frames: 512,
    inputs: 0,
    outputs: 2,
};

#[test]
fn the_module_lists_its_four_classes() {
    let module = Module::load(plugin()).expect("load");
    let classes = module.classes();
    assert_eq!(classes.len(), 4);
    assert_eq!(classes[0].name, "Deterministic Test Instrument");
    assert_eq!(classes[0].category, "Audio Module Class");
    assert_eq!(classes[0].vendor, "Test Vendor");
    assert_eq!(classes[0].subcategories, "Instrument|Synth");
    assert_eq!(classes[2].subcategories, "Fx");
}

#[test]
fn a_missing_or_foreign_file_is_refused_with_a_reason() {
    let missing = std::path::Path::new("/nonexistent/nothing.vst3");
    assert!(matches!(
        Module::load(missing),
        Err(ModuleError::NotFound(_))
    ));
    let dir = std::env::temp_dir().join(format!("lumen-host-not-a-module-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.so");
    std::fs::write(&junk, b"this is not a shared library").unwrap();
    assert!(matches!(Module::load(&junk), Err(ModuleError::Load(_))));
    // A real library that is not a VST3 module has no factory export.
    #[cfg(target_os = "linux")]
    {
        let libc = ["/lib/x86_64-linux-gnu/libm.so.6", "/usr/lib/libm.so.6"]
            .into_iter()
            .map(std::path::Path::new)
            .find(|p| p.is_file());
        if let Some(libc) = libc {
            assert!(matches!(Module::load(libc), Err(ModuleError::NoFactory)));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn a_bundle_folder_resolves_to_the_binary_inside() {
    use lumen_plugin_host::module::binary_path;
    let root = std::env::temp_dir().join(format!("lumen-host-bundle-{}", std::process::id()));
    let inside = root.join("Test.vst3/Contents/x86_64-linux");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::copy(plugin(), inside.join("Test.so")).unwrap();
    let bundle = root.join("Test.vst3");
    assert_eq!(binary_path(&bundle).unwrap(), inside.join("Test.so"));
    assert_eq!(Module::load(&bundle).expect("load").classes().len(), 4);
    assert!(matches!(
        binary_path(&root.join("Empty.vst3")),
        Err(ModuleError::NotFound(_))
    ));
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_bundle_uses_info_plist_executable_and_requires_bundle_entry() {
    use lumen_plugin_host::module::binary_path;

    let root = std::env::temp_dir().join(format!("lumen-host-mac-bundle-{}", std::process::id()));
    let bundle = root.join("Test.vst3");
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    // A lexically earlier file must not be selected in place of the plist's
    // executable name.
    std::fs::write(macos.join("A-decoy"), b"not a module").unwrap();
    std::fs::copy(
        plugin().join("Contents/MacOS/lumen_test_plugin"),
        macos.join("Z-selected"),
    )
    .unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Z-selected</string><key>CFBundleIdentifier</key><string>org.lumen.test.bundle</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>",
    )
    .unwrap();

    assert_eq!(binary_path(&bundle).unwrap(), macos.join("Z-selected"));
    assert_eq!(
        Module::load(&bundle).expect("load bundle").classes().len(),
        4
    );
    assert_eq!(
        Module::load(&macos.join("Z-selected"))
            .expect("load bare dylib path")
            .classes()
            .len(),
        4
    );

    let no_executable = root.join("Missing.vst3");
    std::fs::create_dir_all(no_executable.join("Contents/MacOS")).unwrap();
    std::fs::write(
        no_executable.join("Contents/Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>org.lumen.test.missing</string></dict></plist>",
    )
    .unwrap();
    assert!(matches!(
        binary_path(&no_executable),
        Err(ModuleError::NotFound(_))
    ));

    let invalid = root.join("Invalid.vst3");
    let invalid_contents = invalid.join("Contents");
    let invalid_macos = invalid_contents.join("MacOS");
    std::fs::create_dir_all(&invalid_macos).unwrap();
    std::fs::write(invalid_macos.join("Invalid"), b"not a Mach-O executable").unwrap();
    std::fs::write(
        invalid_contents.join("Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Invalid</string><key>CFBundleIdentifier</key><string>org.lumen.test.invalid</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>",
    )
    .unwrap();
    let Err(ModuleError::Load(message)) = Module::load(&invalid) else {
        panic!("an invalid executable should report CoreFoundation's load reason");
    };
    assert!(
        message.contains("CoreFoundation could not load"),
        "{message}"
    );
    assert!(
        !message.contains("CoreFoundation supplied no load error"),
        "{message}"
    );
    std::fs::remove_dir_all(&root).ok();
}

fn instrument() -> (Module, Instance) {
    let module = Module::load(plugin()).expect("load");
    let instance = Instance::create(&module, None).expect("create");
    (module, instance)
}

#[test]
fn a_note_sounds_from_its_sample_offset() {
    let (_module, mut instance) = instrument();
    instance.configure(FORMAT).expect("configure");
    let level = ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    };
    let done = instance
        .process(
            64,
            &[],
            &[note_on(10, 69), note_off(40, 69)],
            std::slice::from_ref(&level),
        )
        .expect("process");
    assert_eq!(done.status, 0);
    assert_eq!(done.repaired, 0);
    assert_eq!(done.output.len(), 2);
    let out = &done.output[0];
    assert!(out[..10].iter().all(|&s| s == 0.0));
    assert!(out[10..40].iter().all(|&s| s == 1.0 || s == -1.0));
    assert!(out[40..].iter().all(|&s| s == 0.0));
    assert_eq!(done.output[0], done.output[1]);
}

#[test]
fn the_output_does_not_depend_on_how_blocks_are_cut() {
    let (_module, mut whole) = instrument();
    whole.configure(FORMAT).unwrap();
    let level = ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    };
    let one = whole
        .process(
            128,
            &[],
            &[note_on(5, 60), note_off(100, 60)],
            std::slice::from_ref(&level),
        )
        .unwrap();

    let (_module, mut split) = instrument();
    split.configure(FORMAT).unwrap();
    let first = split
        .process(32, &[], &[note_on(5, 60)], std::slice::from_ref(&level))
        .unwrap();
    let second = split.process(96, &[], &[note_off(68, 60)], &[]).unwrap();
    let joined: Vec<f32> = first.output[0]
        .iter()
        .chain(&second.output[0])
        .copied()
        .collect();
    assert_eq!(joined, one.output[0]);
}

#[test]
fn the_effect_delays_by_its_latency_and_reports_it() {
    let module = Module::load(plugin()).expect("load");
    let mut instance = Instance::create(&module, Some(effect_class())).expect("create");
    instance
        .configure(Format {
            inputs: 2,
            ..FORMAT
        })
        .expect("configure");
    let mut impulse = vec![0.0_f32; 64];
    impulse[0] = 1.0;
    let delay = ParamQueue {
        id: LATENCY,
        points: vec![(0, 24.0 / f64::from(MAX_LATENCY))],
    };
    let done = instance
        .process(
            64,
            &[impulse.clone(), impulse],
            &[],
            std::slice::from_ref(&delay),
        )
        .expect("process");
    let at: Vec<usize> = (0..64).filter(|&i| done.output[0][i] != 0.0).collect();
    assert_eq!(at, [24]);
    assert_eq!(done.latency, 24);
    assert_eq!(instance.tail(), 24);
}

#[test]
fn state_round_trips_between_instances() {
    let (_module, mut a) = instrument();
    a.configure(FORMAT).unwrap();
    a.process(
        8,
        &[],
        &[],
        &[ParamQueue {
            id: LEVEL,
            points: vec![(0, 0.25)],
        }],
    )
    .unwrap();
    let state = a.state().expect("state");
    let (_module2, mut b) = instrument();
    b.configure(FORMAT).unwrap();
    b.set_state(&state).expect("restore");
    assert_eq!(b.state().unwrap(), state);
    // Damaged state is refused, and says so.
    assert!(b.set_state(&[1, 2, 3]).is_err());
}

#[test]
fn parameters_are_listed_from_the_controller() {
    let (_module, instance) = instrument();
    let (total, list) = instance.parameters();
    assert_eq!(total, 2);
    assert_eq!(list[0].title, "Level");
    assert_eq!(list[1].title, "Wave");
    assert_eq!(list[1].steps, 1);
}

#[test]
fn a_configuration_the_plugin_does_not_have_is_refused() {
    let (_module, mut instance) = instrument();
    // The instrument has no audio input bus, and a mono main output is
    // something it will not agree to.
    let err = instance
        .configure(Format {
            outputs: 1,
            ..FORMAT
        })
        .unwrap_err();
    assert!(err.to_string().contains("channels"), "{err}");
    assert!(
        instance.process(8, &[], &[], &[]).is_err(),
        "a failed configuration leaves nothing to process"
    );
}
