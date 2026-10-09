//! Format migrations: pure functions over untyped JSON.
//!
//! Only the newest format has typed structs. A file of version N is parsed
//! into a `serde_json::Value`, upgraded one version at a time, then
//! deserialized. Rules:
//! - every change to what is persisted bumps [`CURRENT_VERSION`] and appends
//!   exactly one migration to [`MIGRATIONS`];
//! - migrations are never edited or removed once released;
//! - every version keeps a golden fixture under `tests/fixtures/vN/`, loaded
//!   by the tests forever.

use serde_json::Value;

/// Upgrades a document from version `n` to `n + 1`.
pub type Migration = fn(Value) -> Result<Value, String>;

/// The oldest version Lumen has ever written.
pub const FIRST_VERSION: u64 = 1;

/// `MIGRATIONS[i]` upgrades version `FIRST_VERSION + i` to the next one.
pub const MIGRATIONS: &[Migration] = &[v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v7];

/// Version 2 (M3 slice 3) adds the saved loop region (`transport`) and view
/// state (`view`). Version 1 files get the defaults written out.
fn v1_to_v2(mut document: Value) -> Result<Value, String> {
    let project = document
        .get_mut("project")
        .and_then(Value::as_object_mut)
        .ok_or("missing project")?;
    let defaults = |value: serde_json::Result<Value>| value.map_err(|e| e.to_string());
    project.insert(
        "transport".into(),
        defaults(serde_json::to_value(crate::model::TransportState::default()))?,
    );
    project.insert(
        "view".into(),
        defaults(serde_json::to_value(crate::model::ViewState::default()))?,
    );
    Ok(document)
}

/// Version 3 (M3 slice 4) adds record arm per track, and the arrangement's
/// vertical scroll, track heights and unfolded tracks to the view. The defaults are written
/// out.
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

/// Version 4 (M3 slice 5) adds named locators (`markers`), the punch
/// switches to the transport, and the grid to the view. The defaults are
/// written out.
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
        view.entry("grid").or_insert(
            serde_json::to_value(lumen_core::grid::Grid::default()).map_err(|e| e.to_string())?,
        );
    }
    Ok(document)
}

/// Version 5 (M3 slice 6) gives clips a colour, a lock, fades and a loop.
/// The defaults are written out on every clip here; saving writes them
/// only when set.
fn v4_to_v5(mut document: Value) -> Result<Value, String> {
    // Version 5's own words, frozen: later versions change fades.
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

/// Version 6 (M3 slice 6d, owner's choice C3) bends fades freely: a fade's
/// `shape` (one of four names) becomes a `curve`, four numbers for a cubic
/// Bézier, written only when it is not straight. Each named shape is an
/// exact curve, so old fades sound the same.
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

/// Version 7 (M4 slice 2) routes tracks: a master track in every project
/// (kept apart from `tracks`), each track's output, sends, polarity and
/// input trim, and the project's pan law. A master track already among the
/// tracks moves out to `master`; otherwise one is made with the next free
/// id. The defaults are written out on every track. The pan law is −3 dB
/// constant power (owner, M4): projects from before play 3 dB quieter at
/// the centre.
fn v6_to_v7(mut document: Value) -> Result<Value, String> {
    let next_id = highest_id(&document)? + 1;
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
        // Version 7's own words, frozen.
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

/// The highest id written anywhere in `document` (every `"id"` field).
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

/// The version this build writes.
pub const CURRENT_VERSION: u64 = FIRST_VERSION + MIGRATIONS.len() as u64;

/// Applies `migrations` to bring `document` from `from` to the latest
/// version they describe, updating the `version` field after each step.
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

    // A synthetic history used only to test the mechanism. Version 1 had a
    // single `bpm`; version 2 a tempo list; version 3 renamed `title`.
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
