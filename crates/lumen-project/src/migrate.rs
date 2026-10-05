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
pub const MIGRATIONS: &[Migration] = &[];

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
}
