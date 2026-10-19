//! The wire form of every request and response (a tag, then fields in order).

use super::*;
use crate::shm::{Layout, Waiting, Wake};
use crate::wire::{Reader, WireError, Writer};

fn write_bounds(w: &mut Writer, bounds: Option<&EditorBounds>) {
    match bounds {
        Some(b) => {
            w.bool(true);
            w.i32(b.x);
            w.i32(b.y);
            w.u32(b.width);
            w.u32(b.height);
            w.u32(b.dpi);
        }
        None => w.bool(false),
    }
}

fn read_bounds(r: &mut Reader<'_>) -> Result<Option<EditorBounds>, WireError> {
    if !r.bool()? {
        return Ok(None);
    }
    let bounds = EditorBounds {
        x: r.i32()?,
        y: r.i32()?,
        width: r.u32()?,
        height: r.u32()?,
        dpi: r.u32()?,
    };
    // A size a window could never have is a message that lies.
    if bounds.width > MAX_EDITOR_SIDE || bounds.height > MAX_EDITOR_SIDE || bounds.dpi > 2_000 {
        return Err(WireError::Invalid("editor bounds"));
    }
    Ok(Some(bounds))
}

pub(super) const REQ_LOAD: u16 = 1;
pub(super) const REQ_CONFIGURE: u16 = 2;
pub(super) const REQ_PROCESS: u16 = 3;
pub(super) const REQ_GET_STATE: u16 = 4;
pub(super) const REQ_SET_STATE: u16 = 5;
pub(super) const REQ_PARAMETERS: u16 = 6;
pub(super) const REQ_QUIT: u16 = 7;
pub(super) const REQ_ATTACH: u16 = 8;
pub(super) const REQ_UNITS: u16 = 9;
pub(super) const REQ_PARAM_TEXT: u16 = 10;
pub(super) const REQ_SET_PARAM: u16 = 11;
pub(super) const REQ_TAKE_EDITS: u16 = 12;
pub(super) const REQ_PARAM_VALUE: u16 = 13;
pub(super) const REQ_OPEN_EDITOR: u16 = 14;
pub(super) const REQ_CLOSE_EDITOR: u16 = 15;
pub(super) const REQ_EDITOR_STATUS: u16 = 16;
pub(super) const REQ_CAPTURE_KEYS: u16 = 17;
pub(super) const REQ_SHOW_EDITOR: u16 = 18;
pub(super) const REQ_PROGRAMS: u16 = 19;
pub(super) const REQ_GET_CONTROLLER_STATE: u16 = 20;
pub(super) const REQ_SET_CONTROLLER_STATE: u16 = 21;
pub(super) const REQ_BUSES: u16 = 22;

pub(super) const RES_LOADED: u16 = 101;
pub(super) const RES_CONFIGURED: u16 = 102;
pub(super) const RES_PROCESSED: u16 = 103;
pub(super) const RES_STATE: u16 = 104;
pub(super) const RES_PARAMETERS: u16 = 105;
pub(super) const RES_DONE: u16 = 106;
pub(super) const RES_FAILED: u16 = 107;
pub(super) const RES_ATTACHED: u16 = 108;
pub(super) const RES_UNITS: u16 = 109;
pub(super) const RES_TEXT: u16 = 110;
pub(super) const RES_EDITS: u16 = 111;
pub(super) const RES_VALUE: u16 = 112;
pub(super) const RES_EDITOR: u16 = 113;
pub(super) const RES_KEY: u16 = 114;
pub(super) const RES_PROGRAMS: u16 = 115;
pub(super) const RES_BUSES: u16 = 116;

fn put_audio(w: &mut Writer, channels: &[Vec<f32>]) {
    w.u32(channels.len() as u32);
    for channel in channels {
        w.u32(channel.len() as u32);
        for &s in channel {
            w.f32(s);
        }
    }
}

fn get_audio(r: &mut Reader<'_>, what: &'static str) -> Result<Vec<Vec<f32>>, WireError> {
    let channels = r.count(what, MAX_CHANNELS, 4)?;
    let mut out = Vec::with_capacity(channels);
    for _ in 0..channels {
        let n = r.count("frames", MAX_FRAMES, 4)?;
        let mut channel = Vec::with_capacity(n);
        for _ in 0..n {
            channel.push(r.f32()?);
        }
        out.push(channel);
    }
    // A block is rectangular: every channel is the same length.
    if out.windows(2).any(|w| w[0].len() != w[1].len()) {
        return Err(WireError::Invalid("channel lengths"));
    }
    Ok(out)
}

fn put_class(w: &mut Writer, c: &ClassInfo) {
    w.raw(&c.id);
    w.str(&c.name);
    w.str(&c.category);
    w.str(&c.vendor);
    w.str(&c.subcategories);
}

fn get_class(r: &mut Reader<'_>) -> Result<ClassInfo, WireError> {
    Ok(ClassInfo {
        id: r.raw()?,
        name: r.str("name", MAX_TEXT)?,
        category: r.str("category", MAX_TEXT)?,
        vendor: r.str("vendor", MAX_TEXT)?,
        subcategories: r.str("subcategories", MAX_TEXT)?,
    })
}

impl Request {
    pub(super) fn encode(&self) -> (u16, Vec<u8>) {
        let mut w = Writer::new();
        let kind = match self {
            Request::Load { path, class } => {
                w.str(path);
                match class {
                    Some(id) => {
                        w.bool(true);
                        w.raw(id);
                    }
                    None => w.bool(false),
                }
                REQ_LOAD
            }
            Request::Configure {
                generation,
                sample_rate,
                max_frames,
                inputs,
                outputs,
                aux_inputs,
                aux_outputs,
            } => {
                w.u32(*generation);
                w.f64(*sample_rate);
                w.u32(*max_frames);
                w.u8(*inputs);
                w.u8(*outputs);
                w.u32(*aux_inputs);
                w.u32(*aux_outputs);
                REQ_CONFIGURE
            }
            Request::Buses => REQ_BUSES,
            Request::Process {
                generation,
                seq,
                frames,
                input,
                events,
                params,
            } => {
                w.u32(*generation);
                w.u64(*seq);
                w.u32(*frames);
                put_audio(&mut w, input);
                w.u32(events.len() as u32);
                for e in events {
                    w.u32(e.offset);
                    let (tag, channel, pitch, velocity) = e.kind.parts();
                    w.u8(tag);
                    w.u16(channel as u16);
                    w.u16(pitch as u16);
                    w.f32(velocity);
                }
                w.u32(params.len() as u32);
                for q in params {
                    w.u32(q.id);
                    w.u32(q.points.len() as u32);
                    for &(offset, value) in &q.points {
                        w.u32(offset);
                        w.f64(value);
                    }
                }
                REQ_PROCESS
            }
            Request::GetState => REQ_GET_STATE,
            Request::SetState(bytes) => {
                w.bytes(bytes);
                REQ_SET_STATE
            }
            Request::Parameters => REQ_PARAMETERS,
            Request::Units => REQ_UNITS,
            Request::Programs => REQ_PROGRAMS,
            Request::GetControllerState => REQ_GET_CONTROLLER_STATE,
            Request::SetControllerState(bytes) => {
                w.bytes(bytes);
                REQ_SET_CONTROLLER_STATE
            }
            Request::ParamText { id, value } => {
                w.u32(*id);
                w.f64(*value);
                REQ_PARAM_TEXT
            }
            Request::ParamValue { id, text } => {
                w.u32(*id);
                w.str(text);
                REQ_PARAM_VALUE
            }
            Request::SetParam { id, value } => {
                w.u32(*id);
                w.f64(*value);
                REQ_SET_PARAM
            }
            Request::OpenEditor { bounds } => {
                write_bounds(&mut w, bounds.as_ref());
                REQ_OPEN_EDITOR
            }
            Request::CloseEditor => REQ_CLOSE_EDITOR,
            Request::ShowEditor(visible) => {
                w.u8(u8::from(*visible));
                REQ_SHOW_EDITOR
            }
            Request::EditorStatus => REQ_EDITOR_STATUS,
            Request::TakeEdits => REQ_TAKE_EDITS,
            Request::Attach { base, layout, wake } => {
                w.str(base);
                w.u32(layout.max_frames);
                w.u8(layout.inputs);
                w.u8(layout.outputs);
                w.u32(layout.max_events);
                w.u32(layout.max_queues);
                w.u32(layout.max_points);
                for waiting in [wake.host, wake.lumen] {
                    let (tag, micros) = waiting.to_parts();
                    w.u8(tag);
                    w.u32(micros);
                }
                REQ_ATTACH
            }
            Request::CaptureKeys(keys) => {
                w.bytes(keys);
                REQ_CAPTURE_KEYS
            }
            Request::Quit => REQ_QUIT,
        };
        (kind, w.into_bytes())
    }

    pub(super) fn decode(kind: u16, payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        let request = match kind {
            REQ_LOAD => Request::Load {
                path: r.str("path", MAX_TEXT)?,
                class: if r.bool()? { Some(r.raw()?) } else { None },
            },
            REQ_CONFIGURE => {
                let request = Request::Configure {
                    generation: r.u32()?,
                    sample_rate: r.f64()?,
                    max_frames: r.u32()?,
                    inputs: r.u8()?,
                    outputs: r.u8()?,
                    aux_inputs: r.u32()?,
                    aux_outputs: r.u32()?,
                };
                if let Request::Configure {
                    sample_rate,
                    max_frames,
                    inputs,
                    outputs,
                    ..
                } = request
                    && (!(sample_rate.is_finite() && (8_000.0..=768_000.0).contains(&sample_rate))
                        || max_frames == 0
                        || max_frames as usize > MAX_FRAMES
                        || inputs as usize > MAX_CHANNELS
                        || outputs as usize > MAX_CHANNELS)
                {
                    return Err(WireError::Invalid("format"));
                }
                request
            }
            REQ_PROCESS => {
                let generation = r.u32()?;
                let seq = r.u64()?;
                let frames = r.u32()?;
                if frames == 0 || frames as usize > MAX_FRAMES {
                    return Err(WireError::Invalid("frame count"));
                }
                let input = get_audio(&mut r, "input channels")?;
                if input.iter().any(|c| c.len() != frames as usize) {
                    return Err(WireError::Invalid("input length"));
                }
                let n = r.count("events", MAX_EVENTS, 13)?;
                let mut events = Vec::with_capacity(n);
                for _ in 0..n {
                    let offset = r.u32()?;
                    if offset >= frames {
                        return Err(WireError::Invalid("event offset"));
                    }
                    let tag = r.u8()?;
                    let channel = r.u16()? as i16;
                    let pitch = r.u16()? as i16;
                    let velocity = r.f32()?;
                    let kind = EventKind::from_parts(tag, channel, pitch, velocity)
                        .ok_or(WireError::Invalid("event kind"))?;
                    events.push(Event { offset, kind });
                }
                let n = r.count("parameter queues", MAX_QUEUES, 8)?;
                let mut params = Vec::with_capacity(n);
                let mut points_total = 0;
                for _ in 0..n {
                    let id = r.u32()?;
                    let m = r.count("points", MAX_POINTS, 12)?;
                    points_total += m;
                    if points_total > MAX_POINTS {
                        return Err(WireError::TooLarge {
                            what: "points in a block",
                            max: MAX_POINTS,
                            got: points_total,
                        });
                    }
                    let mut points = Vec::with_capacity(m);
                    for _ in 0..m {
                        let offset = r.u32()?;
                        if offset >= frames {
                            return Err(WireError::Invalid("point offset"));
                        }
                        points.push((offset, r.f64()?));
                    }
                    params.push(ParamQueue { id, points });
                }
                Request::Process {
                    generation,
                    seq,
                    frames,
                    input,
                    events,
                    params,
                }
            }
            REQ_GET_STATE => Request::GetState,
            REQ_SET_STATE => Request::SetState(r.bytes("state", MAX_STATE)?.to_vec()),
            REQ_PARAMETERS => Request::Parameters,
            REQ_UNITS => Request::Units,
            REQ_PROGRAMS => Request::Programs,
            REQ_BUSES => Request::Buses,
            REQ_GET_CONTROLLER_STATE => Request::GetControllerState,
            REQ_SET_CONTROLLER_STATE => {
                Request::SetControllerState(r.bytes("controller state", MAX_STATE)?.to_vec())
            }
            REQ_PARAM_TEXT => Request::ParamText {
                id: r.u32()?,
                value: r.f64()?,
            },
            REQ_PARAM_VALUE => Request::ParamValue {
                id: r.u32()?,
                text: r.str("typed value", MAX_TEXT)?,
            },
            REQ_SET_PARAM => Request::SetParam {
                id: r.u32()?,
                value: r.f64()?,
            },
            REQ_OPEN_EDITOR => Request::OpenEditor {
                bounds: read_bounds(&mut r)?,
            },
            REQ_CLOSE_EDITOR => Request::CloseEditor,
            REQ_SHOW_EDITOR => Request::ShowEditor(r.bool()?),
            REQ_EDITOR_STATUS => Request::EditorStatus,
            REQ_TAKE_EDITS => Request::TakeEdits,
            REQ_ATTACH => {
                let base = r.str("base name", MAX_BASE)?;
                if base.is_empty() || !base.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(WireError::Invalid("base name"));
                }
                let layout = Layout {
                    max_frames: r.u32()?,
                    inputs: r.u8()?,
                    outputs: r.u8()?,
                    max_events: r.u32()?,
                    max_queues: r.u32()?,
                    max_points: r.u32()?,
                };
                layout
                    .check()
                    .map_err(|_| WireError::Invalid("shared layout"))?;
                let mut waiting = || -> Result<Waiting, WireError> {
                    let tag = r.u8()?;
                    let micros = r.u32()?;
                    Waiting::from_parts(tag, micros).ok_or(WireError::Invalid("wake mode"))
                };
                let host = waiting()?;
                let lumen = waiting()?;
                Request::Attach {
                    base,
                    layout,
                    wake: Wake { host, lumen },
                }
            }
            REQ_CAPTURE_KEYS => Request::CaptureKeys(r.bytes("keys", MAX_KEYS)?.to_vec()),
            REQ_QUIT => Request::Quit,
            _ => return Err(WireError::Invalid("request kind")),
        };
        r.finish()?;
        Ok(request)
    }
}

impl Response {
    pub(super) fn encode(&self) -> (u16, Vec<u8>) {
        let mut w = Writer::new();
        let kind = match self {
            Response::Loaded(class) => {
                put_class(&mut w, class);
                RES_LOADED
            }
            Response::Configured {
                generation,
                latency,
                tail,
                inputs,
                outputs,
            } => {
                w.u32(*generation);
                w.u32(*latency);
                w.u32(*tail);
                w.u8(*inputs);
                w.u8(*outputs);
                RES_CONFIGURED
            }
            Response::Buses { inputs, outputs } => {
                for side in [inputs, outputs] {
                    w.u32(side.len() as u32);
                    for bus in side {
                        w.str(&bus.name);
                        w.u8(bus.channels);
                        w.bool(bus.main);
                        w.bool(bus.default_active);
                    }
                }
                RES_BUSES
            }
            Response::Processed {
                generation,
                seq,
                status,
                latency,
                restart,
                repaired,
                output,
            } => {
                w.u32(*generation);
                w.u64(*seq);
                w.i32(*status);
                w.u32(*latency);
                w.u32(*restart);
                w.u32(*repaired);
                put_audio(&mut w, output);
                RES_PROCESSED
            }
            Response::State(bytes) => {
                w.bytes(bytes);
                RES_STATE
            }
            Response::Parameters { total, list } => {
                w.u32(*total);
                w.u32(list.len() as u32);
                for p in list {
                    w.u32(p.id);
                    w.str(&p.title);
                    w.str(&p.units);
                    w.i32(p.steps);
                    w.f64(p.default);
                    w.u32(p.flags);
                    w.i32(p.unit);
                    w.f64(p.value);
                }
                RES_PARAMETERS
            }
            Response::Units(list) => {
                w.u32(list.len() as u32);
                for u in list {
                    w.i32(u.id);
                    w.i32(u.parent);
                    w.str(&u.name);
                }
                RES_UNITS
            }
            Response::Programs(lists) => {
                w.u32(lists.len() as u32);
                for l in lists {
                    w.i32(l.id);
                    w.str(&l.name);
                    w.i32(l.unit);
                    w.bool(l.parameter.is_some());
                    w.u32(l.parameter.unwrap_or(0));
                    w.u32(l.programs.len() as u32);
                    for name in &l.programs {
                        w.str(name);
                    }
                }
                RES_PROGRAMS
            }
            Response::Text(text) => {
                w.str(text);
                RES_TEXT
            }
            Response::Value(value) => {
                w.f64(*value);
                RES_VALUE
            }
            Response::Editor(status) => {
                w.bool(status.open);
                w.bool(status.visible);
                write_bounds(&mut w, status.bounds.as_ref());
                RES_EDITOR
            }
            Response::Edits {
                list,
                dropped,
                ignored,
            } => {
                w.u32(*dropped);
                w.u32(*ignored);
                w.u32(list.len() as u32);
                for e in list {
                    w.u32(e.id);
                    w.f64(e.value);
                    w.u8(e.kind as u8);
                }
                RES_EDITS
            }
            Response::Done => RES_DONE,
            Response::Attached => RES_ATTACHED,
            Response::Failed { kind, message } => {
                w.u8(*kind as u8);
                w.str(message);
                RES_FAILED
            }
            Response::Key { code, down } => {
                w.u8(*code);
                w.bool(*down);
                RES_KEY
            }
        };
        (kind, w.into_bytes())
    }

    pub(super) fn decode(kind: u16, payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        let response = match kind {
            RES_LOADED => Response::Loaded(get_class(&mut r)?),
            RES_CONFIGURED => Response::Configured {
                generation: r.u32()?,
                latency: r.u32()?,
                tail: r.u32()?,
                inputs: r.u8()?,
                outputs: r.u8()?,
            },
            RES_BUSES => {
                let mut sides = [Vec::new(), Vec::new()];
                for side in &mut sides {
                    let n = r.count("buses", MAX_BUSES, 7)?;
                    for _ in 0..n {
                        side.push(BusInfo {
                            name: r.str("bus name", MAX_TEXT)?,
                            channels: r.u8()?,
                            main: r.bool()?,
                            default_active: r.bool()?,
                        });
                    }
                }
                let [inputs, outputs] = sides;
                Response::Buses { inputs, outputs }
            }
            RES_PROCESSED => Response::Processed {
                generation: r.u32()?,
                seq: r.u64()?,
                status: r.i32()?,
                latency: r.u32()?,
                restart: r.u32()?,
                repaired: r.u32()?,
                output: get_audio(&mut r, "output channels")?,
            },
            RES_STATE => Response::State(r.bytes("state", MAX_STATE)?.to_vec()),
            RES_PARAMETERS => {
                let total = r.u32()?;
                let n = r.count("parameters", MAX_PARAMETERS, 40)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(ParamInfo {
                        id: r.u32()?,
                        title: r.str("title", MAX_TEXT)?,
                        units: r.str("units", MAX_TEXT)?,
                        steps: r.i32()?,
                        default: r.f64()?,
                        flags: r.u32()?,
                        unit: r.i32()?,
                        value: r.f64()?,
                    });
                }
                Response::Parameters { total, list }
            }
            RES_UNITS => {
                let n = r.count("units", MAX_UNITS, 12)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(UnitInfo {
                        id: r.i32()?,
                        parent: r.i32()?,
                        name: r.str("unit name", MAX_TEXT)?,
                    });
                }
                Response::Units(list)
            }
            RES_PROGRAMS => {
                let n = r.count("program lists", MAX_PROGRAM_LISTS, 21)?;
                let mut all = 0;
                let mut lists = Vec::with_capacity(n);
                for _ in 0..n {
                    let id = r.i32()?;
                    let name = r.str("list name", MAX_TEXT)?;
                    let unit = r.i32()?;
                    let has = r.bool()?;
                    let parameter = r.u32()?;
                    let count = r.count("programs", MAX_PROGRAMS, 4)?;
                    all += count;
                    if all > MAX_PROGRAMS_ALL {
                        return Err(WireError::TooLarge {
                            what: "programs in all",
                            max: MAX_PROGRAMS_ALL,
                            got: all,
                        });
                    }
                    let mut programs = Vec::with_capacity(count);
                    for _ in 0..count {
                        programs.push(r.str("program name", MAX_TEXT)?);
                    }
                    lists.push(ProgramList {
                        id,
                        name,
                        unit,
                        parameter: has.then_some(parameter),
                        programs,
                    });
                }
                Response::Programs(lists)
            }
            RES_TEXT => Response::Text(r.str("text", MAX_TEXT)?),
            RES_VALUE => Response::Value(r.f64()?),
            RES_EDITOR => Response::Editor(EditorStatus {
                open: r.bool()?,
                visible: r.bool()?,
                bounds: read_bounds(&mut r)?,
            }),
            RES_EDITS => {
                let dropped = r.u32()?;
                let ignored = r.u32()?;
                let n = r.count("edits", MAX_EDITS, 13)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(Edit {
                        id: r.u32()?,
                        value: r.f64()?,
                        kind: EditKind::from_u8(r.u8()?)?,
                    });
                }
                Response::Edits {
                    list,
                    dropped,
                    ignored,
                }
            }
            RES_DONE => Response::Done,
            RES_ATTACHED => Response::Attached,
            RES_FAILED => Response::Failed {
                kind: FailureKind::from_u8(r.u8()?)?,
                message: r.str("message", MAX_TEXT)?,
            },
            RES_KEY => Response::Key {
                code: r.u8()?,
                down: r.bool()?,
            },
            _ => return Err(WireError::Invalid("response kind")),
        };
        r.finish()?;
        Ok(response)
    }
}
