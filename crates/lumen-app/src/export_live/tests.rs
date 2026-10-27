use super::*;
use lumen_engine::{OFFLINE_WAIT, SourceRender, render_source};
use lumen_project::testing::TempDir;

struct Signal {
    remaining: usize,
    tail: bool,
}

impl Source for Signal {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        let n = out.frames();
        for channel in 0..out.channels().min(2) {
            for (i, sample) in out.channel(channel).iter_mut().enumerate() {
                *sample = if self.tail {
                    if i < self.remaining { 0.02 } else { 0.0 }
                } else {
                    ((cx.start + i as i64 - 32) as f32 * 0.07).sin() * 0.2
                };
            }
        }
        if self.tail {
            self.remaining = self.remaining.saturating_sub(n);
        }
    }
    fn latency(&self) -> u32 {
        32
    }
    fn stopped(&mut self) {
        self.tail = true;
    }
    fn ringing(&self) -> bool {
        self.tail && self.remaining > 0
    }
}

fn timeline() -> Timeline {
    Timeline::default()
}

fn request(file: PathBuf) -> Request {
    Request {
        source: Box::new(Signal {
            remaining: 1234,
            tail: false,
        }),
        timeline: timeline(),
        rate: 48_000,
        from: 155,
        frames: 10_000,
        most_tail: 4800,
        file,
        statuses: vec![],
        keepalive: Box::new(()),
    }
}

fn driven(
    file: PathBuf,
    device_rate: u32,
    cancel: Arc<AtomicBool>,
) -> Result<Option<Rendered>, String> {
    let job = request(file);
    drive(job, device_rate, cancel)
}

fn drive(
    job: Request,
    device_rate: u32,
    cancel: Arc<AtomicBool>,
) -> Result<Option<Rendered>, String> {
    let (handle, mut tap) = channel();
    let worker = thread::spawn(move || handle.capture(job, &cancel, &mut |_| {}));
    let mut output = vec![0.0f32; 640];
    let end = Instant::now() + Duration::from_secs(10);
    let mut block_index = 0;
    let mut violations = 0;
    while !worker.is_finished() && Instant::now() < end {
        let frames = [64, 128, 320][block_index % 3];
        block_index += 1;
        let mut block = lumen_audio::Block {
            sample_rate: device_rate,
            inputs: &[],
            input_channels: 0,
            outputs: &mut output[..2 * frames],
            output_channels: 2,
            frames,
            position: None,
        };
        let (_, found) = lumen_rt::count_rt_violations(|| tap.process(&mut block));
        violations += found;
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        violations, 0,
        "callback installs, drives and retires without heap work"
    );
    assert!(worker.is_finished(), "capture finishes");
    worker.join().unwrap()
}

#[test]
fn actual_callback_matches_offline_with_latency_and_a_trimmed_tail() {
    assert!(lumen_rt::guard_is_active());
    let dir = TempDir::new("export-live-golden");
    let file = dir.path().join("live.wav");
    let made = driven(file.clone(), 48_000, Arc::new(AtomicBool::new(false)))
        .unwrap()
        .unwrap();
    let got = lumen_media::decode(&file, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let mut source = Signal {
        remaining: 1234,
        tail: false,
    };
    let mut expected = [Vec::new(), Vec::new()];
    let ended = render_source(
        &mut source,
        SourceRender {
            timeline: &timeline(),
            sample_rate: 48_000,
            from: 155,
            frames: 10_000,
            most_tail: 4800,
            block: MOST_BLOCK,
            wait: OFFLINE_WAIT,
        },
        &AtomicBool::new(false),
        &mut || false,
        &mut |_| {},
        &mut |l, r| {
            expected[0].extend_from_slice(l);
            expected[1].extend_from_slice(r);
        },
    );
    assert_eq!(ended, lumen_engine::Ended::Done);
    assert_eq!(made.frames, 11_234);
    assert_eq!(got.channel(0), expected[0]);
    assert_eq!(got.channel(1), expected[1]);
}

#[test]
fn a_device_rate_change_fails_and_leaves_no_file() {
    let dir = TempDir::new("export-live-rate");
    let file = dir.path().join("live.wav");
    let why = driven(file.clone(), 44_100, Arc::new(AtomicBool::new(false))).unwrap_err();
    assert!(why.contains("sample rate changed"), "{why}");
    assert!(!file.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_without_a_device_callback_returns_and_frees_off_the_callback() {
    let dir = TempDir::new("export-live-cancel");
    let (handle, mut tap) = channel();
    let file = dir.path().join("cancelled.wav");
    assert!(
        handle
            .capture(request(file.clone()), &AtomicBool::new(true), &mut |_| {})
            .unwrap()
            .is_none()
    );
    let mut out = [0.0; 128];
    let mut block = lumen_audio::Block {
        sample_rate: 48_000,
        inputs: &[],
        input_channels: 0,
        outputs: &mut out,
        output_channels: 2,
        frames: 64,
        position: None,
    };
    assert_eq!(
        lumen_rt::count_rt_violations(|| tap.process(&mut block)).1,
        0
    );
    assert!(!file.exists());
}

#[test]
fn a_prepared_project_mixer_installs_and_renders_under_the_allocation_guard() {
    let dir = TempDir::new("export-live-mixer-rt");
    let file = dir.path().join("mix.wav");
    let job = crate::export_render::Job {
        project: lumen_project::testing::project_of("Export", vec![]),
        clips: vec![],
        plugins: vec![],
        rate: 48_000,
        from: 0,
        frames: 4800,
        tail: false,
        file: file.clone(),
        stem: None,
    };
    let prepared = crate::export_render::prepare(&job, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let made = drive(
        Request {
            source: prepared.source,
            timeline: prepared.timeline,
            rate: prepared.rate,
            from: 0,
            frames: 4800,
            most_tail: 0,
            file,
            statuses: prepared.statuses,
            keepalive: Box::new(prepared.manager),
        },
        48_000,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(made.frames, 4800);
}
