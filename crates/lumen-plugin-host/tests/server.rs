//! The host's protocol handling, driven with raw frames: what it does with
//! a block from an older configuration and with bytes it cannot trust.

mod support;

use std::io::Write;
use std::process::{Command, Stdio};

use lumen_plugin_host::protocol::{FailureKind, Request, Response, read_response, write_request};
use lumen_plugin_host::server::{Exit, serve};
use support::{HOST, plugin};

fn script(requests: &[Request]) -> (Vec<Response>, Exit) {
    let mut input = Vec::new();
    for request in requests {
        write_request(&mut input, request).unwrap();
    }
    let mut output = Vec::new();
    let exit = serve(&mut input.as_slice(), &mut output).unwrap();
    let mut responses = Vec::new();
    let mut bytes = output.as_slice();
    while !bytes.is_empty() {
        responses.push(read_response(&mut bytes).expect("a whole response"));
    }
    (responses, exit)
}

fn load() -> Request {
    Request::Load {
        path: plugin().to_string_lossy().into_owned(),
        class: None,
    }
}

fn configure(generation: u32) -> Request {
    Request::Configure {
        generation,
        sample_rate: 48_000.0,
        max_frames: 256,
        inputs: 0,
        outputs: 2,
        aux_inputs: 0,
        aux_outputs: 0,
    }
}

fn block(generation: u32, seq: u64) -> Request {
    Request::Process {
        generation,
        seq,
        frames: 16,
        input: vec![],
        events: vec![],
        params: vec![],
    }
}

#[test]
fn a_block_from_an_older_configuration_is_refused_and_not_processed() {
    let (responses, exit) = script(&[
        load(),
        configure(1),
        block(1, 0),
        configure(2),
        // A late block from generation 1.
        block(1, 1),
        block(2, 2),
        Request::Quit,
    ]);
    assert_eq!(exit, Exit::Quit);
    assert!(matches!(responses[0], Response::Loaded(_)));
    assert!(matches!(
        responses[2],
        Response::Processed {
            generation: 1,
            seq: 0,
            ..
        }
    ));
    assert!(matches!(
        responses[4],
        Response::Failed {
            kind: FailureKind::StaleConfiguration,
            ..
        }
    ));
    assert!(matches!(
        responses[5],
        Response::Processed {
            generation: 2,
            seq: 2,
            ..
        }
    ));
    assert_eq!(responses[6], Response::Done);
}

#[test]
fn a_failed_configuration_leaves_no_generation_to_process_in() {
    let (responses, _) = script(&[
        load(),
        configure(1),
        Request::Configure {
            generation: 2,
            sample_rate: 48_000.0,
            max_frames: 256,
            inputs: 0,
            // Not something the instrument will agree to.
            outputs: 1,
            aux_inputs: 0,
            aux_outputs: 0,
        },
        block(2, 0),
        block(1, 0),
    ]);
    assert!(matches!(responses[2], Response::Failed { .. }));
    for response in &responses[3..] {
        assert!(matches!(
            response,
            Response::Failed {
                kind: FailureKind::Sequence,
                ..
            }
        ));
    }
}

#[test]
fn a_closed_stream_ends_the_loop_quietly() {
    let (responses, exit) = script(&[load()]);
    assert_eq!(exit, Exit::Closed);
    assert_eq!(responses.len(), 1);
}

#[test]
fn garbage_gets_one_protocol_failure_and_ends_the_loop() {
    let mut input = b"this is not a frame at all".to_vec();
    input.extend_from_slice(&[0; 64]);
    let mut output = Vec::new();
    let exit = serve(&mut input.as_slice(), &mut output).unwrap();
    assert_eq!(exit, Exit::Malformed);
    let response = read_response(&mut output.as_slice()).unwrap();
    assert!(matches!(
        response,
        Response::Failed {
            kind: FailureKind::Protocol,
            ..
        }
    ));
}

#[test]
fn the_host_binary_exits_with_a_code_on_garbage_and_cleanly_on_close() {
    let mut child = Command::new(HOST)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"garbage that is long enough to be a header")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let response = read_response(&mut output.stdout.as_slice()).unwrap();
    assert!(matches!(response, Response::Failed { .. }));

    // Closing stdin with nothing sent is how a host learns Lumen is gone.
    let mut child = Command::new(HOST)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn an_exchange_through_the_real_binary_parses_end_to_end() {
    // The host's stdout carries only frames (a plugin's own prints are
    // pointed at stderr); a whole exchange must parse with nothing left over.
    let mut child = Command::new(HOST)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    write_request(&mut stdin, &load()).unwrap();
    write_request(&mut stdin, &Request::Quit).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let mut bytes = output.stdout.as_slice();
    assert!(matches!(
        read_response(&mut bytes).unwrap(),
        Response::Loaded(_)
    ));
    assert_eq!(read_response(&mut bytes).unwrap(), Response::Done);
    assert!(bytes.is_empty());
}
