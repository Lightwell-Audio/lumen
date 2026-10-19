//! The device thread's tests.

use super::*;

#[test]
fn a_crash_is_worded_short_with_its_cause_first() {
    let failure = Failure {
        step: Step::Load,
        error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
        detail: None,
    };
    assert_eq!(
        failure.to_string(),
        "Crashed while loading: access violation, exception 0xc0000005."
    );
    let located = Failure {
        step: Step::Running,
        error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
        detail: Some("in Plug.dll+0x1f2a, reading address 0x0".into()),
    };
    assert_eq!(
        located.to_string(),
        "Crashed while running: access violation, exception 0xc0000005, in Plug.dll+0x1f2a, reading address 0x0."
    );
    let timeout = Failure {
        step: Step::Configure,
        error: HostError::Timeout,
        detail: None,
    };
    assert!(timeout.to_string().contains("did not answer"));
}

use std::sync::atomic::AtomicUsize;

fn describe(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .map(|c| match c {
            Command::Set(id, v) => format!("set {id}={v}"),
            Command::Text(id, v) => format!("text {id}={v}"),
            Command::Texts(id, v) => format!("texts {id} x{}", v.len()),
            Command::Typed(id, t) => format!("typed {id}={t}"),
            Command::OpenEditor(_) => "open editor".into(),
            Command::CloseEditor => "close editor".into(),
            Command::ShowEditor(_) => "show or hide editor".into(),
            Command::End => "end".into(),
            Command::Relink(..) => "relink".into(),
            Command::Capture => "capture".into(),
            Command::Load { .. } => "load".into(),
            Command::Refresh => "refresh".into(),
            Command::Keys(keys) => format!("keys {}", keys.len()),
            Command::Quit => "quit".into(),
        })
        .collect()
}

#[test]
fn a_drag_of_one_parameter_is_one_value_and_the_order_of_the_rest_is_kept() {
    let kept = coalesce(vec![
        Command::Set(1, 0.1),
        Command::Set(2, 0.5),
        Command::Text(1, 0.1),
        Command::Set(1, 0.2),
        Command::Relink(48_000.0, 128, None),
        Command::Text(1, 0.2),
        Command::Set(1, 0.3),
        Command::Keys(vec![1, 2]),
        Command::Keys(vec![]),
        Command::Quit,
    ]);
    assert_eq!(
        describe(&kept),
        [
            "set 2=0.5",
            "relink",
            "text 1=0.2",
            "set 1=0.3",
            "keys 0",
            "quit"
        ]
    );
}

#[test]
fn the_gate_lets_only_its_permits_through_at_once() {
    let gate = Gate::new(2);
    let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (gate, inside, most) = (gate.clone(), inside.clone(), most.clone());
            std::thread::spawn(move || {
                let _place = gate.permit();
                let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                inside.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(most.load(Ordering::SeqCst), 2);
}

#[test]
fn a_load_that_panics_gives_its_permit_back() {
    let gate = Gate::new(1);
    for _ in 0..4 {
        let gate = gate.clone();
        let result = std::thread::spawn(move || {
            let _place = gate.permit();
            panic!("a load that went wrong");
        })
        .join();
        assert!(result.is_err());
    }
    // The one permit is free: this does not wait for ever.
    let (done, heard) = mpsc::channel();
    let again = gate.clone();
    std::thread::spawn(move || {
        let _place = again.permit();
        done.send(()).unwrap();
    });
    assert!(heard.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn a_thread_that_panics_says_so() {
    let said = Cell::new(false);
    catching(|| panic!("a bug"), || said.set(true));
    assert!(said.get());
    let said = Cell::new(false);
    catching(|| {}, || said.set(true));
    assert!(!said.get());
}

#[test]
fn failures_say_what_was_tried_and_how_it_went() {
    let hung = Failure {
        step: Step::Load,
        error: HostError::Timeout,
        detail: None,
    };
    assert!(hung.hung() && !hung.crashed());
    assert!(hung.to_string().contains("did not answer"));
    assert!(hung.to_string().contains("load the plugin"));
}
