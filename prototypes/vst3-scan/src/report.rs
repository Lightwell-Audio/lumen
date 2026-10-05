//! What a scan learns about a module, from either source.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassInfo {
    pub name: String,
    pub category: String,
    pub sub_categories: String,
    pub vendor: String,
    pub version: String,
    pub sdk_version: String,
    pub cid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryReport {
    pub vendor: String,
    pub url: String,
    pub classes: Vec<ClassInfo>,
    pub load_ms: f64,
    pub init_dll: Option<bool>,
    pub enumerate_ms: f64,
}

impl FactoryReport {
    /// Classes that are plugins (instruments and effects), not controllers.
    pub fn audio_classes(&self) -> impl Iterator<Item = &ClassInfo> {
        self.classes
            .iter()
            .filter(|c| c.category == "Audio Module Class")
    }
}

/// Parses `moduleinfo.json`. The format is JSON5-flavoured; plain JSON is
/// tried first, then a lenient pass that drops comments and trailing commas.
pub fn parse_moduleinfo(text: &str) -> Result<FactoryReport, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&relax_json5(text)))
        .map_err(|e| format!("moduleinfo.json: {e}"))?;
    let s = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let factory = value.get("Factory Info").cloned().unwrap_or_default();
    let classes = value
        .get("Classes")
        .and_then(|c| c.as_array())
        .map(|classes| {
            classes
                .iter()
                .map(|c| ClassInfo {
                    name: s(c, "Name"),
                    category: s(c, "Category"),
                    sub_categories: c
                        .get("Sub Categories")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join("|")
                        })
                        .unwrap_or_default(),
                    vendor: s(c, "Vendor"),
                    version: s(c, "Version"),
                    sdk_version: s(c, "SDKVersion"),
                    cid: s(c, "CID"),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(FactoryReport {
        vendor: s(&factory, "Vendor"),
        url: s(&factory, "URL"),
        classes,
        load_ms: 0.0,
        init_dll: None,
        enumerate_ms: 0.0,
    })
}

fn relax_json5(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ',' => {
                // Drop the comma if the next significant character closes a container.
                let rest: String = chars.clone().collect();
                let next = rest.trim_start().chars().next();
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "Name": "Example",
        "Version": "1.0.0",
        "Factory Info": { "Vendor": "Acme", "URL": "https://acme.example", },
        // JSON5 allows comments and trailing commas
        "Classes": [
            { "CID": "ABCD", "Category": "Audio Module Class", "Name": "Acme Synth",
              "Sub Categories": ["Instrument", "Synth"], "Vendor": "Acme",
              "Version": "1.0.0", "SDKVersion": "VST 3.7.9", },
            { "CID": "EF01", "Category": "Component Controller Class", "Name": "Acme Synth" },
        ],
    }"#;

    #[test]
    fn parses_json5_flavoured_moduleinfo() {
        let report = parse_moduleinfo(SAMPLE).unwrap();
        assert_eq!(report.vendor, "Acme");
        assert_eq!(report.classes.len(), 2);
        let synth: Vec<_> = report.audio_classes().collect();
        assert_eq!(synth.len(), 1);
        assert_eq!(synth[0].sub_categories, "Instrument|Synth");
    }

    #[test]
    fn keeps_commas_and_slashes_inside_strings() {
        let report =
            parse_moduleinfo(r#"{"Factory Info": {"Vendor": "a, b // c", }, "Classes": []}"#)
                .unwrap();
        assert_eq!(report.vendor, "a, b // c");
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_moduleinfo("{ not json").is_err());
        assert!(parse_moduleinfo("").is_err());
    }
}
