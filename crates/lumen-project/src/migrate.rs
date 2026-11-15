use serde_json::Value;

pub type Migration = fn(Value) -> Result<Value, String>;

pub const FIRST_VERSION: u64 = 1;

pub const MIGRATIONS: &[Migration] = &[
    v1_to_v2,
    v2_to_v3,
    v3_to_v4,
    v4_to_v5,
    v5_to_v6,
    v6_to_v7,
    v7_to_v8,
    v8_to_v9,
    v9_to_v10,
    v10_to_v11,
    v11_to_v12,
    v12_to_v13,
    v13_to_v14,
    v14_to_v15,
    v15_to_v16,
    v16_to_v17,
    v17_to_v18,
    v18_to_v19,
    v19_to_v20,
    v20_to_v21,
    v21_to_v22,
    v22_to_v23,
    v23_to_v24,
    v24_to_v25,
    v25_to_v26,
    v26_to_v27,
    v27_to_v28,
    v28_to_v29,
    v29_to_v30,
    v30_to_v31,
    v31_to_v32,
    v32_to_v33,
    v33_to_v34,
    add_drum_maps,
    add_synth_parameters,
    add_sampler_samples,
];

fn v1_to_v2(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;

    project.insert(
        "transport".into(),
        serde_json::json!({
            "loop_start": 0,
            "loop_end": 22_579_200,
            "loop_enabled": false,
            "punch_in": false,
            "punch_out": false,
        }),
    );
    project.insert(
        "view".into(),
        serde_json::json!({
            "left_beat": 0.0,
            "px_per_beat": 24.0,
            "browser_open": true,
            "detail_open": true,
            "browser_size": 220.0,
            "detail_size": 220.0,
            "top": 0.0,
            "track_heights": {},
            "expanded": [],
            "grid": {
                "fixed": false,
                "width": "narrow",
                "size": { "note": 16 },
                "feel": "straight",
                "snap": true,
            },
            "collapsed": [],
            "header_width": 204.0,
            "returns_folded": false,
        }),
    );
    Ok(document)
}

fn v2_to_v3(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    if let Some(tracks) = project.get_mut("tracks").and_then(Value::as_array_mut) {
        for track in tracks {
            if let Some(track) = track.as_object_mut() {
                track.entry("armed").or_insert(Value::Bool(false));
            }
        }
    }
    if let Some(view) = project.get_mut("view").and_then(Value::as_object_mut) {
        view.entry("top").or_insert(Value::from(0.0));
        view.entry("track_heights")
            .or_insert(Value::Object(serde_json::Map::new()));
        view.entry("expanded").or_insert(Value::Array(Vec::new()));
    }
    Ok(document)
}

fn v3_to_v4(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    project.entry("markers").or_insert(Value::Array(Vec::new()));
    if let Some(transport) = project.get_mut("transport").and_then(Value::as_object_mut) {
        for key in ["punch_in", "punch_out"] {
            transport.entry(key).or_insert(Value::Bool(false));
        }
    }
    if let Some(view) = project.get_mut("view").and_then(Value::as_object_mut) {
        view.entry("grid").or_insert(serde_json::json!({

            "fixed": false,
            "width": "narrow",
            "size": { "note": 16 },
            "feel": "straight",
            "snap": true,
        }));
    }
    Ok(document)
}

fn v4_to_v5(mut document: Value) -> Result<Value, String> {
    let fade = serde_json::json!({ "length": 0, "shape": "linear" });
    let tracks = document
        .get_mut("project")
        .and_then(|p| p.get_mut("tracks"))
        .and_then(Value::as_array_mut);
    for track in tracks.into_iter().flatten() {
        let clips = track.get_mut("clips").and_then(Value::as_array_mut);
        for clip in clips.into_iter().flatten() {
            let Some(clip) = clip.as_object_mut() else {
                return Err("a clip is not an object".into());
            };
            clip.entry("color").or_insert(Value::Null);
            clip.entry("locked").or_insert(Value::Bool(false));
            clip.entry("fade_in").or_insert(fade.clone());
            clip.entry("fade_out").or_insert(fade.clone());
            clip.entry("loop_length").or_insert(Value::Null);
        }
    }
    Ok(document)
}

fn v5_to_v6(mut document: Value) -> Result<Value, String> {
    const THIRD: f64 = 1.0 / 3.0;
    let curve = |shape: &str| -> Result<Option<Value>, String> {
        Ok(match shape {
            "linear" => None,
            "slow" => Some(serde_json::json!([THIRD, 0.0, 2.0 * THIRD, THIRD])),
            "fast" => Some(serde_json::json!([THIRD, 2.0 * THIRD, 2.0 * THIRD, 1.0])),
            "s_curve" => Some(serde_json::json!([THIRD, 0.0, 2.0 * THIRD, 1.0])),
            other => return Err(format!("unknown fade shape {other:?}")),
        })
    };
    let tracks = document
        .get_mut("project")
        .and_then(|p| p.get_mut("tracks"))
        .and_then(Value::as_array_mut);
    for track in tracks.into_iter().flatten() {
        let clips = track.get_mut("clips").and_then(Value::as_array_mut);
        for clip in clips.into_iter().flatten() {
            for side in ["fade_in", "fade_out"] {
                let Some(fade) = clip.get_mut(side).and_then(Value::as_object_mut) else {
                    continue;
                };
                let shape = match fade.remove("shape") {
                    Some(Value::String(shape)) => shape,
                    Some(_) => return Err("a fade shape is not a name".into()),
                    None => continue,
                };
                if let Some(curve) = curve(&shape)? {
                    fade.insert("curve".into(), curve);
                }
            }
        }
    }
    Ok(document)
}

fn v6_to_v7(mut document: Value) -> Result<Value, String> {
    let next_id = highest_id(&document)?
        .checked_add(1)
        .ok_or("the highest id is the largest possible, so no id is free for the master track")?;
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    project.insert("pan_law".into(), Value::from("constant_power"));
    let tracks = project
        .get_mut("tracks")
        .and_then(Value::as_array_mut)
        .ok_or("missing tracks")?;
    let found = tracks
        .iter()
        .position(|t| t.get("kind").and_then(Value::as_str) == Some("master"));
    let mut master = match found {
        Some(index) => tracks.remove(index),

        None => serde_json::json!({
            "id": format!("{next_id:016x}"),
            "name": "Master",
            "kind": "master",
            "color": "#8A8F99",
            "gain_db": 0.0,
            "pan": 0.0,
            "mute": false,
            "solo": false,
            "armed": false,
            "devices": [],
            "clips": [],
        }),
    };
    if let Some(master) = master.as_object_mut() {
        master.remove("parent");
    }
    for track in tracks.iter_mut().chain(std::iter::once(&mut master)) {
        let Some(track) = track.as_object_mut() else {
            return Err("a track is not an object".into());
        };
        track.entry("output").or_insert(Value::from("auto"));
        track.entry("sends").or_insert(Value::Array(Vec::new()));
        track.entry("invert").or_insert(Value::Bool(false));
        track.entry("trim_db").or_insert(Value::from(0.0));
    }
    project.insert("master".into(), master);
    Ok(document)
}

fn v7_to_v8(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    if let Some(view) = project.get_mut("view").and_then(Value::as_object_mut) {
        view.entry("collapsed").or_insert(Value::Array(Vec::new()));
    }
    let tracks = project
        .get_mut("tracks")
        .and_then(Value::as_array_mut)
        .ok_or("missing tracks")?;
    let text = |t: &Value, key: &str| t.get(key).and_then(Value::as_str).map(str::to_owned);
    for track in tracks.iter_mut() {
        let kind = text(track, "kind");
        if !matches!(kind.as_deref(), Some("audio" | "midi" | "group"))
            && let Some(track) = track.as_object_mut()
        {
            track.remove("parent");
        }
    }
    let groups: Vec<String> = tracks
        .iter()
        .filter(|t| text(t, "kind").as_deref() == Some("group"))
        .filter_map(|t| text(t, "id"))
        .collect();

    let parents: Vec<Option<String>> = tracks
        .iter()
        .map(|t| text(t, "parent").filter(|p| groups.contains(p)))
        .collect();
    let mut order = Vec::with_capacity(tracks.len());
    let mut placed = vec![false; tracks.len()];
    let mut stack: Vec<usize> = (0..tracks.len())
        .rev()
        .filter(|&i| parents[i].is_none())
        .collect();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut placed[i], true) {
            continue;
        }
        order.push(i);
        let id = text(&tracks[i], "id");
        stack.extend(
            (0..tracks.len())
                .rev()
                .filter(|&j| parents[j].is_some() && parents[j] == id),
        );
    }

    order.extend((0..tracks.len()).filter(|&i| !placed[i]));
    let mut old: Vec<Option<Value>> = tracks.drain(..).map(Some).collect();
    tracks.extend(order.into_iter().filter_map(|i| old[i].take()));
    Ok(document)
}

fn v8_to_v9(mut document: Value) -> Result<Value, String> {
    let view = document
        .get_mut("project")
        .and_then(|p| p.get_mut("view"))
        .and_then(Value::as_object_mut);
    if let Some(view) = view {
        view.entry("header_width").or_insert(Value::from(204.0));
    }
    Ok(document)
}

fn v9_to_v10(mut document: Value) -> Result<Value, String> {
    let view = document
        .get_mut("project")
        .and_then(|p| p.get_mut("view"))
        .and_then(Value::as_object_mut);
    if let Some(view) = view {
        view.entry("returns_folded").or_insert(Value::from(false));
    }
    Ok(document)
}

fn v10_to_v11(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v11_to_v12(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v12_to_v13(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v13_to_v14(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v14_to_v15(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v15_to_v16(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v16_to_v17(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v17_to_v18(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v18_to_v19(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v19_to_v20(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v20_to_v21(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v21_to_v22(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v22_to_v23(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v23_to_v24(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    let signature = project
        .remove("time_signature")
        .unwrap_or_else(|| serde_json::json!({ "numerator": 4, "denominator": 4 }));
    project.insert(
        "meter".into(),
        serde_json::json!([{ "at": 0, "meter": signature }]),
    );
    Ok(document)
}

fn v24_to_v25(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v25_to_v26(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v26_to_v27(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v27_to_v28(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    project
        .entry("auto_crossfade")
        .or_insert(Value::Bool(false));
    if let Some(tracks) = project.get_mut("tracks").and_then(Value::as_array_mut) {
        for track in tracks {
            if let Some(clips) = track.get_mut("clips").and_then(Value::as_array_mut) {
                for clip in clips {
                    if let Some(object) = clip.as_object_mut() {
                        object.entry("crossfade").or_insert(Value::Bool(false));
                    }
                }
            }
        }
    }
    Ok(document)
}

fn v28_to_v29(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v29_to_v30(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v30_to_v31(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v31_to_v32(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v32_to_v33(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn v33_to_v34(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn add_drum_maps(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn add_synth_parameters(mut document: Value) -> Result<Value, String> {
    let values_of = synth_values_of;
    let Some(project) = document.get_mut("project").and_then(Value::as_object_mut) else {
        return Ok(document);
    };

    fn track_values(track: &mut Value, values_of: fn(&str) -> &'static [(u32, f64)]) {
        let Some(devices) = track.get_mut("devices").and_then(Value::as_array_mut) else {
            return;
        };
        for device in devices {
            let Some(device) = device.as_object_mut() else {
                continue;
            };

            let is_synth = device.get("plugin").is_some_and(|plugin| {
                plugin.get("format").and_then(Value::as_str) == Some("builtin")
                    && plugin.get("name").and_then(Value::as_str) == Some(SYNTH)
            });
            if !is_synth {
                continue;
            }
            let already = device
                .get("params")
                .and_then(Value::as_object)
                .is_some_and(|params| !params.is_empty());
            if already {
                continue;
            }
            let preset = device
                .get("state")
                .and_then(Value::as_str)
                .map(|text| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(text)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let mut params = serde_json::Map::new();
            for &(id, value) in values_of(&preset) {
                params.insert(id.to_string(), Value::from(value));
            }
            device.insert("params".into(), Value::Object(params));
        }
    }
    if let Some(list) = project.get_mut("tracks").and_then(Value::as_array_mut) {
        for track in list.iter_mut() {
            track_values(track, values_of);
        }
    }
    if let Some(master) = project.get_mut("master") {
        track_values(master, values_of);
    }
    Ok(document)
}

pub fn synth_values_of(preset: &str) -> &'static [(u32, f64)] {
    match preset {
        "keys" => &KEYS,
        "bass" => &BASS,
        "pluck" => &PLUCK,
        "pad" => &PAD,
        _ => &POLY,
    }
}

const SYNTH: &str = "synth";

const POLY: [(u32, f64); 34] = [
    (1, 0.666_666_687),
    (2, 0.500_000_000),
    (3, 0.430_000_007),
    (4, 1.000_000_000),
    (5, 0.324_999_988),
    (6, 0.666_666_687),
    (7, 0.500_000_000),
    (8, 0.569_999_993),
    (9, 1.000_000_000),
    (10, 0.675_000_012),
    (11, 0.000_000_000),
    (12, 0.000_000_000),
    (13, 0.492_032_528),
    (14, 0.254_823_595),
    (15, 0.500_000_000),
    (16, 0.000_000_000),
    (17, 0.300_000_012),
    (18, 0.058_481_321),
    (19, 0.246_350_974),
    (20, 0.699_999_988),
    (21, 0.231_773_436),
    (22, 0.058_481_321),
    (23, 0.246_350_974),
    (24, 0.500_000_000),
    (25, 0.231_773_436),
    (26, 0.000_000_000),
    (27, 0.498_116_732),
    (28, 0.285_714_298),
    (29, 0.000_000_000),
    (30, 0.000_000_000),
    (31, 0.000_000_000),
    (32, 1.000_000_000),
    (33, 1.000_000_000),
    (34, 0.159_999_996),
];

const KEYS: [(u32, f64); 34] = [
    (1, 0.333_333_343),
    (2, 0.500_000_000),
    (3, 0.460_000_008),
    (4, 1.000_000_000),
    (5, 0.400_000_006),
    (6, 0.333_333_343),
    (7, 0.750_000_000),
    (8, 0.529_999_971),
    (9, 0.600_000_024),
    (10, 0.600_000_024),
    (11, 0.000_000_000),
    (12, 0.000_000_000),
    (13, 0.629_329_264),
    (14, 0.227_921_158),
    (15, 0.500_000_000),
    (16, 0.000_000_000),
    (17, 0.500_000_000),
    (18, 0.036_840_927),
    (19, 0.326_956_213),
    (20, 0.349_999_994),
    (21, 0.215_087_369),
    (22, 0.058_481_321),
    (23, 0.246_350_974),
    (24, 0.500_000_000),
    (25, 0.231_773_436),
    (26, 0.000_000_000),
    (27, 0.498_116_732),
    (28, 0.285_714_298),
    (29, 0.000_000_000),
    (30, 0.000_000_000),
    (31, 0.000_000_000),
    (32, 1.000_000_000),
    (33, 1.000_000_000),
    (34, 0.239_999_995),
];

const BASS: [(u32, f64); 34] = [
    (1, 0.666_666_687),
    (2, 0.500_000_000),
    (3, 0.500_000_000),
    (4, 1.000_000_000),
    (5, 0.500_000_000),
    (6, 1.000_000_000),
    (7, 0.250_000_000),
    (8, 0.500_000_000),
    (9, 0.699_999_988),
    (10, 0.500_000_000),
    (11, 0.000_000_000),
    (12, 0.000_000_000),
    (13, 0.324_069_232),
    (14, 0.410_890_698),
    (15, 0.500_000_000),
    (16, 0.000_000_000),
    (17, 0.600_000_024),
    (18, 0.036_840_927),
    (19, 0.215_087_369),
    (20, 0.800_000_012),
    (21, 0.170_428_514),
    (22, 0.058_481_321),
    (23, 0.246_350_974),
    (24, 0.500_000_000),
    (25, 0.231_773_436),
    (26, 0.000_000_000),
    (27, 0.498_116_732),
    (28, 0.285_714_298),
    (29, 0.000_000_000),
    (30, 0.000_000_000),
    (31, 0.000_000_000),
    (32, 1.000_000_000),
    (33, 1.000_000_000),
    (34, 0.219_999_999),
];

const PLUCK: [(u32, f64); 34] = [
    (1, 0.666_666_687),
    (2, 0.500_000_000),
    (3, 0.449_999_988),
    (4, 1.000_000_000),
    (5, 0.349_999_994),
    (6, 0.666_666_687),
    (7, 0.500_000_000),
    (8, 0.550_000_012),
    (9, 0.800_000_012),
    (10, 0.649_999_976),
    (11, 0.000_000_000),
    (12, 0.000_000_000),
    (13, 0.558_463_216),
    (14, 0.301_511_347),
    (15, 0.500_000_000),
    (16, 0.000_000_000),
    (17, 0.500_000_000),
    (18, 0.000_000_000),
    (19, 0.231_773_436),
    (20, 0.000_000_000),
    (21, 0.195_310_667),
    (22, 0.058_481_321),
    (23, 0.246_350_974),
    (24, 0.500_000_000),
    (25, 0.231_773_436),
    (26, 0.000_000_000),
    (27, 0.498_116_732),
    (28, 0.285_714_298),
    (29, 0.000_000_000),
    (30, 0.000_000_000),
    (31, 0.000_000_000),
    (32, 1.000_000_000),
    (33, 1.000_000_000),
    (34, 0.200_000_003),
];

const PAD: [(u32, f64); 34] = [
    (1, 0.666_666_687),
    (2, 0.500_000_000),
    (3, 0.379_999_995),
    (4, 1.000_000_000),
    (5, 0.250_000_000),
    (6, 0.666_666_687),
    (7, 0.500_000_000),
    (8, 0.620_000_005),
    (9, 1.000_000_000),
    (10, 0.750_000_000),
    (11, 0.000_000_000),
    (12, 0.000_000_000),
    (13, 0.429_227_144),
    (14, 0.227_921_158),
    (15, 0.500_000_000),
    (16, 0.000_000_000),
    (17, 0.150_000_006),
    (18, 0.271_219_879),
    (19, 0.292_211_562),
    (20, 0.800_000_012),
    (21, 0.355_563_462),
    (22, 0.058_481_321),
    (23, 0.246_350_974),
    (24, 0.500_000_000),
    (25, 0.231_773_436),
    (26, 0.000_000_000),
    (27, 0.498_116_732),
    (28, 0.285_714_298),
    (29, 0.000_000_000),
    (30, 0.000_000_000),
    (31, 0.000_000_000),
    (32, 1.000_000_000),
    (33, 1.000_000_000),
    (34, 0.140_000_001),
];

fn add_sampler_samples(document: Value) -> Result<Value, String> {
    Ok(document)
}

fn highest_id(document: &Value) -> Result<u64, String> {
    fn walk(value: &Value, highest: &mut u64) -> Result<(), String> {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "id"
                        && let Value::String(text) = value
                    {
                        let id = u64::from_str_radix(text, 16)
                            .map_err(|_| format!("invalid id {text:?}"))?;
                        *highest = (*highest).max(id);
                    }
                    walk(value, highest)?;
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, highest)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut highest = 0;
    walk(document, &mut highest)?;
    Ok(highest)
}

pub const CURRENT_VERSION: u64 = FIRST_VERSION + MIGRATIONS.len() as u64;

pub fn upgrade(
    mut document: Value,
    from: u64,
    first: u64,
    migrations: &[Migration],
) -> Result<Value, (u64, String)> {
    let latest = first + migrations.len() as u64;
    if from < first || from > latest {
        return Err((from, format!("no migration path from version {from}")));
    }
    for version in from..latest {
        let step = migrations[(version - first) as usize];
        document = step(document).map_err(|message| (version, message))?;
        if let Some(object) = document.as_object_mut() {
            object.insert("version".into(), Value::from(version + 1));
        }
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v1_to_v2(mut doc: Value) -> Result<Value, String> {
        let project = doc["project"].as_object_mut().ok_or("missing project")?;
        let bpm = project.remove("bpm").ok_or("missing bpm")?;
        project.insert("tempo".into(), json!([{ "at": 0, "bpm": bpm }]));
        Ok(doc)
    }

    fn v2_to_v3(mut doc: Value) -> Result<Value, String> {
        let project = doc["project"].as_object_mut().ok_or("missing project")?;
        let title = project.remove("title").ok_or("missing title")?;
        project.insert("name".into(), title);
        Ok(doc)
    }

    const HISTORY: &[Migration] = &[v1_to_v2, v2_to_v3];

    #[test]
    fn migrations_apply_in_order_and_stamp_the_version() {
        let v1 = json!({ "version": 1, "project": { "title": "Song", "bpm": 128.0 } });
        let latest = upgrade(v1, 1, 1, HISTORY).unwrap();
        assert_eq!(
            latest,
            json!({ "version": 3, "project": { "name": "Song", "tempo": [{ "at": 0, "bpm": 128.0 }] } })
        );
    }

    #[test]
    fn a_document_already_current_is_untouched() {
        let v3 = json!({ "version": 3, "project": { "name": "x" } });
        assert_eq!(upgrade(v3.clone(), 3, 1, HISTORY).unwrap(), v3);
    }

    #[test]
    fn a_failing_step_reports_its_version() {
        let broken = json!({ "version": 1, "project": { "title": "no bpm" } });
        let (version, message) = upgrade(broken, 1, 1, HISTORY).unwrap_err();
        assert_eq!(version, 1);
        assert!(message.contains("bpm"));
    }

    #[test]
    fn unknown_versions_have_no_path() {
        assert!(upgrade(json!({}), 0, 1, HISTORY).is_err());
        assert!(upgrade(json!({}), 4, 1, HISTORY).is_err());
    }

    #[test]
    fn v25_to_v26_keeps_existing_clips_legacy_defaults() {
        let v25 = json!({"version":25,"project":{"tracks":[{"clips":[{
            "loop_length": 100, "fade_in":{"length":20}, "fade_out":{"length":30}
        }]}]}});
        let v26 = upgrade(v25, 25, FIRST_VERSION, &MIGRATIONS[..25]).unwrap();
        assert_eq!(v26["version"], json!(26));
        assert!(
            v26["project"]["tracks"][0]["clips"][0]
                .get("loop_phase")
                .is_none()
        );
        assert!(
            v26["project"]["tracks"][0]["clips"][0]
                .get("audio_phase_seconds")
                .is_none()
        );
        assert!(
            v26["project"]["tracks"][0]["clips"][0]["fade_in"]
                .get("offset")
                .is_none()
        );
        assert!(
            v26["project"]["tracks"][0]["clips"][0]["fade_in"]
                .get("span")
                .is_none()
        );
    }

    #[test]
    fn v26_to_v27_adds_no_flags_to_existing_clips() {
        let v26 = json!({"version":26,"project":{"tracks":[{"clips":[{"loop_length": 100}]}]}});
        let v27 = v26_to_v27(v26).unwrap();
        let clip = &v27["project"]["tracks"][0]["clips"][0];
        assert!(clip.get("reversed").is_none() && clip.get("inverted").is_none());
    }

    #[test]
    fn v27_to_v28_preserves_old_playback_and_adds_unset_clip_flags() {
        let v27 =
            json!({"version":27,"project":{"tracks":[{"clips":[{"content":{"type":"audio"}}]}]}});
        let v28 = v27_to_v28(v27).unwrap();
        assert_eq!(v28["project"]["auto_crossfade"], json!(false));
        assert_eq!(
            v28["project"]["tracks"][0]["clips"][0]["crossfade"],
            json!(false)
        );
    }

    #[test]
    fn v28_to_v29_freezes_nothing() {
        let v28 = json!({"version":28,"project":{"tracks":[{"clips":[]}]}});
        let v29 = v28_to_v29(v28).unwrap();
        assert!(v29["project"]["tracks"][0].get("frozen").is_none());
    }

    #[test]
    fn v29_to_v30_routes_no_input() {
        let v29 = json!({"version":29,"project":{"tracks":[{"clips":[]}]}});
        let v30 = v29_to_v30(v29).unwrap();
        assert!(v30["project"]["tracks"][0].get("input").is_none());
    }

    #[test]
    fn v30_to_v31_keeps_no_takes() {
        let v30 = json!({"version":30,"project":{"tracks":[{"clips":[{}]}]}});
        let v31 = v30_to_v31(v30).unwrap();
        assert!(
            v31["project"]["tracks"][0]["clips"][0]
                .get("takes")
                .is_none()
        );
    }

    #[test]
    fn v31_to_v32_keeps_no_notes_and_no_colour() {
        let v31 = json!({"version":31,"project":{"name":"x"}});
        let v32 = v31_to_v32(v31).unwrap();
        assert!(v32["project"].get("notes").is_none());
        assert!(v32["project"].get("new_track_color").is_none());
    }

    #[test]
    fn v32_to_v33_keeps_every_clip_unwarped() {
        let v32 = json!({"version":32,"project":{"tracks":[{"clips":[{}]}]}});
        let v33 = v32_to_v33(v32).unwrap();
        assert!(
            v33["project"]["tracks"][0]["clips"][0]
                .get("warp")
                .is_none()
        );
    }

    #[test]
    fn v33_to_v34_keeps_every_clip_without_a_pitch_envelope() {
        let v33 =
            json!({"version":33,"project":{"tracks":[{"clips":[{"envelopes":{"gain":[]}}]}]}});
        let v34 = v33_to_v34(v33).unwrap();
        let envelopes = &v34["project"]["tracks"][0]["clips"][0]["envelopes"];
        assert!(envelopes.get("pitch").is_none());
        assert!(envelopes.get("gain").is_some(), "what was there stays");
    }

    #[test]
    fn v1_and_v3_migrations_write_their_own_literals_not_live_defaults() {
        let v1 = json!({ "format": "lumen-project", "version": 1, "project": { "name": "x" } });
        let v2 = upgrade(v1, 1, FIRST_VERSION, &MIGRATIONS[..1]).unwrap();
        assert_eq!(
            v2["project"]["transport"],
            json!({
                "loop_start": 0,
                "loop_end": 22_579_200,
                "loop_enabled": false,
                "punch_in": false,
                "punch_out": false,
            })
        );
        assert_eq!(
            v2["project"]["view"],
            json!({
                "left_beat": 0.0,
                "px_per_beat": 24.0,
                "browser_open": true,
                "detail_open": true,
                "browser_size": 220.0,
                "detail_size": 220.0,
                "top": 0.0,
                "track_heights": {},
                "expanded": [],
                "grid": {
                    "fixed": false,
                    "width": "narrow",
                    "size": { "note": 16 },
                    "feel": "straight",
                    "snap": true,
                },
                "collapsed": [],
                "header_width": 204.0,
                "returns_folded": false,
            })
        );

        let v3 = json!({
            "format": "lumen-project",
            "version": 3,
            "project": { "transport": {}, "view": { "left_beat": 3.0 } }
        });
        let v4 = upgrade(v3, 3, FIRST_VERSION, &MIGRATIONS[..3]).unwrap();
        assert_eq!(
            v4["project"]["view"]["grid"],
            json!({
                "fixed": false,
                "width": "narrow",
                "size": { "note": 16 },
                "feel": "straight",
                "snap": true,
            })
        );
    }

    #[test]
    fn current_version_matches_the_registry() {
        assert_eq!(CURRENT_VERSION, FIRST_VERSION + MIGRATIONS.len() as u64);
    }

    #[test]
    fn v1_to_v2_writes_the_default_transport_and_view() {
        let v1 = json!({ "format": "lumen-project", "version": 1, "project": { "name": "x" } });
        let v2 = upgrade(v1, 1, FIRST_VERSION, &MIGRATIONS[..1]).unwrap();
        assert_eq!(v2["version"], 2);
        assert_eq!(v2["project"]["transport"]["loop_enabled"], false);
        assert_eq!(v2["project"]["view"]["px_per_beat"], 24.0);
        assert_eq!(v2["project"]["name"], "x");
    }

    #[test]
    fn v2_to_v3_writes_arm_and_track_heights() {
        let v2 = json!({
            "format": "lumen-project",
            "version": 2,
            "project": { "tracks": [{ "id": "1" }], "view": { "left_beat": 3.0 } }
        });
        let v3 = upgrade(v2, 2, FIRST_VERSION, &MIGRATIONS[..2]).unwrap();
        assert_eq!(v3["version"], 3);
        assert_eq!(v3["project"]["tracks"][0]["armed"], false);
        assert_eq!(v3["project"]["view"]["top"], 0.0);
        assert_eq!(v3["project"]["view"]["left_beat"], 3.0);
        assert!(
            v3["project"]["view"]["track_heights"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(v3["project"]["view"]["expanded"], serde_json::json!([]));
    }

    #[test]
    fn v3_to_v4_writes_markers_punch_and_the_grid() {
        let v3 = json!({
            "format": "lumen-project",
            "version": 3,
            "project": {
                "transport": { "loop_enabled": true },
                "view": { "left_beat": 3.0 }
            }
        });
        let v4 = upgrade(v3, 3, FIRST_VERSION, &MIGRATIONS[..3]).unwrap();
        assert_eq!(v4["version"], 4);
        assert_eq!(v4["project"]["markers"], json!([]));
        let transport = &v4["project"]["transport"];
        assert_eq!(transport["loop_enabled"], true, "kept");
        assert_eq!(transport["punch_in"], false);
        assert_eq!(transport["punch_out"], false);
        assert_eq!(v4["project"]["view"]["grid"]["width"], "narrow");
        assert_eq!(v4["project"]["view"]["left_beat"], 3.0);
    }

    #[test]
    fn v4_to_v5_writes_clip_colour_lock_fades_and_loop() {
        let v4 = json!({
            "format": "lumen-project",
            "version": 4,
            "project": {
                "tracks": [
                    { "clips": [{ "name": "Kick", "muted": true }] },
                    { "clips": [] },
                    { "name": "no clips" }
                ]
            }
        });
        let v5 = upgrade(v4, 4, FIRST_VERSION, &MIGRATIONS[..4]).unwrap();
        assert_eq!(v5["version"], 5);
        let clip = &v5["project"]["tracks"][0]["clips"][0];
        assert_eq!(clip["muted"], true, "kept");
        assert_eq!(clip["color"], Value::Null);
        assert_eq!(clip["locked"], false);
        assert_eq!(clip["fade_in"], json!({ "length": 0, "shape": "linear" }));
        assert_eq!(clip["fade_out"], json!({ "length": 0, "shape": "linear" }));
        assert_eq!(clip["loop_length"], Value::Null);
    }

    #[test]
    fn v5_to_v6_turns_named_fade_shapes_into_exact_curves() {
        let v5 = json!({
            "format": "lumen-project",
            "version": 5,
            "project": {
                "tracks": [{ "clips": [{
                    "fade_in": { "length": 10, "shape": "s_curve" },
                    "fade_out": { "length": 0, "shape": "linear" }
                }, {
                    "fade_in": { "length": 5, "shape": "slow" },
                    "fade_out": { "length": 5, "shape": "fast" }
                }]}, { "clips": [{ "name": "no fades" }] }]
            }
        });
        let v6 = upgrade(v5, 5, FIRST_VERSION, &MIGRATIONS[..5]).unwrap();
        assert_eq!(v6["version"], 6);
        let clips = &v6["project"]["tracks"][0]["clips"];
        let curve = |v: &Value| -> lumen_core::curve::Curve {
            serde_json::from_value(v["curve"].clone()).unwrap()
        };
        use lumen_core::curve::Curve;
        assert_eq!(curve(&clips[0]["fade_in"]), Curve::S_CURVE);
        assert_eq!(clips[0]["fade_out"], json!({ "length": 0 }), "straight");
        assert_eq!(curve(&clips[1]["fade_in"]), Curve::SLOW);
        assert_eq!(curve(&clips[1]["fade_out"]), Curve::FAST);
        let bad = json!({ "version": 5, "project": { "tracks": [{ "clips": [{
            "fade_in": { "length": 1, "shape": "wobbly" }
        }]}]}});
        assert!(upgrade(bad, 5, FIRST_VERSION, &MIGRATIONS[..5]).is_err());
    }

    #[test]
    fn v6_to_v7_adds_a_master_with_the_next_free_id_and_routing_defaults() {
        let v6 = json!({ "version": 6, "project": {
            "name": "x",
            "tracks": [
                { "id": "0000000000000003", "kind": "audio", "clips": [
                    { "id": "00000000000000a0" }
                ] },
            ],
            "markers": [{ "id": "00000000000000b0" }],
        } });
        let v7 = upgrade(v6, 6, FIRST_VERSION, MIGRATIONS).unwrap();
        let project = &v7["project"];
        assert_eq!(project["pan_law"], "constant_power");
        assert_eq!(project["master"]["id"], "00000000000000b1");
        assert_eq!(project["master"]["kind"], "master");
        let track = &project["tracks"][0];
        assert_eq!(track["output"], "auto");
        assert_eq!(track["sends"], json!([]));
        assert_eq!(
            (&track["invert"], &track["trim_db"]),
            (&json!(false), &json!(0.0))
        );
    }

    #[test]
    fn v7_to_v8_puts_tracks_under_their_groups_and_returns_out_of_them() {
        let id = |n: u64| Value::from(format!("{n:016x}"));
        let v7 = json!({ "version": 7, "project": { "view": {}, "tracks": [
            { "id": id(1), "kind": "audio", "parent": id(3) },
            { "id": id(2), "kind": "audio" },
            { "id": id(3), "kind": "group" },
            { "id": id(4), "kind": "return", "parent": id(3) },
            { "id": id(5), "kind": "midi", "parent": id(3) },
            { "id": id(6), "kind": "group", "parent": id(3) },
            { "id": id(7), "kind": "audio", "parent": id(6) },
        ] } });
        let v8 = upgrade(v7, 7, FIRST_VERSION, MIGRATIONS).unwrap();
        let project = &v8["project"];
        let order: Vec<&Value> = project["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| &t["id"])
            .collect();
        assert_eq!(
            order,
            [&id(2), &id(3), &id(1), &id(5), &id(6), &id(7), &id(4)]
        );
        assert!(
            project["tracks"][6].get("parent").is_none(),
            "the return left"
        );
        assert_eq!(project["view"]["collapsed"], json!([]));
    }

    #[test]
    fn v8_to_v9_writes_the_header_width() {
        let v8 = json!({ "version": 8, "project": { "view": {}, "tracks": [] } });
        let v9 = upgrade(v8, 8, FIRST_VERSION, MIGRATIONS).unwrap();
        assert_eq!(v9["project"]["view"]["header_width"], json!(204.0));
    }

    #[test]
    fn v9_to_v10_writes_the_returns_area_unfolded_at_its_natural_size() {
        let v9 = json!({ "version": 9, "project": { "view": {}, "tracks": [] } });
        let v10 = upgrade(v9, 9, FIRST_VERSION, MIGRATIONS).unwrap();
        assert_eq!(v10["project"]["view"]["returns_folded"], json!(false));
        assert!(v10["project"]["view"].get("returns_height").is_none());
    }

    #[test]
    fn v10_to_v11_leaves_midi_clips_as_they_were() {
        let clip = json!({ "type": "midi", "notes": [[0, 352_800, 60, 100]] });
        let v10 = json!({ "version": 10, "project": { "tracks": [{ "clips": [{ "content": clip.clone() }] }] } });
        let v11 = upgrade(v10, 10, FIRST_VERSION, MIGRATIONS).unwrap();
        assert_eq!(v11["project"]["tracks"][0]["clips"][0]["content"], clip);
    }

    #[test]
    fn an_id_at_the_limit_leaves_no_room_for_a_master() {
        let v6 = json!({ "version": 6, "project": { "tracks": [
            { "id": "ffffffffffffffff", "kind": "audio" },
        ] } });
        let (version, message) = upgrade(v6, 6, FIRST_VERSION, MIGRATIONS).unwrap_err();
        assert_eq!(version, 6);
        assert!(message.contains("largest possible"), "{message}");
    }

    #[test]
    fn v6_to_v7_moves_a_master_track_out_of_the_tracks() {
        let v6 = json!({ "version": 6, "project": { "tracks": [
            { "id": "0000000000000001", "kind": "audio" },
            { "id": "0000000000000002", "kind": "master", "name": "Main", "parent": "0000000000000009" },
        ] } });
        let v7 = upgrade(v6, 6, FIRST_VERSION, MIGRATIONS).unwrap();
        let project = &v7["project"];
        assert_eq!(project["tracks"].as_array().unwrap().len(), 1);
        assert_eq!(project["master"]["id"], "0000000000000002");
        assert_eq!(project["master"]["name"], "Main");
        assert!(project["master"].get("parent").is_none());
    }
}
